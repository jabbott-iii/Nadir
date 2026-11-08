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

//! The sound file, `.kpcm`, version 1: 16-bit samples as they are, which
//! the cook writes for short sounds (QOA is for music and long ones).
//!
//! [`encode`] turns samples into a file, and [`decode`] reads one back.
//! The format is lossless, and canonical: each sound has one file, so a
//! file that decodes encodes back to the same bytes.
//!
//! ```
//! use nadir_codec::Limits;
//! use nadir_codec::kpcm::{self, Header};
//!
//! // Two frames of stereo: left, then right.
//! let samples = [0x0102, -32768, 32767, -1];
//! let header = Header { channels: 2, sample_rate: 48_000, frames: 2 };
//! let file = kpcm::encode(&header, &samples)?;
//! assert_eq!(file.len(), kpcm::HEADER_SIZE + 8);
//! assert_eq!(&file[..4], b"KPCM");
//!
//! let sound = kpcm::decode(&file, Limits::default())?;
//! assert_eq!((sound.header, sound.samples.as_slice()), (header, &samples[..]));
//! # Ok::<(), nadir_codec::CodecError>(())
//! ```
//!
//! # The format
//!
//! ```text
//! header (16 bytes):
//!   "KPCM" | version u16 (1) | channels u8 (1 or 2) | reserved u8 (0) |
//!   sample_rate u32 (1 to 384000) | frames u32 (1 or more)
//! samples: frames × channels i16, interleaved (left, then right)
//! ```
//!
//! Numbers are little-endian. The file is exactly `16 + frames × channels
//! × 2` bytes long: nothing follows the samples. A sound plays whole, and
//! loops whole when the game asks: version 1 holds no loop points.
//!
//! # What is checked
//!
//! [`decode`] checks, in this order, and the first that fails is the
//! error, with the offset of the field, of the input's end where it is cut
//! short, or of the first byte after the samples where there are more:
//!
//! 1. the input starts with `KPCM`, or with as much of it as there is;
//! 2. the header's 16 bytes are there;
//! 3. the version is 1: any other is not supported;
//! 4. the channels are 1 or 2, the mixer's;
//! 5. the reserved byte is 0;
//! 6. the sample rate is 1 to [`MAX_SAMPLE_RATE`], the mixer's;
//! 7. there is a frame or more;
//! 8. the samples, at 2 bytes each, fit [`Limits::max_decoded_bytes`];
//! 9. the file holds the samples, and nothing after them.
//!
//! Only then is anything allocated.

use std::fmt;

use crate::{CodecError, Limits};

/// The first four bytes of a sound file.
pub const MAGIC: [u8; 4] = *b"KPCM";

/// The version this module reads and writes.
pub const VERSION: u16 = 1;

/// The bytes of the header, before the samples.
pub const HEADER_SIZE: usize = 16;

/// The highest sample rate a file may have, in frames a second: the
/// mixer's, `nadir_audio::MAX_SAMPLE_RATE`.
pub const MAX_SAMPLE_RATE: u32 = 384_000;

/// What a sound file holds besides its samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Header {
    /// The samples of a frame: 1 or 2, left then right.
    pub channels: u8,
    /// The frames a second: 1 to [`MAX_SAMPLE_RATE`].
    pub sample_rate: u32,
    /// The frames: 1 or more.
    pub frames: u32,
}

/// A sound file's contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sound {
    /// Its header.
    pub header: Header,
    /// Its samples, `frames × channels` of them, interleaved.
    pub samples: Vec<i16>,
}

/// What is wrong with a sound file. See [`CodecError::Kpcm`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The input does not start with `KPCM`.
    NotKpcm,
    /// The input ends too soon.
    UnexpectedEnd {
        /// What should have come next.
        expected: &'static str,
    },
    /// The version is not [`VERSION`].
    Version {
        /// The version in the header.
        found: u16,
    },
    /// The channels are not 1 or 2.
    BadChannels {
        /// The value in the header.
        found: u8,
    },
    /// The reserved byte is not 0.
    Reserved,
    /// The sample rate is 0 or more than [`MAX_SAMPLE_RATE`].
    BadSampleRate {
        /// The value in the header.
        found: u32,
    },
    /// The header gives no frames.
    NoFrames,
    /// The samples would take more than [`Limits::max_decoded_bytes`], at 2
    /// bytes each.
    TooManyBytes {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// Bytes follow the samples.
    TrailingBytes,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotKpcm => f.write_str("the file does not start with `KPCM`"),
            Self::UnexpectedEnd { expected } => {
                write!(f, "the file ends where {expected} should be")
            }
            Self::Version { found } => write!(
                f,
                "the version is {found}, not {VERSION}: it is not supported"
            ),
            Self::BadChannels { found } => {
                write!(f, "the file has {found} channels, not 1 or 2")
            }
            Self::Reserved => f.write_str("a reserved field is not 0"),
            Self::BadSampleRate { found } => write!(
                f,
                "the sample rate is {found} Hz, not 1 to {MAX_SAMPLE_RATE}"
            ),
            Self::NoFrames => f.write_str("the header gives no frames"),
            Self::TooManyBytes { limit } => write!(
                f,
                "the samples would take more than the limit of {limit} bytes"
            ),
            Self::TrailingBytes => f.write_str("there are bytes after the samples"),
        }
    }
}

/// Why samples cannot be encoded. See [`CodecError::KpcmEncode`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EncodeErrorKind {
    /// The channels are not 1 or 2.
    BadChannels {
        /// The channels the header gives.
        found: u8,
    },
    /// The sample rate is 0 or more than [`MAX_SAMPLE_RATE`].
    BadSampleRate {
        /// The sample rate the header gives.
        found: u32,
    },
    /// The header gives no frames, which a file cannot hold.
    NoFrames,
    /// The samples are not `frames × channels` of them.
    WrongDataSize {
        /// The frames the header gives.
        frames: u32,
        /// The channels the header gives.
        channels: u8,
        /// The number of samples given.
        found: usize,
    },
}

impl fmt::Display for EncodeErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadChannels { found } => write!(f, "{found} channels, not 1 or 2"),
            Self::BadSampleRate { found } => {
                write!(f, "a sample rate of {found} Hz, not 1 to {MAX_SAMPLE_RATE}")
            }
            Self::NoFrames => f.write_str("there are no frames"),
            Self::WrongDataSize {
                frames,
                channels,
                found,
            } => write!(f, "{found} samples, not {frames} × {channels}"),
        }
    }
}

fn error(offset: usize, kind: ErrorKind) -> CodecError {
    CodecError::Kpcm { offset, kind }
}

fn encode_error(kind: EncodeErrorKind) -> CodecError {
    CodecError::KpcmEncode { kind }
}

/// Reads a sound file.
///
/// # Errors
///
/// A [`CodecError::Kpcm`] for the first of the module's checks that fails:
/// the offset is the field's in the header, the input's end where it is cut
/// short, or the first byte after the samples where there are more.
pub fn decode(bytes: &[u8], limits: Limits) -> Result<Sound, CodecError> {
    let start = &bytes[..bytes.len().min(MAGIC.len())];
    if !MAGIC.starts_with(start) {
        return Err(error(0, ErrorKind::NotKpcm));
    }
    let Some(head) = bytes.first_chunk::<HEADER_SIZE>() else {
        return Err(error(
            bytes.len(),
            ErrorKind::UnexpectedEnd {
                expected: "the header",
            },
        ));
    };
    let version = u16::from_le_bytes([head[4], head[5]]);
    if version != VERSION {
        return Err(error(4, ErrorKind::Version { found: version }));
    }
    let channels = head[6];
    if !(1..=2).contains(&channels) {
        return Err(error(6, ErrorKind::BadChannels { found: channels }));
    }
    if head[7] != 0 {
        return Err(error(7, ErrorKind::Reserved));
    }
    let sample_rate = u32::from_le_bytes([head[8], head[9], head[10], head[11]]);
    if !(1..=MAX_SAMPLE_RATE).contains(&sample_rate) {
        return Err(error(8, ErrorKind::BadSampleRate { found: sample_rate }));
    }
    let frames = u32::from_le_bytes([head[12], head[13], head[14], head[15]]);
    if frames == 0 {
        return Err(error(12, ErrorKind::NoFrames));
    }
    // At most 2³² × 2 samples of 2 bytes: within `u64`, which bounds them
    // before `usize`, which may be smaller.
    let sample_bytes = u64::from(frames) * u64::from(channels) * 2;
    let too_many = error(
        12,
        ErrorKind::TooManyBytes {
            limit: limits.max_decoded_bytes,
        },
    );
    let Ok(sample_bytes) = usize::try_from(sample_bytes) else {
        return Err(too_many);
    };
    if sample_bytes > limits.max_decoded_bytes {
        return Err(too_many);
    }
    let data = &bytes[HEADER_SIZE..];
    match data.len().cmp(&sample_bytes) {
        std::cmp::Ordering::Less => {
            return Err(error(
                bytes.len(),
                ErrorKind::UnexpectedEnd {
                    expected: "the samples",
                },
            ));
        }
        std::cmp::Ordering::Greater => {
            return Err(error(HEADER_SIZE + sample_bytes, ErrorKind::TrailingBytes));
        }
        std::cmp::Ordering::Equal => {}
    }
    let samples = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| i16::from_le_bytes(pair))
        .collect();
    Ok(Sound {
        header: Header {
            channels,
            sample_rate,
            frames,
        },
        samples,
    })
}

/// Writes a sound file of `samples`, `header.frames × header.channels` of
/// them, interleaved.
///
/// # Errors
///
/// A [`CodecError::KpcmEncode`] for channels other than 1 or 2, a sample
/// rate of 0 or above [`MAX_SAMPLE_RATE`], no frames, or samples that are
/// not `frames × channels` of them, checked in that order.
pub fn encode(header: &Header, samples: &[i16]) -> Result<Vec<u8>, CodecError> {
    if !(1..=2).contains(&header.channels) {
        return Err(encode_error(EncodeErrorKind::BadChannels {
            found: header.channels,
        }));
    }
    if !(1..=MAX_SAMPLE_RATE).contains(&header.sample_rate) {
        return Err(encode_error(EncodeErrorKind::BadSampleRate {
            found: header.sample_rate,
        }));
    }
    if header.frames == 0 {
        return Err(encode_error(EncodeErrorKind::NoFrames));
    }
    let wrong_size = || {
        encode_error(EncodeErrorKind::WrongDataSize {
            frames: header.frames,
            channels: header.channels,
            found: samples.len(),
        })
    };
    let expected = usize::try_from(header.frames)
        .ok()
        .and_then(|frames| frames.checked_mul(usize::from(header.channels)))
        .ok_or_else(wrong_size)?;
    if samples.len() != expected {
        return Err(wrong_size());
    }
    let mut file = Vec::with_capacity(HEADER_SIZE + samples.len() * 2);
    file.extend_from_slice(&MAGIC);
    file.extend_from_slice(&VERSION.to_le_bytes());
    file.push(header.channels);
    file.push(0);
    file.extend_from_slice(&header.sample_rate.to_le_bytes());
    file.extend_from_slice(&header.frames.to_le_bytes());
    for sample in samples {
        file.extend_from_slice(&sample.to_le_bytes());
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use nadir_core::testing::check;

    use super::*;

    /// A file's header, with the fields given.
    fn head(version: u16, channels: u8, reserved: u8, rate: u32, frames: u32) -> Vec<u8> {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&version.to_le_bytes());
        bytes.extend_from_slice(&[channels, reserved]);
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&frames.to_le_bytes());
        bytes
    }

    fn err(bytes: &[u8]) -> (usize, ErrorKind) {
        match decode(bytes, Limits::default()) {
            Err(CodecError::Kpcm { offset, kind }) => (offset, kind),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_file_is_its_header_and_its_samples_little_endian() {
        let header = Header {
            channels: 1,
            sample_rate: 0x0001_0203,
            frames: 2,
        };
        let file = encode(&header, &[0x0102, -2]).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            file,
            [
                b'K', b'P', b'C', b'M', 1, 0, 1, 0, 0x03, 0x02, 0x01, 0, 2, 0, 0, 0, 0x02, 0x01,
                0xfe, 0xff
            ]
        );
    }

    #[test]
    fn the_checks_run_in_their_order() {
        assert_eq!(err(b"RIFF"), (0, ErrorKind::NotKpcm));
        assert_eq!(err(b"KPQ"), (0, ErrorKind::NotKpcm));
        // A prefix of the magic is a file cut short.
        let end = |at| {
            (
                at,
                ErrorKind::UnexpectedEnd {
                    expected: "the header",
                },
            )
        };
        assert_eq!(err(b""), end(0));
        assert_eq!(err(b"KP"), end(2));
        assert_eq!(err(&head(1, 1, 0, 48_000, 1)[..15]), end(15));
        // Each field, with every later one wrong too.
        assert_eq!(
            err(&head(2, 9, 1, 0, 0)),
            (4, ErrorKind::Version { found: 2 })
        );
        assert_eq!(
            err(&head(1, 3, 1, 0, 0)),
            (6, ErrorKind::BadChannels { found: 3 })
        );
        assert_eq!(
            err(&head(1, 0, 1, 0, 0)).1,
            ErrorKind::BadChannels { found: 0 }
        );
        assert_eq!(err(&head(1, 2, 1, 0, 0)), (7, ErrorKind::Reserved));
        assert_eq!(
            err(&head(1, 2, 0, 0, 0)),
            (8, ErrorKind::BadSampleRate { found: 0 })
        );
        assert_eq!(
            err(&head(1, 2, 0, MAX_SAMPLE_RATE + 1, 0)),
            (8, ErrorKind::BadSampleRate { found: 384_001 })
        );
        assert_eq!(err(&head(1, 2, 0, 1, 0)), (12, ErrorKind::NoFrames));
        // Short and long, where the samples are.
        let mut file = head(1, 2, 0, 1, 1);
        file.extend_from_slice(&[1, 2, 3]);
        assert_eq!(
            err(&file),
            (
                19,
                ErrorKind::UnexpectedEnd {
                    expected: "the samples"
                }
            )
        );
        file.extend_from_slice(&[4, 5]);
        assert_eq!(err(&file), (20, ErrorKind::TrailingBytes));
    }

    #[test]
    fn the_samples_must_fit_the_limit_before_anything_is_allocated() {
        let limits = |bytes| Limits {
            max_decoded_bytes: bytes,
            ..Limits::default()
        };
        // Four samples of two bytes, but a file of none: refused by the
        // limit, not read past its end.
        let file = head(1, 2, 0, 48_000, 2);
        let refused = decode(&file, limits(7));
        assert_eq!(
            refused,
            Err(CodecError::Kpcm {
                offset: 12,
                kind: ErrorKind::TooManyBytes { limit: 7 }
            })
        );
        assert!(matches!(
            decode(&file, limits(8)),
            Err(CodecError::Kpcm {
                kind: ErrorKind::UnexpectedEnd { .. },
                ..
            })
        ));
        // The most a header can claim.
        let file = head(1, 2, 0, 48_000, u32::MAX);
        assert_eq!(
            err(&file).1,
            ErrorKind::TooManyBytes {
                limit: Limits::default().max_decoded_bytes
            }
        );
    }

    #[test]
    fn the_encoder_refuses_what_a_file_cannot_hold_in_its_order() {
        let header = |channels, sample_rate, frames| Header {
            channels,
            sample_rate,
            frames,
        };
        let kind = |header: Header, samples: &[i16]| match encode(&header, samples) {
            Err(CodecError::KpcmEncode { kind }) => kind,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            kind(header(3, 0, 0), &[]),
            EncodeErrorKind::BadChannels { found: 3 }
        );
        assert_eq!(
            kind(header(0, 1, 1), &[]),
            EncodeErrorKind::BadChannels { found: 0 }
        );
        assert_eq!(
            kind(header(1, 0, 0), &[]),
            EncodeErrorKind::BadSampleRate { found: 0 }
        );
        assert_eq!(
            kind(header(1, MAX_SAMPLE_RATE + 1, 0), &[]),
            EncodeErrorKind::BadSampleRate { found: 384_001 }
        );
        assert_eq!(kind(header(1, 1, 0), &[]), EncodeErrorKind::NoFrames);
        assert_eq!(
            kind(header(2, 1, 2), &[0; 3]),
            EncodeErrorKind::WrongDataSize {
                frames: 2,
                channels: 2,
                found: 3
            }
        );
        // Too many, too.
        assert_eq!(
            kind(header(2, 1, 2), &[0; 5]),
            EncodeErrorKind::WrongDataSize {
                frames: 2,
                channels: 2,
                found: 5
            }
        );
    }

    #[test]
    fn errors_say_what_is_wrong() {
        let cases = [
            (
                CodecError::Kpcm {
                    offset: 4,
                    kind: ErrorKind::Version { found: 2 },
                },
                "KPCM error at byte 4: the version is 2, not 1: it is not supported",
            ),
            (
                CodecError::Kpcm {
                    offset: 0,
                    kind: ErrorKind::NotKpcm,
                },
                "KPCM error at byte 0: the file does not start with `KPCM`",
            ),
            (
                CodecError::Kpcm {
                    offset: 8,
                    kind: ErrorKind::BadSampleRate { found: 0 },
                },
                "KPCM error at byte 8: the sample rate is 0 Hz, not 1 to 384000",
            ),
            (
                CodecError::KpcmEncode {
                    kind: EncodeErrorKind::WrongDataSize {
                        frames: 2,
                        channels: 2,
                        found: 3,
                    },
                },
                "cannot encode as KPCM: 3 samples, not 2 × 2",
            ),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
        }
    }

    #[test]
    fn any_sound_round_trips_and_its_file_encodes_back_to_its_bytes() {
        check(
            "kpcm round trip",
            200,
            |rng| {
                let channels = if rng.range(0..2u32) == 0 { 1 } else { 2 };
                let frames = rng.range(1..300u32);
                let sample_rate = rng.range(1..MAX_SAMPLE_RATE + 1);
                let samples: Vec<i16> = (0..frames * u32::from(channels))
                    .map(|_| rng.range(-32768..32768i32) as i16)
                    .collect();
                (
                    Header {
                        channels,
                        sample_rate,
                        frames,
                    },
                    samples,
                )
            },
            |(header, samples)| {
                let file = encode(header, samples).map_err(|error| error.to_string())?;
                let sound = decode(&file, Limits::default()).map_err(|error| error.to_string())?;
                if sound.header != *header || sound.samples != *samples {
                    return Err("the sound changed".to_string());
                }
                let again = encode(&sound.header, &sound.samples).map_err(|e| e.to_string())?;
                if again != file {
                    return Err("the file changed".to_string());
                }
                Ok(())
            },
        );
    }

    #[test]
    fn random_bytes_never_make_it_panic() {
        check(
            "kpcm random bytes",
            500,
            |rng| {
                // Mostly a valid header, so that the later checks are
                // reached, then a few random bytes in place.
                let mut bytes = head(1, 2, 0, 48_000, rng.range(0..40u32));
                bytes.resize(16 + rng.range(0..200u32) as usize, 0x55);
                for _ in 0..rng.range(0..4u32) {
                    let at = rng.range(0..bytes.len() as u32) as usize;
                    bytes[at] = rng.range(0..256u32) as u8;
                }
                bytes.truncate(rng.range(0..bytes.len() as u32 + 1) as usize);
                bytes
            },
            |bytes| {
                let _ = decode(bytes, Limits::default());
                Ok(())
            },
        );
    }
}
