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

//! QOA tests: the vectors in `tests/qoa/`, frames decoded alone, files cut
//! short or damaged, and the round trip's signal-to-noise ratio. The error
//! cases, one by one, are beside the codec, in `src/qoa.rs`.
//!
//! `tests/qoa/make_vectors.py` wrote the vectors with an encoder of its
//! own, a port of the reference's that shares no code with this crate: the
//! samples it encoded, the file, and the samples it reconstructed as it
//! chose each residual, which the file must decode to.

use std::f64::consts::TAU;

use nadir_codec::qoa::{self, FRAME_LEN, Header};
use nadir_codec::{CodecError, Limits};
use nadir_core::rng::Pcg32;
use nadir_core::testing::check;

const VECTORS: &str = include_str!("qoa/vectors.txt");

fn vector(name: &str, extension: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/qoa/{name}.{extension}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn samples(name: &str, extension: &str) -> Vec<i16> {
    vector(name, extension)
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&bytes| i16::from_le_bytes(bytes))
        .collect()
}

/// Each vector's name and header.
fn vectors() -> Vec<(String, Header)> {
    VECTORS
        .lines()
        .map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let header = Header {
                channels: fields[1].parse().expect("channels"),
                sample_rate: fields[2].parse().expect("a rate"),
                samples: fields[3].parse().expect("samples"),
            };
            (fields[0].to_string(), header)
        })
        .collect()
}

fn decode(bytes: &[u8]) -> Result<qoa::Sound, CodecError> {
    qoa::decode(bytes, Limits::default())
}

#[test]
fn every_vector_decodes_to_the_samples_its_encoder_reconstructed() {
    let all = vectors();
    for (name, header) in &all {
        let sound = decode(&vector(name, "qoa")).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(sound.header, *header, "{name}");
        assert!(
            sound.samples == samples(name, "i16"),
            "{name}: other samples"
        );
    }
    assert_eq!(all.len(), 13);
}

#[test]
fn the_encoder_writes_every_vector_byte_for_byte() {
    for (name, header) in vectors() {
        let file = qoa::encode(&header, &samples(&name, "src")).unwrap();
        assert!(file == vector(&name, "qoa"), "{name}: other bytes");
    }
}

#[test]
fn every_frame_decodes_alone_and_in_any_order() {
    for (name, header) in vectors() {
        let file = vector(&name, "qoa");
        let whole = decode(&file).unwrap().samples;
        let channels = usize::from(header.channels);
        let mut starts = vec![8];
        let mut out = vec![0; FRAME_LEN * channels];
        loop {
            let frame =
                qoa::decode_frame(&file, &header, *starts.last().unwrap(), &mut out).unwrap();
            if frame.next == file.len() {
                break;
            }
            starts.push(frame.next);
        }
        // Last first, into a buffer left dirty by the frame before.
        for (index, &at) in starts.iter().enumerate().rev() {
            out.fill(0x5555);
            let frame = qoa::decode_frame(&file, &header, at, &mut out).unwrap();
            let first = index * FRAME_LEN * channels;
            let len = frame.samples * channels;
            assert_eq!(
                out[..len],
                whole[first..first + len],
                "{name}, frame {index}"
            );
            assert!(
                out[len..].iter().all(|&sample| sample == 0x5555),
                "{name}, frame {index}"
            );
        }
    }
}

#[test]
fn every_vector_cut_short_or_lengthened_is_rejected() {
    for (name, ..) in vectors() {
        let bytes = vector(&name, "qoa");
        for len in 0..bytes.len() {
            match decode(&bytes[..len]) {
                Err(CodecError::Qoa { .. }) => {}
                other => panic!("{name}, {len} bytes: {other:?}"),
            }
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(
            decode(&longer),
            Err(CodecError::Qoa {
                offset: bytes.len(),
                kind: qoa::ErrorKind::TrailingBytes
            }),
            "{name}"
        );
    }
}

#[test]
fn damaged_vectors_are_decoded_or_rejected_without_a_panic() {
    let files: Vec<Vec<u8>> = vectors()
        .iter()
        .map(|(name, _)| vector(name, "qoa"))
        .collect();
    check(
        "qoa damage",
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
            Ok(sound) => {
                let expected = sound.header.samples as usize * usize::from(sound.header.channels);
                if sound.samples.len() == expected {
                    Ok(())
                } else {
                    Err(format!("{} samples, not {expected}", sound.samples.len()))
                }
            }
            Err(CodecError::Qoa { .. }) => Ok(()),
            Err(other) => Err(format!("not a QOA error: {other:?}")),
        },
    );
}

#[test]
fn a_sound_past_the_limit_is_refused_before_it_is_decoded() {
    let file = vector("stereo_frames", "qoa");
    // 5121 samples of 2 channels, 2 bytes each.
    let limits = |max_decoded_bytes| Limits {
        max_decoded_bytes,
        ..Limits::default()
    };
    assert!(qoa::decode(&file, limits(20_484)).is_ok());
    assert_eq!(
        qoa::decode(&file, limits(20_483)),
        Err(CodecError::Qoa {
            offset: 4,
            kind: qoa::ErrorKind::TooManyBytes { limit: 20_483 }
        })
    );
}

/// A second of a signal, `channels` interleaved, at `rate`.
struct Signal {
    name: &'static str,
    channels: u8,
    rate: u32,
    samples: Vec<i16>,
}

fn signal(
    name: &'static str,
    channels: u8,
    rate: u32,
    mut sample: impl FnMut(f64, usize) -> f64,
) -> Signal {
    let mut samples = Vec::new();
    for i in 0..rate {
        let t = f64::from(i) / f64::from(rate);
        for channel in 0..usize::from(channels) {
            samples.push(sample(t, channel).round().clamp(-32768.0, 32767.0) as i16);
        }
    }
    Signal {
        name,
        channels,
        rate,
        samples,
    }
}

fn signals() -> Vec<Signal> {
    let mut rng = Pcg32::new(0x514f_4153_4e52, 1);
    let mut noise = move || f64::from(rng.next_u32()) / f64::from(u32::MAX) * 2.0 - 1.0;
    let mut pluck_noise = {
        let mut rng = Pcg32::new(0x514f_4153_4e52, 2);
        move || f64::from(rng.next_u32()) / f64::from(u32::MAX) * 2.0 - 1.0
    };
    vec![
        // A tone at -6 dBFS.
        signal("tone", 1, 44_100, |t, _| 16_384.0 * (TAU * 440.0 * t).sin()),
        // A chord of three notes and their harmonics, each channel its own
        // voicing.
        signal("chord", 2, 48_000, |t, channel| {
            let notes: [f64; 3] = if channel == 0 {
                [220.0, 277.18, 329.63]
            } else {
                [164.81, 246.94, 392.0]
            };
            notes
                .iter()
                .flat_map(|&f| {
                    (1..=4).map(move |h| (TAU * f * f64::from(h) * t).sin() / f64::from(h))
                })
                .sum::<f64>()
                * 3_500.0
        }),
        // An exponential sweep from 20 Hz to 20 kHz.
        signal("sweep", 1, 44_100, |t, _| {
            let k = (20_000.0f64 / 20.0).ln();
            12_000.0 * (TAU * 20.0 / k * ((k * t).exp() - 1.0)).sin()
        }),
        // White noise, which nothing predicts: the format's worst case.
        signal("noise", 1, 44_100, |_, _| 8_000.0 * noise()),
        // A tone at -50 dBFS, which the smallest scale factors encode.
        signal("quiet", 1, 44_100, |t, _| 100.0 * (TAU * 1_000.0 * t).sin()),
        // A square wave near full scale: its edges.
        signal("square", 1, 44_100, |t, _| {
            if (t * 100.0).fract() < 0.5 {
                30_000.0
            } else {
                -30_000.0
            }
        }),
        // Plucks: bursts of noise that die away, four a second.
        signal("plucks", 1, 44_100, |t, _| {
            let since = (t * 4.0).fract() / 4.0;
            20_000.0 * (-since * 30.0).exp() * pluck_noise()
        }),
    ]
}

/// The signal-to-noise ratio of `decoded` to `original`, in dB.
fn snr(original: &[i16], decoded: &[i16]) -> f64 {
    let signal: f64 = original.iter().map(|&x| f64::from(x).powi(2)).sum();
    let noise: f64 = original
        .iter()
        .zip(decoded)
        .map(|(&x, &y)| (f64::from(x) - f64::from(y)).powi(2))
        .sum();
    10.0 * (signal / noise).log10()
}

#[test]
fn the_round_trip_keeps_each_signal_above_its_measured_snr() {
    // Measured on 2026-11-07 with this encoder, each at 3.23 to 3.24 bits a
    // sample: tone 71.51 dB, chord 58.73, sweep 64.53, noise 16.44, quiet
    // 39.99, square 19.24, plucks 13.48. Each threshold is its measurement
    // rounded down to the dB. The encoder is deterministic, so a lower
    // result is a change in what it writes. Prediction is what lifts the
    // tones above noise's 16 dB, which 3 bits of residual give alone; the
    // square's edges and the plucks' onsets are where it fails, and their
    // error is there.
    let thresholds = [
        ("tone", 71.0),
        ("chord", 58.0),
        ("sweep", 64.0),
        ("noise", 16.0),
        ("quiet", 39.0),
        ("square", 19.0),
        ("plucks", 13.0),
    ];
    let signals = signals();
    assert_eq!(signals.len(), thresholds.len());
    for (signal, (name, threshold)) in signals.iter().zip(thresholds) {
        assert_eq!(signal.name, name);
        let header = Header {
            channels: signal.channels,
            sample_rate: signal.rate,
            samples: signal.rate,
        };
        let file = qoa::encode(&header, &signal.samples).unwrap();
        let sound = decode(&file).unwrap();
        assert_eq!(sound.header, header);
        let measured = snr(&signal.samples, &sound.samples);
        assert!(
            measured >= threshold,
            "{name}: {measured:.2} dB, below {threshold} dB"
        );
        // 3.2 bits a sample, and the frames' headers.
        let bits = (file.len() * 8) as f64 / signal.samples.len() as f64;
        assert!((3.2..3.25).contains(&bits), "{name}: {bits} bits a sample");
    }
}
