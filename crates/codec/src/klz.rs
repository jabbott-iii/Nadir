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

//! KLZ, our byte-oriented LZ77 compression for pack entries. It is LZ4-like
//! in spirit, but its own format, which this module and the vectors in
//! `crates/codec/tests/klz/` define.
//!
//! ```text
//! stream := block*            ; until the input is exhausted
//! block  := token:u8
//!           [extra literal length: 255-run bytes + final byte]  if token>>4 == 15
//!           literals[lit_len]
//!           offset:u16 LE                                       (absent on the last block)
//!           [extra match length: 255-run bytes + final byte]    if token&15 == 15
//! lit_len   = (token >> 4) + extras
//! match_len = (token & 15) + 4 + extras
//! ```
//!
//! The last block holds literals only, maybe none: the input ends right
//! after them, and its token's match half is 0. Every stream but the empty
//! one, which is no bytes, ends with one.
//!
//! ```
//! use nadir_codec::{Limits, klz};
//!
//! let data = b"abcabcabcabcabc";
//! let stream = klz::compress(data);
//! assert_eq!(klz::decompress(&stream, data.len(), Limits::default())?, data);
//! # Ok::<(), nadir_codec::CodecError>(())
//! ```
//!
//! The caller knows how long the data is (a pack's table of contents
//! holds it), and [`decompress`] must produce exactly that many bytes. It
//! rejects a match before the start of the output or at offset 0, a
//! stream that ends after a match, a last block whose token has a match
//! half, and anything shorter or longer than the length given.

use std::fmt;

use crate::{CodecError, Limits};

/// What is wrong with a KLZ stream. See [`CodecError::Klz`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The input ends too soon.
    UnexpectedEnd {
        /// What should have come next.
        expected: &'static str,
    },
    /// A length's extra bytes add up past what a `usize` can count.
    LengthTooLarge,
    /// A match has offset 0.
    ZeroOffset,
    /// A match reaches back past the start of the output.
    OffsetTooFar {
        /// How far back it reaches.
        offset: usize,
        /// How many bytes of output there are.
        available: usize,
    },
    /// The last block's token has a match half, but no match follows.
    MatchWithoutOffset,
    /// The output would be longer than the length given.
    TooLong {
        /// The length given.
        raw_size: usize,
    },
    /// The output is shorter than the length given.
    TooShort {
        /// The length given.
        raw_size: usize,
        /// The bytes the stream gives.
        found: usize,
    },
    /// The length given is more than [`Limits::max_decoded_bytes`].
    TooManyBytes {
        /// The limit that was exceeded.
        limit: usize,
    },
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd { expected } => {
                write!(f, "the stream ends where {expected} should be")
            }
            Self::LengthTooLarge => f.write_str("a length is too large to count"),
            Self::ZeroOffset => f.write_str("a match has offset 0"),
            Self::OffsetTooFar { offset, available } => write!(
                f,
                "a match reaches {offset} bytes back, but there are {available}"
            ),
            Self::MatchWithoutOffset => {
                f.write_str("the last block's token gives a match length, but no match follows")
            }
            Self::TooLong { raw_size } => {
                write!(
                    f,
                    "the stream gives more than the {raw_size} bytes expected"
                )
            }
            Self::TooShort { raw_size, found } => write!(
                f,
                "the stream gives {found} bytes, not the {raw_size} expected"
            ),
            Self::TooManyBytes { limit } => write!(
                f,
                "the data would take more than the limit of {limit} bytes"
            ),
        }
    }
}

/// The most bytes of output one byte of stream can give: the extra bytes
/// of a match length add 255 each.
const MOST_PER_BYTE: usize = 255;

/// The shortest match.
const MIN_MATCH: usize = 4;

/// The farthest a match can reach back.
const MAX_OFFSET: usize = 0xffff;

/// The encoder's hash table: one entry for each 16-bit hash of 4 bytes.
const HASH_BITS: u32 = 16;

fn error(offset: usize, kind: ErrorKind) -> CodecError {
    CodecError::Klz { offset, kind }
}

/// Decompresses a KLZ stream that holds `raw_size` bytes.
///
/// # Errors
///
/// [`CodecError::Klz`], with the byte offset of the problem, for a stream
/// that is malformed or does not hold exactly `raw_size` bytes, and for a
/// `raw_size` past [`Limits::max_decoded_bytes`] or past what the stream
/// could hold.
pub fn decompress(stream: &[u8], raw_size: usize, limits: Limits) -> Result<Vec<u8>, CodecError> {
    if raw_size > limits.max_decoded_bytes {
        return Err(error(
            0,
            ErrorKind::TooManyBytes {
                limit: limits.max_decoded_bytes,
            },
        ));
    }
    // A stream too short for the length given is cut short, however it
    // goes on: checking that first keeps a short stream from claiming a
    // large allocation.
    if raw_size > stream.len().saturating_mul(MOST_PER_BYTE) {
        return Err(error(
            stream.len(),
            ErrorKind::UnexpectedEnd {
                expected: "more blocks",
            },
        ));
    }
    let mut out = Vec::with_capacity(raw_size);
    let mut at = 0;
    // Whether the last block read was a last block: literals only.
    let mut ended = stream.is_empty();
    while let Some(&token) = stream.get(at) {
        let token_at = at;
        at += 1;
        let literals = length(stream, &mut at, token >> 4, 0, "a literal length")?;
        let Some(run) = stream.get(at..).and_then(|rest| rest.get(..literals)) else {
            return Err(cut_short(stream, "literals"));
        };
        if out.len() + run.len() > raw_size {
            return Err(error(at, ErrorKind::TooLong { raw_size }));
        }
        out.extend_from_slice(run);
        at += run.len();
        if at == stream.len() {
            // The last block: literals only.
            if token & 0x0f != 0 {
                return Err(error(token_at, ErrorKind::MatchWithoutOffset));
            }
            ended = true;
            break;
        }
        let offset_at = at;
        let Some(&[lo, hi]) = stream.get(at..).and_then(<[u8]>::first_chunk::<2>) else {
            return Err(cut_short(stream, "a match's offset"));
        };
        at += 2;
        let offset = usize::from(u16::from_le_bytes([lo, hi]));
        let length = length(stream, &mut at, token & 0x0f, MIN_MATCH, "a match length")?;
        if offset == 0 {
            return Err(error(offset_at, ErrorKind::ZeroOffset));
        }
        if offset > out.len() {
            return Err(error(
                offset_at,
                ErrorKind::OffsetTooFar {
                    offset,
                    available: out.len(),
                },
            ));
        }
        if length > raw_size - out.len() {
            return Err(error(offset_at, ErrorKind::TooLong { raw_size }));
        }
        // A match may overlap what it writes: each pass copies everything
        // written from its start, so the copies double.
        let from = out.len() - offset;
        let mut left = length;
        while left > 0 {
            let part = left.min(out.len() - from);
            out.extend_from_within(from..from + part);
            left -= part;
        }
    }
    if !ended {
        return Err(cut_short(stream, "the last block, of literals only"));
    }
    if out.len() < raw_size {
        return Err(error(
            stream.len(),
            ErrorKind::TooShort {
                raw_size,
                found: out.len(),
            },
        ));
    }
    Ok(out)
}

/// Reads a length: `base` plus the token's 4-bit part `nibble`, and when
/// that is 15, plus each extra byte, going on while it is 255.
fn length(
    stream: &[u8],
    at: &mut usize,
    nibble: u8,
    base: usize,
    expected: &'static str,
) -> Result<usize, CodecError> {
    let mut length = base + usize::from(nibble);
    if nibble < 15 {
        return Ok(length);
    }
    let start = *at;
    loop {
        let Some(&byte) = stream.get(*at) else {
            return Err(cut_short(stream, expected));
        };
        *at += 1;
        length = length
            .checked_add(usize::from(byte))
            .ok_or_else(|| error(start, ErrorKind::LengthTooLarge))?;
        if byte != 255 {
            return Ok(length);
        }
    }
}

/// The stream ends where `expected` should be.
fn cut_short(stream: &[u8], expected: &'static str) -> CodecError {
    error(stream.len(), ErrorKind::UnexpectedEnd { expected })
}

/// Compresses `data` into a KLZ stream, the same bytes for the same data.
///
/// It is greedy: at each position it takes the match the hash table offers
/// for the next 4 bytes, if there is one within reach, and makes it as long
/// as it goes. Empty data gives an empty stream.
pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2 + 16);
    if data.is_empty() {
        return out;
    }
    // Positions plus one, so that 0 is empty.
    let mut table = vec![0u32; 1 << HASH_BITS];
    let mut anchor = 0;
    let mut i = 0;
    while i + MIN_MATCH <= data.len() {
        let Some(&word) = data.get(i..).and_then(<[u8]>::first_chunk::<4>) else {
            break;
        };
        let slot = hash(word);
        let candidate = table[slot] as usize;
        table[slot] = (i + 1) as u32;
        let Some(start) = candidate.checked_sub(1) else {
            i += 1;
            continue;
        };
        if i - start > MAX_OFFSET || data.get(start..start + MIN_MATCH) != Some(&word[..]) {
            i += 1;
            continue;
        }
        let mut length = MIN_MATCH;
        while data
            .get(i + length)
            .is_some_and(|&byte| Some(&byte) == data.get(start + length))
        {
            length += 1;
        }
        write_block(&mut out, &data[anchor..i], Some((i - start, length)));
        i += length;
        anchor = i;
    }
    write_block(&mut out, &data[anchor..], None);
    out
}

/// The hash of 4 bytes: Knuth's multiplicative hash, top 16 bits.
fn hash(word: [u8; 4]) -> usize {
    (u32::from_le_bytes(word).wrapping_mul(2_654_435_761) >> (32 - HASH_BITS)) as usize
}

/// Writes one block: its literals, then its match, if there is one, as an
/// offset and a length of at least 4.
fn write_block(out: &mut Vec<u8>, literals: &[u8], matched: Option<(usize, usize)>) {
    let match_extra = matched.map_or(0, |(_, length)| length - MIN_MATCH);
    let token = (nibble(literals.len()) << 4) | nibble(match_extra);
    out.push(token);
    write_extra(out, literals.len());
    out.extend_from_slice(literals);
    if let Some((offset, _)) = matched {
        out.extend_from_slice(&(offset as u16).to_le_bytes());
        write_extra(out, match_extra);
    }
}

/// The 4-bit part of a length: itself below 15, else 15.
fn nibble(length: usize) -> u8 {
    length.min(15) as u8
}

/// Writes the extra bytes of a length of 15 or more: 255 while more than
/// that is left, then the rest.
fn write_extra(out: &mut Vec<u8>, length: usize) {
    if length < 15 {
        return;
    }
    let mut left = length - 15;
    while left >= 255 {
        out.push(255);
        left -= 255;
    }
    out.push(left as u8);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(stream: &[u8], raw_size: usize) -> (usize, ErrorKind) {
        match decompress(stream, raw_size, Limits::default()) {
            Err(CodecError::Klz { offset, kind }) => (offset, kind),
            other => panic!("not a KLZ error: {other:?}"),
        }
    }

    fn ok(stream: &[u8], raw_size: usize) -> Vec<u8> {
        decompress(stream, raw_size, Limits::default()).unwrap()
    }

    #[test]
    fn blocks_decode_by_hand() {
        // Literals only: token 0x30, "abc".
        assert_eq!(ok(b"\x30abc", 3), b"abc");
        // "ab", then 6 from 2 back (token 0x22: 2 literals, 2 + 4), then an
        // empty last block.
        assert_eq!(ok(b"\x22ab\x02\x00\x00", 8), b"abababab");
        // A run at offset 1, then "!".
        assert_eq!(ok(b"\x10x\x01\x00\x10!", 6), b"xxxxx!");
        // 15 literals need an extra byte of 0; 15 + 4 a match extra of 0.
        let mut stream = vec![0xf0, 0];
        stream.extend_from_slice(&[b'q'; 15]);
        assert_eq!(ok(&stream, 15), [b'q'; 15]);
        let stream = [0x1f, b'z', 1, 0, 0, 0x00];
        assert_eq!(ok(&stream, 20), [b'z'; 20]);
        // Extra bytes of 255 go on: 15 + 255 + 1 literals.
        let mut stream = vec![0xf0, 255, 1];
        stream.extend_from_slice(&[7; 271]);
        assert_eq!(ok(&stream, 271).len(), 271);
        // No blocks is no bytes.
        assert_eq!(ok(b"", 0), b"");
    }

    #[test]
    fn a_stream_cut_short_is_rejected_where_it_ends() {
        let end = |at, expected| (at, ErrorKind::UnexpectedEnd { expected });
        assert_eq!(err(b"\x30ab", 3), end(3, "literals"));
        assert_eq!(err(b"\xf0", 15), end(1, "a literal length"));
        assert_eq!(err(b"\xf0\xff", 270), end(2, "a literal length"));
        assert_eq!(err(b"\x11a\x01", 6), end(3, "a match's offset"));
        assert_eq!(err(b"\x1fa\x01\x00", 20), end(4, "a match length"));
        // A stream that ends after a match has no last block.
        assert_eq!(
            err(b"\x10x\x01\x00", 5),
            end(4, "the last block, of literals only")
        );
    }

    #[test]
    fn matches_are_checked() {
        assert_eq!(err(b"\x10x\x00\x00\x00", 5), (2, ErrorKind::ZeroOffset));
        assert_eq!(
            err(b"\x10x\x02\x00\x00", 5),
            (
                2,
                ErrorKind::OffsetTooFar {
                    offset: 2,
                    available: 1
                }
            )
        );
        // The last block's token has a match half.
        assert_eq!(err(b"\x31abc", 3), (0, ErrorKind::MatchWithoutOffset));
    }

    #[test]
    fn the_output_must_be_the_length_given() {
        assert_eq!(err(b"\x30abc", 2), (1, ErrorKind::TooLong { raw_size: 2 }));
        assert_eq!(
            err(b"\x30abc", 4),
            (
                4,
                ErrorKind::TooShort {
                    raw_size: 4,
                    found: 3
                }
            )
        );
        // A match that would pass it.
        assert_eq!(
            err(b"\x10x\x01\x00\x00", 4),
            (2, ErrorKind::TooLong { raw_size: 4 })
        );
        assert_eq!(
            err(b"", 1),
            (
                0,
                ErrorKind::UnexpectedEnd {
                    expected: "more blocks"
                }
            )
        );
    }

    #[test]
    fn a_length_past_the_limits_is_rejected_before_allocating() {
        let limits = Limits {
            max_decoded_bytes: 10,
            ..Limits::default()
        };
        assert!(matches!(
            decompress(b"\x30abc", 11, limits),
            Err(CodecError::Klz {
                offset: 0,
                kind: ErrorKind::TooManyBytes { limit: 10 }
            })
        ));
        // 4 bytes of stream cannot hold more than 255 bytes each.
        assert_eq!(
            err(b"\x10x\x01\x00", 4 * 255 + 1),
            (
                4,
                ErrorKind::UnexpectedEnd {
                    expected: "more blocks"
                }
            )
        );
        // Sizes no allocation could hold, so that reserving room before
        // either check would panic here.
        assert_eq!(
            err(b"", usize::MAX),
            (
                0,
                ErrorKind::TooManyBytes {
                    limit: Limits::default().max_decoded_bytes
                }
            )
        );
        let unlimited = Limits {
            max_decoded_bytes: usize::MAX,
            ..Limits::default()
        };
        assert!(matches!(
            decompress(b"", usize::MAX, unlimited),
            Err(CodecError::Klz {
                offset: 0,
                kind: ErrorKind::UnexpectedEnd {
                    expected: "more blocks"
                }
            })
        ));
    }

    #[test]
    fn a_length_too_large_to_count_is_rejected() {
        // Extra bytes of 255 that never end would pass `usize::MAX` only
        // after an impossibly long stream; check the arithmetic directly.
        let mut at = 0;
        let stream = [255u8; 3];
        assert_eq!(
            length(&stream, &mut at, 15, usize::MAX - 300, "a match length"),
            Err(error(0, ErrorKind::LengthTooLarge))
        );
    }

    #[test]
    fn the_encoder_writes_the_blocks_it_should() {
        assert_eq!(compress(b""), b"");
        assert_eq!(compress(b"abc"), b"\x30abc");
        // "abcd" and then the same 4 again: a match of 4 from 4 back, and
        // an empty last block.
        assert_eq!(compress(b"abcdabcd"), b"\x40abcd\x04\x00\x00");
        // 20 of the same byte: a literal, then 19 from 1 back.
        assert_eq!(compress(&[9; 20]), [0x1f, 9, 1, 0, 0, 0x00]);
    }

    #[test]
    fn error_kinds_explain_themselves() {
        assert_eq!(
            CodecError::Klz {
                offset: 2,
                kind: ErrorKind::OffsetTooFar {
                    offset: 2,
                    available: 1
                }
            }
            .to_string(),
            "KLZ error at byte 2: a match reaches 2 bytes back, but there are 1"
        );
        assert_eq!(
            ErrorKind::TooShort {
                raw_size: 4,
                found: 3
            }
            .to_string(),
            "the stream gives 3 bytes, not the 4 expected"
        );
    }
}
