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

//! Sound file tests: the vectors in `tests/kpcm/`, the encoder against them,
//! and files cut short, lengthened or with a byte changed.
//!
//! `tests/kpcm/make_vectors.py` wrote the vectors, field by field from the
//! format, with no code of the reader's: files that read, each with its
//! samples in a file of their own, and files that each break one rule of
//! the format, with the error and the byte it is at.

use nadir_codec::kpcm::{self, ErrorKind, Header};
use nadir_codec::{CodecError, Limits};
use nadir_core::testing::check;

const VECTORS: &str = include_str!("kpcm/vectors.txt");

fn vector(name: &str, extension: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/kpcm/{name}.{extension}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// A vector's samples, from its `.i16`.
fn samples(name: &str) -> Vec<i16> {
    vector(name, "i16")
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| i16::from_le_bytes(pair))
        .collect()
}

fn decode(bytes: &[u8]) -> Result<kpcm::Sound, CodecError> {
    kpcm::decode(bytes, Limits::default())
}

/// The variant's name, as the vectors give it.
fn variant(kind: &ErrorKind) -> String {
    let debug = format!("{kind:?}");
    debug
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        .unwrap_or_default()
        .to_string()
}

/// The vectors that read: each name and header.
fn good() -> Vec<(&'static str, Header)> {
    VECTORS
        .lines()
        .filter_map(|line| {
            let words: Vec<&str> = line.split(' ').collect();
            let ["ok", channels, rate, frames] = words[1..] else {
                return None;
            };
            let number = |word: &str| word.parse::<u32>().expect("a number");
            Some((
                words[0],
                Header {
                    channels: u8::try_from(number(channels)).expect("a channel count"),
                    sample_rate: number(rate),
                    frames: number(frames),
                },
            ))
        })
        .collect()
}

#[test]
fn every_vector_reads_or_is_refused_as_its_script_says() {
    let mut count = 0;
    for line in VECTORS.lines() {
        let words: Vec<&str> = line.split(' ').collect();
        let name = words[0];
        let file = vector(name, "kpcm");
        match words[1..] {
            ["ok", ..] => {
                let sound = decode(&file).unwrap_or_else(|error| panic!("{name}: {error}"));
                let header = good()
                    .into_iter()
                    .find(|&(good, _)| good == name)
                    .map(|(_, header)| header);
                assert_eq!(Some(sound.header), header, "{name}");
                assert_eq!(sound.samples, samples(name), "{name}");
            }
            ["error", kind, offset] => {
                let offset: usize = offset.parse().expect("an offset");
                match decode(&file) {
                    Err(CodecError::Kpcm {
                        offset: at,
                        kind: found,
                    }) => {
                        assert_eq!((variant(&found).as_str(), at), (kind, offset), "{name}");
                    }
                    other => panic!("{name}: expected a KPCM error, found {other:?}"),
                }
            }
            _ => panic!("a line of vectors.txt: {line}"),
        }
        count += 1;
    }
    assert_eq!(count, 23);
}

#[test]
fn the_encoder_writes_every_good_vector_byte_for_byte() {
    let all = good();
    assert_eq!(all.len(), 6);
    for (name, header) in all {
        let file =
            kpcm::encode(&header, &samples(name)).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(file, vector(name, "kpcm"), "{name}");
    }
}

#[test]
fn every_good_vector_cut_short_or_lengthened_is_refused() {
    for (name, _) in good() {
        let file = vector(name, "kpcm");
        for end in 0..file.len() {
            assert!(
                matches!(decode(&file[..end]), Err(CodecError::Kpcm { .. })),
                "{name} cut to {end} bytes"
            );
        }
        let mut longer = file.clone();
        longer.push(0);
        assert_eq!(
            decode(&longer),
            Err(CodecError::Kpcm {
                offset: file.len(),
                kind: ErrorKind::TrailingBytes
            }),
            "{name}"
        );
    }
}

#[test]
fn damaged_vectors_are_read_or_refused_without_a_panic() {
    let files: Vec<Vec<u8>> = good()
        .into_iter()
        .map(|(name, _)| vector(name, "kpcm"))
        .collect();
    check(
        "kpcm damage",
        3000,
        |rng| {
            let mut file = files[rng.range(0..files.len() as u32) as usize].clone();
            for _ in 0..rng.range(1..4u32) {
                let at = rng.range(0..file.len() as u32) as usize;
                file[at] = rng.range(0..256u32) as u8;
            }
            file
        },
        |file| match decode(file) {
            // A file that reads encodes back to its bytes.
            Ok(sound) => match kpcm::encode(&sound.header, &sound.samples) {
                Ok(again) if again == *file => Ok(()),
                other => Err(format!("it read, and encoded to {other:?}")),
            },
            Err(CodecError::Kpcm { .. }) => Ok(()),
            Err(other) => Err(format!("another error: {other}")),
        },
    );
}
