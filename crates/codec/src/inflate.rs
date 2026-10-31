//! Inflate: the decompressing half of DEFLATE (RFC 1951), and the zlib
//! wrapper around it (RFC 1950). The cook tool reads PNG image data with it;
//! the game does not use it.
//!
//! [`zlib`] reads a zlib stream and [`raw`] a bare DEFLATE stream, each
//! into a new `Vec` no longer than [`Limits::max_decoded_bytes`].
//!
//! ```
//! use nadir_codec::{Limits, inflate};
//!
//! // "hello" in a zlib stream of one fixed-Huffman block.
//! let stream = [0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00, 0x06, 0x2c, 0x02, 0x15];
//! assert_eq!(inflate::zlib(&stream, Limits::default())?, b"hello");
//! # Ok::<(), nadir_codec::CodecError>(())
//! ```
//!
//! It accepts the streams zlib 1.3 decodes to their end, given its largest
//! window, and rejects the rest. As zlib does, it:
//!
//! - rejects a set of code lengths that is over-subscribed, or incomplete
//!   unless it is a single code of one bit (a code-length code may never
//!   be incomplete), and a literal/length code without symbol 256; a
//!   distance code may have no codes at all, for a block of literals only
//! - rejects more than 286 literal/length codes or 30 distance codes, and
//!   the symbols that do not exist (286, 287, distances 30 and 31)
//! - rejects a distance further back than the output so far: there is no
//!   preset dictionary, and a zlib header that asks for one is rejected
//! - decodes length code 284 with extra bits 31 as 258
//! - checks the zlib header's method, window size (at most 32 KiB) and
//!   check bits, and the Adler-32 at the end
//!
//! What it rejects, although zlib accepts it:
//!
//! - bytes after the end of the stream, which zlib leaves unread
//! - output past [`Limits::max_decoded_bytes`]
//!
//! Like zlib given its largest window, it does not check that every
//! distance fits the window the zlib header declares. libpng asks zlib for
//! the header's window instead, and rejects a PNG whose image data reaches
//! further back.

use std::fmt;

use crate::{CodecError, Limits, adler32};

/// Which code a problem with a set of code lengths is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Code {
    /// The code that the lengths of the other two are written in.
    CodeLengths,
    /// The literal/length code.
    LiteralLength,
    /// The distance code.
    Distance,
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::CodeLengths => "code-length code",
            Self::LiteralLength => "literal/length code",
            Self::Distance => "distance code",
        })
    }
}

/// What is wrong with a DEFLATE or zlib stream. See [`CodecError::Inflate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The input ends too soon.
    UnexpectedEnd {
        /// What should have come next.
        expected: &'static str,
    },
    /// A block has type 3, which does not exist.
    BadBlockType,
    /// A stored block's length and its complement do not match.
    BadStoredLength,
    /// A block has more than 286 literal/length codes or 30 distance codes.
    TooManyCodes,
    /// A block's code lengths give more codes than the bits allow.
    OverSubscribed {
        /// The code they are for.
        code: Code,
    },
    /// A block's code lengths leave codes unused, which only a code of a
    /// single one-bit code may.
    Incomplete {
        /// The code they are for.
        code: Code,
    },
    /// A code length repeats the one before it, but it is the first.
    RepeatWithoutLength,
    /// A code length repeats past the last code.
    RepeatTooLong,
    /// The literal/length code has no code for the end of the block.
    NoEndOfBlock,
    /// The bits are not a code, or are the code of a symbol that does not
    /// exist.
    BadCode {
        /// The code they were read with.
        code: Code,
    },
    /// A match reaches back past the start of the output.
    DistanceTooFar {
        /// How far back it reaches.
        distance: usize,
        /// How many bytes of output there are.
        available: usize,
    },
    /// The output would be longer than the limit.
    TooMuchOutput {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// The zlib header's check bits are wrong.
    BadHeaderCheck,
    /// The zlib header names a method other than 8, DEFLATE.
    BadMethod {
        /// The method in the header.
        found: u8,
    },
    /// The zlib header asks for a window larger than 32 KiB.
    BadWindow {
        /// The header's window size field: the window is `2^(found + 8)`.
        found: u8,
    },
    /// The zlib header asks for a preset dictionary.
    PresetDictionary,
    /// The Adler-32 at the end of a zlib stream does not match the output.
    BadChecksum {
        /// The checksum in the stream.
        stored: u32,
        /// The checksum of the output.
        computed: u32,
    },
    /// Bytes follow the end of the stream.
    TrailingBytes,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd { expected } => {
                write!(f, "the stream ends where {expected} should be")
            }
            Self::BadBlockType => f.write_str("a block has type 3, which does not exist"),
            Self::BadStoredLength => {
                f.write_str("a stored block's length and its complement do not match")
            }
            Self::TooManyCodes => {
                f.write_str("a block has more than 286 literal/length codes or 30 distance codes")
            }
            Self::OverSubscribed { code } => {
                write!(
                    f,
                    "the {code}'s lengths give more codes than its bits allow"
                )
            }
            Self::Incomplete { code } => write!(f, "the {code}'s lengths leave codes unused"),
            Self::RepeatWithoutLength => {
                f.write_str("the first code length repeats a length before it")
            }
            Self::RepeatTooLong => f.write_str("a code length repeats past the last code"),
            Self::NoEndOfBlock => {
                f.write_str("the literal/length code has no code for the end of the block")
            }
            Self::BadCode { code } => write!(f, "the bits are no symbol of the {code}"),
            Self::DistanceTooFar {
                distance,
                available,
            } => write!(
                f,
                "a match reaches {distance} bytes back, but there are {available}"
            ),
            Self::TooMuchOutput { limit } => {
                write!(
                    f,
                    "the output would be longer than the limit of {limit} bytes"
                )
            }
            Self::BadHeaderCheck => f.write_str("the zlib header's check bits are wrong"),
            Self::BadMethod { found } => {
                write!(f, "the zlib header names method {found}, not 8")
            }
            Self::BadWindow { found } => write!(
                f,
                "the zlib header's window size is {found}, more than 7 (32 KiB)"
            ),
            Self::PresetDictionary => f.write_str("the zlib header asks for a preset dictionary"),
            Self::BadChecksum { stored, computed } => write!(
                f,
                "the stream's Adler-32 is {stored:#010x}, but its output's is {computed:#010x}"
            ),
            Self::TrailingBytes => f.write_str("there are bytes after the end of the stream"),
        }
    }
}

/// A problem in a stream: the byte offset of the item at fault in the input,
/// and what is wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fault {
    pub(crate) offset: usize,
    pub(crate) kind: ErrorKind,
}

impl Fault {
    const fn new(offset: usize, kind: ErrorKind) -> Self {
        Self { offset, kind }
    }
}

impl From<Fault> for CodecError {
    fn from(Fault { offset, kind }: Fault) -> Self {
        Self::Inflate { offset, kind }
    }
}

/// Decompresses a zlib stream (RFC 1950).
///
/// # Errors
///
/// [`CodecError::Inflate`], with the byte offset of the problem, for a
/// stream that is malformed, is followed by more bytes, or would decompress
/// to more than [`Limits::max_decoded_bytes`].
pub fn zlib(bytes: &[u8], limits: Limits) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::new();
    let end = zlib_into(bytes, &mut out, limits.max_decoded_bytes)?;
    whole(bytes, end)?;
    Ok(out)
}

/// Decompresses a bare DEFLATE stream (RFC 1951). The bits after its last
/// block, up to the end of that byte, are ignored.
///
/// # Errors
///
/// [`CodecError::Inflate`], with the byte offset of the problem, for a
/// stream that is malformed, is followed by more bytes, or would decompress
/// to more than [`Limits::max_decoded_bytes`].
pub fn raw(bytes: &[u8], limits: Limits) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::new();
    let end = raw_into(bytes, 0, &mut out, limits.max_decoded_bytes)?;
    whole(bytes, end)?;
    Ok(out)
}

/// Fails if the stream that ended at `end` is not the whole input.
fn whole(bytes: &[u8], end: usize) -> Result<(), Fault> {
    if end < bytes.len() {
        return Err(Fault::new(end, ErrorKind::TrailingBytes));
    }
    Ok(())
}

/// Decompresses the zlib stream at the start of `bytes`, appending to `out`,
/// which may grow to `cap` bytes. Returns where the stream ends.
pub(crate) fn zlib_into(bytes: &[u8], out: &mut Vec<u8>, cap: usize) -> Result<usize, Fault> {
    let Some(&[cmf, flg]) = bytes.first_chunk::<2>() else {
        return Err(cut_short(bytes, "the zlib header"));
    };
    // In zlib's order.
    if ((u16::from(cmf) << 8) | u16::from(flg)) % 31 != 0 {
        return Err(Fault::new(0, ErrorKind::BadHeaderCheck));
    }
    if cmf & 0x0f != 8 {
        return Err(Fault::new(0, ErrorKind::BadMethod { found: cmf & 0x0f }));
    }
    if cmf >> 4 > 7 {
        return Err(Fault::new(0, ErrorKind::BadWindow { found: cmf >> 4 }));
    }
    if flg & 0x20 != 0 {
        return Err(Fault::new(1, ErrorKind::PresetDictionary));
    }
    let start = out.len();
    let end = raw_into(bytes, 2, out, cap)?;
    let Some(&trailer) = bytes.get(end..).and_then(<[u8]>::first_chunk::<4>) else {
        return Err(cut_short(bytes, "the Adler-32"));
    };
    let stored = u32::from_be_bytes(trailer);
    let computed = adler32::checksum(out.get(start..).unwrap_or_default());
    if stored != computed {
        return Err(Fault::new(end, ErrorKind::BadChecksum { stored, computed }));
    }
    Ok(end + 4)
}

/// Decompresses the DEFLATE stream that starts at `bytes[start]`, appending
/// to `out`, which may grow to `cap` bytes. Matches may reach back only to
/// the start of this stream's output. Returns where the stream ends: the
/// byte after the one that holds its last bit.
pub(crate) fn raw_into(
    bytes: &[u8],
    start: usize,
    out: &mut Vec<u8>,
    cap: usize,
) -> Result<usize, Fault> {
    let mut inflater = Inflater {
        bits: Bits::new(bytes, start),
        out,
        first: 0,
        cap,
        fixed: None,
    };
    inflater.first = inflater.out.len();
    loop {
        let header = inflater.bits.offset();
        let last = inflater.bits.take(1, "a block header")? == 1;
        match inflater.bits.take(2, "a block header")? {
            0 => inflater.stored()?,
            1 => {
                let fixed = inflater.fixed.take().unwrap_or_else(fixed_codes);
                inflater.codes(&fixed.0, &fixed.1)?;
                inflater.fixed = Some(fixed);
            }
            2 => {
                let (literals, distances) = inflater.dynamic(header)?;
                inflater.codes(&literals, &distances)?;
            }
            _ => return Err(Fault::new(header, ErrorKind::BadBlockType)),
        }
        if last {
            inflater.bits.align();
            return Ok(inflater.bits.offset());
        }
    }
}

/// The state of one stream being decompressed.
struct Inflater<'a, 'o> {
    bits: Bits<'a>,
    out: &'o mut Vec<u8>,
    /// Where this stream's output starts in `out`.
    first: usize,
    cap: usize,
    /// The fixed codes, once a fixed block has needed them.
    fixed: Option<(Huffman, Huffman)>,
}

impl Inflater<'_, '_> {
    /// Copies a stored block.
    fn stored(&mut self) -> Result<(), Fault> {
        self.bits.align();
        let at = self.bits.offset();
        let len = self.bits.take(16, "a stored block's length")?;
        let complement = self.bits.take(16, "a stored block's length")?;
        if len != !complement & 0xffff {
            return Err(Fault::new(at, ErrorKind::BadStoredLength));
        }
        let from = self.bits.unload();
        let Some(data) = self
            .bits
            .bytes
            .get(from..)
            .and_then(|rest| rest.get(..len as usize))
        else {
            return Err(cut_short(self.bits.bytes, "a stored block's data"));
        };
        self.make_room(data.len(), from)?;
        self.out.extend_from_slice(data);
        self.bits.next = from + data.len();
        Ok(())
    }

    /// Reads a dynamic block's codes, whose header starts at `header`.
    fn dynamic(&mut self, header: usize) -> Result<(Huffman, Huffman), Fault> {
        let bits = &mut self.bits;
        let literals = bits.take(5, "a dynamic block's header")? as usize + 257;
        let distances = bits.take(5, "a dynamic block's header")? as usize + 1;
        let code_lengths = bits.take(4, "a dynamic block's header")? as usize + 4;
        if literals > 286 || distances > 30 {
            return Err(Fault::new(header, ErrorKind::TooManyCodes));
        }
        let mut lengths = [0u8; 19];
        for &symbol in CODE_LENGTH_ORDER.iter().take(code_lengths) {
            if let Some(length) = lengths.get_mut(symbol) {
                *length = bits.take(3, "a code-length code length")? as u8;
            }
        }
        let code =
            Huffman::new(&lengths, Code::CodeLengths).map_err(|kind| Fault::new(header, kind))?;

        let count = literals + distances;
        let mut lengths = [0u8; 286 + 30];
        let mut filled = 0;
        while filled < count {
            let at = bits.offset();
            let (length, repeat) = match code.decode(bits)? {
                symbol @ 0..=15 => (symbol as u8, 1),
                16 => {
                    let Some(&previous) = filled.checked_sub(1).and_then(|i| lengths.get(i)) else {
                        return Err(Fault::new(at, ErrorKind::RepeatWithoutLength));
                    };
                    (previous, 3 + bits.take(2, "a repeat count")? as usize)
                }
                17 => (0, 3 + bits.take(3, "a repeat count")? as usize),
                _ => (0, 11 + bits.take(7, "a repeat count")? as usize),
            };
            let Some(run) = lengths
                .get_mut(filled..filled + repeat)
                .filter(|_| filled + repeat <= count)
            else {
                return Err(Fault::new(at, ErrorKind::RepeatTooLong));
            };
            run.fill(length);
            filled += repeat;
        }
        let (literal_lengths, distance_lengths) = lengths.split_at(literals);
        if literal_lengths.get(256) == Some(&0) {
            return Err(Fault::new(header, ErrorKind::NoEndOfBlock));
        }
        let at_header = |kind| Fault::new(header, kind);
        let literal = Huffman::new(literal_lengths, Code::LiteralLength).map_err(at_header)?;
        let distance = Huffman::new(
            distance_lengths.get(..distances).unwrap_or_default(),
            Code::Distance,
        )
        .map_err(at_header)?;
        Ok((literal, distance))
    }

    /// Decodes a block's literals and matches with its codes, up to and
    /// including the end of the block.
    fn codes(&mut self, literals: &Huffman, distances: &Huffman) -> Result<(), Fault> {
        loop {
            let at = self.bits.offset();
            let symbol = literals.decode(&mut self.bits)?;
            let index = match symbol {
                0..=255 => {
                    self.make_room(1, at)?;
                    self.out.push(symbol as u8);
                    continue;
                }
                256 => return Ok(()),
                _ => usize::from(symbol - 257),
            };
            let (Some(&base), Some(&extra)) = (LENGTH_BASE.get(index), LENGTH_EXTRA.get(index))
            else {
                return Err(Fault::new(
                    at,
                    ErrorKind::BadCode {
                        code: Code::LiteralLength,
                    },
                ));
            };
            let length =
                usize::from(base) + self.bits.take(extra, "a length's extra bits")? as usize;
            let at_distance = self.bits.offset();
            let index = usize::from(distances.decode(&mut self.bits)?);
            let (Some(&base), Some(&extra)) = (DISTANCE_BASE.get(index), DISTANCE_EXTRA.get(index))
            else {
                return Err(Fault::new(
                    at_distance,
                    ErrorKind::BadCode {
                        code: Code::Distance,
                    },
                ));
            };
            let distance =
                usize::from(base) + self.bits.take(extra, "a distance's extra bits")? as usize;
            let available = self.out.len() - self.first;
            if distance > available {
                return Err(Fault::new(
                    at,
                    ErrorKind::DistanceTooFar {
                        distance,
                        available,
                    },
                ));
            }
            self.make_room(length, at)?;
            // A match may overlap what it writes. Each pass copies all
            // that has been written from its start, which is the pattern
            // repeated, so the copies double: a run of 258 at distance 1
            // takes 9 passes, not 258.
            let from = self.out.len() - distance;
            let mut left = length;
            while left > 0 {
                let part = left.min(self.out.len() - from);
                self.out.extend_from_within(from..from + part);
                left -= part;
            }
        }
    }

    /// Makes room in the output for `extra` more bytes, which the item at
    /// `at` gives, or fails if they would take it past the cap. It never
    /// allocates past the cap.
    fn make_room(&mut self, extra: usize, at: usize) -> Result<(), Fault> {
        let needed = self
            .out
            .len()
            .checked_add(extra)
            .filter(|&needed| needed <= self.cap)
            .ok_or_else(|| Fault::new(at, ErrorKind::TooMuchOutput { limit: self.cap }))?;
        if needed > self.out.capacity() {
            let target = needed
                .max(self.out.capacity().saturating_mul(2))
                .max(MIN_GROWTH)
                .min(self.cap);
            self.out.reserve_exact(target - self.out.len());
        }
        Ok(())
    }
}

/// The least the output grows by, so that short outputs do not grow a byte
/// at a time.
const MIN_GROWTH: usize = 4096;

/// The order in which a dynamic block lists the code-length code's lengths.
const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// The shortest length of each length code, 257 to 285.
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];

/// How many extra bits follow each length code.
const LENGTH_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];

/// The shortest distance of each distance code, 0 to 29.
const DISTANCE_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];

/// How many extra bits follow each distance code.
const DISTANCE_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// The codes of a fixed block: literal/lengths 0–143 in 8 bits, 144–255 in
/// 9, 256–279 in 7 and 280–287 in 8; 32 distances in 5 bits. Symbols 286
/// and 287 and distances 30 and 31 have codes but do not exist.
fn fixed_codes() -> (Huffman, Huffman) {
    let mut lengths = [8u8; 288];
    lengths[144..256].fill(9);
    lengths[256..280].fill(7);
    // Both are complete codes, so they need no checking.
    (
        Huffman::build(&lengths, Code::LiteralLength),
        Huffman::build(&[5; 32], Code::Distance),
    )
}

/// Bits from the input, least significant first, as DEFLATE packs them.
struct Bits<'a> {
    bytes: &'a [u8],
    /// The next byte to load.
    next: usize,
    /// Loaded bits, the next one lowest. Bits above `count` are 0.
    buffer: u64,
    count: u32,
}

impl<'a> Bits<'a> {
    const fn new(bytes: &'a [u8], start: usize) -> Self {
        Self {
            bytes,
            next: start,
            buffer: 0,
            count: 0,
        }
    }

    fn refill(&mut self) {
        while self.count <= 56 {
            let Some(&byte) = self.bytes.get(self.next) else {
                return;
            };
            self.buffer |= u64::from(byte) << self.count;
            self.next += 1;
            self.count += 8;
        }
    }

    /// The offset of the byte that holds the next bit.
    fn offset(&self) -> usize {
        self.next - (self.count as usize).div_ceil(8)
    }

    /// Takes the next `n` bits, at most 16, or fails with what they should
    /// have been.
    fn take(&mut self, n: u32, expected: &'static str) -> Result<u32, Fault> {
        if self.count < n {
            self.refill();
            if self.count < n {
                return Err(cut_short(self.bytes, expected));
            }
        }
        let value = (self.buffer & ((1 << n) - 1)) as u32;
        self.consume(n);
        Ok(value)
    }

    fn consume(&mut self, n: u32) {
        self.buffer >>= n;
        self.count -= n;
    }

    /// Skips to the next byte boundary.
    fn align(&mut self) {
        self.consume(self.count % 8);
    }

    /// Puts the loaded bytes back, at a byte boundary, and returns the
    /// offset of the next byte.
    fn unload(&mut self) -> usize {
        self.next = self.offset();
        self.buffer = 0;
        self.count = 0;
        self.next
    }
}

/// How many bits the lookup table of a code covers. Codes no longer than
/// this, which include every fixed code, decode in one lookup.
const FAST_BITS: u32 = 9;

/// The longest code DEFLATE allows.
const MAX_BITS: usize = 15;

/// A canonical Huffman code, read from its code lengths.
#[derive(Clone)]
struct Huffman {
    /// For each value of the next [`FAST_BITS`] bits: the symbol `<< 4`
    /// and the length of the code they start with, or 0 if that code is
    /// longer or there is none.
    fast: [u16; 1 << FAST_BITS],
    /// How many codes have each length.
    counts: [u16; MAX_BITS + 1],
    /// How many codes there are.
    total: u16,
    /// The symbols, shortest code first, and in order within a length.
    symbols: [u16; 288],
    code: Code,
}

impl Huffman {
    /// Builds the code with these lengths, one per symbol, 0 for a symbol
    /// without a code, after checking them as zlib does.
    fn new(lengths: &[u8], code: Code) -> Result<Self, ErrorKind> {
        let counts = count_lengths(lengths);
        let longest = counts.iter().rposition(|&count| count > 0).unwrap_or(0);
        if longest == 0 {
            // No codes at all. zlib allows that for a distance code: a
            // block of literals only, in which any distance is an error.
            // A code-length code without codes cannot give the end of
            // block a length, which zlib finds later; a literal/length
            // code without codes has no end of block, found earlier.
            if code == Code::CodeLengths {
                return Err(ErrorKind::Incomplete { code });
            }
        } else {
            let mut left = 1i32;
            for &count in &counts[1..] {
                left = (left << 1) - i32::from(count);
                if left < 0 {
                    return Err(ErrorKind::OverSubscribed { code });
                }
            }
            if left > 0 && (code == Code::CodeLengths || longest != 1) {
                return Err(ErrorKind::Incomplete { code });
            }
        }
        Ok(Self::build(lengths, code))
    }

    /// Builds the code with these lengths, which must not be
    /// over-subscribed.
    fn build(lengths: &[u8], code: Code) -> Self {
        let counts = count_lengths(lengths);
        let mut offsets = [0u16; MAX_BITS + 2];
        for length in 1..=MAX_BITS {
            offsets[length + 1] = offsets[length] + counts[length];
        }
        let mut symbols = [0u16; 288];
        let mut fast = [0u16; 1 << FAST_BITS];
        let mut next_code = [0u32; MAX_BITS + 1];
        let mut first = 0u32;
        for length in 1..=MAX_BITS {
            first = (first + u32::from(counts[length - 1])) << 1;
            next_code[length] = first;
        }
        for (symbol, &length) in lengths.iter().enumerate() {
            let length = usize::from(length);
            if length == 0 || length > MAX_BITS {
                continue;
            }
            let slot = &mut offsets[length];
            if let Some(entry) = symbols.get_mut(usize::from(*slot)) {
                *entry = symbol as u16;
            }
            *slot += 1;
            let canonical = next_code[length];
            next_code[length] += 1;
            if length <= FAST_BITS as usize {
                // The bits arrive first bit first, so the table is indexed
                // by the code reversed.
                let reversed = canonical.reverse_bits() >> (32 - length);
                let entry = (symbol as u16) << 4 | length as u16;
                for index in (reversed as usize..fast.len()).step_by(1 << length) {
                    fast[index] = entry;
                }
            }
        }
        Self {
            fast,
            counts,
            total: offsets[MAX_BITS + 1],
            symbols,
            code,
        }
    }

    /// Reads one symbol.
    fn decode(&self, bits: &mut Bits<'_>) -> Result<u16, Fault> {
        if bits.count < MAX_BITS as u32 {
            bits.refill();
        }
        let peek = bits.buffer as u32;
        let entry = self.fast[(peek & ((1 << FAST_BITS) - 1)) as usize];
        let (symbol, length) = if entry != 0 {
            (entry >> 4, u32::from(entry & 0x0f))
        } else {
            self.decode_slowly(peek, bits)?
        };
        if length > bits.count {
            return Err(cut_short(bits.bytes, "a code"));
        }
        bits.consume(length);
        Ok(symbol)
    }

    /// Reads a code longer than the lookup table covers, or finds that the
    /// bits are no code, one bit at a time as `puff.c` does.
    fn decode_slowly(&self, peek: u32, bits: &Bits<'_>) -> Result<(u16, u32), Fault> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for length in 1..=MAX_BITS as u32 {
            if length > bits.count {
                return Err(cut_short(bits.bytes, "a code"));
            }
            code |= ((peek >> (length - 1)) & 1) as i32;
            let count = i32::from(self.counts[length as usize]);
            if code - first < count {
                let symbol = self.symbols.get((index + code - first) as usize);
                return symbol
                    .map(|&symbol| (symbol, length))
                    .ok_or_else(|| self.bad(bits));
            }
            index += count;
            if index >= i32::from(self.total) {
                // No longer code is left to match.
                break;
            }
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(self.bad(bits))
    }

    fn bad(&self, bits: &Bits<'_>) -> Fault {
        Fault::new(bits.offset(), ErrorKind::BadCode { code: self.code })
    }
}

/// How many codes have each length; length 0 is not counted.
fn count_lengths(lengths: &[u8]) -> [u16; MAX_BITS + 1] {
    let mut counts = [0u16; MAX_BITS + 1];
    for &length in lengths {
        if let Some(count) = counts.get_mut(usize::from(length)) {
            *count += 1;
        }
    }
    counts[0] = 0;
    counts
}

/// The input ends where `expected` should be.
fn cut_short(bytes: &[u8], expected: &'static str) -> Fault {
    Fault::new(bytes.len(), ErrorKind::UnexpectedEnd { expected })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes bits as DEFLATE packs them: values least significant bit
    /// first, Huffman codes most significant bit first.
    #[derive(Default)]
    struct Writer {
        bytes: Vec<u8>,
        used: usize,
    }

    impl Writer {
        fn bit(&mut self, bit: u32) {
            if self.used.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if let Some(last) = self.bytes.last_mut() {
                *last |= (bit as u8) << (self.used % 8);
            }
            self.used += 1;
        }

        fn bits(&mut self, value: u32, count: u32) -> &mut Self {
            for i in 0..count {
                self.bit((value >> i) & 1);
            }
            self
        }

        fn code(&mut self, (code, length): (u32, u32)) -> &mut Self {
            for i in (0..length).rev() {
                self.bit((code >> i) & 1);
            }
            self
        }

        /// A block header.
        fn block(&mut self, last: bool, kind: u32) -> &mut Self {
            self.bits(u32::from(last), 1).bits(kind, 2)
        }

        /// Skips to the next byte, then writes whole bytes.
        fn raw_bytes(&mut self, bytes: &[u8]) -> &mut Self {
            self.used = self.bytes.len() * 8;
            self.bytes.extend_from_slice(bytes);
            self.used = self.bytes.len() * 8;
            self
        }

        /// A stored block.
        fn stored(&mut self, last: bool, data: &[u8]) -> &mut Self {
            let len = data.len() as u16;
            self.block(last, 0);
            self.raw_bytes(&len.to_le_bytes())
                .raw_bytes(&(!len).to_le_bytes())
                .raw_bytes(data)
        }

        /// A symbol of the fixed literal/length code.
        fn fixed(&mut self, symbol: u32) -> &mut Self {
            self.code(match symbol {
                0..=143 => (0x30 + symbol, 8),
                144..=255 => (0x190 + symbol - 144, 9),
                256..=279 => (symbol - 256, 7),
                _ => (0xc0 + symbol - 280, 8),
            })
        }

        /// A match in a fixed block, from RFC 1951's tables (3.2.5).
        fn fixed_match(&mut self, length: u32, distance: u32) -> &mut Self {
            let (symbol, extra, bits) = length_code(length);
            self.fixed(symbol).bits(extra, bits);
            let (symbol, extra, bits) = distance_code(distance);
            self.code((symbol, 5)).bits(extra, bits)
        }

        fn finish(&self) -> Vec<u8> {
            self.bytes.clone()
        }
    }

    /// RFC 1951's length codes, written out: (code, extra value, extra bits).
    fn length_code(length: u32) -> (u32, u32, u32) {
        match length {
            3..=10 => (257 + length - 3, 0, 0),
            258 => (285, 0, 0),
            _ => {
                // Codes 265 on come in fours, each four with one more extra bit.
                let mut base = 11;
                for bits in 1..=5 {
                    for k in 0..4 {
                        let span = 1 << bits;
                        if length < base + span {
                            return (265 + (bits - 1) * 4 + k, length - base, bits);
                        }
                        base += span;
                    }
                }
                unreachable!("lengths run from 3 to 258")
            }
        }
    }

    /// RFC 1951's distance codes, written out: (code, extra value, extra bits).
    fn distance_code(distance: u32) -> (u32, u32, u32) {
        if distance <= 4 {
            return (distance - 1, 0, 0);
        }
        // Codes 4 on come in pairs, each pair with one more extra bit.
        let mut base = 5;
        for bits in 1..=13 {
            for k in 0..2 {
                let span = 1 << bits;
                if distance < base + span {
                    return (4 + (bits - 1) * 2 + k, distance - base, bits);
                }
                base += span;
            }
        }
        unreachable!("distances run from 1 to 32768")
    }

    /// The canonical codes for these lengths (RFC 1951, 3.2.2).
    fn canonical(lengths: &[u32]) -> Vec<(u32, u32)> {
        let mut count = [0u32; 16];
        for &length in lengths {
            count[length as usize] += 1;
        }
        count[0] = 0;
        let mut next = [0u32; 16];
        let mut code = 0;
        for bits in 1..16 {
            code = (code + count[bits - 1]) << 1;
            next[bits] = code;
        }
        lengths
            .iter()
            .map(|&length| {
                let code = next[length as usize];
                next[length as usize] += 1;
                (code, length)
            })
            .collect()
    }

    /// A dynamic block's header: the code-length code with `cl_lengths`
    /// (by symbol), then `items`, each a code-length symbol and its extra
    /// bits.
    fn dynamic_header(
        w: &mut Writer,
        last: bool,
        literals: u32,
        distances: u32,
        cl_lengths: &[u32; 19],
        items: &[(u32, u32)],
    ) {
        let count = CODE_LENGTH_ORDER
            .iter()
            .rposition(|&symbol| cl_lengths[symbol] != 0)
            .map_or(4, |i| (i + 1).max(4));
        w.block(last, 2)
            .bits(literals - 257, 5)
            .bits(distances - 1, 5)
            .bits(count as u32 - 4, 4);
        for &symbol in &CODE_LENGTH_ORDER[..count] {
            w.bits(cl_lengths[symbol], 3);
        }
        let codes = canonical(cl_lengths);
        for &(symbol, extra) in items {
            w.code(codes[symbol as usize]);
            match symbol {
                16 => w.bits(extra, 2),
                17 => w.bits(extra, 3),
                18 => w.bits(extra, 7),
                _ => w,
            };
        }
    }

    fn raw_ok(bytes: &[u8]) -> Vec<u8> {
        raw(bytes, Limits::default()).unwrap()
    }

    fn raw_err(bytes: &[u8]) -> (usize, ErrorKind) {
        raw_err_with(bytes, Limits::default())
    }

    fn raw_err_with(bytes: &[u8], limits: Limits) -> (usize, ErrorKind) {
        match raw(bytes, limits) {
            Err(CodecError::Inflate { offset, kind }) => (offset, kind),
            other => panic!("not an inflate error: {other:?}"),
        }
    }

    fn zlib_err(bytes: &[u8]) -> (usize, ErrorKind) {
        match zlib(bytes, Limits::default()) {
            Err(CodecError::Inflate { offset, kind }) => (offset, kind),
            other => panic!("not an inflate error: {other:?}"),
        }
    }

    /// "hello", as zlib 1.3 compresses it at level 6: a fixed block.
    const HELLO: [u8; 13] = [
        0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00, 0x06, 0x2c, 0x02, 0x15,
    ];

    #[test]
    fn the_tables_are_rfc_1951s() {
        // Each code's range starts where the one before ends, and the last
        // ones reach 258 and 32768.
        for i in 0..27 {
            assert_eq!(
                LENGTH_BASE[i] + (1 << LENGTH_EXTRA[i]),
                LENGTH_BASE[i + 1],
                "{i}"
            );
        }
        assert_eq!(LENGTH_BASE[27] + (1 << LENGTH_EXTRA[27]) - 1, 258);
        assert_eq!((LENGTH_BASE[28], LENGTH_EXTRA[28]), (258, 0));
        for i in 0..29 {
            assert_eq!(
                DISTANCE_BASE[i] + (1 << DISTANCE_EXTRA[i]),
                DISTANCE_BASE[i + 1],
                "{i}"
            );
        }
        assert_eq!(
            u32::from(DISTANCE_BASE[29]) + (1 << DISTANCE_EXTRA[29]) - 1,
            32768
        );
        for length in 3..=258 {
            let (code, extra, _) = length_code(length);
            let i = (code - 257) as usize;
            assert_eq!(u32::from(LENGTH_BASE[i]) + extra, length);
        }
        for distance in 1..=32768 {
            let (code, extra, _) = distance_code(distance);
            assert_eq!(u32::from(DISTANCE_BASE[code as usize]) + extra, distance);
        }
    }

    #[test]
    fn stored_blocks_are_copied() {
        // Final, stored; LEN 3, NLEN !3; "abc".
        assert_eq!(
            raw_ok(&[0x01, 0x03, 0x00, 0xfc, 0xff, b'a', b'b', b'c']),
            b"abc"
        );
        // An empty final stored block.
        assert_eq!(raw_ok(&[0x01, 0x00, 0x00, 0xff, 0xff]), b"");
        // Two blocks, the first not final; and an empty one between.
        let mut w = Writer::default();
        w.stored(false, b"ab").stored(false, b"").stored(true, b"c");
        assert_eq!(raw_ok(&w.finish()), b"abc");
        // The largest stored block.
        let big: Vec<u8> = (0..65535u32).map(|i| (i % 251) as u8).collect();
        let mut w = Writer::default();
        w.stored(true, &big);
        assert_eq!(raw_ok(&w.finish()), big);
    }

    #[test]
    fn a_stored_block_after_bits_starts_at_the_next_byte() {
        // A fixed block with "x", then a stored block: the stored block's
        // header bits share a byte with the fixed block's end, and its
        // lengths start at the next byte.
        let mut w = Writer::default();
        w.block(false, 1).fixed(u32::from(b'x')).fixed(256);
        w.stored(true, b"yz");
        assert_eq!(raw_ok(&w.finish()), b"xyz");
    }

    #[test]
    fn fixed_blocks_decode() {
        // The empty fixed block: final, type 1, then the 7 zero bits of
        // the end-of-block code.
        assert_eq!(raw_ok(&[0x03, 0x00]), b"");
        assert_eq!(zlib(&HELLO, Limits::default()).unwrap(), b"hello");
        // Every literal, in each length of code.
        let mut w = Writer::default();
        w.block(true, 1);
        for byte in 0..=255 {
            w.fixed(byte);
        }
        w.fixed(256);
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(raw_ok(&w.finish()), all);
    }

    #[test]
    fn matches_copy_from_the_output_and_may_overlap_it() {
        let mut w = Writer::default();
        // "abc", then 6 from 3 back: overlapping, so "abcabc" repeats.
        w.block(true, 1)
            .fixed(u32::from(b'a'))
            .fixed(u32::from(b'b'))
            .fixed(u32::from(b'c'))
            .fixed_match(6, 3)
            // A run: 10 from 1 back.
            .fixed_match(10, 1)
            .fixed(256);
        assert_eq!(raw_ok(&w.finish()), b"abcabcabccccccccccc");
    }

    #[test]
    fn every_length_and_distance_code_decodes() {
        // 40,000 bytes of history, then a match of every length at every
        // distance code's edges.
        let history: Vec<u8> = (0..40_000u32).map(|i| (i * 7 % 253) as u8).collect();
        let mut w = Writer::default();
        w.stored(false, &history[..32768])
            .stored(false, &history[32768..]);
        w.block(true, 1);
        let mut expected = history;
        let mut matches = Vec::new();
        for length in 3..=258 {
            matches.push((length, 1 + length * 113 % 32768));
        }
        for (code, base) in DISTANCE_BASE.iter().enumerate() {
            let last = u32::from(*base) + (1 << DISTANCE_EXTRA[code]) - 1;
            matches.push((258, u32::from(*base)));
            matches.push((3, last));
        }
        for (length, distance) in matches {
            w.fixed_match(length, distance);
            let from = expected.len() - distance as usize;
            for i in 0..length as usize {
                expected.push(expected[from + i]);
            }
        }
        w.fixed(256);
        assert_eq!(raw_ok(&w.finish()), expected);
    }

    #[test]
    fn the_longest_distance_and_length_decode() {
        // 258 bytes from 32,768 back, the most of each.
        let history: Vec<u8> = (0..32768u32).map(|i| (i % 255) as u8).collect();
        let mut w = Writer::default();
        w.stored(false, &history);
        w.block(true, 1).fixed_match(258, 32768).fixed(256);
        let out = raw_ok(&w.finish());
        assert_eq!(out.len(), 32768 + 258);
        assert_eq!(out[32768..], history[..258]);
    }

    #[test]
    fn length_code_284_with_extra_bits_31_is_258() {
        // RFC 1951 gives 258 to code 285 alone; zlib decodes 284 + 31 as
        // 258 too, and so does this.
        let mut w = Writer::default();
        w.block(true, 1)
            .fixed(u32::from(b'q'))
            .fixed(284)
            .bits(31, 5)
            .code((0, 5))
            .fixed(256);
        assert_eq!(raw_ok(&w.finish()), [b'q'; 259]);
    }

    /// A dynamic block of "ababacdddd": literals a–d and lengths 3 and 4 in
    /// 3 bits each, distances 1 and 2 in 1 bit each. The code lengths use
    /// every kind of code-length symbol.
    fn dynamic_abcd() -> Vec<u8> {
        let mut cl = [0; 19];
        // Six code-length symbols: two of 2 bits, four of 3.
        for (symbol, length) in [(0, 3), (1, 2), (3, 2), (16, 3), (17, 3), (18, 3)] {
            cl[symbol] = length;
        }
        let mut w = Writer::default();
        dynamic_header(
            &mut w,
            true,
            260,
            2,
            &cl,
            &[
                (18, 97 - 11), // 0–96: no code
                (3, 0),        // a
                (16, 0),       // b, c, d: the length before, 3 times
                (18, 138 - 11),
                (17, 10 - 3),
                (0, 0),
                (0, 0),
                (0, 0),
                (0, 0),
                (17, 0), // 3 more: 101–255, 155 without a code
                (3, 0),  // 256
                (16, 0), // 257–259
                (1, 0),  // distance 1
                (1, 0),  // distance 2
            ],
        );
        let mut literal = [0; 260];
        literal[97..101].fill(3);
        literal[256..260].fill(3);
        let lit = canonical(&literal);
        let dist = canonical(&[1, 1]);
        for byte in *b"ab" {
            w.code(lit[usize::from(byte)]);
        }
        // 3 from 2 back: "aba".
        w.code(lit[257]).code(dist[1]);
        for byte in *b"cd" {
            w.code(lit[usize::from(byte)]);
        }
        // 3 from 1 back: "ddd".
        w.code(lit[257]).code(dist[0]);
        w.code(lit[256]);
        w.finish()
    }

    #[test]
    fn dynamic_blocks_decode() {
        assert_eq!(raw_ok(&dynamic_abcd()), b"ababacdddd");
    }

    #[test]
    fn a_dynamic_block_may_have_one_code_of_one_bit() {
        // zlib allows an incomplete code when it is one code of one bit:
        // here the only distance, and a literal/length code of only the
        // end of the block.
        let mut cl = [0; 19];
        cl[0] = 1;
        cl[1] = 2;
        cl[18] = 2;
        let mut w = Writer::default();
        // Literal/length: 256 alone, 1 bit. Distance: one code, 1 bit.
        dynamic_header(
            &mut w,
            true,
            257,
            1,
            &cl,
            &[(18, 138 - 11), (18, 118 - 11), (1, 0), (1, 0)],
        );
        w.code((0, 1));
        assert_eq!(raw_ok(&w.finish()), b"");
    }

    #[test]
    fn a_dynamic_block_may_have_no_distance_codes() {
        // Literals only: every distance length is 0, which zlib allows.
        let mut cl = [0; 19];
        cl[0] = 1;
        cl[1] = 1;
        let mut w = Writer::default();
        let mut items = vec![(0, 0); 256];
        items[usize::from(b'z')] = (1, 0);
        items.push((1, 0)); // 256
        items.push((0, 0)); // the one distance: no code
        dynamic_header(&mut w, true, 257, 1, &cl, &items);
        // 'z' is code 0 and the end of block 1, both 1 bit.
        w.code((0, 1)).code((0, 1)).code((1, 1));
        assert_eq!(raw_ok(&w.finish()), b"zz");

        // A match in such a block has no distance code to use: 'z' in 1
        // bit, the end of block and length 3 in 2.
        let mut cl = [0; 19];
        cl[0] = 1;
        cl[1] = 2;
        cl[2] = 2;
        let mut items = vec![(0, 0); 256];
        items[usize::from(b'z')] = (1, 0);
        items.extend([(2, 0), (2, 0), (0, 0)]);
        let mut w = Writer::default();
        dynamic_header(&mut w, true, 258, 1, &cl, &items);
        let mut literal = [0; 258];
        literal[usize::from(b'z')] = 1;
        literal[256] = 2;
        literal[257] = 2;
        let lit = canonical(&literal);
        w.code(lit[usize::from(b'z')]).code(lit[257]);
        let distance_at = w.used / 8;
        // Bits enough that the code, not the input, runs out.
        w.bits(0, 16);
        assert_eq!(
            raw_err(&w.finish()),
            (
                distance_at,
                ErrorKind::BadCode {
                    code: Code::Distance
                }
            )
        );
    }

    #[test]
    fn a_stream_cut_short_is_rejected_where_it_ends() {
        let end = |expected| ErrorKind::UnexpectedEnd { expected };
        assert_eq!(raw_err(&[]), (0, end("a block header")));
        assert_eq!(raw_err(&[0x01, 0x03]), (2, end("a stored block's length")));
        assert_eq!(
            raw_err(&[0x01, 0x03, 0x00, 0xfc, 0xff, b'a']),
            (6, end("a stored block's data"))
        );
        // A fixed block without its end.
        let mut w = Writer::default();
        w.block(true, 1).fixed(u32::from(b'a'));
        assert_eq!(raw_err(&w.finish()), (2, end("a code")));
        // A non-final block, and nothing after it.
        assert_eq!(
            raw_err(&[0x00, 0x00, 0x00, 0xff, 0xff]),
            (5, end("a block header"))
        );
        // A match without all its extra bits: 3 + 8 + 9 + 8 bits leave 4
        // in the last byte, and code 284 has 5.
        let mut w = Writer::default();
        w.block(true, 1)
            .fixed(u32::from(b'a'))
            .fixed(200)
            .fixed(284);
        let bytes = w.finish();
        assert_eq!(raw_err(&bytes), (bytes.len(), end("a length's extra bits")));

        assert_eq!(zlib_err(&[0x78]), (1, end("the zlib header")));
        assert_eq!(zlib_err(&HELLO[..9]), (9, end("the Adler-32")));
        assert_eq!(zlib_err(&HELLO[..12]), (12, end("the Adler-32")));
        assert_eq!(zlib_err(&HELLO[..2]), (2, end("a block header")));
    }

    #[test]
    fn a_stream_cut_short_inside_a_code_is_cut_short_not_bad() {
        // The empty fixed stream 03 00, and "a" as 4b 04 00, each without
        // its last byte: the zero bits a peek pads with would read as the
        // end of the block.
        let end = |at| (at, ErrorKind::UnexpectedEnd { expected: "a code" });
        assert_eq!(raw_err(&[0x03]), end(1));
        assert_eq!(raw_err(&[0x4b, 0x04]), end(2));
        // Six 9-bit literals, then 7 of the 8 bits of symbol 286's code:
        // padded with a zero they would be 286, which does not exist, but
        // the stream is cut short first.
        let mut w = Writer::default();
        w.block(true, 1);
        for _ in 0..6 {
            w.fixed(200);
        }
        w.code((0b110_0011, 7));
        let bytes = w.finish();
        assert_eq!(bytes.len(), 8);
        assert_eq!(raw_err(&bytes), end(8));
    }

    #[test]
    fn block_headers_are_checked() {
        // Final, type 3.
        assert_eq!(raw_err(&[0x07]), (0, ErrorKind::BadBlockType));
        // A second block of type 3, after an empty fixed one (10 bits).
        let mut w = Writer::default();
        w.block(false, 1).fixed(256).block(true, 3);
        assert_eq!(raw_err(&w.finish()), (1, ErrorKind::BadBlockType));
        // LEN 3, NLEN 0.
        assert_eq!(
            raw_err(&[0x01, 0x03, 0x00, 0x00, 0x00]),
            (1, ErrorKind::BadStoredLength)
        );
    }

    #[test]
    fn a_dynamic_block_has_at_most_286_and_30_codes() {
        for (literals, distances) in [(287, 1), (288, 1), (257, 31), (257, 32)] {
            let mut w = Writer::default();
            w.block(true, 2)
                .bits(literals - 257, 5)
                .bits(distances - 1, 5)
                .bits(0, 4)
                .bits(0, 12);
            assert_eq!(raw_err(&w.finish()), (0, ErrorKind::TooManyCodes));
        }
    }

    /// The error a dynamic block gives with these code-length code lengths
    /// and code-length items.
    fn dynamic_err(
        literals: u32,
        distances: u32,
        cl: &[u32; 19],
        items: &[(u32, u32)],
    ) -> (usize, ErrorKind) {
        let mut w = Writer::default();
        // A block before it, so that the offsets are not all 0.
        w.stored(false, b"x");
        dynamic_header(&mut w, true, literals, distances, cl, items);
        w.bits(0, 32);
        raw_err(&w.finish())
    }

    #[test]
    fn code_length_codes_are_checked_as_zlib_does() {
        let block = 6;
        // Over-subscribed: three codes of one bit.
        let mut cl = [0; 19];
        cl[0] = 1;
        cl[1] = 1;
        cl[2] = 1;
        assert_eq!(
            dynamic_err(257, 1, &cl, &[]),
            (
                block,
                ErrorKind::OverSubscribed {
                    code: Code::CodeLengths
                }
            )
        );
        // Incomplete, even as one code of one bit, and with no codes.
        let mut cl = [0; 19];
        cl[1] = 1;
        assert_eq!(
            dynamic_err(257, 1, &cl, &[]),
            (
                block,
                ErrorKind::Incomplete {
                    code: Code::CodeLengths
                }
            )
        );
        assert_eq!(
            dynamic_err(257, 1, &[0; 19], &[]),
            (
                block,
                ErrorKind::Incomplete {
                    code: Code::CodeLengths
                }
            )
        );
    }

    #[test]
    fn repeats_are_checked() {
        let mut cl = [0; 19];
        for symbol in [0, 1, 16, 18] {
            cl[symbol] = 2;
        }
        // After the stored block's 6 bytes: the block header, the counts,
        // and 18 code-length code lengths (symbol 1 is 18th in order).
        let header = 6 * 8 + 3 + 14 + 3 * 18;
        // 16 first: nothing to repeat.
        let (offset, kind) = dynamic_err(257, 1, &cl, &[(16, 0)]);
        assert_eq!((offset, kind), (header / 8, ErrorKind::RepeatWithoutLength));
        // 258 lengths, then zeros: 138 + 138 is past them.
        let (offset, kind) = dynamic_err(257, 1, &cl, &[(18, 127), (18, 127)]);
        assert_eq!(
            (offset, kind),
            // The second repeat starts after the first's 2-bit code and
            // 7 extra bits.
            ((header + 9) / 8, ErrorKind::RepeatTooLong)
        );
        // A repeat may run from the literal/length lengths into the
        // distance lengths, as zlib allows: 256 zeros, then 1 for 256 and
        // three times more for 257 and the two distances.
        let mut w = Writer::default();
        dynamic_header(
            &mut w,
            true,
            258,
            2,
            &cl,
            &[(18, 127), (18, 118 - 11), (1, 0), (16, 0)],
        );
        // The end of block is code 0 of the literal/length code.
        w.code((0, 1));
        assert_eq!(raw_ok(&w.finish()), b"");
    }

    #[test]
    fn literal_length_and_distance_codes_are_checked_as_zlib_does() {
        let block = 6;
        let mut cl = [0; 19];
        for symbol in [0, 1, 2, 18] {
            cl[symbol] = 2;
        }
        // 256 without a code: 257 zeros, then a distance.
        assert_eq!(
            dynamic_err(257, 1, &cl, &[(18, 127), (18, 119 - 11), (1, 0)]),
            (block, ErrorKind::NoEndOfBlock)
        );
        let zeros = |n: u32| -> Vec<(u32, u32)> {
            let mut items = Vec::new();
            let mut left = n;
            while left >= 11 {
                let run = left.min(138);
                items.push((18, run - 11));
                left -= run;
            }
            items.extend(std::iter::repeat_n((0, 0), left as usize));
            items
        };
        // Literal/length: 'a', 'b' and 256 in one bit each: over-subscribed.
        let mut items = zeros(97);
        items.extend([(1, 0), (1, 0)]);
        items.extend(zeros(256 - 99));
        items.extend([(1, 0), (1, 0)]);
        assert_eq!(
            dynamic_err(257, 1, &cl, &items),
            (
                block,
                ErrorKind::OverSubscribed {
                    code: Code::LiteralLength
                }
            )
        );
        // 'a' in one bit, 256 in two: incomplete, and longer than one bit.
        let mut items = zeros(97);
        items.push((1, 0));
        items.extend(zeros(256 - 98));
        items.extend([(2, 0), (1, 0)]);
        assert_eq!(
            dynamic_err(257, 1, &cl, &items),
            (
                block,
                ErrorKind::Incomplete {
                    code: Code::LiteralLength
                }
            )
        );
        // A complete literal/length code; distances of one and two bits:
        // incomplete.
        let mut items = zeros(97);
        items.push((1, 0));
        items.extend(zeros(256 - 98));
        items.extend([(1, 0), (1, 0), (2, 0)]);
        assert_eq!(
            dynamic_err(257, 2, &cl, &items),
            (
                block,
                ErrorKind::Incomplete {
                    code: Code::Distance
                }
            )
        );
        // Three distances of one bit: over-subscribed.
        let mut items = zeros(97);
        items.push((1, 0));
        items.extend(zeros(256 - 98));
        items.extend([(1, 0), (1, 0), (1, 0), (1, 0)]);
        assert_eq!(
            dynamic_err(257, 3, &cl, &items),
            (
                block,
                ErrorKind::OverSubscribed {
                    code: Code::Distance
                }
            )
        );
    }

    #[test]
    fn the_shapes_zlib_accepts_and_rejects_at_the_edges() {
        // 30 distance codes, the most: two of 4 bits and 28 of 5, a
        // complete code (2/16 + 28/32 = 1). Literal 'a' and the end of
        // block take a bit each.
        let mut cl = [0; 19];
        for (symbol, length) in [(0, 2), (1, 2), (18, 2), (4, 3), (5, 3)] {
            cl[symbol] = length;
        }
        let mut items = vec![(18, 97 - 11), (1, 0), (18, 138 - 11), (18, 20 - 11), (1, 0)];
        items.extend([(4, 0), (4, 0)]);
        items.extend(std::iter::repeat_n((5, 0), 28));
        let mut w = Writer::default();
        dynamic_header(&mut w, true, 257, 30, &cl, &items);
        // 'a' is code 0, the end of block code 1.
        w.code((0, 1)).code((1, 1));
        assert_eq!(raw_ok(&w.finish()), b"a");

        // The end of block alone, in 2 bits: incomplete, and not one code
        // of one bit.
        let mut cl = [0; 19];
        cl[2] = 1;
        cl[18] = 2;
        cl[0] = 2;
        assert_eq!(
            dynamic_err(257, 1, &cl, &[(18, 127), (18, 107), (2, 0), (0, 0)]),
            (
                6,
                ErrorKind::Incomplete {
                    code: Code::LiteralLength
                }
            )
        );
    }

    #[test]
    fn symbols_that_do_not_exist_are_rejected() {
        // Literal/length codes 286 and 287 exist only in the fixed code.
        for symbol in [286, 287] {
            let mut w = Writer::default();
            w.block(true, 1)
                .fixed(u32::from(b'a'))
                .fixed(symbol)
                .bits(0, 16);
            assert_eq!(
                raw_err(&w.finish()),
                (
                    1,
                    ErrorKind::BadCode {
                        code: Code::LiteralLength
                    }
                )
            );
        }
        // So do distance codes 30 and 31.
        for symbol in [30, 31] {
            let mut w = Writer::default();
            w.block(true, 1)
                .fixed(u32::from(b'a'))
                .fixed(257)
                .code((symbol, 5))
                .bits(0, 16);
            assert_eq!(
                raw_err(&w.finish()),
                (
                    2,
                    ErrorKind::BadCode {
                        code: Code::Distance
                    }
                )
            );
        }
        // The unused one-bit pattern of a one-code literal/length code.
        let mut cl = [0; 19];
        cl[0] = 1;
        cl[1] = 2;
        cl[18] = 2;
        let mut w = Writer::default();
        dynamic_header(
            &mut w,
            true,
            257,
            1,
            &cl,
            &[(18, 127), (18, 107), (1, 0), (1, 0)],
        );
        let at = w.used / 8;
        let mut short = Writer {
            bytes: w.bytes.clone(),
            used: w.used,
        };
        w.code((1, 1)).bits(0, 16);
        assert_eq!(
            raw_err(&w.finish()),
            (
                at,
                ErrorKind::BadCode {
                    code: Code::LiteralLength
                }
            )
        );
        // The same with no bits after the pattern: fewer than 15 bits are
        // left, but no longer code can start with it, so it is still a bad
        // code, not a stream cut short.
        short.code((1, 1));
        assert_eq!(
            raw_err(&short.finish()),
            (
                at,
                ErrorKind::BadCode {
                    code: Code::LiteralLength
                }
            )
        );
    }

    #[test]
    fn a_match_may_not_reach_before_the_output() {
        let mut w = Writer::default();
        w.block(true, 1).fixed_match(3, 1).fixed(256);
        assert_eq!(
            raw_err(&w.finish()),
            (
                0,
                ErrorKind::DistanceTooFar {
                    distance: 1,
                    available: 0
                }
            )
        );
        let mut w = Writer::default();
        w.stored(false, b"abcd");
        w.block(true, 1).fixed_match(3, 5).fixed(256);
        assert_eq!(
            raw_err(&w.finish()),
            (
                9,
                ErrorKind::DistanceTooFar {
                    distance: 5,
                    available: 4
                }
            )
        );
    }

    #[test]
    fn output_past_the_limit_is_rejected_without_allocating_past_it() {
        let limits = |max_decoded_bytes| Limits {
            max_decoded_bytes,
            ..Limits::default()
        };
        let stored = [0x01, 0x03, 0x00, 0xfc, 0xff, b'a', b'b', b'c'];
        assert_eq!(raw(&stored, limits(3)).unwrap(), b"abc");
        assert_eq!(
            raw_err_with(&stored, limits(2)),
            (5, ErrorKind::TooMuchOutput { limit: 2 })
        );
        // A match of 258 that would pass a limit of 100.
        let mut w = Writer::default();
        w.block(true, 1)
            .fixed(u32::from(b'a'))
            .fixed_match(258, 1)
            .fixed(256);
        assert_eq!(
            raw_err_with(&w.finish(), limits(100)),
            (1, ErrorKind::TooMuchOutput { limit: 100 })
        );
        // Growing to the limit allocates no more than it.
        let mut w = Writer::default();
        w.block(true, 1).fixed(u32::from(b'a'));
        for _ in 0..1000 {
            w.fixed_match(258, 1);
        }
        w.fixed(256);
        let out = raw(&w.finish(), limits(258_001)).unwrap();
        assert_eq!(out.len(), 258_001);
        assert!(out.capacity() <= 258_001, "{}", out.capacity());
    }

    #[test]
    fn zlib_headers_are_checked_as_zlib_does() {
        assert_eq!(
            zlib_err(&[0x78, 0x9d, 0x03, 0x00]),
            (0, ErrorKind::BadHeaderCheck)
        );
        // Method 7, window 7; check bits right.
        assert_eq!(
            zlib_err(&[0x77, 0x85, 0x03, 0x00]),
            (0, ErrorKind::BadMethod { found: 7 })
        );
        // Method 8, window 8 (64 KiB).
        assert_eq!(
            zlib_err(&[0x88, 0x1c, 0x03, 0x00]),
            (0, ErrorKind::BadWindow { found: 8 })
        );
        // A preset dictionary.
        assert_eq!(
            zlib_err(&[0x78, 0xbb, 0x03, 0x00]),
            (1, ErrorKind::PresetDictionary)
        );
        // The smallest window, 256 bytes, is fine, even for a distance
        // past it, as zlib allows.
        let mut w = Writer::default();
        w.raw_bytes(&[0x08, 0x1d]);
        let history: Vec<u8> = (0..300u32).map(|i| i as u8).collect();
        w.stored(false, &history);
        w.block(true, 1).fixed_match(3, 300).fixed(256);
        let mut expected = history.clone();
        expected.extend_from_slice(&history[..3]);
        w.raw_bytes(&adler32::checksum(&expected).to_be_bytes());
        assert_eq!(zlib(&w.finish(), Limits::default()).unwrap(), expected);
    }

    #[test]
    fn the_checksum_must_match_and_end_the_stream() {
        let mut bad = HELLO;
        bad[12] ^= 1;
        assert_eq!(
            zlib_err(&bad),
            (
                9,
                ErrorKind::BadChecksum {
                    stored: 0x062c_0214,
                    computed: 0x062c_0215
                }
            )
        );
        let mut long = HELLO.to_vec();
        long.push(0);
        assert_eq!(zlib_err(&long), (13, ErrorKind::TrailingBytes));
        assert_eq!(raw_err(&[0x03, 0x00, 0x00]), (2, ErrorKind::TrailingBytes));
        // The rest of the last byte is padding, whatever it holds.
        assert_eq!(raw_ok(&[0x03, 0xfc]), b"");
    }

    #[test]
    fn the_raw_stream_may_start_inside_a_buffer() {
        // `raw_into` appends, and matches reach back only to its own start.
        let mut out = b"prefix".to_vec();
        let mut w = Writer::default();
        w.block(true, 1)
            .fixed(u32::from(b'a'))
            .fixed_match(3, 1)
            .fixed(256);
        let bytes = w.finish();
        assert_eq!(raw_into(&bytes, 0, &mut out, 100), Ok(bytes.len()));
        assert_eq!(out, b"prefixaaaa");
        let mut w = Writer::default();
        w.block(true, 1).fixed_match(3, 2).fixed(256);
        let mut out = b"prefix".to_vec();
        assert_eq!(
            raw_into(&w.finish(), 0, &mut out, 100),
            Err(Fault::new(
                0,
                ErrorKind::DistanceTooFar {
                    distance: 2,
                    available: 0
                }
            ))
        );
    }

    #[test]
    fn error_kinds_explain_themselves() {
        let cases = [
            (
                ErrorKind::UnexpectedEnd { expected: "a code" },
                "the stream ends where a code should be",
            ),
            (
                ErrorKind::OverSubscribed {
                    code: Code::CodeLengths,
                },
                "the code-length code's lengths give more codes than its bits allow",
            ),
            (
                ErrorKind::Incomplete {
                    code: Code::Distance,
                },
                "the distance code's lengths leave codes unused",
            ),
            (
                ErrorKind::BadCode {
                    code: Code::LiteralLength,
                },
                "the bits are no symbol of the literal/length code",
            ),
            (
                ErrorKind::DistanceTooFar {
                    distance: 5,
                    available: 4,
                },
                "a match reaches 5 bytes back, but there are 4",
            ),
            (
                ErrorKind::BadChecksum {
                    stored: 1,
                    computed: 0x062c_0215,
                },
                "the stream's Adler-32 is 0x00000001, but its output's is 0x062c0215",
            ),
        ];
        for (kind, text) in cases {
            assert_eq!(kind.to_string(), text);
        }
        assert_eq!(
            CodecError::Inflate {
                offset: 7,
                kind: ErrorKind::BadBlockType
            }
            .to_string(),
            "inflate error at byte 7: a block has type 3, which does not exist"
        );
    }
}
