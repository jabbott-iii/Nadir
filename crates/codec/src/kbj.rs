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

//! KBJ, our binary form of the JSON value tree, for cooked data: what
//! [`json::parse`](crate::json::parse) reads, written compactly and read
//! without parsing text.
//!
//! ```text
//! file  := "KBJ1" value
//! value := tag:u8 payload
//!   0x00 null | 0x01 false | 0x02 true
//!   0x03 i64 (zigzag LEB128) | 0x04 f64 (8 bytes LE)
//!   0x05 string: len:LEB128 bytes(UTF-8)
//!   0x06 array:  count:LEB128 value*
//!   0x07 object: count:LEB128 (key:string-payload value)*
//! ```
//!
//! ```
//! use nadir_codec::json::Value;
//! use nadir_codec::{Limits, kbj};
//!
//! let value = Value::Array(vec![Value::Int(-1), Value::String("hp".into())]);
//! let bytes = kbj::encode(&value, Limits::default())?;
//! assert_eq!(bytes, b"KBJ1\x06\x02\x03\x01\x05\x02hp");
//! assert_eq!(kbj::decode(&bytes, Limits::default())?, value);
//! # Ok::<(), nadir_codec::CodecError>(())
//! ```
//!
//! Every value has one encoding: [`decode`] accepts only the bytes
//! [`encode`] writes. It rejects a LEB128 number with more bytes than it
//! needs, an object with the same key twice (as the JSON parser does), and
//! a NaN other than the one [`encode`] writes for every NaN, the quiet NaN
//! `0x7ff8_0000_0000_0000`. Infinities are kept as they are, unlike in JSON
//! text, so that a save keeps its floats (ADR-0028).
//!
//! [`Limits`] bound both: nesting depth, string length, and entries per
//! array or object, as they bound the JSON parser. [`encode`] refuses what
//! [`decode`] would, so that a file written is a file that can be read
//! with the same limits. So a value survives encoding and decoding, NaN
//! payloads aside, and decoding then encoding gives back the same bytes.

use std::fmt;

use crate::json::Value;
use crate::{CodecError, Limits};

/// What is wrong with a KBJ file. See [`CodecError::Kbj`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The input does not start with `KBJ`.
    NotKbj,
    /// The file is a version of KBJ other than 1.
    UnsupportedVersion {
        /// The byte after `KBJ`.
        found: u8,
    },
    /// The input ends too soon.
    UnexpectedEnd {
        /// What should have come next.
        expected: &'static str,
    },
    /// A value starts with a tag that does not exist.
    BadTag {
        /// The tag.
        found: u8,
    },
    /// A LEB128 number takes more bytes than it needs.
    OverlongNumber,
    /// A LEB128 number is larger than 64 bits, or a length larger than
    /// this machine can count.
    NumberTooLarge,
    /// A float is a NaN other than the quiet NaN `0x7ff8_0000_0000_0000`.
    OtherNan,
    /// A string is not UTF-8.
    BadUtf8,
    /// A string is longer than [`Limits::max_string_bytes`].
    StringTooLong {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// An array or object has more than [`Limits::max_entries`] entries.
    TooManyEntries {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// Arrays and objects nest deeper than [`Limits::max_depth`].
    TooDeep {
        /// The limit that was exceeded.
        limit: u32,
    },
    /// An object has the same key twice.
    DuplicateKey {
        /// The repeated key.
        key: String,
    },
    /// Bytes follow the value.
    TrailingBytes,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotKbj => f.write_str("the file does not start with `KBJ`"),
            Self::UnsupportedVersion { found } => write!(
                f,
                "the file is KBJ version `{}`, and only version 1 is supported",
                found.escape_ascii()
            ),
            Self::UnexpectedEnd { expected } => {
                write!(f, "the file ends where {expected} should be")
            }
            Self::BadTag { found } => {
                write!(f, "a value has tag {found:#04x}, which does not exist")
            }
            Self::OverlongNumber => f.write_str("a number takes more bytes than it needs"),
            Self::NumberTooLarge => f.write_str("a number is too large"),
            Self::OtherNan => f.write_str("a float is a NaN other than the one KBJ writes"),
            Self::BadUtf8 => f.write_str("a string is not UTF-8"),
            Self::StringTooLong { limit } => {
                write!(f, "a string is longer than the limit of {limit} bytes")
            }
            Self::TooManyEntries { limit } => write!(
                f,
                "an array or object has more than the limit of {limit} entries"
            ),
            Self::TooDeep { limit } => write!(
                f,
                "arrays and objects nest deeper than the limit of {limit}"
            ),
            Self::DuplicateKey { key } => write!(f, "the key {key:?} appears twice in one object"),
            Self::TrailingBytes => f.write_str("there are bytes after the value"),
        }
    }
}

/// Why a value cannot be encoded. See [`CodecError::KbjEncode`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncodeErrorKind {
    /// An object has the same key twice.
    DuplicateKey {
        /// The repeated key.
        key: String,
    },
    /// Arrays and objects nest deeper than [`Limits::max_depth`].
    TooDeep {
        /// The limit that was exceeded.
        limit: u32,
    },
    /// An array or object has more than [`Limits::max_entries`] entries.
    TooManyEntries {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// A string is longer than [`Limits::max_string_bytes`].
    StringTooLong {
        /// The limit that was exceeded.
        limit: usize,
    },
}

impl fmt::Display for EncodeErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateKey { key } => write!(f, "the key {key:?} appears twice in one object"),
            Self::TooDeep { limit } => write!(
                f,
                "arrays and objects nest deeper than the limit of {limit}"
            ),
            Self::TooManyEntries { limit } => write!(
                f,
                "an array or object has more than the limit of {limit} entries"
            ),
            Self::StringTooLong { limit } => {
                write!(f, "a string is longer than the limit of {limit} bytes")
            }
        }
    }
}

const MAGIC: [u8; 4] = *b"KBJ1";

/// The one NaN KBJ holds: the quiet NaN with no payload.
const NAN_BITS: u64 = 0x7ff8_0000_0000_0000;

const NULL: u8 = 0x00;
const FALSE: u8 = 0x01;
const TRUE: u8 = 0x02;
const INT: u8 = 0x03;
const FLOAT: u8 = 0x04;
const STRING: u8 = 0x05;
const ARRAY: u8 = 0x06;
const OBJECT: u8 = 0x07;

/// The most entries an array or object reserves room for before it reads
/// them; it grows as they come. A count is checked against the bytes left,
/// but up to `max_depth` containers are open at once, each claiming those
/// same bytes, so reserving every claim in full could take `max_depth`
/// times the input over again.
const MOST_RESERVED: usize = 1024;

/// Objects up to this many members are checked for duplicate keys as each
/// key is read; larger ones once at the end by sorting, as the JSON parser
/// does, so that a huge object cannot make the check quadratic.
const SMALL_OBJECT: usize = 16;

/// Encodes a value as a KBJ file. Every NaN is written as the quiet NaN
/// `0x7ff8_0000_0000_0000`.
///
/// # Errors
///
/// [`CodecError::KbjEncode`] for a value [`decode`] would refuse with the
/// same `limits`: an object with the same key twice, or nesting, entries
/// or a string past a limit.
pub fn encode(value: &Value, limits: Limits) -> Result<Vec<u8>, CodecError> {
    let mut out = MAGIC.to_vec();
    write_value(&mut out, value, limits, 0)?;
    Ok(out)
}

fn refuse(kind: EncodeErrorKind) -> CodecError {
    CodecError::KbjEncode { kind }
}

/// Writes one value, which `depth` arrays and objects hold.
fn write_value(
    out: &mut Vec<u8>,
    value: &Value,
    limits: Limits,
    depth: u32,
) -> Result<(), CodecError> {
    match value {
        Value::Null => out.push(NULL),
        Value::Bool(false) => out.push(FALSE),
        Value::Bool(true) => out.push(TRUE),
        Value::Int(n) => {
            out.push(INT);
            // Zigzag: 0, -1, 1, -2, … become 0, 1, 2, 3, …
            write_leb128(out, ((n << 1) ^ (n >> 63)).cast_unsigned());
        }
        Value::Float(x) => {
            out.push(FLOAT);
            let bits = if x.is_nan() { NAN_BITS } else { x.to_bits() };
            out.extend_from_slice(&bits.to_le_bytes());
        }
        Value::String(text) => {
            out.push(STRING);
            write_string(out, text, limits)?;
        }
        Value::Array(items) => {
            enter(items.len(), limits, depth)?;
            out.push(ARRAY);
            write_leb128(out, items.len() as u64);
            for item in items {
                write_value(out, item, limits, depth + 1)?;
            }
        }
        Value::Object(members) => {
            enter(members.len(), limits, depth)?;
            if let Some(repeat) = first_repeated_key(members) {
                let key = members
                    .get(repeat)
                    .map(|(key, _)| key.clone())
                    .unwrap_or_default();
                return Err(refuse(EncodeErrorKind::DuplicateKey { key }));
            }
            out.push(OBJECT);
            write_leb128(out, members.len() as u64);
            for (key, value) in members {
                write_string(out, key, limits)?;
                write_value(out, value, limits, depth + 1)?;
            }
        }
    }
    Ok(())
}

/// Fails if a container of `len` entries may not start at `depth`.
fn enter(len: usize, limits: Limits, depth: u32) -> Result<(), CodecError> {
    if depth >= limits.max_depth {
        return Err(refuse(EncodeErrorKind::TooDeep {
            limit: limits.max_depth,
        }));
    }
    if len > limits.max_entries {
        return Err(refuse(EncodeErrorKind::TooManyEntries {
            limit: limits.max_entries,
        }));
    }
    Ok(())
}

fn write_string(out: &mut Vec<u8>, text: &str, limits: Limits) -> Result<(), CodecError> {
    if text.len() > limits.max_string_bytes {
        return Err(refuse(EncodeErrorKind::StringTooLong {
            limit: limits.max_string_bytes,
        }));
    }
    write_leb128(out, text.len() as u64);
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

fn write_leb128(out: &mut Vec<u8>, mut n: u64) {
    loop {
        let byte = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn error(offset: usize, kind: ErrorKind) -> CodecError {
    CodecError::Kbj { offset, kind }
}

/// Decodes a KBJ file.
///
/// # Errors
///
/// [`CodecError::Kbj`], with the byte offset of the problem, for a file
/// that is malformed, is not the one encoding of its value (see the module
/// documentation), or exceeds one of `limits`.
pub fn decode(bytes: &[u8], limits: Limits) -> Result<Value, CodecError> {
    match bytes.first_chunk::<4>() {
        Some(&MAGIC) => {}
        Some(&[b'K', b'B', b'J', found]) => {
            return Err(error(3, ErrorKind::UnsupportedVersion { found }));
        }
        _ if MAGIC.starts_with(bytes) => {
            return Err(error(
                bytes.len(),
                ErrorKind::UnexpectedEnd {
                    expected: "the rest of `KBJ1`",
                },
            ));
        }
        _ => return Err(error(0, ErrorKind::NotKbj)),
    }
    let mut reader = Reader {
        bytes,
        at: MAGIC.len(),
        limits,
    };
    let value = reader.value(0)?;
    if reader.at < bytes.len() {
        return Err(error(reader.at, ErrorKind::TrailingBytes));
    }
    Ok(value)
}

struct Reader<'a> {
    bytes: &'a [u8],
    /// The next byte to read.
    at: usize,
    limits: Limits,
}

impl Reader<'_> {
    fn cut_short(&self, expected: &'static str) -> CodecError {
        error(self.bytes.len(), ErrorKind::UnexpectedEnd { expected })
    }

    fn left(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }

    fn take<const N: usize>(&mut self, expected: &'static str) -> Result<[u8; N], CodecError> {
        let taken = self
            .bytes
            .get(self.at..)
            .and_then(<[u8]>::first_chunk::<N>)
            .ok_or_else(|| self.cut_short(expected))?;
        self.at += N;
        Ok(*taken)
    }

    /// Reads a LEB128 number in as few bytes as it needs.
    fn leb128(&mut self, expected: &'static str) -> Result<u64, CodecError> {
        let start = self.at;
        let mut n = 0u64;
        let mut shift = 0u32;
        loop {
            let [byte] = self.take::<1>(expected)?;
            let bits = u64::from(byte & 0x7f);
            // The tenth byte holds bit 63 alone, and any byte after it
            // nothing: a bit there is past 64.
            if (shift == 63 && bits > 1) || (shift > 63 && bits != 0) {
                return Err(error(start, ErrorKind::NumberTooLarge));
            }
            if shift < 64 {
                n |= bits << shift;
            }
            if byte & 0x80 == 0 {
                // A last byte of 0 after the first adds nothing.
                if byte == 0 && shift > 0 {
                    return Err(error(start, ErrorKind::OverlongNumber));
                }
                return Ok(n);
            }
            shift = shift.saturating_add(7);
        }
    }

    /// Reads a length or a count, which must fit a `usize`.
    fn length(&mut self, expected: &'static str) -> Result<(usize, usize), CodecError> {
        let start = self.at;
        let n = self.leb128(expected)?;
        let n = usize::try_from(n).map_err(|_| error(start, ErrorKind::NumberTooLarge))?;
        Ok((start, n))
    }

    fn string(&mut self) -> Result<String, CodecError> {
        let (start, len) = self.length("a string's length")?;
        if len > self.limits.max_string_bytes {
            return Err(error(
                start,
                ErrorKind::StringTooLong {
                    limit: self.limits.max_string_bytes,
                },
            ));
        }
        // Checked against what is left before anything is copied.
        let Some(raw) = self.bytes.get(self.at..).and_then(|rest| rest.get(..len)) else {
            return Err(self.cut_short("a string's bytes"));
        };
        let text = std::str::from_utf8(raw).map_err(|_| error(self.at, ErrorKind::BadUtf8))?;
        self.at += len;
        Ok(text.to_string())
    }

    /// Reads a count of entries, each at least `least` bytes long, and
    /// checks it against the limit and against what is left of the input.
    fn count(&mut self, least: usize, what: &'static str) -> Result<usize, CodecError> {
        let (start, count) = self.length(what)?;
        if count > self.limits.max_entries {
            return Err(error(
                start,
                ErrorKind::TooManyEntries {
                    limit: self.limits.max_entries,
                },
            ));
        }
        if count.saturating_mul(least) > self.left() {
            return Err(self.cut_short(what));
        }
        Ok(count)
    }

    /// Reads one value. `depth` is the number of arrays and objects that
    /// hold it.
    fn value(&mut self, depth: u32) -> Result<Value, CodecError> {
        let at = self.at;
        let [tag] = self.take::<1>("a value")?;
        Ok(match tag {
            NULL => Value::Null,
            FALSE => Value::Bool(false),
            TRUE => Value::Bool(true),
            INT => {
                let zigzag = self.leb128("an integer")?;
                Value::Int((zigzag >> 1).cast_signed() ^ -((zigzag & 1).cast_signed()))
            }
            FLOAT => {
                let start = self.at;
                let x = f64::from_le_bytes(self.take::<8>("a float")?);
                if x.is_nan() && x.to_bits() != NAN_BITS {
                    return Err(error(start, ErrorKind::OtherNan));
                }
                Value::Float(x)
            }
            STRING => Value::String(self.string()?),
            ARRAY | OBJECT => {
                if depth >= self.limits.max_depth {
                    return Err(error(
                        at,
                        ErrorKind::TooDeep {
                            limit: self.limits.max_depth,
                        },
                    ));
                }
                if tag == ARRAY {
                    self.array(depth)?
                } else {
                    self.object(depth)?
                }
            }
            found => return Err(error(at, ErrorKind::BadTag { found })),
        })
    }

    fn array(&mut self, depth: u32) -> Result<Value, CodecError> {
        // Each item takes a byte at least.
        let count = self.count(1, "an array's items")?;
        let mut items = Vec::with_capacity(count.min(MOST_RESERVED));
        reserved(&items);
        for _ in 0..count {
            items.push(self.value(depth + 1)?);
        }
        Ok(Value::Array(items))
    }

    fn object(&mut self, depth: u32) -> Result<Value, CodecError> {
        // Each member takes a byte for its key's length and one for its
        // value at least.
        let count = self.count(2, "an object's members")?;
        let mut members: Vec<(String, Value)> = Vec::with_capacity(count.min(MOST_RESERVED));
        reserved(&members);
        let mut key_offsets = Vec::new();
        for _ in 0..count {
            let key_at = self.at;
            let key = self.string()?;
            if members.len() < SMALL_OBJECT && members.iter().any(|(existing, _)| *existing == key)
            {
                return Err(error(key_at, ErrorKind::DuplicateKey { key }));
            }
            if count > SMALL_OBJECT {
                key_offsets.push(key_at);
            }
            let value = self.value(depth + 1)?;
            members.push((key, value));
        }
        if count > SMALL_OBJECT {
            reject_duplicate_keys(&members, &key_offsets)?;
        }
        Ok(Value::Object(members))
    }
}

/// Counts, for the tests, the room an array or object reserved before it
/// read its entries.
fn reserved<T>(entries: &Vec<T>) {
    #[cfg(test)]
    tests::RESERVED.with(|bytes| bytes.set(bytes.get() + entries.capacity() * size_of::<T>()));
    #[cfg(not(test))]
    let _ = entries;
}

/// The duplicate-key check for large objects: the repeat a reader meets
/// first, placed at its key.
fn reject_duplicate_keys(
    members: &[(String, Value)],
    key_offsets: &[usize],
) -> Result<(), CodecError> {
    let Some(repeat) = first_repeated_key(members) else {
        return Ok(());
    };
    let at = key_offsets.get(repeat).copied().unwrap_or(0);
    let key = members
        .get(repeat)
        .map(|(key, _)| key.clone())
        .unwrap_or_default();
    Err(error(at, ErrorKind::DuplicateKey { key }))
}

/// The index of the first member whose key an earlier member has: by
/// comparing for small objects, and by sorting for large ones, so that a
/// huge object cannot make the check quadratic.
fn first_repeated_key(members: &[(String, Value)]) -> Option<usize> {
    if members.len() <= SMALL_OBJECT {
        return (1..members.len())
            .find(|&i| members[..i].iter().any(|(key, _)| *key == members[i].0));
    }
    let key_of = |index: usize| members.get(index).map(|(key, _)| key.as_str());
    let mut order: Vec<usize> = (0..members.len()).collect();
    order.sort_unstable_by(|&a, &b| key_of(a).cmp(&key_of(b)).then(a.cmp(&b)));
    order
        .windows(2)
        .filter(|pair| key_of(pair[0]) == key_of(pair[1]))
        .map(|pair| pair[1])
        .min()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        /// The bytes that decoding on this thread has reserved for entries.
        pub(super) static RESERVED: Cell<usize> = const { Cell::new(0) };
    }

    fn err(bytes: &[u8]) -> (usize, ErrorKind) {
        err_with(bytes, Limits::default())
    }

    fn err_with(bytes: &[u8], limits: Limits) -> (usize, ErrorKind) {
        match decode(bytes, limits) {
            Err(CodecError::Kbj { offset, kind }) => (offset, kind),
            other => panic!("not a KBJ error: {other:?}"),
        }
    }

    /// A file: the magic, then `body`.
    fn file(body: &[u8]) -> Vec<u8> {
        [&MAGIC[..], body].concat()
    }

    fn string(text: &str) -> Value {
        Value::String(text.to_string())
    }

    #[test]
    fn values_encode_as_worked_out_by_hand() {
        let cases: Vec<(Value, &[u8])> = vec![
            (Value::Null, b"\x00"),
            (Value::Bool(false), b"\x01"),
            (Value::Bool(true), b"\x02"),
            // Zigzag: 0, -1, 1, -2, 63, -64, 64 are 0, 1, 2, 3, 126, 127,
            // 128; 128 takes two bytes, 0x80 0x01.
            (Value::Int(0), b"\x03\x00"),
            (Value::Int(-1), b"\x03\x01"),
            (Value::Int(1), b"\x03\x02"),
            (Value::Int(-2), b"\x03\x03"),
            (Value::Int(63), b"\x03\x7e"),
            (Value::Int(-64), b"\x03\x7f"),
            (Value::Int(64), b"\x03\x80\x01"),
            // The extremes take ten bytes: u64::MAX and u64::MAX - 1.
            (
                Value::Int(i64::MIN),
                b"\x03\xff\xff\xff\xff\xff\xff\xff\xff\xff\x01",
            ),
            (
                Value::Int(i64::MAX),
                b"\x03\xfe\xff\xff\xff\xff\xff\xff\xff\xff\x01",
            ),
            (Value::Float(1.5), b"\x04\x00\x00\x00\x00\x00\x00\xf8\x3f"),
            (Value::Float(-0.0), b"\x04\x00\x00\x00\x00\x00\x00\x00\x80"),
            (
                Value::Float(f64::INFINITY),
                b"\x04\x00\x00\x00\x00\x00\x00\xf0\x7f",
            ),
            (
                Value::Float(f64::NEG_INFINITY),
                b"\x04\x00\x00\x00\x00\x00\x00\xf0\xff",
            ),
            (string(""), b"\x05\x00"),
            (string("é"), b"\x05\x02\xc3\xa9"),
            (Value::Array(vec![]), b"\x06\x00"),
            (
                Value::Array(vec![Value::Null, Value::Array(vec![Value::Bool(true)])]),
                b"\x06\x02\x00\x06\x01\x02",
            ),
            (
                Value::Object(vec![("a".to_string(), Value::Int(2))]),
                b"\x07\x01\x01a\x03\x04",
            ),
        ];
        for (value, body) in cases {
            let bytes = encode(&value, Limits::default()).unwrap();
            assert_eq!(bytes, file(body), "{value:?}");
            let back = decode(&bytes, Limits::default()).unwrap();
            // -0.0 == 0.0 as floats: compare the bits too.
            assert_eq!(back, value);
            if let (Value::Float(a), Value::Float(b)) = (&back, &value) {
                assert_eq!(a.to_bits(), b.to_bits());
            }
        }
        // A string of 128 bytes has a two-byte length.
        let long = "x".repeat(128);
        assert_eq!(
            encode(&string(&long), Limits::default()).unwrap()[4..7],
            [0x05, 0x80, 0x01]
        );
    }

    #[test]
    fn every_nan_is_written_as_one_and_only_that_one_is_read() {
        let quiet = b"\x04\x00\x00\x00\x00\x00\x00\xf8\x7f";
        for bits in [
            NAN_BITS,
            0xfff8_0000_0000_0000,
            0x7ff0_0000_0000_0001,
            0x7fff_ffff_ffff_ffff,
        ] {
            let x = f64::from_bits(bits);
            assert!(x.is_nan());
            assert_eq!(
                encode(&Value::Float(x), Limits::default()).unwrap(),
                file(quiet)
            );
        }
        match decode(&file(quiet), Limits::default()) {
            Ok(Value::Float(x)) => assert_eq!(x.to_bits(), NAN_BITS),
            other => panic!("{other:?}"),
        }
        for bits in [0xfff8_0000_0000_0000u64, 0x7ff8_0000_0000_0001] {
            let mut body = vec![FLOAT];
            body.extend_from_slice(&bits.to_le_bytes());
            assert_eq!(err(&file(&body)), (5, ErrorKind::OtherNan));
        }
    }

    fn refusal(value: &Value, limits: Limits) -> EncodeErrorKind {
        match encode(value, limits) {
            Err(CodecError::KbjEncode { kind }) => kind,
            other => panic!("not a KBJ encoding error: {other:?}"),
        }
    }

    #[test]
    fn the_encoder_refuses_what_the_decoder_would() {
        let pair = |key: &str| (key.to_string(), Value::Null);
        assert_eq!(
            refusal(
                &Value::Object(vec![pair("a"), pair("b"), pair("a")]),
                Limits::default()
            ),
            EncodeErrorKind::DuplicateKey {
                key: "a".to_string()
            }
        );
        for repeated in ["k7", "k0"] {
            let large: Vec<(String, Value)> = (0..40)
                .map(|i| pair(&format!("k{i}")))
                .chain([pair(repeated)])
                .collect();
            assert_eq!(
                refusal(&Value::Object(large), Limits::default()),
                EncodeErrorKind::DuplicateKey {
                    key: repeated.to_string()
                }
            );
        }
        let limits = Limits {
            max_depth: 1,
            max_entries: 2,
            max_string_bytes: 3,
            ..Limits::default()
        };
        let nested = Value::Array(vec![Value::Array(vec![])]);
        assert_eq!(
            refusal(&nested, limits),
            EncodeErrorKind::TooDeep { limit: 1 }
        );
        let three = Value::Array(vec![Value::Null; 3]);
        assert_eq!(
            refusal(&three, limits),
            EncodeErrorKind::TooManyEntries { limit: 2 }
        );
        assert_eq!(
            refusal(&string("four"), limits),
            EncodeErrorKind::StringTooLong { limit: 3 }
        );
        let key = Value::Object(vec![pair("four")]);
        assert_eq!(
            refusal(&key, limits),
            EncodeErrorKind::StringTooLong { limit: 3 }
        );
        // Exactly at the limits is fine, and decodes with them.
        let at_limits = Value::Array(vec![string("abc"), Value::Null]);
        let bytes = encode(&at_limits, limits).unwrap();
        assert_eq!(decode(&bytes, limits).unwrap(), at_limits);
        // A value nested as deep as the stack would not allow is refused,
        // not followed down.
        let mut deep = Value::Null;
        for _ in 0..100 {
            deep = Value::Array(vec![deep]);
        }
        assert_eq!(
            refusal(&deep, Limits::default()),
            EncodeErrorKind::TooDeep { limit: 64 }
        );
    }

    #[test]
    fn the_magic_and_version_are_checked() {
        assert_eq!(err(b"JSON"), (0, ErrorKind::NotKbj));
        assert_eq!(
            err(b"KBJ2\x00"),
            (3, ErrorKind::UnsupportedVersion { found: b'2' })
        );
        assert_eq!(
            err(b"KB"),
            (
                2,
                ErrorKind::UnexpectedEnd {
                    expected: "the rest of `KBJ1`"
                }
            )
        );
        assert_eq!(
            err(b"KBJ1"),
            (
                4,
                ErrorKind::UnexpectedEnd {
                    expected: "a value"
                }
            )
        );
    }

    #[test]
    fn numbers_must_take_no_more_bytes_than_they_need() {
        // 0 as two bytes, and 1 as three.
        assert_eq!(err(&file(b"\x03\x80\x00")), (5, ErrorKind::OverlongNumber));
        assert_eq!(
            err(&file(b"\x03\x81\x80\x00")),
            (5, ErrorKind::OverlongNumber)
        );
        // Past 64 bits: a tenth byte above 1, or bits in an eleventh.
        assert_eq!(
            err(&file(b"\x03\xff\xff\xff\xff\xff\xff\xff\xff\xff\x02")),
            (5, ErrorKind::NumberTooLarge)
        );
        assert_eq!(
            err(&file(b"\x03\xff\xff\xff\xff\xff\xff\xff\xff\xff\x81\x01")),
            (5, ErrorKind::NumberTooLarge)
        );
        // Within 64 bits but longer than ten bytes: u64::MAX, and 0, in
        // eleven and more.
        assert_eq!(
            err(&file(b"\x03\xff\xff\xff\xff\xff\xff\xff\xff\xff\x81\x00")),
            (5, ErrorKind::OverlongNumber)
        );
        let mut zero = vec![INT];
        zero.extend([0x80; 40]);
        zero.push(0);
        assert_eq!(err(&file(&zero)), (5, ErrorKind::OverlongNumber));
        assert_eq!(
            err(&file(b"\x03\x80")),
            (
                6,
                ErrorKind::UnexpectedEnd {
                    expected: "an integer"
                }
            )
        );
    }

    #[test]
    fn strings_are_checked() {
        assert_eq!(
            err(&file(b"\x05\x02\xc3")),
            (
                7,
                ErrorKind::UnexpectedEnd {
                    expected: "a string's bytes"
                }
            )
        );
        assert_eq!(err(&file(b"\x05\x01\xff")), (6, ErrorKind::BadUtf8));
        let limits = Limits {
            max_string_bytes: 2,
            ..Limits::default()
        };
        assert_eq!(
            err_with(&file(b"\x05\x03abc"), limits),
            (5, ErrorKind::StringTooLong { limit: 2 })
        );
        // A key is a string too.
        assert_eq!(err(&file(b"\x07\x01\x01\xff\x00")), (7, ErrorKind::BadUtf8));
    }

    #[test]
    fn counts_are_checked_before_allocating() {
        // A million items claimed by a file of a few bytes.
        assert_eq!(
            err(&file(b"\x06\xc0\x84\x3d\x00")),
            (
                9,
                ErrorKind::UnexpectedEnd {
                    expected: "an array's items"
                }
            )
        );
        // Two members need four bytes at least.
        assert_eq!(
            err(&file(b"\x07\x02\x00\x00\x00")),
            (
                9,
                ErrorKind::UnexpectedEnd {
                    expected: "an object's members"
                }
            )
        );
        let limits = Limits {
            max_entries: 1,
            ..Limits::default()
        };
        assert_eq!(
            err_with(&file(b"\x06\x02\x00\x00"), limits),
            (5, ErrorKind::TooManyEntries { limit: 1 })
        );
    }

    #[test]
    fn nesting_is_bounded() {
        let limits = Limits {
            max_depth: 2,
            ..Limits::default()
        };
        assert!(decode(&file(b"\x06\x01\x06\x00"), limits).is_ok());
        assert_eq!(
            err_with(&file(b"\x06\x01\x06\x01\x07\x00"), limits),
            (8, ErrorKind::TooDeep { limit: 2 })
        );
        // Deeper than the stack would allow, with the default limit.
        let mut body = [ARRAY, 1].repeat(100_000);
        body.push(NULL);
        assert_eq!(
            err(&file(&body)),
            (4 + 2 * 64, ErrorKind::TooDeep { limit: 64 })
        );
        // The default allows 64 levels, as JSON's does, and not 65.
        let nested = |levels: usize| [[ARRAY, 1].repeat(levels), vec![NULL]].concat();
        assert!(decode(&file(&nested(64)), Limits::default()).is_ok());
        assert_eq!(
            err(&file(&nested(65))),
            (4 + 2 * 64, ErrorKind::TooDeep { limit: 64 })
        );
    }

    #[test]
    fn what_is_reserved_does_not_grow_with_the_nesting() {
        // 64 levels, arrays and objects in turn, each claiming as many
        // entries as the input could hold, and the innermost holding them.
        // Reserving each claim in full would reserve about 1.9 GB for this
        // 1 MB file; capped, it reserves about 3 MB.
        let mut body = Vec::new();
        for level in 0..63 {
            if level % 2 == 0 {
                body.push(ARRAY);
                write_leb128(&mut body, 1_000_000);
            } else {
                body.push(OBJECT);
                write_leb128(&mut body, 500_000);
                body.push(0);
            }
        }
        body.push(ARRAY);
        write_leb128(&mut body, 1_000_000);
        body.extend(std::iter::repeat_n(NULL, 1_000_000));
        // The innermost array is whole; the array around it then ends too
        // soon.
        RESERVED.with(|bytes| bytes.set(0));
        assert_eq!(
            err(&file(&body)),
            (
                MAGIC.len() + body.len(),
                ErrorKind::UnexpectedEnd {
                    expected: "a value"
                }
            )
        );
        let most = 64 * MOST_RESERVED * size_of::<(String, Value)>();
        let reserved = RESERVED.with(Cell::get);
        assert!(reserved <= most, "{reserved} bytes reserved");
    }

    #[test]
    fn keys_may_appear_once_in_an_object() {
        assert_eq!(
            err(&file(b"\x07\x02\x01a\x00\x01a\x00")),
            (
                9,
                ErrorKind::DuplicateKey {
                    key: "a".to_string()
                }
            )
        );
        // An object of null members, and where the repeat is reported:
        // its key's length byte, then the key.
        let repeat = |keys: &[String]| {
            let mut bytes = file(&[OBJECT, keys.len() as u8]);
            for key in keys {
                bytes.push(key.len() as u8);
                bytes.extend_from_slice(key.as_bytes());
                bytes.push(NULL);
            }
            match err(&bytes) {
                (offset, ErrorKind::DuplicateKey { key }) => {
                    assert_eq!(&bytes[offset + 1..offset + 1 + key.len()], key.as_bytes());
                    (offset, key)
                }
                other => panic!("{other:?}"),
            }
        };
        let keys = |names: &[&str]| -> Vec<String> {
            names.iter().map(|name| (*name).to_string()).collect()
        };
        let numbered = |count: usize| (0..count).map(|i| format!("k{i}")).collect::<Vec<_>>();
        // A small object is checked as each key is read, past the first
        // two keys too: the third `b`, at 4 + 2 + 3 × 3.
        assert_eq!(
            repeat(&keys(&["a", "b", "c", "b", "d"])),
            (15, "b".to_string())
        );
        assert_eq!(repeat(&keys(&["a", "b", "c", "a"])).1, "a");
        // 16 members are still a small object.
        let mut sixteen = numbered(15);
        sixteen.push("k0".to_string());
        assert_eq!(repeat(&sixteen).1, "k0");
        // In a large object, the check sorts at the end, and the repeat a
        // reader meets first is reported, of the first key too.
        for (extra, first) in [
            (["k3", "k1"], "k3"),
            (["k1", "k0"], "k1"),
            (["k0", "k39"], "k0"),
        ] {
            let mut large = numbered(40);
            large.extend(keys(&extra));
            assert_eq!(repeat(&large).1, first, "{extra:?}");
        }
    }

    #[test]
    fn tags_and_the_end_are_checked() {
        assert_eq!(err(&file(b"\x08")), (4, ErrorKind::BadTag { found: 8 }));
        assert_eq!(err(&file(b"\x00\x00")), (5, ErrorKind::TrailingBytes));
        assert_eq!(
            err(&file(b"\x04\x00\x00")),
            (
                7,
                ErrorKind::UnexpectedEnd {
                    expected: "a float"
                }
            )
        );
    }

    #[test]
    fn error_kinds_explain_themselves() {
        assert_eq!(
            CodecError::Kbj {
                offset: 3,
                kind: ErrorKind::UnsupportedVersion { found: b'2' }
            }
            .to_string(),
            "KBJ error at byte 3: the file is KBJ version `2`, and only version 1 is supported"
        );
        assert_eq!(
            ErrorKind::BadTag { found: 8 }.to_string(),
            "a value has tag 0x08, which does not exist"
        );
    }
}
