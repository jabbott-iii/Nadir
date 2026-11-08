// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! QOA, the "Quite OK Audio" format (qoaformat.org, whose reference is
//! `qoa.h`): lossy audio of 16-bit samples at 3.2 bits a sample, which the
//! game plays its music and long sounds from.
//!
//! [`encode`] turns samples into a file, and [`decode`] reads a file back
//! whole. [`decode_frame`] reads one frame, [`FRAME_LEN`] samples of each
//! channel, into a buffer the caller keeps, so that music can be decoded as
//! it plays, with nothing allocated; [`header`] gives what it needs.
//!
//! ```
//! use nadir_codec::Limits;
//! use nadir_codec::qoa::{self, Header};
//!
//! // A tenth of a second of a 440 Hz tone, at 44.1 kHz.
//! let samples: Vec<i16> = (0..4410)
//!     .map(|i| (f64::from(i) * 440.0 / 44_100.0 * std::f64::consts::TAU).sin())
//!     .map(|x| (x * 8000.0).round() as i16)
//!     .collect();
//! let header = Header { channels: 1, sample_rate: 44_100, samples: 4410 };
//! let file = qoa::encode(&header, &samples)?;
//! assert_eq!(file.len(), 8 + 8 + 16 + 221 * 8);
//!
//! let sound = qoa::decode(&file, Limits::default())?;
//! assert_eq!(sound.header, header);
//! // Lossy: near the samples, not equal to them.
//! let worst = samples
//!     .iter()
//!     .zip(&sound.samples)
//!     .map(|(&a, &b)| (i32::from(a) - i32::from(b)).abs())
//!     .max();
//! assert!(worst < Some(200));
//! # Ok::<(), nadir_codec::CodecError>(())
//! ```
//!
//! # The format
//!
//! Every number is big-endian. A file is an 8-byte header, `qoaf` and the
//! samples each channel has (a `u32`), then frames. Each frame holds
//! [`FRAME_LEN`] samples of each channel, but the last, which holds the
//! rest, and is:
//!
//! - an 8-byte header: the channels (a byte), the sample rate (24 bits), the
//!   samples each channel has in the frame (16 bits), and the frame's size
//!   in bytes, its header included (16 bits);
//! - for each channel, the predictor's state as the frame starts: the last
//!   four samples, then four weights, each an `i16`;
//! - slices of 8 bytes, one for each 20 samples of each channel, the
//!   channels taking turns slice by slice: a 4-bit scale factor, then 20
//!   residuals of 3 bits. The last slice of a channel may hold fewer
//!   samples, with the residuals it does not use left 0.
//!
//! Each sample is a prediction from the last four, by their weights, plus
//! its residual scaled by the slice's factor; the weights then move toward
//! what would have predicted it. Every frame starts the predictor again from
//! its state, so frames decode alone, in any order.
//!
//! # What is accepted
//!
//! Both sides take 1 to [`MAX_CHANNELS`] channels, as the reference does,
//! and sample rates of 1 to [`MAX_SAMPLE_RATE`] Hz. The decoder holds a file
//! to the specification. Like the reference, it rejects a file whose header
//! gives 0 samples (a stream, whose length its encoder did not know), no
//! channels or more than [`MAX_CHANNELS`], or a sample rate of 0. Where the
//! reference stops at a frame it cannot read and gives the samples before
//! it, or reads more than the format allows, it rejects the file:
//!
//! - a frame whose channels or sample rate differ from the first one's;
//! - a frame whose samples, size or place is not the one the file's sample
//!   count gives it: every frame but the last holds [`FRAME_LEN`] samples a
//!   channel, and the last the rest, each in exactly the bytes its samples
//!   take (the reference also reads a frame larger than its slices);
//! - a file shorter or longer than its frames;
//! - a last slice whose unused residuals are not 0, which the reference
//!   does not look at and the specification says must be.
//!
//! [`encode`] chooses each slice's scale factor as the reference encoder
//! does, with its penalty for weights grown large, in its 32-bit arithmetic,
//! which wraps where C's `int` does on the machines it runs on. It writes
//! the same bytes, but in one case: where the weights the reference carries
//! into a frame no longer fit in 16 bits, it writes their low 16 bits and
//! goes on with the weights it had, which its file no longer holds. This
//! encoder goes on from the 16 bits the file holds, as the decoder will, so
//! that what it measured is what decodes. Loud, regular signals reach the
//! case: a pattern of period 4 at full scale carries weights of 57,865
//! into its second frame.

use std::fmt;

use crate::{CodecError, Limits};

/// The samples each channel has in every frame but the last, which has 1
/// to this many.
pub const FRAME_LEN: usize = SLICES_PER_FRAME * SLICE_LEN;

/// The most channels a file may have, as the reference's encoder and
/// decoder take.
pub const MAX_CHANNELS: u8 = 8;

/// The highest sample rate a file can hold, in 24 bits.
pub const MAX_SAMPLE_RATE: u32 = 0xff_ffff;

/// The description of a sound, from its file's header and its first
/// frame's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Header {
    /// 1 for mono, 2 for stereo, up to [`MAX_CHANNELS`].
    pub channels: u8,
    /// Samples a second of each channel, 1 to [`MAX_SAMPLE_RATE`].
    pub sample_rate: u32,
    /// The samples each channel has, at least 1.
    pub samples: u32,
}

/// A decoded sound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sound {
    /// Its header.
    pub header: Header,
    /// Its samples, interleaved (left, right, left, … in stereo):
    /// `samples × channels` of them.
    pub samples: Vec<i16>,
}

/// What [`decode_frame`] read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Frame {
    /// The samples of each channel it wrote: [`FRAME_LEN`], but in the
    /// last frame.
    pub samples: usize,
    /// The offset of the next frame, or the file's length after the last.
    pub next: usize,
}

/// What is wrong with a QOA file. See [`CodecError::Qoa`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The input does not start with `qoaf`.
    NotQoa,
    /// The header gives 0 samples: a stream, whose length is not known.
    Streaming,
    /// The channels are 0 or more than [`MAX_CHANNELS`].
    BadChannels {
        /// The value in the header.
        found: u8,
    },
    /// The sample rate is 0.
    BadSampleRate,
    /// The samples would take more than [`Limits::max_decoded_bytes`], at 2
    /// bytes each.
    TooManyBytes {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// The input ends too soon.
    UnexpectedEnd {
        /// What should have come next.
        expected: &'static str,
    },
    /// Bytes follow the last frame.
    TrailingBytes,
    /// The offset given to [`decode_frame`] is not where one of the file's
    /// frames starts.
    NotAFrame,
    /// A frame's channels are not the file's.
    ChannelsDiffer {
        /// The frame's channels.
        found: u8,
        /// The file's, from its first frame.
        expected: u8,
    },
    /// A frame's sample rate is not the file's.
    SampleRateDiffers {
        /// The frame's sample rate.
        found: u32,
        /// The file's, from its first frame.
        expected: u32,
    },
    /// A frame does not hold the samples its place in the file gives it.
    BadFrameSamples {
        /// The samples of each channel the frame's header gives.
        found: u16,
        /// The samples it should have.
        expected: usize,
    },
    /// A frame's size is not the one its channels and samples give it.
    BadFrameSize {
        /// The size the frame's header gives.
        found: u16,
        /// The size it should have.
        expected: usize,
    },
    /// The last slice of a channel has residuals it does not use that are
    /// not 0.
    UnusedNotZero,
    /// The buffer given to [`decode_frame`] cannot hold the frame.
    OutputTooShort {
        /// The samples the frame has, of every channel.
        needed: usize,
    },
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotQoa => f.write_str("the file does not start with `qoaf`"),
            Self::Streaming => f.write_str(
                "the header gives 0 samples, as a stream does, and only files of known length are read",
            ),
            Self::BadChannels { found } => write!(
                f,
                "the file has {found} channels, not 1 to {MAX_CHANNELS}"
            ),
            Self::BadSampleRate => f.write_str("the sample rate is 0"),
            Self::TooManyBytes { limit } => write!(
                f,
                "the samples would take more than the limit of {limit} bytes"
            ),
            Self::UnexpectedEnd { expected } => {
                write!(f, "the file ends where {expected} should be")
            }
            Self::TrailingBytes => f.write_str("there are bytes after the last frame"),
            Self::NotAFrame => f.write_str("no frame of the file starts here"),
            Self::ChannelsDiffer { found, expected } => write!(
                f,
                "the frame has {found} channels, but the file {expected}"
            ),
            Self::SampleRateDiffers { found, expected } => write!(
                f,
                "the frame's sample rate is {found} Hz, but the file's {expected} Hz"
            ),
            Self::BadFrameSamples { found, expected } => write!(
                f,
                "the frame holds {found} samples a channel, not {expected}"
            ),
            Self::BadFrameSize { found, expected } => {
                write!(f, "the frame's size is {found} bytes, not {expected}")
            }
            Self::UnusedNotZero => {
                f.write_str("the residuals a last slice does not use are not 0")
            }
            Self::OutputTooShort { needed } => {
                write!(f, "the buffer holds fewer than the frame's {needed} samples")
            }
        }
    }
}

/// Why samples cannot be encoded. See [`CodecError::QoaEncode`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EncodeErrorKind {
    /// The channels are 0 or more than [`MAX_CHANNELS`].
    BadChannels {
        /// The channels the header gives.
        found: u8,
    },
    /// The sample rate is 0 or more than [`MAX_SAMPLE_RATE`].
    BadSampleRate {
        /// The sample rate the header gives.
        found: u32,
    },
    /// The header gives 0 samples, which a file cannot hold: it would read
    /// as a stream.
    NoSamples,
    /// The samples are not `samples × channels` of them.
    WrongDataSize {
        /// The samples of each channel the header gives.
        samples: u32,
        /// The channels the header gives.
        channels: u8,
        /// The number of samples given.
        found: usize,
    },
}

impl fmt::Display for EncodeErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadChannels { found } => {
                write!(f, "{found} channels, not 1 to {MAX_CHANNELS}")
            }
            Self::BadSampleRate { found } => {
                write!(f, "a sample rate of {found} Hz, not 1 to {MAX_SAMPLE_RATE}")
            }
            Self::NoSamples => f.write_str("there are no samples"),
            Self::WrongDataSize {
                samples,
                channels,
                found,
            } => write!(f, "{found} samples, not {samples} × {channels}"),
        }
    }
}

const MAGIC: [u8; 4] = *b"qoaf";
const FILE_HEADER_LEN: usize = 8;
const FRAME_HEADER_LEN: usize = 8;
/// The bytes of one channel's predictor state: four samples, four weights.
const LMS_BYTES: usize = 16;
/// The samples of one channel a slice holds.
const SLICE_LEN: usize = 20;
const SLICE_BYTES: usize = 8;
const SLICES_PER_FRAME: usize = 256;

/// The scale factors: `(s + 1)^2.75`, rounded, for `s` from 0 to 15.
const SCALEFACTORS: [i32; 16] = [
    1, 7, 21, 45, 84, 138, 211, 304, 421, 562, 731, 928, 1157, 1419, 1715, 2048,
];

/// `2^16 / factor`, rounded up, for each scale factor: dividing by a factor
/// is a multiplication by this and a shift.
const RECIPROCALS: [i32; 16] = reciprocals();

const fn reciprocals() -> [i32; 16] {
    let mut table = [0; 16];
    let mut s = 0;
    while s < 16 {
        table[s] = ((1 << 16) + SCALEFACTORS[s] - 1) / SCALEFACTORS[s];
        s += 1;
    }
    table
}

/// The quantized residual for each scaled residual from -8 to 8, at index
/// `residual + 8`. 0 shares 1's.
const QUANTIZE: [u8; 17] = [7, 7, 7, 5, 5, 3, 3, 1, 0, 0, 2, 2, 4, 4, 6, 6, 6];

/// What each quantized residual stands for, under each scale factor: the
/// factor times 0.75, -0.75, 2.5, -2.5, 4.5, -4.5, 7 and -7, rounded to the
/// nearest, ties away from 0.
const DEQUANTIZE: [[i32; 8]; 16] = dequantize();

const fn dequantize() -> [[i32; 8]; 16] {
    // The multipliers, in quarters.
    const QUARTERS: [i32; 8] = [3, -3, 10, -10, 18, -18, 28, -28];
    let mut table = [[0; 8]; 16];
    let mut s = 0;
    while s < 16 {
        let mut q = 0;
        while q < 8 {
            let quarters = SCALEFACTORS[s] * QUARTERS[q];
            let rounded = (quarters.abs() + 2) / 4;
            table[s][q] = if quarters < 0 { -rounded } else { rounded };
            q += 1;
        }
        s += 1;
    }
    table
}

/// The predictor of one channel: a sign-sign LMS filter over the last four
/// samples.
///
/// The arithmetic is the reference's, in C's 32-bit `int`, which wraps
/// where it overflows on the machines the reference runs on. Only weights
/// grown far past what an encoder writes overflow it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Lms {
    /// The last four samples, the latest last.
    history: [i32; 4],
    /// Their weights, in units of 2^-13.
    weights: [i32; 4],
}

impl Lms {
    /// The state an encoder starts each channel from: weights of 0, 0, -1
    /// and 2, which continue a line through the last two samples.
    const START: Self = Self {
        history: [0; 4],
        weights: [0, 0, -(1 << 13), 1 << 14],
    };

    fn predict(&self) -> i32 {
        let sum = (0..4).fold(0i32, |sum, i| {
            sum.wrapping_add(self.weights[i].wrapping_mul(self.history[i]))
        });
        sum >> 13
    }

    /// Moves the weights after `sample`, whose dequantized residual was
    /// `residual`, and takes it into the history.
    fn update(&mut self, sample: i32, residual: i32) {
        let delta = residual >> 4;
        for (weight, &past) in self.weights.iter_mut().zip(&self.history) {
            *weight = weight.wrapping_add(if past < 0 { -delta } else { delta });
        }
        self.history = [self.history[1], self.history[2], self.history[3], sample];
    }

    /// Reads a state as a frame holds it.
    fn read(bytes: &[u8; LMS_BYTES]) -> Self {
        let value = |at: usize| i32::from(i16::from_be_bytes([bytes[at], bytes[at + 1]]));
        Self {
            history: [value(0), value(2), value(4), value(6)],
            weights: [value(8), value(10), value(12), value(14)],
        }
    }

    /// Writes the state as a frame holds it, each value in 16 bits.
    fn write(&self, out: &mut Vec<u8>) {
        for value in self.history.iter().chain(&self.weights) {
            // The history holds samples, and the encoder keeps its weights
            // in 16 bits at a frame's start, so this keeps every bit.
            out.extend_from_slice(&(*value as i16).to_be_bytes());
        }
    }
}

/// `value / SCALEFACTORS[factor]`, rounded to the nearest, ties up, but
/// never to 0 when `value` is not 0, as the reference divides.
fn divide(value: i32, factor: usize) -> i32 {
    let n = value
        .wrapping_mul(RECIPROCALS[factor])
        .wrapping_add(1 << 15)
        >> 16;
    n + value.signum() - n.signum()
}

/// The penalty the reference's encoder adds to each sample's squared error
/// when its weights have grown large, which keeps them from growing
/// further and making pops.
fn weights_penalty(weights: &[i32; 4]) -> u64 {
    let squares = weights.iter().fold(0i32, |sum, &weight| {
        sum.wrapping_add(weight.wrapping_mul(weight))
    });
    // At most (2^31 >> 18) - 0x8ff = 5889, whose square fits.
    let penalty = ((squares >> 18) - 0x8ff).max(0).unsigned_abs();
    u64::from(penalty * penalty)
}

fn error(offset: usize, kind: ErrorKind) -> CodecError {
    CodecError::Qoa { offset, kind }
}

/// The bytes of a frame of `slices` slices a channel.
fn frame_size(channels: usize, slices: usize) -> usize {
    FRAME_HEADER_LEN + channels * (LMS_BYTES + slices * SLICE_BYTES)
}

/// The bytes of a file of `samples` samples in each of `channels`.
fn file_len(channels: usize, samples: u32) -> u64 {
    let samples = u64::from(samples);
    let frame_len = FRAME_LEN as u64;
    let full = samples / frame_len;
    // Below `FRAME_LEN`.
    let rest = (samples % frame_len) as usize;
    let mut len = FILE_HEADER_LEN as u64 + full * frame_size(channels, SLICES_PER_FRAME) as u64;
    if rest > 0 {
        len += frame_size(channels, rest.div_ceil(SLICE_LEN)) as u64;
    }
    len
}

/// Reads and checks a file's header and its first frame's, and checks that
/// the file is as long as its frames.
///
/// # Errors
///
/// [`CodecError::Qoa`], with the byte offset of the problem, for a file
/// that does not start as a QOA file of known length should, or whose
/// length is not its frames'.
pub fn header(bytes: &[u8]) -> Result<Header, CodecError> {
    if !MAGIC.starts_with(bytes.get(..MAGIC.len()).unwrap_or(bytes)) {
        return Err(error(0, ErrorKind::NotQoa));
    }
    let Some(&[_, _, _, _, s0, s1, s2, s3]) = bytes.first_chunk::<FILE_HEADER_LEN>() else {
        return Err(cut_short(bytes, "the rest of the file's header"));
    };
    let samples = u32::from_be_bytes([s0, s1, s2, s3]);
    if samples == 0 {
        return Err(error(4, ErrorKind::Streaming));
    }
    let Some(&[channels, r0, r1, r2, ..]) = bytes
        .get(FILE_HEADER_LEN..)
        .and_then(<[u8]>::first_chunk::<FRAME_HEADER_LEN>)
    else {
        return Err(cut_short(bytes, "the first frame's header"));
    };
    if channels == 0 || channels > MAX_CHANNELS {
        return Err(error(8, ErrorKind::BadChannels { found: channels }));
    }
    let sample_rate = u32::from_be_bytes([0, r0, r1, r2]);
    if sample_rate == 0 {
        return Err(error(9, ErrorKind::BadSampleRate));
    }
    let len = file_len(usize::from(channels), samples);
    // A `usize` fits in a `u64` on every target.
    let found = bytes.len() as u64;
    if found < len {
        return Err(cut_short(bytes, "the rest of the frames"));
    }
    if found > len {
        // Below `bytes.len()`, so it fits.
        return Err(error(len as usize, ErrorKind::TrailingBytes));
    }
    Ok(Header {
        channels,
        sample_rate,
        samples,
    })
}

/// Decodes the frame that starts `at` bytes into the file `bytes`, whose
/// header [`header`] read, into `out`: its samples, interleaved, in the
/// first `frame.samples × channels` of `out`. The file's first frame is at
/// 8, and each [`Frame::next`] gives the one after.
///
/// `out` must hold the frame's samples: [`FRAME_LEN`] `× channels` holds
/// any frame's. Nothing is allocated.
///
/// # Errors
///
/// [`CodecError::Qoa`] for a frame that is malformed, or not the file's at
/// `at`, and for an `out` too short to hold it. `out` may then hold some of
/// the frame's samples.
pub fn decode_frame(
    bytes: &[u8],
    header: &Header,
    at: usize,
    out: &mut [i16],
) -> Result<Frame, CodecError> {
    let expected_channels = header.channels;
    if expected_channels == 0 || expected_channels > MAX_CHANNELS {
        return Err(error(
            at,
            ErrorKind::BadChannels {
                found: expected_channels,
            },
        ));
    }
    let channels = usize::from(expected_channels);
    // Every frame but the last is full, so a frame's place gives its
    // samples.
    let full = frame_size(channels, SLICES_PER_FRAME);
    let index = at
        .checked_sub(FILE_HEADER_LEN)
        .filter(|offset| offset % full == 0)
        .map(|offset| offset / full)
        .ok_or_else(|| error(at, ErrorKind::NotAFrame))?;
    let first = index
        .checked_mul(FRAME_LEN)
        .filter(|&first| u64::try_from(first).is_ok_and(|first| first < u64::from(header.samples)))
        .ok_or_else(|| error(at, ErrorKind::NotAFrame))?;
    // `first` is below `header.samples`, a `u32`.
    let expected_samples = FRAME_LEN.min(header.samples as usize - first);
    let slices = expected_samples.div_ceil(SLICE_LEN);
    let expected_size = frame_size(channels, slices);

    let Some(&[found_channels, r0, r1, r2, n0, n1, z0, z1]) = bytes
        .get(at..)
        .and_then(<[u8]>::first_chunk::<FRAME_HEADER_LEN>)
    else {
        return Err(cut_short(bytes, "a frame's header"));
    };
    if found_channels != expected_channels {
        return Err(error(
            at,
            ErrorKind::ChannelsDiffer {
                found: found_channels,
                expected: expected_channels,
            },
        ));
    }
    let sample_rate = u32::from_be_bytes([0, r0, r1, r2]);
    if sample_rate != header.sample_rate {
        return Err(error(
            at + 1,
            ErrorKind::SampleRateDiffers {
                found: sample_rate,
                expected: header.sample_rate,
            },
        ));
    }
    let found_samples = u16::from_be_bytes([n0, n1]);
    if usize::from(found_samples) != expected_samples {
        return Err(error(
            at + 4,
            ErrorKind::BadFrameSamples {
                found: found_samples,
                expected: expected_samples,
            },
        ));
    }
    let found_size = u16::from_be_bytes([z0, z1]);
    if usize::from(found_size) != expected_size {
        return Err(error(
            at + 6,
            ErrorKind::BadFrameSize {
                found: found_size,
                expected: expected_size,
            },
        ));
    }
    let Some(frame) = bytes.get(at..).and_then(|rest| rest.get(..expected_size)) else {
        return Err(cut_short(bytes, "the rest of a frame"));
    };
    let needed = expected_samples * channels;
    let Some(out) = out.get_mut(..needed) else {
        return Err(error(at, ErrorKind::OutputTooShort { needed }));
    };

    let (states, slice_bytes) = frame[FRAME_HEADER_LEN..].split_at(channels * LMS_BYTES);
    let mut lms = [Lms::START; MAX_CHANNELS as usize];
    for (lms, state) in lms.iter_mut().zip(states.as_chunks::<LMS_BYTES>().0) {
        *lms = Lms::read(state);
    }
    let mut slices = slice_bytes.as_chunks::<SLICE_BYTES>().0.iter();
    let mut offset = at + FRAME_HEADER_LEN + channels * LMS_BYTES;
    for start in (0..expected_samples).step_by(SLICE_LEN) {
        let len = SLICE_LEN.min(expected_samples - start);
        for (channel, lms) in lms[..channels].iter_mut().enumerate() {
            // The frame's size gives it `slices × channels` slices.
            let Some(&slice) = slices.next() else {
                return Err(cut_short(bytes, "a slice"));
            };
            let slice = u64::from_be_bytes(slice);
            let unused = (SLICE_LEN - len) * 3;
            if unused > 0 && slice & ((1 << unused) - 1) != 0 {
                return Err(error(offset, ErrorKind::UnusedNotZero));
            }
            let factor = (slice >> 60) as usize;
            for i in 0..len {
                let quantized = (slice >> (57 - 3 * i)) & 7;
                let predicted = lms.predict();
                let dequantized = DEQUANTIZE[factor][quantized as usize];
                let sample = (predicted + dequantized).clamp(-32768, 32767);
                // `sample` is within an `i16`'s range.
                out[(start + i) * channels + channel] = sample as i16;
                lms.update(sample, dequantized);
            }
            offset += SLICE_BYTES;
        }
    }
    Ok(Frame {
        samples: expected_samples,
        next: at + expected_size,
    })
}

/// Decodes a QOA file.
///
/// # Errors
///
/// [`CodecError::Qoa`], with the byte offset of the problem, for a file
/// that is malformed or breaks a limit of `limits`: its samples, at 2 bytes
/// each, may take at most [`Limits::max_decoded_bytes`].
pub fn decode(bytes: &[u8], limits: Limits) -> Result<Sound, CodecError> {
    let header = header(bytes)?;
    let channels = usize::from(header.channels);
    // The header checked that the file holds its frames, so the file's
    // length bounds this, at about 5 bytes of samples to a byte of file.
    let total = usize::try_from(header.samples)
        .ok()
        .and_then(|samples| samples.checked_mul(channels))
        .filter(|total| {
            total
                .checked_mul(2)
                .is_some_and(|len| len <= limits.max_decoded_bytes)
        })
        .ok_or_else(|| {
            error(
                4,
                ErrorKind::TooManyBytes {
                    limit: limits.max_decoded_bytes,
                },
            )
        })?;
    let mut samples = vec![0; total];
    let mut at = FILE_HEADER_LEN;
    for out in samples.chunks_mut(FRAME_LEN * channels) {
        at = decode_frame(bytes, &header, at, out)?.next;
    }
    Ok(Sound { header, samples })
}

/// Encodes `samples`, interleaved (left, right, left, … in stereo), as the
/// reference encoder would, but where its weights outgrow the file (see
/// the [module's documentation](self)).
///
/// # Errors
///
/// [`CodecError::QoaEncode`] for channels or a sample rate outside what a
/// file holds, for 0 samples, and for samples that are not
/// `header.samples × header.channels`.
pub fn encode(header: &Header, samples: &[i16]) -> Result<Vec<u8>, CodecError> {
    let Header {
        channels,
        sample_rate,
        samples: per_channel,
    } = *header;
    let refuse = |kind| Err(CodecError::QoaEncode { kind });
    if channels == 0 || channels > MAX_CHANNELS {
        return refuse(EncodeErrorKind::BadChannels { found: channels });
    }
    if sample_rate == 0 || sample_rate > MAX_SAMPLE_RATE {
        return refuse(EncodeErrorKind::BadSampleRate { found: sample_rate });
    }
    if per_channel == 0 {
        return refuse(EncodeErrorKind::NoSamples);
    }
    if u64::try_from(samples.len()).ok() != Some(u64::from(per_channel) * u64::from(channels)) {
        return refuse(EncodeErrorKind::WrongDataSize {
            samples: per_channel,
            channels,
            found: samples.len(),
        });
    }
    let channels = usize::from(channels);
    let len = usize::try_from(file_len(channels, per_channel)).unwrap_or(0);
    let mut out = Vec::with_capacity(len);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&per_channel.to_be_bytes());

    let mut lms = [Lms::START; MAX_CHANNELS as usize];
    for frame in samples.chunks(FRAME_LEN * channels) {
        encode_frame(frame, header, &mut lms[..channels], &mut out);
    }
    Ok(out)
}

/// Encodes one frame of `samples`, interleaved, of 1 to [`FRAME_LEN`]
/// samples of each of `lms.len()` channels, as `header` describes them,
/// from the predictors' state in `lms`, which it moves on.
fn encode_frame(samples: &[i16], header: &Header, lms: &mut [Lms], out: &mut Vec<u8>) {
    let channels = lms.len();
    let frame_len = samples.len() / channels;
    out.push(header.channels);
    out.extend_from_slice(&header.sample_rate.to_be_bytes()[1..]);
    // At most `FRAME_LEN`, and a frame of 8 channels takes 16,520 bytes:
    // both fit in 16 bits.
    out.extend_from_slice(&(frame_len as u16).to_be_bytes());
    let size = frame_size(channels, frame_len.div_ceil(SLICE_LEN));
    out.extend_from_slice(&(size as u16).to_be_bytes());
    for lms in lms.iter_mut() {
        // The file holds 16 bits of each weight: go on from those, as the
        // decoder will.
        lms.weights = lms.weights.map(|weight| i32::from(weight as i16));
        lms.write(out);
    }
    // The reference starts each frame's search from factor 0.
    let mut factors = [0; MAX_CHANNELS as usize];
    for start in (0..frame_len).step_by(SLICE_LEN) {
        let len = SLICE_LEN.min(frame_len - start);
        for (channel, (lms, factor)) in lms.iter_mut().zip(&mut factors).enumerate() {
            let sample = |i: usize| i32::from(samples[(start + i) * channels + channel]);
            let slice = encode_slice(sample, len, lms, factor);
            out.extend_from_slice(&(slice << ((SLICE_LEN - len) * 3)).to_be_bytes());
        }
    }
}

/// Encodes `len` samples of one channel into a slice, as the reference
/// does: it tries each scale factor, from the last slice's on, and keeps
/// the one whose samples come out nearest, a penalty counted for weights
/// grown large, then moves `lms` and `factor` on. The slice's residuals
/// end at its lowest bits; one of fewer than 20 is shifted up after.
fn encode_slice(
    sample: impl Fn(usize) -> i32,
    len: usize,
    lms: &mut Lms,
    factor: &mut usize,
) -> u64 {
    let mut best_rank = u64::MAX;
    let mut best = (0, *lms, 0);
    for step in 0..16 {
        let candidate = (*factor + step) % 16;
        let mut state = *lms;
        let mut slice = candidate as u64;
        let mut rank = 0u64;
        for i in 0..len {
            let sample = sample(i);
            let predicted = state.predict();
            let scaled = divide(sample - predicted, candidate);
            // The clamp keeps the index within the table.
            let quantized = QUANTIZE[(scaled.clamp(-8, 8) + 8) as usize];
            let dequantized = DEQUANTIZE[candidate][usize::from(quantized)];
            let reconstructed = (predicted + dequantized).clamp(-32768, 32767);
            let error = i64::from(sample - reconstructed);
            rank += (error * error).unsigned_abs() + weights_penalty(&state.weights);
            if rank > best_rank {
                break;
            }
            state.update(reconstructed, dequantized);
            slice = (slice << 3) | u64::from(quantized);
        }
        if rank < best_rank {
            best_rank = rank;
            best = (slice, state, candidate);
        }
    }
    (*lms, *factor) = (best.1, best.2);
    best.0
}

/// The input ends where `expected` should be.
fn cut_short(bytes: &[u8], expected: &'static str) -> CodecError {
    error(bytes.len(), ErrorKind::UnexpectedEnd { expected })
}

#[cfg(test)]
mod tests {
    use nadir_core::testing::check;

    use super::*;

    /// A sound of `samples` samples of each channel, a tone in each.
    fn tone(channels: u8, samples: u32) -> (Header, Vec<i16>) {
        let header = Header {
            channels,
            sample_rate: 44_100,
            samples,
        };
        let values = (0..samples)
            .flat_map(|i| {
                (0..channels).map(move |channel| {
                    let x = f64::from(i) * f64::from(channel + 3) * 0.031;
                    (x.sin() * 9_000.0) as i16
                })
            })
            .collect();
        (header, values)
    }

    fn file(channels: u8, samples: u32) -> Vec<u8> {
        let (header, values) = tone(channels, samples);
        encode(&header, &values).unwrap()
    }

    fn decoded(bytes: &[u8]) -> Result<Sound, CodecError> {
        decode(bytes, Limits::default())
    }

    fn fails(bytes: &[u8], offset: usize, kind: ErrorKind) {
        assert_eq!(decoded(bytes), Err(CodecError::Qoa { offset, kind }));
    }

    fn cut(offset: usize, expected: &'static str) -> CodecError {
        CodecError::Qoa {
            offset,
            kind: ErrorKind::UnexpectedEnd { expected },
        }
    }

    #[test]
    fn the_tables_follow_the_formulas_of_the_specification() {
        for s in 0..16 {
            let factor = f64::from(s as u8 + 1).powf(2.75).round() as i32;
            assert_eq!(SCALEFACTORS[s], factor, "factor {s}");
            assert_eq!(
                RECIPROCALS[s],
                ((1 << 16) + factor - 1) / factor,
                "reciprocal {s}"
            );
            for (q, multiplier) in [0.75, -0.75, 2.5, -2.5, 4.5, -4.5, 7.0, -7.0]
                .into_iter()
                .enumerate()
            {
                // `f64::round` rounds ties away from 0.
                let expected = (f64::from(factor) * multiplier).round() as i32;
                assert_eq!(DEQUANTIZE[s][q], expected, "{s}, {q}");
            }
        }
        assert_eq!(DEQUANTIZE[0], [1, -1, 3, -3, 5, -5, 7, -7]);
        assert_eq!(
            DEQUANTIZE[15],
            [1536, -1536, 5120, -5120, 9216, -9216, 14336, -14336]
        );
        assert_eq!(
            (RECIPROCALS[0], RECIPROCALS[1], RECIPROCALS[15]),
            (65536, 9363, 32)
        );
        // Each scaled residual takes the quantized one of its sign whose
        // multiplier is nearest: 0.75 for 0 and 1, 2.5 for 2 and 3, 4.5 for
        // 4 and 5, 7 from 6 on.
        for residual in -8i32..=8 {
            let quantized = QUANTIZE[(residual + 8) as usize];
            let magnitude = [1, 1, 3, 3, 5, 5, 7, 7, 7][residual.unsigned_abs() as usize];
            let value = DEQUANTIZE[0][usize::from(quantized)];
            assert_eq!(value.abs(), magnitude, "{residual}");
            assert_eq!(value < 0, residual < 0, "{residual}");
        }
    }

    #[test]
    fn division_rounds_to_the_nearest_ties_up_and_never_to_0_but_for_0() {
        // Factor 1 is 7.
        assert_eq!(divide(0, 1), 0);
        assert_eq!([divide(3, 1), divide(-3, 1)], [1, -1]);
        assert_eq!([divide(10, 1), divide(-10, 1)], [1, -1]);
        assert_eq!([divide(11, 1), divide(-11, 1)], [2, -2]);
        // Factor 15 is 2048: ties round up, -1.5 to -1, as the reference's.
        assert_eq!([divide(3072, 15), divide(-3072, 15)], [2, -1]);
        assert_eq!(divide(1, 15), 1);
        // Factor 0 is 1: residuals as they are, but the largest wraps in 32
        // bits, as the reference's arithmetic does: 65,535 × 65,536 is
        // -65,536, which rounds to -1, and the step away from 0 gives 1.
        assert_eq!(divide(-8, 0), -8);
        assert_eq!(divide(32_767 + 32_768, 0), 1);
    }

    #[test]
    fn the_predictor_moves_as_the_reference_does() {
        let mut lms = Lms::START;
        assert_eq!(lms.predict(), 0);
        lms.history = [0, 0, 100, 200];
        // 2 × 200 - 100: the line through the last two.
        assert_eq!(lms.predict(), 300);
        lms.update(310, 32);
        assert_eq!(lms.history, [0, 100, 200, 310]);
        assert_eq!(lms.weights, [2, 2, -(1 << 13) + 2, (1 << 14) + 2]);
        lms.history = [-1, 0, 1, -5];
        // A residual of -1 moves the weights by 1, as -1 >> 4 is -1.
        lms.update(7, -1);
        assert_eq!(lms.weights, [3, 1, -(1 << 13) + 1, (1 << 14) + 3]);
        assert_eq!(lms.history, [0, 1, -5, 7]);
        // Weights past what a frame holds wrap in 32 bits, as C's `int`.
        lms.history = [32_767; 4];
        lms.weights = [i32::MAX; 4];
        assert_eq!(
            lms.predict(),
            (i32::MAX.wrapping_mul(32_767).wrapping_mul(4)) >> 13
        );
    }

    #[test]
    fn the_penalty_starts_where_the_weights_grow_large() {
        assert_eq!(weights_penalty(&Lms::START.weights), 0);
        // 0x8ff << 18 is where it starts: one weight of 24,570.
        assert_eq!(weights_penalty(&[24_570, 0, 0, 0]), 0);
        assert_eq!(weights_penalty(&[24_600, 0, 0, 0]), 5 * 5);
        assert_eq!(weights_penalty(&[i32::MAX, 0, 0, 0]), 0);
    }

    #[test]
    fn a_frame_starts_from_the_weights_the_file_can_hold() {
        // An encoder whose weights have grown past 16 bits writes their low
        // 16 bits, and goes on from those, so the frame is the one it would
        // write from them.
        let (header, values) = tone(1, 300);
        let mut large = [Lms {
            history: [10, -20, 30, -40],
            weights: [70_000, -70_000, 0x1_2345, -(1 << 13)],
        }];
        let mut small = [Lms {
            history: large[0].history,
            weights: [70_000 - 65_536, -70_000 + 65_536, 0x2345, -(1 << 13)],
        }];
        let (mut a, mut b) = (Vec::new(), Vec::new());
        encode_frame(&values, &header, &mut large, &mut a);
        encode_frame(&values, &header, &mut small, &mut b);
        assert_eq!(a, b);
        assert_eq!(large, small);
        assert_eq!(
            a[8..24],
            [
                0, 10, 255, 236, 0, 30, 255, 216, 17, 112, 238, 144, 35, 69, 224, 0
            ]
        );
    }

    #[test]
    fn the_header_is_read_and_checked() {
        let bytes = file(2, 5121);
        assert_eq!(
            header(&bytes),
            Ok(Header {
                channels: 2,
                sample_rate: 44_100,
                samples: 5121
            })
        );
        // Two frames: one full, and one of a slice a channel.
        assert_eq!(
            bytes.len(),
            8 + (8 + 2 * (16 + 256 * 8)) + (8 + 2 * (16 + 8))
        );

        assert_eq!(header(b""), Err(cut(0, "the rest of the file's header")));
        assert_eq!(header(b"qoa"), Err(cut(3, "the rest of the file's header")));
        assert_eq!(
            header(b"qoaX\0\0\0\x01"),
            Err(CodecError::Qoa {
                offset: 0,
                kind: ErrorKind::NotQoa
            })
        );
        assert_eq!(header(b"RIFF"), header(b"qoaX"));
        assert_eq!(
            header(&bytes[..12]),
            Err(cut(12, "the first frame's header"))
        );

        let mut streaming = bytes.clone();
        streaming[4..8].fill(0);
        fails(&streaming, 4, ErrorKind::Streaming);
        for channels in [0, 9, 255] {
            let mut bad = bytes.clone();
            bad[8] = channels;
            fails(&bad, 8, ErrorKind::BadChannels { found: channels });
        }
        // Whatever the sample count, the shortest file's too.
        for mut no_rate in [bytes.clone(), file(1, 1)] {
            no_rate[9..12].fill(0);
            fails(&no_rate, 9, ErrorKind::BadSampleRate);
        }
        // The sample count gives the file's length: one more sample needs
        // another slice a channel.
        let mut longer = bytes.clone();
        longer[7] += 20;
        assert_eq!(
            decoded(&longer),
            Err(cut(bytes.len(), "the rest of the frames"))
        );
        let mut shorter = bytes.clone();
        shorter[7] -= 1;
        fails(&shorter, 8 + frame_size(2, 256), ErrorKind::TrailingBytes);
    }

    #[test]
    fn each_frame_is_held_to_its_place_in_the_file() {
        let bytes = file(2, FRAME_LEN as u32 * 2 + 25);
        let second = 8 + frame_size(2, 256);
        let third = second + frame_size(2, 256);
        let frame_bytes = |at: usize, value: u8| {
            let mut bad = bytes.clone();
            bad[at] = value;
            bad
        };
        for found in [1, 3] {
            fails(
                &frame_bytes(second, found),
                second,
                ErrorKind::ChannelsDiffer { found, expected: 2 },
            );
        }
        // 44,100 is 0x00ac44.
        for (low, found) in [(0x45, 44_101), (0x43, 44_099)] {
            fails(
                &frame_bytes(third + 3, low),
                third + 1,
                ErrorKind::SampleRateDiffers {
                    found,
                    expected: 44_100,
                },
            );
        }
        // Every frame but the last is full.
        fails(
            &frame_bytes(second + 5, 0xff),
            second + 4,
            ErrorKind::BadFrameSamples {
                found: 0x14ff,
                expected: FRAME_LEN,
            },
        );
        fails(
            &frame_bytes(third + 5, 26),
            third + 4,
            ErrorKind::BadFrameSamples {
                found: 26,
                expected: 25,
            },
        );
        let size = frame_size(2, 2);
        for found in [size + 8, size - 8] {
            fails(
                &frame_bytes(third + 7, found as u8),
                third + 6,
                ErrorKind::BadFrameSize {
                    found: found as u16,
                    expected: size,
                },
            );
        }
    }

    #[test]
    fn a_last_slice_must_leave_its_unused_residuals_0() {
        // 25 samples: the second slice holds 5, and 15 unused residuals.
        let bytes = file(1, 25);
        let last = bytes.len() - 8;
        assert!(decoded(&bytes).is_ok());
        for bit in [0, 44] {
            let mut bad = bytes.clone();
            bad[bytes.len() - 1 - bit / 8] ^= 1 << (bit % 8);
            fails(&bad, last, ErrorKind::UnusedNotZero);
        }
        // Bit 45 is the fifth sample's.
        let mut used = bytes.clone();
        used[bytes.len() - 6] ^= 1 << 5;
        assert!(decoded(&used).is_ok());
        // In stereo, each channel's last slice, at its own offset.
        let bytes = file(2, 25);
        for (channel, at) in [(1, bytes.len() - 8), (0, bytes.len() - 16)] {
            let mut bad = bytes.clone();
            bad[at + 7] ^= 1;
            assert_eq!(
                decoded(&bad),
                Err(CodecError::Qoa {
                    offset: at,
                    kind: ErrorKind::UnusedNotZero
                }),
                "channel {channel}"
            );
        }
    }

    #[test]
    fn decode_frame_reads_only_the_frames_of_the_file() {
        let bytes = file(1, FRAME_LEN as u32 + 10);
        let header = header(&bytes).unwrap();
        let mut out = vec![0; FRAME_LEN];
        let first = decode_frame(&bytes, &header, 8, &mut out).unwrap();
        assert_eq!(
            first,
            Frame {
                samples: FRAME_LEN,
                next: 8 + frame_size(1, 256)
            }
        );
        let last = decode_frame(&bytes, &header, first.next, &mut out).unwrap();
        assert_eq!(
            last,
            Frame {
                samples: 10,
                next: bytes.len()
            }
        );
        let full = frame_size(1, 256);
        for at in [
            0,
            7,
            9,
            first.next - 1,
            first.next + 8,
            bytes.len(),
            // Where a third frame would start, and one whose first sample
            // is past what a `usize` counts.
            8 + 2 * full,
            8 + full * (usize::MAX / FRAME_LEN + 1),
            usize::MAX,
        ] {
            assert_eq!(
                decode_frame(&bytes, &header, at, &mut out),
                Err(CodecError::Qoa {
                    offset: at,
                    kind: ErrorKind::NotAFrame
                }),
                "{at}"
            );
        }
        assert_eq!(
            decode_frame(&bytes, &header, 8, &mut out[..FRAME_LEN - 1]),
            Err(CodecError::Qoa {
                offset: 8,
                kind: ErrorKind::OutputTooShort { needed: FRAME_LEN }
            })
        );
        assert!(decode_frame(&bytes, &header, first.next, &mut out[..10]).is_ok());
        // A file of whole frames has no frame after its last.
        let whole = file(1, FRAME_LEN as u32);
        let whole_header = Header {
            samples: FRAME_LEN as u32,
            ..header
        };
        assert_eq!(
            decode_frame(&whole, &whole_header, 8 + full, &mut out),
            Err(CodecError::Qoa {
                offset: 8 + full,
                kind: ErrorKind::NotAFrame
            })
        );
        // A header that is not the file's: its first frame is checked
        // against it.
        assert_eq!(
            decode_frame(
                &bytes,
                &Header {
                    channels: 2,
                    ..header
                },
                8,
                &mut out
            ),
            Err(CodecError::Qoa {
                offset: 8,
                kind: ErrorKind::ChannelsDiffer {
                    found: 1,
                    expected: 2
                }
            })
        );
        assert_eq!(
            decode_frame(
                &bytes,
                &Header {
                    sample_rate: 48_000,
                    ..header
                },
                8,
                &mut out
            ),
            Err(CodecError::Qoa {
                offset: 9,
                kind: ErrorKind::SampleRateDiffers {
                    found: 44_100,
                    expected: 48_000
                }
            })
        );
        // A header that is not the file's.
        for channels in [0, 9] {
            let wrong = Header { channels, ..header };
            assert_eq!(
                decode_frame(&bytes, &wrong, 8, &mut out),
                Err(CodecError::Qoa {
                    offset: 8,
                    kind: ErrorKind::BadChannels { found: channels }
                })
            );
        }
        let longer = Header {
            samples: header.samples + FRAME_LEN as u32,
            ..header
        };
        assert_eq!(
            decode_frame(&bytes, &longer, first.next, &mut out),
            Err(CodecError::Qoa {
                offset: first.next + 4,
                kind: ErrorKind::BadFrameSamples {
                    found: 10,
                    expected: FRAME_LEN
                }
            })
        );
        let further = Header {
            samples: header.samples + 2 * FRAME_LEN as u32,
            ..header
        };
        let past = first.next + frame_size(1, 256);
        assert_eq!(
            decode_frame(&bytes, &further, past, &mut out),
            Err(cut(bytes.len(), "a frame's header"))
        );
        assert_eq!(
            decode_frame(&bytes[..first.next + 20], &header, first.next, &mut out),
            Err(cut(first.next + 20, "the rest of a frame"))
        );
    }

    #[test]
    fn the_encoder_refuses_what_a_file_cannot_hold() {
        let refused = |header: Header, samples: &[i16]| match encode(&header, samples) {
            Err(CodecError::QoaEncode { kind }) => kind,
            other => panic!("{other:?}"),
        };
        let header = Header {
            channels: 2,
            sample_rate: 48_000,
            samples: 3,
        };
        for channels in [0, 9] {
            assert_eq!(
                refused(Header { channels, ..header }, &[]),
                EncodeErrorKind::BadChannels { found: channels }
            );
        }
        for sample_rate in [0, MAX_SAMPLE_RATE + 1] {
            assert_eq!(
                refused(
                    Header {
                        sample_rate,
                        ..header
                    },
                    &[0; 6]
                ),
                EncodeErrorKind::BadSampleRate { found: sample_rate }
            );
        }
        assert_eq!(
            refused(
                Header {
                    samples: 0,
                    ..header
                },
                &[]
            ),
            EncodeErrorKind::NoSamples
        );
        assert_eq!(
            refused(header, &[0; 5]),
            EncodeErrorKind::WrongDataSize {
                samples: 3,
                channels: 2,
                found: 5
            }
        );
        let edges = Header {
            channels: MAX_CHANNELS,
            sample_rate: MAX_SAMPLE_RATE,
            samples: 1,
        };
        let sound = decoded(&encode(&edges, &[i16::MIN; 8]).unwrap()).unwrap();
        assert_eq!(sound.header, edges);
    }

    #[test]
    fn errors_say_what_is_wrong() {
        let error = CodecError::Qoa {
            offset: 4,
            kind: ErrorKind::Streaming,
        };
        assert_eq!(
            error.to_string(),
            "QOA error at byte 4: the header gives 0 samples, as a stream does, and only files of \
             known length are read"
        );
        let error = CodecError::Qoa {
            offset: 6168,
            kind: ErrorKind::BadFrameSamples {
                found: 26,
                expected: 25,
            },
        };
        assert_eq!(
            error.to_string(),
            "QOA error at byte 6168: the frame holds 26 samples a channel, not 25"
        );
        let error = CodecError::QoaEncode {
            kind: EncodeErrorKind::BadChannels { found: 9 },
        };
        assert_eq!(
            error.to_string(),
            "cannot encode as QOA: 9 channels, not 1 to 8"
        );
    }

    #[test]
    fn any_sound_round_trips_to_its_header_and_length() {
        check(
            "qoa round trip",
            300,
            |rng| {
                let channels = 1 + rng.range(0..MAX_CHANNELS as u32) as u8;
                let samples = 1 + rng.range(0..700u32);
                let header = Header {
                    channels,
                    sample_rate: 1 + rng.range(0..MAX_SAMPLE_RATE),
                    samples,
                };
                // Noise of a random loudness, from 1 bit to full scale.
                let bits = 1 + rng.range(0..16u32);
                let values: Vec<i16> = (0..samples * u32::from(channels))
                    .map(|_| (rng.next_u32() as i16) >> (16 - bits))
                    .collect();
                (header, values)
            },
            |(header, values)| {
                let file = encode(header, values).map_err(|error| error.to_string())?;
                let sound = decoded(&file).map_err(|error| error.to_string())?;
                if sound.header != *header || sound.samples.len() != values.len() {
                    return Err(format!(
                        "{:?}, {} samples",
                        sound.header,
                        sound.samples.len()
                    ));
                }
                Ok(())
            },
        );
    }

    #[test]
    fn random_bytes_never_make_it_panic() {
        check(
            "qoa random bytes",
            4000,
            |rng| {
                // A header that often holds, then bytes at random.
                let mut bytes = b"qoaf".to_vec();
                let samples = rng.range(0..12_000u32);
                bytes.extend_from_slice(&samples.to_be_bytes());
                bytes.push(rng.range(0..10u32) as u8);
                let len = rng.range(0..9_000u32);
                bytes.extend((0..len).map(|_| rng.next_u32() as u8));
                bytes
            },
            |bytes| {
                let _ = decoded(bytes);
                if let Ok(header) = header(bytes) {
                    let mut out = vec![0; FRAME_LEN * usize::from(header.channels)];
                    let _ = decode_frame(bytes, &header, 8, &mut out);
                }
                Ok(())
            },
        );
    }
}
