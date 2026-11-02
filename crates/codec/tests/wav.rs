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

//! WAV tests: the vectors in `tests/wav/`, files cut short, and damaged
//! files. The error cases, one by one, are beside the decoder, in
//! `src/wav.rs`.
//!
//! `tests/wav/make_vectors.py` wrote the vectors, with a RIFF writer of its
//! own and the expected samples computed from the ones it wrote. libsndfile
//! reads every vector to the same samples.

use nadir_codec::wav::{self, SampleFormat};
use nadir_codec::{CodecError, Limits};
use nadir_core::testing::check;

const VECTORS: &str = include_str!("wav/vectors.txt");

fn vector(name: &str, extension: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/wav/{name}.{extension}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn vectors() -> Vec<(String, u32, u16, SampleFormat)> {
    VECTORS
        .lines()
        .map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let format = match fields[3] {
                "Int16" => SampleFormat::Int16,
                "Int24" => SampleFormat::Int24,
                "Float32" => SampleFormat::Float32,
                other => panic!("{other}"),
            };
            (
                fields[0].to_string(),
                fields[1].parse().expect("a rate"),
                fields[2].parse().expect("channels"),
                format,
            )
        })
        .collect()
}

fn decode(bytes: &[u8]) -> Result<wav::Wave, CodecError> {
    wav::decode(bytes, Limits::default())
}

#[test]
fn every_vector_decodes_to_its_samples() {
    let all = vectors();
    for (name, rate, channels, format) in &all {
        let wave = decode(&vector(name, "wav")).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            (wave.sample_rate, wave.channels, wave.format),
            (*rate, *channels, *format),
            "{name}"
        );
        let expected: Vec<u32> = vector(name, "f32")
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&bytes| u32::from_le_bytes(bytes))
            .collect();
        // Bit for bit, so that -0.0 and subnormals are checked too.
        let got: Vec<u32> = wave.samples.iter().map(|x| x.to_bits()).collect();
        assert!(got == expected, "{name}: other samples");
    }
    assert_eq!(all.len(), 21);
}

#[test]
fn every_vector_cut_short_is_rejected() {
    for (name, ..) in vectors() {
        let bytes = vector(&name, "wav");
        for len in 0..bytes.len() {
            match decode(&bytes[..len]) {
                Err(CodecError::Wav { .. }) => {}
                other => panic!("{name}, {len} bytes: {other:?}"),
            }
        }
    }
}

#[test]
fn damaged_vectors_are_decoded_or_rejected_without_a_panic() {
    let files: Vec<Vec<u8>> = vectors()
        .iter()
        .map(|(name, ..)| vector(name, "wav"))
        .collect();
    check(
        "wav damage",
        3000,
        |rng| {
            let mut bytes = rng.choose(&files).expect("vectors").clone();
            for _ in 0..1 + rng.range(0..4u32) {
                let at = rng.range(0..bytes.len() as u32) as usize;
                bytes[at] = rng.range(0..256u32) as u8;
            }
            bytes
        },
        |bytes| match decode(bytes) {
            Ok(wave) if wave.samples.len() % usize::from(wave.channels) == 0 => Ok(()),
            Ok(wave) => Err(format!(
                "{} samples in {} channels",
                wave.samples.len(),
                wave.channels
            )),
            Err(CodecError::Wav { .. }) => Ok(()),
            Err(other) => Err(format!("not a WAV error: {other:?}")),
        },
    );
}
