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

//! Inflate benchmarks.

use std::hint::black_box;

use nadir_codec::{Limits, adler32, inflate};
use nadir_core::Pcg32;

use crate::harness::Bencher;

/// How many bytes the benchmark stream decompresses to: 10 MiB.
const LEN: usize = 10 << 20;

/// How many bytes a block of the stream decompresses to, but perhaps the
/// last. A stored block holds at most 65,535.
const BLOCK: usize = 32 * 1024;

/// How far back a match may reach: DEFLATE's window.
const WINDOW: usize = 32 * 1024;

/// `inflate_10mb`: decompresses a zlib stream of [`LEN`] bytes of mixed
/// data, as the cook tool does a PNG's image data: blocks of [`BLOCK`]
/// bytes of three kinds (see [`Kind`]), drawn at random.
///
/// The workspace has no DEFLATE encoder, so the stream is written here from
/// literals and matches drawn at random, in stored and fixed-Huffman blocks.
/// It has no dynamic blocks, which zlib writes for most data: their headers,
/// and the codes longer than 9 bits that the decoder reads a bit at a time,
/// are not timed.
pub(crate) fn zlib(bencher: &mut Bencher) {
    let (stream, _) = stream(LEN);
    bencher.iter(|| inflate::zlib(black_box(&stream), Limits::default()));
}

/// What a block of the stream holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Noise, in a stored block.
    Noise,
    /// Literals of every byte value, three in five symbols, and matches of
    /// 3 to 34 bytes from anywhere in the window, in a fixed block.
    Literals,
    /// Matches as long as 258 bytes, from a pixel or a row back, and a
    /// literal one symbol in eight, in a fixed block.
    Repeats,
}

/// The kinds of block: a fifth noise, and two fifths each of the others.
const KINDS: [Kind; 5] = [
    Kind::Noise,
    Kind::Literals,
    Kind::Literals,
    Kind::Repeats,
    Kind::Repeats,
];

/// How far back a match of a [`Kind::Repeats`] block reaches: a pixel of 1,
/// 3 or 4 bytes, or a row of RGBA pixels from 256 to 8192 wide.
const STRIDES: [usize; 9] = [1, 3, 4, 1024, 2048, 4096, 8192, 16384, 32768];

/// The benchmark stream: a zlib stream of `len` bytes of data, and the
/// data. The same every time.
fn stream(len: usize) -> (Vec<u8>, Vec<u8>) {
    let mut rng = Pcg32::new(0x7a6c_6962, 0);
    let mut data = Vec::with_capacity(len);
    // DEFLATE with a 32 KiB window, at the default level.
    let mut out = Writer {
        bytes: vec![0x78, 0x9c],
        ..Writer::default()
    };
    loop {
        let end = (data.len() + BLOCK).min(len);
        let last = end == len;
        out.bits(u32::from(last), 1);
        match rng.choose(&KINDS).copied().unwrap_or(Kind::Noise) {
            Kind::Noise => noise(&mut rng, &mut data, end, &mut out),
            kind => {
                out.bits(1, 2);
                matches(kind, &mut rng, &mut data, end, &mut out);
                out.symbol(END_OF_BLOCK);
            }
        }
        if last {
            break;
        }
    }
    out.align();
    out.bytes
        .extend_from_slice(&adler32::checksum(&data).to_be_bytes());
    (out.bytes, data)
}

/// Writes a stored block of noise that takes `data` to `end` bytes.
fn noise(rng: &mut Pcg32, data: &mut Vec<u8>, end: usize, out: &mut Writer) {
    out.bits(0, 2);
    out.align();
    let start = data.len();
    let len = u16::try_from(end - start).expect("a block is at most 65,535 bytes");
    out.bytes.extend_from_slice(&len.to_le_bytes());
    out.bytes.extend_from_slice(&(!len).to_le_bytes());
    data.extend((start..end).map(|_| rng.next_u32() as u8));
    out.bytes.extend_from_slice(&data[start..]);
}

/// Writes the literals and matches of a fixed block of `kind` that take
/// `data` to `end` bytes.
fn matches(kind: Kind, rng: &mut Pcg32, data: &mut Vec<u8>, end: usize, out: &mut Writer) {
    while data.len() < end {
        let left = end - data.len();
        let literal = match kind {
            Kind::Literals => rng.range(0..5u32) < 3,
            _ => rng.range(0..8u32) == 0,
        };
        if literal || data.is_empty() || left < 3 {
            let byte = rng.next_u32() as u8;
            out.symbol(u16::from(byte));
            data.push(byte);
            continue;
        }
        let (length, distance) = match kind {
            Kind::Literals => (
                rng.range(3..35u32) as usize,
                1 + rng.range(0..data.len().min(WINDOW) as u32) as usize,
            ),
            _ => {
                let length = match rng.range(0..2u32) {
                    0 => 258,
                    _ => rng.range(3..259u32) as usize,
                };
                let stride = rng.choose(&STRIDES).copied().unwrap_or(1);
                (length, stride.min(data.len()))
            }
        };
        let length = length.min(left);
        out.matched(length, distance);
        // A match may overlap what it writes.
        let from = data.len() - distance;
        for at in from..from + length {
            data.push(data[at]);
        }
    }
}

/// The end of a block, in the literal/length code.
const END_OF_BLOCK: u16 = 256;

/// The shortest length of each length code, 257 to 285, and how many extra
/// bits follow it (RFC 1951, 3.2.5).
const LENGTHS: [(usize, u32); 29] = [
    (3, 0),
    (4, 0),
    (5, 0),
    (6, 0),
    (7, 0),
    (8, 0),
    (9, 0),
    (10, 0),
    (11, 1),
    (13, 1),
    (15, 1),
    (17, 1),
    (19, 2),
    (23, 2),
    (27, 2),
    (31, 2),
    (35, 3),
    (43, 3),
    (51, 3),
    (59, 3),
    (67, 4),
    (83, 4),
    (99, 4),
    (115, 4),
    (131, 5),
    (163, 5),
    (195, 5),
    (227, 5),
    (258, 0),
];

/// Writes bits as DEFLATE packs them, least significant first.
#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
    /// Bits not yet in `bytes`, the first lowest.
    buffer: u64,
    count: u32,
}

impl Writer {
    /// Writes the low `count` bits of `value`, at most 32.
    fn bits(&mut self, value: u32, count: u32) {
        self.buffer |= u64::from(value) << self.count;
        self.count += count;
        while self.count >= 8 {
            self.bytes.push(self.buffer as u8);
            self.buffer >>= 8;
            self.count -= 8;
        }
    }

    /// Fills the last byte with zeros.
    fn align(&mut self) {
        if self.count > 0 {
            self.bytes.push(self.buffer as u8);
            self.buffer = 0;
            self.count = 0;
        }
    }

    /// Writes a symbol of the fixed literal/length code (RFC 1951, 3.2.6).
    fn symbol(&mut self, symbol: u16) {
        let symbol = u32::from(symbol);
        let (code, length) = match symbol {
            0..=143 => (0x30 + symbol, 8),
            144..=255 => (0x190 + symbol - 144, 9),
            256..=279 => (symbol - 256, 7),
            _ => (0xc0 + symbol - 280, 8),
        };
        self.code(code, length);
    }

    /// Writes a match of 3 to 258 bytes from 1 to 32,768 back, with the
    /// fixed codes.
    fn matched(&mut self, length: usize, distance: usize) {
        let index = if length == 258 {
            LENGTHS.len() - 1
        } else {
            LENGTHS
                .iter()
                .rposition(|&(base, _)| base <= length)
                .expect("a length of at least 3")
        };
        let (base, extra) = LENGTHS[index];
        self.symbol(257 + index as u16);
        self.bits((length - base) as u32, extra);

        // Distance codes 0 to 3 have no extra bits; then each pair has one
        // more than the pair before.
        let mut base = 1;
        for code in 0..30 {
            let extra = if code < 4 { 0 } else { code / 2 - 1 };
            if distance < base + (1 << extra) {
                self.code(code, 5);
                self.bits((distance - base) as u32, extra);
                return;
            }
            base += 1 << extra;
        }
        unreachable!("a distance of at most 32,768");
    }

    /// Writes a Huffman code, its most significant bit first.
    fn code(&mut self, code: u32, length: u32) {
        self.bits(code.reverse_bits() >> (32 - length), length);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Config;

    #[test]
    fn the_stream_decompresses_to_the_data() {
        // Twenty blocks and a short one, the smallest streams, and streams
        // that end at a block's end or just after it.
        for len in [20 * BLOCK + 5, 0, 1, 2, 3, 4, BLOCK - 1, BLOCK, BLOCK + 1] {
            let (zlib, data) = stream(len);
            assert_eq!(data.len(), len);
            assert!(
                inflate::zlib(&zlib, Limits::default()).unwrap() == data,
                "{len}"
            );
        }
    }

    #[test]
    fn the_stream_is_the_same_every_time() {
        assert!(stream(5 * BLOCK) == stream(5 * BLOCK));
    }

    #[test]
    fn the_benchmark_stream_is_mixed() {
        let (zlib, data) = stream(LEN);
        assert!(inflate::zlib(&zlib, Limits::default()).unwrap() == data);
        // About a third of the size of the data: the noise, about a fifth
        // of it, does not compress, and the rest compresses well.
        assert!(
            (LEN / 4..LEN * 2 / 5).contains(&zlib.len()),
            "{}",
            zlib.len()
        );
    }

    #[test]
    fn the_benchmark_times_the_decompressor() {
        let mut bencher = Bencher::new(Config::fast());
        zlib(&mut bencher);
        let measurement = bencher.finish().unwrap();
        assert!(measurement.median_ns() > 0.0);
    }
}
