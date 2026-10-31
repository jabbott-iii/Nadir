//! Inflate property tests: random streams of stored, fixed and dynamic
//! blocks, written by an encoder of the test's own, decode to what they
//! encode; every stream cut short is rejected where it ends; damaged
//! streams never panic. The hand-built vectors and each error, one by one,
//! are beside the decoder, in `src/inflate.rs`. A run against zlib 1.3 on
//! 100,000 streams is recorded in the roadmap (M5.1).

use nadir_codec::inflate::{self, ErrorKind};
use nadir_codec::{CodecError, Limits, adler32};
use nadir_core::Pcg32;
use nadir_core::testing::{check, check_sized};

/// One thing a block holds.
#[derive(Clone, Copy, Debug)]
enum Item {
    Literal(u8),
    /// A length and a distance.
    Match(u32, u32),
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Stored,
    Fixed,
    Dynamic,
}

/// A random stream, as blocks of items, and the bytes it decodes to.
#[derive(Debug)]
struct Stream {
    blocks: Vec<(Kind, Vec<Item>)>,
    output: Vec<u8>,
    /// For dynamic blocks: whether to make codes as long as they can be.
    deep: bool,
}

fn random_stream(rng: &mut Pcg32, size: u32) -> Stream {
    let mut output: Vec<u8> = Vec::new();
    let mut blocks = Vec::new();
    // A few symbols, so that matches and repeats are common.
    let alphabet: Vec<u8> = (0..1 + rng.range(0..12u32)).map(|_| byte(rng)).collect();
    for _ in 0..1 + rng.range(0..4u32) {
        let kind = match rng.range(0..3u32) {
            0 => Kind::Stored,
            1 => Kind::Fixed,
            _ => Kind::Dynamic,
        };
        let mut items = Vec::new();
        for _ in 0..rng.range(0..size * 4 + 1) {
            let available = output.len() as u32;
            if matches!(kind, Kind::Stored) || available == 0 || rng.range(0..3u32) == 0 {
                let literal = if rng.range(0..4u32) == 0 {
                    byte(rng)
                } else {
                    *rng.choose(&alphabet).expect("an alphabet")
                };
                output.push(literal);
                items.push(Item::Literal(literal));
            } else {
                let length = match rng.range(0..4u32) {
                    0 => 258,
                    1 => 3 + rng.range(0..8u32),
                    _ => 3 + rng.range(0..256u32),
                };
                let distance = 1 + rng.range(0..available.min(32768));
                let from = output.len() - distance as usize;
                for i in 0..length as usize {
                    output.push(output[from + i]);
                }
                items.push(Item::Match(length, distance));
            }
        }
        blocks.push((kind, items));
    }
    Stream {
        blocks,
        output,
        deep: rng.range(0..3u32) == 0,
    }
}

fn byte(rng: &mut Pcg32) -> u8 {
    rng.range(0..256u32) as u8
}

/// Writes bits as DEFLATE packs them.
#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
    used: usize,
}

impl Writer {
    fn bits(&mut self, value: u32, count: u32) {
        for i in 0..count {
            if self.used.is_multiple_of(8) {
                self.bytes.push(0);
            }
            let last = self.bytes.len() - 1;
            self.bytes[last] |= (((value >> i) & 1) as u8) << (self.used % 8);
            self.used += 1;
        }
    }

    /// A Huffman code, most significant bit first.
    fn code(&mut self, (code, length): (u32, u32)) {
        for i in (0..length).rev() {
            self.bits((code >> i) & 1, 1);
        }
    }

    fn align(&mut self) {
        self.used = self.bytes.len() * 8;
    }
}

/// RFC 1951's length code for a length: (symbol, extra value, extra bits).
fn length_code(length: u32) -> (usize, u32, u32) {
    const BASE: [u32; 29] = [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115,
        131, 163, 195, 227, 258,
    ];
    const EXTRA: [u32; 29] = [
        0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
    ];
    let i = if length == 258 {
        28
    } else {
        BASE.iter()
            .rposition(|&base| base <= length)
            .expect("a length of 3 to 258")
    };
    (257 + i, length - BASE[i], EXTRA[i])
}

/// RFC 1951's distance code for a distance.
fn distance_code(distance: u32) -> (usize, u32, u32) {
    let mut base = 1;
    for code in 0..30 {
        let extra = if code < 4 { 0 } else { code / 2 - 1 };
        if distance < base + (1 << extra) {
            return (code as usize, distance - base, extra);
        }
        base += 1 << extra;
    }
    unreachable!("a distance of 1 to 32768")
}

/// Each symbol's code and its length in bits.
type Codes = Vec<(u32, u32)>;

/// The canonical codes for these lengths (RFC 1951, 3.2.2).
fn canonical(lengths: &[u32]) -> Codes {
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

/// Code lengths of at most `longest` bits for `n` symbols, of which those
/// in `used` get a code: a complete code, or one code of one bit. `deep`
/// splits the longest codes first, which makes them as long as it can.
fn code_lengths(rng: &mut Pcg32, n: usize, used: &[usize], longest: u32, deep: bool) -> Vec<u32> {
    let mut lengths = vec![0; n];
    if let [only] = used {
        lengths[*only] = 1;
        return lengths;
    }
    let mut leaves = vec![0u32];
    while leaves.len() < used.len() {
        let candidates: Vec<usize> = (0..leaves.len()).filter(|&i| leaves[i] < longest).collect();
        let pick = if deep {
            *candidates
                .iter()
                .max_by_key(|&&i| leaves[i])
                .expect("room for a code")
        } else {
            candidates[rng.range(0..candidates.len() as u32) as usize]
        };
        let depth = leaves.swap_remove(pick);
        leaves.extend([depth + 1, depth + 1]);
    }
    let mut symbols = used.to_vec();
    rng.shuffle(&mut symbols);
    for (symbol, depth) in symbols.into_iter().zip(leaves) {
        lengths[symbol] = depth;
    }
    lengths
}

const ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// The fixed codes.
fn fixed() -> (Codes, Codes) {
    let mut lengths = vec![8; 288];
    lengths[144..256].fill(9);
    lengths[256..280].fill(7);
    (canonical(&lengths), canonical(&[5; 30]))
}

fn encode_raw(stream: &Stream, rng: &mut Pcg32) -> Vec<u8> {
    let mut w = Writer::default();
    let last = stream.blocks.len() - 1;
    for (index, (kind, items)) in stream.blocks.iter().enumerate() {
        w.bits(u32::from(index == last), 1);
        match kind {
            Kind::Stored => {
                w.bits(0, 2);
                w.align();
                let data: Vec<u8> = items
                    .iter()
                    .map(|item| match item {
                        Item::Literal(byte) => *byte,
                        Item::Match(..) => unreachable!("stored blocks hold literals"),
                    })
                    .collect();
                let len = data.len() as u16;
                w.bytes.extend_from_slice(&len.to_le_bytes());
                w.bytes.extend_from_slice(&(!len).to_le_bytes());
                w.bytes.extend_from_slice(&data);
                w.align();
            }
            Kind::Fixed => {
                w.bits(1, 2);
                let (literals, distances) = fixed();
                symbols(&mut w, items, &literals, &distances);
            }
            Kind::Dynamic => {
                w.bits(2, 2);
                let (literals, distances) = dynamic_header(&mut w, items, stream.deep, rng);
                symbols(&mut w, items, &literals, &distances);
            }
        }
    }
    w.bytes
}

/// Writes a dynamic block's header for `items` and returns its codes.
fn dynamic_header(w: &mut Writer, items: &[Item], deep: bool, rng: &mut Pcg32) -> (Codes, Codes) {
    let mut literal_used = vec![256];
    let mut distance_used = Vec::new();
    for item in items {
        match *item {
            Item::Literal(byte) => literal_used.push(usize::from(byte)),
            Item::Match(length, distance) => {
                literal_used.push(length_code(length).0);
                distance_used.push(distance_code(distance).0);
            }
        }
    }
    literal_used.sort_unstable();
    literal_used.dedup();
    distance_used.sort_unstable();
    distance_used.dedup();
    let hlit = (literal_used.last().copied().unwrap_or(256) + 1).max(257);
    let hdist = (distance_used.last().map_or(0, |&d| d + 1)).max(1);
    let literal = code_lengths(rng, hlit, &literal_used, 15, deep);
    // A block of literals only has no distance codes at all, which zlib
    // allows.
    let distance = code_lengths(rng, hdist, &distance_used, 15, deep);

    // The lengths of both codes as one sequence, which repeats may cross.
    let all: Vec<u32> = literal.iter().chain(&distance).copied().collect();
    let mut runs: Vec<(usize, u32)> = Vec::new();
    let mut i = 0;
    while i < all.len() {
        let length = all[i];
        let mut run = 1;
        while i + run < all.len() && all[i + run] == length {
            run += 1;
        }
        if length == 0 && run >= 11 {
            let n = run.min(138);
            runs.push((18, n as u32 - 11));
            i += n;
        } else if length == 0 && run >= 3 {
            let n = run.min(10);
            runs.push((17, n as u32 - 3));
            i += n;
        } else if i > 0 && all[i - 1] == length && run >= 3 {
            let n = run.min(6);
            runs.push((16, n as u32 - 3));
            i += n;
        } else {
            runs.push((length as usize, 0));
            i += 1;
        }
    }
    let mut cl_used: Vec<usize> = runs.iter().map(|&(symbol, _)| symbol).collect();
    cl_used.sort_unstable();
    cl_used.dedup();
    if cl_used.len() == 1 {
        // A code-length code may not be one code of one bit.
        cl_used.push(if cl_used[0] == 0 { 1 } else { 0 });
    }
    let cl = code_lengths(rng, 19, &cl_used, 7, false);
    let count = ORDER
        .iter()
        .rposition(|&s| cl[s] != 0)
        .map_or(4, |i| (i + 1).max(4));
    w.bits(hlit as u32 - 257, 5);
    w.bits(hdist as u32 - 1, 5);
    w.bits(count as u32 - 4, 4);
    for &symbol in &ORDER[..count] {
        w.bits(cl[symbol], 3);
    }
    let cl_codes = canonical(&cl);
    for (symbol, extra) in runs {
        w.code(cl_codes[symbol]);
        match symbol {
            16 => w.bits(extra, 2),
            17 => w.bits(extra, 3),
            18 => w.bits(extra, 7),
            _ => {}
        }
    }
    (canonical(&literal), canonical(&distance))
}

fn symbols(w: &mut Writer, items: &[Item], literals: &[(u32, u32)], distances: &[(u32, u32)]) {
    for item in items {
        match *item {
            Item::Literal(byte) => w.code(literals[usize::from(byte)]),
            Item::Match(length, distance) => {
                let (symbol, extra, bits) = length_code(length);
                w.code(literals[symbol]);
                w.bits(extra, bits);
                let (symbol, extra, bits) = distance_code(distance);
                w.code(distances[symbol]);
                w.bits(extra, bits);
            }
        }
    }
    w.code(literals[256]);
}

/// The raw stream in a zlib wrapper.
fn wrap(stream: &Stream, raw: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0x78, 0x9c];
    bytes.extend_from_slice(raw);
    bytes.extend_from_slice(&adler32::checksum(&stream.output).to_be_bytes());
    bytes
}

/// A random stream, and both encodings of it.
#[derive(Debug)]
struct Case {
    stream: Stream,
    raw: Vec<u8>,
    zlib: Vec<u8>,
}

fn random_case(rng: &mut Pcg32, size: u32) -> Case {
    let stream = random_stream(rng, size);
    let raw = encode_raw(&stream, rng);
    let zlib = wrap(&stream, &raw);
    Case { stream, raw, zlib }
}

#[test]
fn random_streams_decode_to_what_they_encode() {
    check_sized("inflate random streams", 400, random_case, |case| {
        let raw =
            inflate::raw(&case.raw, Limits::default()).map_err(|error| format!("raw: {error}"))?;
        if raw != case.stream.output {
            return Err(format!("raw gives {} bytes", raw.len()));
        }
        let zlib = inflate::zlib(&case.zlib, Limits::default())
            .map_err(|error| format!("zlib: {error}"))?;
        if zlib != case.stream.output {
            return Err(format!("zlib gives {} bytes", zlib.len()));
        }
        Ok(())
    });
}

#[test]
fn every_stream_cut_short_is_rejected_where_it_ends() {
    check_sized(
        "inflate prefixes",
        60,
        |rng, size| random_case(rng, size / 4),
        |case| {
            for (name, bytes, decode) in [
                ("raw", &case.raw, inflate::raw as fn(&[u8], Limits) -> _),
                ("zlib", &case.zlib, inflate::zlib),
            ] {
                for len in 0..bytes.len() {
                    match decode(&bytes[..len], Limits::default()) {
                        Err(CodecError::Inflate {
                            offset,
                            kind: ErrorKind::UnexpectedEnd { .. },
                        }) if offset == len => {}
                        other => return Err(format!("{name}, {len} bytes: {other:?}")),
                    }
                }
            }
            Ok(())
        },
    );
}

#[test]
fn the_output_stops_at_the_limit() {
    check_sized(
        "inflate limits",
        200,
        |rng, size| {
            let case = random_case(rng, size);
            let limit = rng.range(0..case.stream.output.len() as u32 + 1) as usize;
            (case, limit)
        },
        |(case, limit)| {
            let limits = Limits {
                max_decoded_bytes: *limit,
                ..Limits::default()
            };
            match inflate::zlib(&case.zlib, limits) {
                Ok(out) if out.len() <= *limit => Ok(()),
                Err(CodecError::Inflate {
                    kind: ErrorKind::TooMuchOutput { limit: found },
                    ..
                }) if found == *limit && case.stream.output.len() > *limit => Ok(()),
                other => Err(format!("{other:?}")),
            }
        },
    );
}

#[test]
fn damaged_streams_are_decoded_or_rejected_without_a_panic() {
    check(
        "inflate damage",
        3000,
        |rng| {
            let mut bytes = random_case(rng, 30).zlib;
            for _ in 0..1 + rng.range(0..4u32) {
                let at = rng.range(0..bytes.len() as u32) as usize;
                bytes[at] = byte(rng);
            }
            bytes
        },
        |bytes| {
            let _ = inflate::raw(bytes, Limits::default());
            match inflate::zlib(bytes, Limits::default()) {
                Ok(_) | Err(CodecError::Inflate { .. }) => Ok(()),
                Err(other) => Err(format!("not an inflate error: {other:?}")),
            }
        },
    );
}
