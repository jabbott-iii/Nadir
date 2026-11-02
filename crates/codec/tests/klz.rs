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

//! KLZ tests: the vectors in `tests/klz/`, round trips of random and
//! repetitive data, streams cut short and damaged. The error cases, one by
//! one, are beside the codec, in `src/klz.rs`.
//!
//! `tests/klz/make_vectors.py` wrote the vectors: streams it assembled
//! block by block for the decoder, and the streams its own port of the
//! encoder writes, which [`klz::compress`] must write byte for byte.

use nadir_codec::klz::{self, ErrorKind};
use nadir_codec::{CodecError, Limits};
use nadir_core::Pcg32;
use nadir_core::testing::{check, check_sized};

const VECTORS: &str = include_str!("klz/vectors.txt");

fn vector(name: &str, extension: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/klz/{name}.{extension}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn decompress(stream: &[u8], raw_size: usize) -> Result<Vec<u8>, CodecError> {
    klz::decompress(stream, raw_size, Limits::default())
}

#[test]
fn every_vector_decodes_and_the_encoder_writes_its_own() {
    let mut count = 0;
    for line in VECTORS.lines() {
        let (name, kind) = line.split_once(' ').expect("a name and a kind");
        let (stream, data) = (vector(name, "klz"), vector(name, "bin"));
        let decoded =
            decompress(&stream, data.len()).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(decoded == data, "{name}: other bytes");
        if kind == "encoder" {
            assert!(
                klz::compress(&data) == stream,
                "{name}: the encoder writes other bytes"
            );
        }
        count += 1;
    }
    assert_eq!(count, 30);
}

/// Data with the patterns a compressor meets: runs, repeats near and far,
/// a small alphabet, and noise.
fn random_data(rng: &mut Pcg32, size: u32) -> Vec<u8> {
    let len = rng.range(0..size * 40 + 1) as usize;
    let alphabet: Vec<u8> = (0..1 + rng.range(0..6u32))
        .map(|_| rng.range(0..256u32) as u8)
        .collect();
    let mut data = Vec::with_capacity(len);
    while data.len() < len {
        match rng.range(0..5u32) {
            0 => data.push(rng.range(0..256u32) as u8),
            1 => data.push(*rng.choose(&alphabet).expect("an alphabet")),
            2 => {
                let byte = rng.range(0..256u32) as u8;
                for _ in 0..rng.range(1..300u32) {
                    data.push(byte);
                }
            }
            _ if !data.is_empty() => {
                let back = 1 + rng.range(0..data.len() as u32) as usize;
                let from = data.len() - back;
                for i in 0..rng.range(1..80u32) as usize {
                    data.push(data[from + i]);
                }
            }
            _ => {}
        }
    }
    data.truncate(len);
    data
}

#[test]
fn random_and_repetitive_data_survive_a_round_trip() {
    check_sized("klz round trip", 400, random_data, |data| {
        let stream = klz::compress(data);
        let back = decompress(&stream, data.len()).map_err(|error| format!("{error}"))?;
        if back != *data {
            return Err("other bytes".to_string());
        }
        // The same data compresses to the same stream.
        if klz::compress(data) != stream {
            return Err("a second compression differs".to_string());
        }
        Ok(())
    });
    // Long runs and a long repeat, past what the property's sizes reach.
    for data in [vec![7u8; 200_000], b"0123456789abcdef".repeat(20_000)] {
        let stream = klz::compress(&data);
        assert!(stream.len() < data.len() / 50, "{}", stream.len());
        assert_eq!(decompress(&stream, data.len()).unwrap(), data);
    }
    // A run of 1 MiB compresses by more than 254 to 1, so it is close to
    // the 255 to 1 that the decoder checks first, and must pass.
    let run = vec![0u8; 1 << 20];
    let stream = klz::compress(&run);
    assert!(stream.len() * 254 < run.len(), "{}", stream.len());
    assert_eq!(decompress(&stream, run.len()).unwrap(), run);
}

#[test]
fn compressed_noise_is_no_longer_than_one_block_of_literals() {
    check_sized(
        "klz expansion",
        200,
        |rng, size| {
            let len = rng.range(0..size * 700 + 1);
            (0..len)
                .map(|_| rng.range(0..256u32) as u8)
                .collect::<Vec<u8>>()
        },
        |data| {
            let stream = klz::compress(data);
            // A token, the literals, and a length byte for every 255 of them.
            let most = data.len() + data.len() / 255 + 2;
            if stream.len() > most {
                return Err(format!("{} bytes, more than {most}", stream.len()));
            }
            match decompress(&stream, data.len()) {
                Ok(back) if back == *data => Ok(()),
                Ok(_) => Err("other bytes".to_string()),
                Err(error) => Err(format!("{error}")),
            }
        },
    );
}

#[test]
fn every_stream_cut_short_is_rejected() {
    check_sized(
        "klz prefixes",
        100,
        |rng, size| {
            let data = random_data(rng, size / 4);
            (klz::compress(&data), data.len())
        },
        |(stream, raw_size)| {
            for len in 0..stream.len() {
                match decompress(&stream[..len], *raw_size) {
                    Err(CodecError::Klz {
                        kind:
                            ErrorKind::UnexpectedEnd { .. }
                            | ErrorKind::TooShort { .. }
                            | ErrorKind::MatchWithoutOffset,
                        ..
                    }) => {}
                    other => return Err(format!("{len} bytes give {other:?}")),
                }
            }
            Ok(())
        },
    );
}

#[test]
fn the_wrong_length_is_rejected() {
    check_sized(
        "klz lengths",
        200,
        |rng, size| {
            let data = random_data(rng, size);
            let wrong = rng.range(0..data.len() as u32 + 50) as usize;
            (klz::compress(&data), data.len(), wrong)
        },
        |(stream, raw_size, wrong)| match decompress(stream, *wrong) {
            Ok(_) if wrong == raw_size => Ok(()),
            Err(CodecError::Klz {
                kind:
                    ErrorKind::TooLong { .. }
                    | ErrorKind::TooShort { .. }
                    | ErrorKind::UnexpectedEnd { .. },
                ..
            }) if wrong != raw_size => Ok(()),
            other => Err(format!("{other:?}")),
        },
    );
}

#[test]
fn damaged_streams_are_decoded_or_rejected_without_a_panic() {
    check(
        "klz damage",
        3000,
        |rng| {
            let data = random_data(rng, 30);
            let mut stream = klz::compress(&data);
            for _ in 0..1 + rng.range(0..4u32) {
                if stream.is_empty() {
                    break;
                }
                let at = rng.range(0..stream.len() as u32) as usize;
                stream[at] = rng.range(0..256u32) as u8;
            }
            (stream, data.len())
        },
        |(stream, raw_size)| match decompress(stream, *raw_size) {
            Ok(out) if out.len() == *raw_size => Ok(()),
            Ok(out) => Err(format!("{} bytes, not {raw_size}", out.len())),
            Err(CodecError::Klz { .. }) => Ok(()),
            Err(other) => Err(format!("not a KLZ error: {other:?}")),
        },
    );
}
