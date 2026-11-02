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

//! WAV (RIFF/WAVE): the sounds artists export, which the cook tool turns
//! into PCM16 or QOA. The game does not read WAV.
//!
//! [`decode`] reads a file into a [`Wave`]: its rate, its channels, and
//! its samples as `f32`, interleaved.
//!
//! - **Supported:** PCM of 16 and 24 bits, and IEEE float of 32 bits, in
//!   mono or stereo, as `WAVE_FORMAT_PCM` (1), `WAVE_FORMAT_IEEE_FLOAT`
//!   (3) or `WAVE_FORMAT_EXTENSIBLE` with either subformat, whose valid
//!   bits may be fewer than its samples hold. Integer samples are scaled
//!   to [-1, 1): 16-bit by 1/32768, 24-bit by 1/8388608, as libsndfile
//!   scales them. Float samples are kept as they are, past ±1 too.
//! - **Ignored:** every chunk other than `fmt ` and `data` (`LIST`, `fact`,
//!   `cue `, `smpl`, `bext`, `iXML`, `JUNK`, …), the byte rate, and the
//!   extensible format's channel mask. Nothing after `data` is read, so a
//!   missing pad byte after odd data, as some writers leave it, does not
//!   matter.
//! - **Rejected:** other sample formats (8-bit, 32-bit integer, 64-bit
//!   float, compressed ones: the error says to export again), more than two
//!   channels, RF64 and BW64, a NaN or infinite float sample, and anything
//!   that breaks the file's structure: a RIFF size that is not the file's
//!   (a file written to a pipe, or cut short), a chunk past the end, a
//!   missing `fmt ` or `data`, a second `fmt `, `data` before `fmt `, a
//!   block alignment that does not match the format, or data that ends
//!   inside a frame.
//!
//! What it rejects, although libsndfile reads it: 8-bit and 32-bit integer
//! PCM, PCM of 12 or 20 bits in larger samples, 64-bit float and compressed
//! formats (A-law, µ-law, ADPCM); more than two channels; RF64 and BW64; a
//! block alignment other
//! than the channels times the bytes a sample (libsndfile takes the frame's
//! size from the bit depth); a RIFF size that is not the file's, as in a
//! file written to a pipe or cut short, and a chunk past the end; NaN and
//! infinite samples; valid bits of 0, or more than the samples hold; data
//! that ends inside a frame.

use std::fmt;

use crate::{CodecError, Limits};

/// How the samples of a file are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SampleFormat {
    /// Signed 16-bit integers.
    Int16,
    /// Signed 24-bit integers, in 3 bytes.
    Int24,
    /// IEEE 754 floats of 32 bits.
    Float32,
}

/// A decoded sound.
#[derive(Clone, Debug, PartialEq)]
pub struct Wave {
    /// Frames a second.
    pub sample_rate: u32,
    /// 1 for mono, 2 for stereo.
    pub channels: u16,
    /// How the file stored its samples.
    pub format: SampleFormat,
    /// The samples, interleaved (left, right, left, … in stereo): `frames
    /// × channels` of them.
    pub samples: Vec<f32>,
}

/// What is wrong with a WAV file. See [`CodecError::Wav`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The input does not start with `RIFF`.
    NotRiff,
    /// The file is RF64 or BW64, the 64-bit forms of RIFF for files past
    /// 4 GiB.
    Rf64,
    /// The RIFF form is not `WAVE`.
    NotWave {
        /// The form type.
        found: [u8; 4],
    },
    /// The input ends too soon.
    UnexpectedEnd {
        /// What should have come next.
        expected: &'static str,
    },
    /// The RIFF size does not match the file's length.
    BadRiffSize {
        /// The size in the header.
        riff_size: u32,
        /// The bytes of the file after the size.
        found: usize,
    },
    /// A chunk runs past the end of the RIFF form.
    ChunkPastEnd {
        /// The chunk's id.
        chunk: [u8; 4],
    },
    /// A second `fmt ` chunk, or `data` before `fmt `.
    Misplaced {
        /// The chunk's id.
        chunk: [u8; 4],
    },
    /// There is no `fmt ` chunk.
    MissingFormat,
    /// There is no `data` chunk.
    MissingData,
    /// The `fmt ` chunk is too short for its format.
    BadFormatLength {
        /// Its length.
        length: u32,
    },
    /// A sample format other than 16- or 24-bit PCM or 32-bit float: at
    /// the format tag, or the extensible format's subformat.
    UnsupportedFormat {
        /// The format tag, or the extensible format's subformat.
        tag: u16,
        /// The bits per sample.
        bits: u16,
    },
    /// An extensible format whose subformat GUID is not
    /// `xxxx0000-0000-0010-8000-00aa00389b71`, so holds no format tag. A
    /// standard one with a tag other than PCM or float is
    /// [`UnsupportedFormat`](Self::UnsupportedFormat).
    UnsupportedSubformat,
    /// An extensible format's valid bits are 0, or more than its samples
    /// hold.
    BadValidBits {
        /// The valid bits.
        valid: u16,
        /// The bits each sample takes.
        bits: u16,
    },
    /// More than two channels, or none.
    UnsupportedChannels {
        /// The channel count.
        found: u16,
    },
    /// The sample rate is 0.
    ZeroSampleRate,
    /// The block alignment is not the channels times the bytes a sample.
    BadBlockAlign {
        /// The value in the file.
        found: u16,
        /// What it should be.
        expected: u16,
    },
    /// The data does not hold a whole number of frames.
    PartialFrame,
    /// A float sample is NaN or infinite.
    BadSample {
        /// The sample, counted from 0.
        index: usize,
    },
    /// The samples would take more than [`Limits::max_decoded_bytes`] as
    /// `f32`.
    TooManyBytes {
        /// The limit that was exceeded.
        limit: usize,
    },
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRiff => f.write_str("the file does not start with `RIFF`"),
            Self::Rf64 => f.write_str("the file is RF64 or BW64, which is not supported"),
            Self::NotWave { found } => {
                write!(f, "the RIFF form is `{}`, not `WAVE`", found.escape_ascii())
            }
            Self::UnexpectedEnd { expected } => {
                write!(f, "the file ends where {expected} should be")
            }
            Self::BadRiffSize { riff_size, found } if *riff_size as usize > *found => write!(
                f,
                "the RIFF size is {riff_size}, but only {found} bytes follow it: \
                 the file was cut short, or written where its sizes could not be filled \
                 in (to a pipe?)"
            ),
            Self::BadRiffSize { riff_size, found } => write!(
                f,
                "the RIFF size is {riff_size}, but {found} bytes follow it: more than the form"
            ),
            Self::ChunkPastEnd { chunk } => write!(
                f,
                "the `{}` chunk runs past the end of the RIFF form",
                chunk.escape_ascii()
            ),
            Self::Misplaced { chunk } => write!(
                f,
                "a `{}` chunk is out of place or repeated",
                chunk.escape_ascii()
            ),
            Self::MissingFormat => f.write_str("there is no `fmt ` chunk"),
            Self::MissingData => f.write_str("there is no `data` chunk"),
            Self::BadFormatLength { length } => {
                write!(
                    f,
                    "the `fmt ` chunk is {length} bytes, too short for its format"
                )
            }
            Self::UnsupportedFormat { tag, bits } => write!(
                f,
                "format {tag:#06x} at {bits} bits is not 16- or 24-bit PCM or 32-bit float: \
                 export it again as one of those"
            ),
            Self::UnsupportedSubformat => {
                f.write_str("the extensible format's subformat is not a standard format tag")
            }
            Self::BadValidBits { valid, bits } => {
                write!(f, "{valid} valid bits do not fit samples of {bits} bits")
            }
            Self::UnsupportedChannels { found } => {
                write!(f, "the file has {found} channels, not 1 or 2")
            }
            Self::ZeroSampleRate => f.write_str("the sample rate is 0"),
            Self::BadBlockAlign { found, expected } => {
                write!(f, "the block alignment is {found}, not {expected}")
            }
            Self::PartialFrame => f.write_str("the data ends inside a frame"),
            Self::BadSample { index } => write!(f, "sample {index} is NaN or infinite"),
            Self::TooManyBytes { limit } => write!(
                f,
                "the samples would take more than the limit of {limit} bytes"
            ),
        }
    }
}

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xfffe;

/// The GUID of an extensible format's subformat, after its first two bytes,
/// which hold the format tag: `xxxx0000-0000-0010-8000-00aa00389b71`.
const SUBFORMAT_TAIL: [u8; 14] = [
    0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
];

fn error(offset: usize, kind: ErrorKind) -> CodecError {
    CodecError::Wav { offset, kind }
}

/// The input ends where `expected` should be.
fn cut_short(bytes: &[u8], expected: &'static str) -> CodecError {
    error(bytes.len(), ErrorKind::UnexpectedEnd { expected })
}

/// What `fmt ` says.
struct Format {
    format: SampleFormat,
    channels: u16,
    sample_rate: u32,
}

/// Decodes a WAV file.
///
/// # Errors
///
/// [`CodecError::Wav`], with the byte offset of the problem, for a file
/// that is malformed, outside the subset the module describes, or breaks
/// a limit of `limits`.
pub fn decode(bytes: &[u8], limits: Limits) -> Result<Wave, CodecError> {
    if matches!(bytes.first_chunk::<4>(), Some(b"RF64" | b"BW64")) {
        return Err(error(0, ErrorKind::Rf64));
    }
    if !b"RIFF".starts_with(bytes.get(..4).unwrap_or(bytes)) {
        return Err(error(0, ErrorKind::NotRiff));
    }
    let Some(&[_, _, _, _, s0, s1, s2, s3, w0, w1, w2, w3]) = bytes.first_chunk::<12>() else {
        return Err(cut_short(bytes, "the RIFF header"));
    };
    let form = [w0, w1, w2, w3];
    if form != *b"WAVE" {
        return Err(error(8, ErrorKind::NotWave { found: form }));
    }
    let riff_size = u32::from_le_bytes([s0, s1, s2, s3]);
    let found = bytes.len() - 8;
    // The form may be followed by the pad byte that makes it even.
    let fits =
        found == riff_size as usize || (riff_size % 2 == 1 && found == riff_size as usize + 1);
    if !fits {
        return Err(error(4, ErrorKind::BadRiffSize { riff_size, found }));
    }
    let end = 8 + riff_size as usize;

    let mut format: Option<Format> = None;
    let mut data: Option<(usize, &[u8])> = None;
    let mut at = 12;
    while at < end {
        let Some(&[i0, i1, i2, i3, l0, l1, l2, l3]) =
            bytes.get(at..end).and_then(<[u8]>::first_chunk::<8>)
        else {
            return Err(cut_short(
                bytes.get(..end).unwrap_or(bytes),
                "a chunk header",
            ));
        };
        let id = [i0, i1, i2, i3];
        let length = u32::from_le_bytes([l0, l1, l2, l3]);
        let data_at = at + 8;
        let Some(body) = bytes
            .get(data_at..end)
            .and_then(|rest| rest.get(..length as usize))
        else {
            return Err(error(at, ErrorKind::ChunkPastEnd { chunk: id }));
        };
        match &id {
            b"fmt " => {
                if format.is_some() {
                    return Err(error(at, ErrorKind::Misplaced { chunk: id }));
                }
                format = Some(read_format(body, at, length)?);
            }
            b"data" => {
                if format.is_none() {
                    return Err(error(at, ErrorKind::Misplaced { chunk: id }));
                }
                data = Some((data_at, body));
                // Nothing after the data is used, so the walk ends here:
                // writers that leave out the pad byte after odd data
                // (GStreamer's, Python's `wave`) then still read.
                break;
            }
            _ => {}
        }
        // Odd chunks are padded to an even length. The last one's pad may
        // be the form's own, past `end`, which ends the walk either way.
        at = data_at + body.len() + body.len() % 2;
    }
    let Some(format) = format else {
        return Err(error(12, ErrorKind::MissingFormat));
    };
    let Some((data_at, data)) = data else {
        return Err(error(12, ErrorKind::MissingData));
    };
    let samples = read_samples(data, data_at, &format, limits)?;
    Ok(Wave {
        sample_rate: format.sample_rate,
        channels: format.channels,
        format: format.format,
        samples,
    })
}

/// Reads `fmt `, which starts at `at`.
fn read_format(body: &[u8], at: usize, length: u32) -> Result<Format, CodecError> {
    let short = || error(at, ErrorKind::BadFormatLength { length });
    let Some(&[t0, t1, c0, c1, r0, r1, r2, r3, _, _, _, _, a0, a1, b0, b1]) =
        body.first_chunk::<16>()
    else {
        return Err(short());
    };
    let mut tag = u16::from_le_bytes([t0, t1]);
    let channels = u16::from_le_bytes([c0, c1]);
    let sample_rate = u32::from_le_bytes([r0, r1, r2, r3]);
    let block_align = u16::from_le_bytes([a0, a1]);
    let bits = u16::from_le_bytes([b0, b1]);
    let field = |offset: usize| at + 8 + offset;
    // Where the format's tag is: the extensible format's, in its subformat.
    let mut tag_at = field(0);
    if tag == WAVE_FORMAT_EXTENSIBLE {
        // cbSize, valid bits, channel mask, then the subformat GUID.
        let Some(&extension) = body.get(16..).and_then(<[u8]>::first_chunk::<24>) else {
            return Err(short());
        };
        let [_, _, v0, v1, _, _, _, _, g0, g1, tail @ ..] = extension;
        if tail != SUBFORMAT_TAIL {
            return Err(error(field(24), ErrorKind::UnsupportedSubformat));
        }
        // Fewer valid bits than the samples hold are the high ones, so
        // the samples read as their full size.
        let valid = u16::from_le_bytes([v0, v1]);
        if valid == 0 || valid > bits {
            return Err(error(field(18), ErrorKind::BadValidBits { valid, bits }));
        }
        tag = u16::from_le_bytes([g0, g1]);
        tag_at = field(24);
    }
    let format = match (tag, bits) {
        (WAVE_FORMAT_PCM, 16) => SampleFormat::Int16,
        (WAVE_FORMAT_PCM, 24) => SampleFormat::Int24,
        (WAVE_FORMAT_IEEE_FLOAT, 32) => SampleFormat::Float32,
        (tag, bits) => return Err(error(tag_at, ErrorKind::UnsupportedFormat { tag, bits })),
    };
    if !(1..=2).contains(&channels) {
        return Err(error(
            field(2),
            ErrorKind::UnsupportedChannels { found: channels },
        ));
    }
    if sample_rate == 0 {
        return Err(error(field(4), ErrorKind::ZeroSampleRate));
    }
    let expected = channels * (bits / 8);
    if block_align != expected {
        return Err(error(
            field(12),
            ErrorKind::BadBlockAlign {
                found: block_align,
                expected,
            },
        ));
    }
    Ok(Format {
        format,
        channels,
        sample_rate,
    })
}

/// Turns the bytes of `data`, which starts at `at`, into samples.
fn read_samples(
    data: &[u8],
    at: usize,
    format: &Format,
    limits: Limits,
) -> Result<Vec<f32>, CodecError> {
    let width = match format.format {
        SampleFormat::Int16 => 2,
        SampleFormat::Int24 => 3,
        SampleFormat::Float32 => 4,
    };
    if !data
        .len()
        .is_multiple_of(width * usize::from(format.channels))
    {
        return Err(error(at + data.len(), ErrorKind::PartialFrame));
    }
    let count = data.len() / width;
    if count.saturating_mul(4) > limits.max_decoded_bytes {
        return Err(error(
            at,
            ErrorKind::TooManyBytes {
                limit: limits.max_decoded_bytes,
            },
        ));
    }
    let mut samples = Vec::with_capacity(count);
    match format.format {
        SampleFormat::Int16 => {
            for &[lo, hi] in data.as_chunks::<2>().0 {
                samples.push(f32::from(i16::from_le_bytes([lo, hi])) / 32768.0);
            }
        }
        SampleFormat::Int24 => {
            for &[b0, b1, b2] in data.as_chunks::<3>().0 {
                // Into the top of an i32, then down again, to extend the sign.
                let v = i32::from_le_bytes([0, b0, b1, b2]) >> 8;
                samples.push(v as f32 / 8_388_608.0);
            }
        }
        SampleFormat::Float32 => {
            for (index, &bytes) in data.as_chunks::<4>().0.iter().enumerate() {
                let x = f32::from_le_bytes(bytes);
                if !x.is_finite() {
                    return Err(error(at + index * 4, ErrorKind::BadSample { index }));
                }
                samples.push(x);
            }
        }
    }
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        if body.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    /// A `fmt ` body of 16 bytes.
    fn format(tag: u16, channels: u16, rate: u32, align: u16, bits: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(align)).to_le_bytes());
        out.extend_from_slice(&align.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out
    }

    /// An extensible `fmt ` body: the subformat's tag, its valid bits, and
    /// the GUID after the tag.
    fn extensible(subformat: u16, bits: u16, valid: u16, tail: [u8; 14]) -> Vec<u8> {
        let mut out = format(WAVE_FORMAT_EXTENSIBLE, 1, 8000, bits / 8, bits);
        out.extend_from_slice(&22u16.to_le_bytes());
        out.extend_from_slice(&valid.to_le_bytes());
        out.extend_from_slice(&4u32.to_le_bytes());
        out.extend_from_slice(&subformat.to_le_bytes());
        out.extend_from_slice(&tail);
        out
    }

    fn riff(chunks: &[Vec<u8>]) -> Vec<u8> {
        let body = [b"WAVE".to_vec(), chunks.concat()].concat();
        [
            b"RIFF".to_vec(),
            (body.len() as u32).to_le_bytes().to_vec(),
            body,
        ]
        .concat()
    }

    /// 16-bit mono at 8 kHz: `fmt ` at 12, the data's bytes at 44.
    fn mono16(data: &[u8]) -> Vec<u8> {
        riff(&[
            chunk(b"fmt ", &format(1, 1, 8000, 2, 16)),
            chunk(b"data", data),
        ])
    }

    fn err(bytes: &[u8]) -> (usize, ErrorKind) {
        match decode(bytes, Limits::default()) {
            Err(CodecError::Wav { offset, kind }) => (offset, kind),
            other => panic!("not a WAV error: {other:?}"),
        }
    }

    fn ok(bytes: &[u8]) -> Wave {
        decode(bytes, Limits::default()).unwrap()
    }

    #[test]
    fn samples_are_scaled_as_libsndfile_scales_them() {
        let data = [0x00, 0x00, 0x00, 0x80, 0xff, 0x7f, 0x00, 0x40];
        let wave = ok(&riff(&[
            chunk(b"fmt ", &format(1, 2, 44100, 4, 16)),
            chunk(b"data", &data),
        ]));
        assert_eq!(
            (wave.sample_rate, wave.channels, wave.format),
            (44100, 2, SampleFormat::Int16)
        );
        assert_eq!(wave.samples, [0.0, -1.0, 32767.0 / 32768.0, 0.5]);
        // 24 bits: the sign of the top byte extends.
        let data = [0xff, 0xff, 0xff, 0x00, 0x00, 0x80, 0xff, 0xff, 0x7f];
        let wave = ok(&riff(&[
            chunk(b"fmt ", &format(1, 1, 8000, 3, 24)),
            chunk(b"data", &data),
        ]));
        assert_eq!(
            wave.samples,
            [-1.0 / 8_388_608.0, -1.0, 8_388_607.0 / 8_388_608.0]
        );
        // Float, past ±1 too, and extensible PCM with 20 valid bits.
        let wave = ok(&riff(&[
            chunk(b"fmt ", &format(3, 1, 8000, 4, 32)),
            chunk(b"data", &[1.5f32, -0.0].map(f32::to_le_bytes).concat()),
        ]));
        assert_eq!(wave.samples.map_bits(), [1.5f32, -0.0].map_bits());
        let wave = ok(&riff(&[
            chunk(b"fmt ", &extensible(1, 24, 20, SUBFORMAT_TAIL)),
            chunk(b"data", &[0x00, 0x00, 0x40]),
        ]));
        assert_eq!(
            (wave.format, wave.samples),
            (SampleFormat::Int24, vec![0.5])
        );
    }

    trait Bits {
        fn map_bits(&self) -> Vec<u32>;
    }

    impl Bits for [f32] {
        fn map_bits(&self) -> Vec<u32> {
            self.iter().map(|x| x.to_bits()).collect()
        }
    }

    #[test]
    fn other_chunks_are_skipped_and_nothing_after_data_is_read() {
        let mut bytes = riff(&[
            chunk(b"JUNK", b"odd"),
            chunk(b"fmt ", &format(1, 1, 8000, 2, 16)),
            chunk(b"LIST", b"INFO"),
            chunk(b"data", &[0x00, 0x40]),
            // It claims more than the form holds, but it comes after the
            // data, so it is never read.
            [b"cue ".as_slice(), &[0xff; 4]].concat(),
        ]);
        assert_eq!(ok(&bytes).samples, [0.5]);
        // The same file with a broken `fmt ` after the data.
        bytes.truncate(bytes.len() - 8);
        bytes.extend_from_slice(&chunk(b"fmt ", b"xx"));
        let size = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        assert_eq!(ok(&bytes).samples, [0.5]);
    }

    #[test]
    fn a_form_of_odd_size_may_be_followed_by_its_pad_byte_or_not() {
        // 24-bit mono, one frame: 3 bytes of data, then the pad.
        let padded = riff(&[
            chunk(b"fmt ", &format(1, 1, 8000, 3, 24)),
            chunk(b"data", &[0, 0, 0x40]),
        ]);
        // A RIFF size of odd length, which leaves the pad out.
        let mut odd = padded.clone();
        let size = (padded.len() - 9) as u32;
        odd[4..8].copy_from_slice(&size.to_le_bytes());
        assert_eq!(ok(&odd).samples, [0.5]);
        // No pad at all, as GStreamer writes it.
        assert_eq!(ok(&odd[..odd.len() - 1]).samples, [0.5]);
        // But no other byte after the form.
        let mut extra = padded.clone();
        extra.push(0);
        assert_eq!(
            err(&extra),
            (
                4,
                ErrorKind::BadRiffSize {
                    riff_size: size + 1,
                    found: padded.len() - 7
                }
            )
        );
    }

    #[test]
    fn the_header_is_checked_first() {
        assert_eq!(err(b"RIFX\0\0\0\0WAVE"), (0, ErrorKind::NotRiff));
        assert_eq!(err(b"OggS"), (0, ErrorKind::NotRiff));
        assert_eq!(err(b"RF64\xff\xff\xff\xffWAVE"), (0, ErrorKind::Rf64));
        assert_eq!(err(b"BW64"), (0, ErrorKind::Rf64));
        let end = |at, expected| (at, ErrorKind::UnexpectedEnd { expected });
        assert_eq!(err(b""), end(0, "the RIFF header"));
        assert_eq!(err(b"RI"), end(2, "the RIFF header"));
        assert_eq!(err(b"RIFF\x04\0\0\0WAV"), end(11, "the RIFF header"));
        assert_eq!(
            err(b"RIFF\x04\0\0\0AVI "),
            (8, ErrorKind::NotWave { found: *b"AVI " })
        );
    }

    #[test]
    fn the_riff_size_must_be_the_files() {
        let bytes = mono16(&[0, 0]);
        let found = bytes.len() - 8;
        // Cut short, or written to a pipe, which leaves the sizes at 0 or
        // at their largest.
        for riff_size in [found as u32 + 2, 0, u32::MAX] {
            let mut wrong = bytes.clone();
            wrong[4..8].copy_from_slice(&riff_size.to_le_bytes());
            let (offset, kind) = err(&wrong);
            assert_eq!(
                (offset, &kind),
                (4, &ErrorKind::BadRiffSize { riff_size, found })
            );
            if riff_size > 0 {
                assert!(kind.to_string().contains("cut short"), "{kind}");
            }
        }
        // Bytes after the form.
        let mut long = bytes.clone();
        long.extend_from_slice(b"tail");
        assert_eq!(
            err(&long),
            (
                4,
                ErrorKind::BadRiffSize {
                    riff_size: found as u32,
                    found: found + 4
                }
            )
        );
    }

    #[test]
    fn chunks_must_fit_the_form() {
        // A chunk header cut short.
        let bytes = riff(&[chunk(b"fmt ", &format(1, 1, 8000, 2, 16)), b"data".to_vec()]);
        assert_eq!(
            err(&bytes),
            (
                bytes.len(),
                ErrorKind::UnexpectedEnd {
                    expected: "a chunk header"
                }
            )
        );
        // A chunk longer than what is left: a `LIST`, then a `data` that
        // claims more than the file holds.
        let mut list = chunk(b"LIST", b"INFO");
        list[4] = 5;
        assert_eq!(
            err(&riff(&[list])),
            (12, ErrorKind::ChunkPastEnd { chunk: *b"LIST" })
        );
        let mut bytes = mono16(&[0, 0]);
        bytes[40] = 4;
        assert_eq!(
            err(&bytes),
            (36, ErrorKind::ChunkPastEnd { chunk: *b"data" })
        );
    }

    #[test]
    fn fmt_must_come_once_and_before_data() {
        let fmt = chunk(b"fmt ", &format(1, 1, 8000, 2, 16));
        let data = chunk(b"data", &[0, 0]);
        assert_eq!(
            err(&riff(&[data.clone(), fmt.clone()])),
            (12, ErrorKind::Misplaced { chunk: *b"data" })
        );
        assert_eq!(
            err(&riff(&[fmt.clone(), fmt.clone(), data])),
            (36, ErrorKind::Misplaced { chunk: *b"fmt " })
        );
        assert_eq!(err(&riff(&[])), (12, ErrorKind::MissingFormat));
        assert_eq!(
            err(&riff(&[chunk(b"LIST", b"INFO")])),
            (12, ErrorKind::MissingFormat)
        );
        assert_eq!(err(&riff(&[fmt])), (12, ErrorKind::MissingData));
    }

    #[test]
    fn fmt_must_be_long_enough_for_its_format() {
        let short = &format(1, 1, 8000, 2, 16)[..14];
        assert_eq!(
            err(&riff(&[chunk(b"fmt ", short)])),
            (12, ErrorKind::BadFormatLength { length: 14 })
        );
        // Extensible needs 24 bytes more, not the 2 of a cbSize of 0.
        let mut ext = format(WAVE_FORMAT_EXTENSIBLE, 1, 8000, 2, 16);
        ext.extend_from_slice(&[0, 0]);
        assert_eq!(
            err(&riff(&[chunk(b"fmt ", &ext)])),
            (12, ErrorKind::BadFormatLength { length: 18 })
        );
    }

    #[test]
    fn formats_outside_the_subset_are_rejected_with_their_tag_and_bits() {
        let reject = |body: Vec<u8>| err(&riff(&[chunk(b"fmt ", &body), chunk(b"data", &[0; 4])]));
        // Each at the format tag, 8 bytes into `fmt `.
        for (tag, bits, align) in [(1, 8, 1), (1, 32, 4), (3, 64, 8), (3, 16, 2), (2, 4, 256)] {
            assert_eq!(
                reject(format(tag, 1, 8000, align, bits)),
                (20, ErrorKind::UnsupportedFormat { tag, bits }),
                "{tag} {bits}"
            );
        }
        // Extensible reports its subformat's tag, where the subformat is,
        // 32 bytes into `fmt `.
        assert_eq!(
            reject(extensible(1, 32, 32, SUBFORMAT_TAIL)),
            (44, ErrorKind::UnsupportedFormat { tag: 1, bits: 32 })
        );
        let message = ErrorKind::UnsupportedFormat { tag: 1, bits: 8 }.to_string();
        assert!(message.contains("export it again"), "{message}");
    }

    #[test]
    fn extensible_subformats_must_be_pcm_or_float_with_bits_that_fit() {
        let reject = |body: Vec<u8>| err(&riff(&[chunk(b"fmt ", &body), chunk(b"data", &[0; 4])]));
        let mut other = SUBFORMAT_TAIL;
        other[13] ^= 1;
        // At the subformat's GUID, 32 bytes into `fmt `.
        assert_eq!(
            reject(extensible(1, 16, 16, other)),
            (44, ErrorKind::UnsupportedSubformat)
        );
        // At the valid bits, 26 bytes into `fmt `.
        for valid in [0, 17] {
            assert_eq!(
                reject(extensible(1, 16, valid, SUBFORMAT_TAIL)),
                (38, ErrorKind::BadValidBits { valid, bits: 16 })
            );
        }
        // From 1 valid bit to all of them.
        for valid in [1, 8, 16] {
            let wave = ok(&riff(&[
                chunk(b"fmt ", &extensible(1, 16, valid, SUBFORMAT_TAIL)),
                chunk(b"data", &[0, 0x40]),
            ]));
            assert_eq!(wave.samples, [0.5]);
        }
    }

    #[test]
    fn channels_rate_and_block_alignment_are_checked() {
        let reject = |body: Vec<u8>| err(&riff(&[chunk(b"fmt ", &body), chunk(b"data", &[0; 4])]));
        for channels in [0, 3, 6] {
            assert_eq!(
                reject(format(1, channels, 8000, channels * 2, 16)),
                (22, ErrorKind::UnsupportedChannels { found: channels })
            );
        }
        assert_eq!(
            reject(format(1, 1, 0, 2, 16)),
            (24, ErrorKind::ZeroSampleRate)
        );
        // The alignment libsndfile would ignore.
        assert_eq!(
            reject(format(1, 1, 8000, 3, 16)),
            (
                32,
                ErrorKind::BadBlockAlign {
                    found: 3,
                    expected: 2
                }
            )
        );
        assert_eq!(
            reject(format(1, 2, 8000, 4, 24)),
            (
                32,
                ErrorKind::BadBlockAlign {
                    found: 4,
                    expected: 6
                }
            )
        );
    }

    #[test]
    fn the_data_must_hold_whole_frames_of_finite_samples() {
        let stereo = |data: &[u8]| {
            riff(&[
                chunk(b"fmt ", &format(1, 2, 8000, 4, 16)),
                chunk(b"data", data),
            ])
        };
        // The data starts at 44: an error at its end.
        assert_eq!(err(&stereo(&[0; 6])), (50, ErrorKind::PartialFrame));
        assert_eq!(err(&stereo(&[0; 3])), (47, ErrorKind::PartialFrame));
        assert_eq!(ok(&stereo(&[])).samples, [] as [f32; 0]);
        let float = |samples: &[f32]| {
            riff(&[
                chunk(b"fmt ", &format(3, 1, 8000, 4, 32)),
                chunk(
                    b"data",
                    &samples
                        .iter()
                        .flat_map(|x| x.to_le_bytes())
                        .collect::<Vec<u8>>(),
                ),
            ])
        };
        assert_eq!(
            err(&float(&[0.0, f32::NAN])),
            (48, ErrorKind::BadSample { index: 1 })
        );
        assert_eq!(
            err(&float(&[0.0, 0.5, f32::NEG_INFINITY])),
            (52, ErrorKind::BadSample { index: 2 })
        );
        assert_eq!(
            err(&float(&[f32::INFINITY])),
            (44, ErrorKind::BadSample { index: 0 })
        );
    }

    #[test]
    fn the_samples_must_fit_the_limit_as_f32() {
        // Four 16-bit samples take 16 bytes as f32.
        let bytes = mono16(&[0; 8]);
        let limits = |max_decoded_bytes| Limits {
            max_decoded_bytes,
            ..Limits::default()
        };
        assert_eq!(decode(&bytes, limits(16)).unwrap().samples.len(), 4);
        assert_eq!(
            decode(&bytes, limits(15)),
            Err(error(44, ErrorKind::TooManyBytes { limit: 15 }))
        );
    }
}
