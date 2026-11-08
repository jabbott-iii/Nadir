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

//! Sounds' samples between formats: [`resample`] changes their rate, and
//! [`to_i16`] makes 16-bit samples of `f32` ones, as the cook writes them.
//!
//! ```
//! use nadir_codec::Limits;
//! use nadir_codec::pcm;
//!
//! // A tenth of a second of a 1 kHz tone at 44.1 kHz, in stereo.
//! let tone: Vec<f32> = (0..4410)
//!     .flat_map(|i| {
//!         let x = (f64::from(i) * 1000.0 / 44_100.0 * std::f64::consts::TAU).sin() as f32;
//!         [x * 0.5, 0.0]
//!     })
//!     .collect();
//! let at_48k = pcm::resample(&tone, 2, 44_100, 48_000, false, Limits::default())?;
//! assert_eq!(at_48k.len(), 4800 * 2);
//! // The right channel stays silent.
//! assert!(at_48k.iter().skip(1).step_by(2).all(|&x| x == 0.0));
//! assert_eq!(pcm::to_i16(0.5), 16384);
//! # Ok::<(), nadir_codec::CodecError>(())
//! ```
//!
//! # The resampler
//!
//! A windowed sinc, for the cook, which can take its time: each output
//! sample is the input's samples weighed by `sinc` over 32 of its zero
//! crossings each side, under a Kaiser window of β = 9, with its cutoff
//! (−6 dB) at 0.91 of the lower of the two rates' Nyquist frequencies.
//! Measured, alike at every ratio tried, from 384,000 Hz to 8,000 and
//! back: a tone up to 0.82 of the lower Nyquist frequency keeps its level
//! within 0.00021 dB and stays 92 dB or more below the tone itself; at
//! 0.84 it loses 0.015 dB, and at 0.86, 0.24 dB; a tone 1% above the
//! output's Nyquist frequency, which would fold back, is down by 91 dB.
//! Each output sample's weights are divided by their sum, so that a
//! constant stays the constant. Ten seconds of mono at 44.1 kHz take about
//! 0.4 s to 48 kHz in an optimized build.
//!
//! - **Length:** the output has `frames × to / from` frames, rounded to the
//!   nearest, and output frame `n` is at input frame `n × frames /
//!   output_frames`, kept as a whole number and a remainder, so that nothing
//!   drifts and the output spans the input exactly.
//! - **Edges:** past either end of a sound, its samples are 0; a `looped`
//!   one goes on from its other end, so that the loop's seam is resampled
//!   as any other point, and its length is exact: the pitch moves by at most
//!   half an output frame over the whole loop.
//! - **Equal rates** give the samples as they are.
//! - **The output can pass ±1** where the input's peaks fell between its
//!   samples: [`to_i16`] clamps.
//! - **Determinism:** the same samples give the same output, bit for bit,
//!   on every system. It computes in `f64` with `+`, `−`, `×`, `÷`,
//!   `sqrt` and `floor` only, which IEEE 754 fixes and Rust never fuses or
//!   reorders: `sin` is a polynomial of this module's, and the Kaiser
//!   window's I₀ a running product, as the system's `sin`, `exp` and
//!   `powi` may differ in their last bit.

use std::f64::consts::PI;
use std::fmt;

use crate::{CodecError, Limits};

/// The zero crossings of the kernel on each side.
const ZERO_CROSSINGS: usize = 32;

/// The kernel's points a zero crossing, between which it is interpolated.
const OVERSAMPLE: usize = 512;

/// The cutoff, as a share of the lower Nyquist frequency.
const CUTOFF: f64 = 0.91;

/// The Kaiser window's β.
const BETA: f64 = 9.0;

/// The highest rate [`resample`] takes, in frames a second: the mixer's,
/// as the sound file's ([`kpcm::MAX_SAMPLE_RATE`](crate::kpcm::MAX_SAMPLE_RATE)).
pub const MAX_RATE: u32 = crate::kpcm::MAX_SAMPLE_RATE;

/// The most [`resample`] divides a rate by: 384,000 Hz to 8,000, which
/// bounds the kernel to 1,688 input frames each side of a point.
pub const MAX_FALL: u32 = 48;

/// What is wrong with what [`resample`] was given. See
/// [`CodecError::Pcm`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// No channels.
    NoChannels,
    /// A rate of 0, or above [`MAX_RATE`].
    BadRate {
        /// The rate.
        found: u32,
    },
    /// A fall in rate of more than [`MAX_FALL`] times, whose kernel would
    /// reach too far.
    TooFarDown {
        /// The rate from.
        from: u32,
        /// The rate to.
        to: u32,
    },
    /// The samples are not a whole number of frames.
    PartialFrame {
        /// The samples given.
        samples: usize,
        /// The channels of a frame.
        channels: usize,
    },
    /// The output would take more than [`Limits::max_decoded_bytes`], at 4
    /// bytes a sample.
    TooManyBytes {
        /// The limit that was exceeded.
        limit: usize,
    },
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoChannels => f.write_str("there are no channels"),
            Self::BadRate { found } => {
                write!(f, "a rate of {found} Hz, not 1 to {MAX_RATE}")
            }
            Self::TooFarDown { from, to } => {
                write!(f, "{from} Hz to {to} Hz falls more than {MAX_FALL} times")
            }
            Self::PartialFrame { samples, channels } => write!(
                f,
                "{samples} samples are not whole frames of {channels} channels"
            ),
            Self::TooManyBytes { limit } => write!(
                f,
                "the output would take more than the limit of {limit} bytes"
            ),
        }
    }
}

fn error(kind: ErrorKind) -> CodecError {
    CodecError::Pcm { kind }
}

/// The frames that [`resample`] makes of `frames` frames from `from` Hz to
/// `to`: `frames × to / from`, rounded to the nearest, and 1 or more of
/// any. `None` where a rate is 0.
pub fn resampled_frames(frames: u64, from: u32, to: u32) -> Option<u64> {
    if from == 0 || to == 0 {
        return None;
    }
    if frames == 0 {
        return Some(0);
    }
    let scaled = u128::from(frames) * u128::from(to) + u128::from(from / 2);
    let out = scaled / u128::from(from);
    Some(u64::try_from(out).unwrap_or(u64::MAX).max(1))
}

/// Resamples `samples`, interleaved frames of `channels`, from `from` to
/// `to` frames a second; `looped` for a sound that repeats. See the
/// module's documentation.
///
/// # Errors
///
/// A [`CodecError::Pcm`] for no channels, a rate of 0 or above [`MAX_RATE`],
/// a fall of more than [`MAX_FALL`] times, samples that are not whole
/// frames, or an output of more than `limits.max_decoded_bytes` at 4 bytes a
/// sample, checked in that order, before anything is allocated.
pub fn resample(
    samples: &[f32],
    channels: usize,
    from: u32,
    to: u32,
    looped: bool,
    limits: Limits,
) -> Result<Vec<f32>, CodecError> {
    if channels == 0 {
        return Err(error(ErrorKind::NoChannels));
    }
    for rate in [from, to] {
        if !(1..=MAX_RATE).contains(&rate) {
            return Err(error(ErrorKind::BadRate { found: rate }));
        }
    }
    if from > to.saturating_mul(MAX_FALL) {
        return Err(error(ErrorKind::TooFarDown { from, to }));
    }
    if !samples.len().is_multiple_of(channels) {
        return Err(error(ErrorKind::PartialFrame {
            samples: samples.len(),
            channels,
        }));
    }
    let frames = samples.len() / channels;
    let out_frames = resampled_frames(frames as u64, from, to).unwrap_or(0);
    let too_many = error(ErrorKind::TooManyBytes {
        limit: limits.max_decoded_bytes,
    });
    let out_samples = u128::from(out_frames) * channels as u128;
    if out_samples * 4 > limits.max_decoded_bytes as u128 {
        return Err(too_many);
    }
    let Ok(out_frames) = usize::try_from(out_frames) else {
        return Err(too_many);
    };
    if from == to || frames == 0 {
        return Ok(samples.to_vec());
    }

    let kernel = Kernel::new(from, to);
    let mut out = Vec::with_capacity(out_frames * channels);
    let mut weights = Vec::with_capacity(2 * kernel.reach + 2);
    let mut sums = vec![0.0f64; channels];
    let (frames_wide, out_wide) = (frames as u128, out_frames as u128);
    for n in 0..out_frames {
        // Output frame n is at input frame n × frames / out_frames.
        let at = n as u128 * frames_wide;
        let whole = (at / out_wide) as usize;
        let fraction = (at % out_wide) as f64 / out_frames as f64;
        let first = whole as i64 - kernel.reach as i64 + 1;
        let last = whole as i64 + kernel.reach as i64;
        weights.clear();
        let mut total = 0.0;
        for j in first..=last {
            let weight = kernel.at((j - whole as i64) as f64 - fraction);
            weights.push(weight);
            total += weight;
        }
        sums.fill(0.0);
        for (j, &weight) in (first..=last).zip(&weights) {
            let frame = if looped {
                j.rem_euclid(frames as i64) as usize
            } else if (0..frames as i64).contains(&j) {
                j as usize
            } else {
                continue;
            };
            let frame = &samples[frame * channels..][..channels];
            for (sum, &sample) in sums.iter_mut().zip(frame) {
                *sum += weight * f64::from(sample);
            }
        }
        out.extend(sums.iter().map(|&sum| (sum / total) as f32));
    }
    Ok(out)
}

/// A 16-bit sample of `sample`: `sample × 32768`, rounded to the nearest,
/// half away from 0, and clamped to −32768 … 32767. A sample the WAV reader
/// read from 16 bits comes back as it was.
pub fn to_i16(sample: f32) -> i16 {
    (f64::from(sample) * 32768.0)
        .round()
        .clamp(-32768.0, 32767.0) as i16
}

/// The windowed sinc, tabulated.
struct Kernel {
    /// `sinc(u) × kaiser(u / ZERO_CROSSINGS)` at `u = i / OVERSAMPLE`, for
    /// `i` of 0 to `ZERO_CROSSINGS × OVERSAMPLE`, then a 0.
    table: Vec<f64>,
    /// The cutoff, as a share of the input's Nyquist frequency.
    cutoff: f64,
    /// The input frames the kernel reaches on each side of a point.
    reach: usize,
}

impl Kernel {
    fn new(from: u32, to: u32) -> Self {
        let ratio = (f64::from(to) / f64::from(from)).min(1.0);
        let cutoff = CUTOFF * ratio;
        let reach = (ZERO_CROSSINGS as f64 / cutoff).floor() as usize + 1;
        let norm = bessel_i0(BETA);
        let points = ZERO_CROSSINGS * OVERSAMPLE;
        let mut table = Vec::with_capacity(points + 2);
        for i in 0..=points {
            let u = i as f64 / OVERSAMPLE as f64;
            let t = u / ZERO_CROSSINGS as f64;
            let window = bessel_i0(BETA * (1.0 - t * t).max(0.0).sqrt()) / norm;
            table.push(sinc(u) * window);
        }
        table.push(0.0);
        Self {
            table,
            cutoff,
            reach,
        }
    }

    /// The weight of an input frame `distance` input frames from the point.
    fn at(&self, distance: f64) -> f64 {
        let u = (distance * self.cutoff).abs() * OVERSAMPLE as f64;
        let index = u.floor();
        if index >= (ZERO_CROSSINGS * OVERSAMPLE) as f64 {
            return 0.0;
        }
        let i = index as usize;
        let fraction = u - index;
        let (a, b) = (self.table[i], self.table[i + 1]);
        a + (b - a) * fraction
    }
}

/// `sin(πu) / (πu)`, and 1 at 0.
fn sinc(u: f64) -> f64 {
    if u == 0.0 { 1.0 } else { sin_pi(u) / (PI * u) }
}

/// `sin(πx)`, from a polynomial: exact at whole and half numbers, and the
/// same bits on every system, for |x| below 2⁵², where the reduction below
/// is exact. The kernel takes it to 32.
fn sin_pi(x: f64) -> f64 {
    // x = 2k + r, with r in −1 … 1, exactly: halving, floor and doubling
    // are exact.
    let r = x - 2.0 * (x / 2.0 + 0.5).floor();
    // sin(π(1 − r)) = sin(πr): fold r into −½ … ½, where ±½ are ±1, which
    // the polynomial misses by a bit, π/2 not being a double.
    let r = if r > 0.5 {
        1.0 - r
    } else if r < -0.5 {
        -1.0 - r
    } else {
        r
    };
    if r.abs() == 0.5 {
        return r * 2.0;
    }
    let y = PI * r;
    let y2 = y * y;
    // Taylor's series to y²¹/21!, whose next term is below 1e−17 for
    // |y| ≤ π/2; Horner's rule in y².
    let mut sum = 0.0;
    for k in (1..=10).rev() {
        let n = (2 * k) as f64;
        sum = 1.0 - y2 / (n * (n + 1.0)) * sum;
    }
    y * sum
}

/// The modified Bessel function of the first kind, of order 0: the sum of
/// `((x/2)^k / k!)²`, 40 terms, each from the last.
fn bessel_i0(x: f64) -> f64 {
    let half = x / 2.0;
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..40 {
        term *= half / k as f64;
        sum += term * term;
    }
    sum
}

#[cfg(test)]
mod tests {
    use nadir_core::hash::fnv1a64;

    use super::*;

    /// A tone, from this module's `sin`, so that it is the same on every
    /// system.
    fn tone(frames: usize, rate: f64, hz: f64, level: f64) -> Vec<f32> {
        (0..frames)
            .map(|i| (sin_pi(i as f64 * hz / rate * 2.0) * level) as f32)
            .collect()
    }

    fn mono(samples: &[f32], from: u32, to: u32, looped: bool) -> Vec<f32> {
        resample(samples, 1, from, to, looped, Limits::default())
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// The level of `samples` against `reference`, in dB: the error's RMS
    /// over the reference's.
    fn error_db(samples: &[f32], reference: &[f64]) -> f64 {
        let (mut error, mut power) = (0.0, 0.0);
        for (&a, &b) in samples.iter().zip(reference) {
            error += (f64::from(a) - b).powi(2);
            power += b * b;
        }
        10.0 * (error / power).log10()
    }

    /// The RMS of `samples`, in dB of full scale.
    fn level_db(samples: &[f32]) -> f64 {
        let power: f64 = samples.iter().map(|&x| f64::from(x).powi(2)).sum();
        10.0 * (power / samples.len() as f64).log10()
    }

    #[test]
    fn sin_pi_is_exact_where_it_can_be_and_close_elsewhere() {
        for n in -40..=40 {
            let n = f64::from(n);
            assert_eq!(sin_pi(n), 0.0, "{n}");
            let half = sin_pi(n + 0.5);
            assert_eq!(half.abs(), 1.0, "{n} + ½");
            assert_eq!(half, if n.rem_euclid(2.0) == 0.0 { 1.0 } else { -1.0 });
        }
        assert!((sin_pi(1.0 / 6.0) - 0.5).abs() <= f64::EPSILON / 2.0);
        for i in -1000..=1000 {
            let r = f64::from(i) / 2000.0;
            assert_eq!(sin_pi(-r), -sin_pi(r));
            assert!((sin_pi(r) - (PI * r).sin()).abs() < 1e-15, "{r}");
            // Periodic: the same after any whole number of turns.
            assert!((sin_pi(r + 24.0) - sin_pi(r)).abs() < 1e-14, "{r}");
        }
    }

    #[test]
    fn bessel_i0_has_its_known_values() {
        assert_eq!(bessel_i0(0.0), 1.0);
        assert!((bessel_i0(1.0) - 1.266_065_877_752_008_4).abs() < 1e-15);
        assert!((bessel_i0(9.0) / 1_093.588_354_511_375 - 1.0).abs() < 1e-14);
    }

    #[test]
    fn the_output_spans_the_input_at_the_new_rate() {
        assert_eq!(resampled_frames(44_100, 44_100, 48_000), Some(48_000));
        assert_eq!(resampled_frames(10_000, 44_100, 48_000), Some(10_884));
        assert_eq!(resampled_frames(1, 48_000, 8_000), Some(1));
        assert_eq!(resampled_frames(0, 48_000, 8_000), Some(0));
        assert_eq!(resampled_frames(5, 0, 8_000), None);
        assert_eq!(resampled_frames(u64::MAX, 1, u32::MAX), Some(u64::MAX));
        let out = resample(&[0.0; 30], 3, 44_100, 22_050, false, Limits::default());
        assert_eq!(out.map(|out| out.len()), Ok(15));
    }

    #[test]
    fn equal_rates_and_no_samples_give_what_was_given() {
        let samples = [0.25, -1.0, 0.75];
        assert_eq!(mono(&samples, 48_000, 48_000, false), samples);
        assert_eq!(mono(&[], 44_100, 48_000, true), []);
    }

    #[test]
    fn what_cannot_be_resampled_is_refused_before_anything_is_allocated() {
        let kind = |result: Result<Vec<f32>, CodecError>| match result {
            Err(CodecError::Pcm { kind }) => kind,
            other => panic!("{other:?}"),
        };
        let limits = Limits::default();
        assert_eq!(
            kind(resample(&[0.0], 0, 1, 1, false, limits)),
            ErrorKind::NoChannels
        );
        for (from, to, found) in [
            (0, 1, 0),
            (48_000, 0, 0),
            (48_000, MAX_RATE + 1, 384_001),
            (u32::MAX, 1, u32::MAX),
        ] {
            assert_eq!(
                kind(resample(&[0.0], 1, from, to, false, limits)),
                ErrorKind::BadRate { found },
                "{from} → {to}"
            );
        }
        // The kernel's reach, and so its weights, are bounded.
        assert_eq!(
            kind(resample(&[0.0], 1, 384_000, 7_999, false, limits)),
            ErrorKind::TooFarDown {
                from: 384_000,
                to: 7_999
            }
        );
        assert_eq!(
            resample(&[0.0; 96], 1, 384_000, 8_000, false, limits).map(|out| out.len()),
            Ok(2)
        );
        assert_eq!(
            kind(resample(&[0.0; 3], 2, 1, 2, false, limits)),
            ErrorKind::PartialFrame {
                samples: 3,
                channels: 2
            }
        );
        // A second at 1 Hz, made 384,000: the limit, not the memory.
        let tight = Limits {
            max_decoded_bytes: 384_000 * 4 - 1,
            ..limits
        };
        assert_eq!(
            kind(resample(&[0.0], 1, 1, 384_000, false, tight)),
            ErrorKind::TooManyBytes {
                limit: 384_000 * 4 - 1
            }
        );
        // The limit itself is not past it.
        let exact = Limits {
            max_decoded_bytes: 384_000 * 4,
            ..limits
        };
        assert_eq!(
            resample(&[0.0], 1, 1, 384_000, false, exact).map(|out| out.len()),
            Ok(384_000)
        );
    }

    #[test]
    fn a_tone_keeps_its_frequency_and_level_and_stays_near_the_tone_itself() {
        for (from, to, hz) in [
            (44_100, 48_000, 1000.0),
            (48_000, 44_100, 1000.0),
            (48_000, 22_050, 9000.0),
            (22_050, 48_000, 7000.0),
        ] {
            let input = tone(4410, f64::from(from), hz, 0.5);
            let out = mono(&input, from, to, false);
            let frames = out.len();
            // Away from the edges, where the sound starts and stops.
            let edge = 200;
            let middle = &out[edge..frames - edge];
            let reference: Vec<f64> = (edge..frames - edge)
                .map(|i| {
                    // Output frame i is at input frame i × in / out.
                    let at = i as f64 * input.len() as f64 / frames as f64;
                    (at * hz / f64::from(from) * 2.0 * PI).sin() * 0.5
                })
                .collect();
            let error = error_db(middle, &reference);
            assert!(error < -80.0, "{from} → {to} Hz, {hz} Hz: {error:.1} dB");
            let level = level_db(middle)
                - level_db(&reference.iter().map(|&x| x as f32).collect::<Vec<_>>());
            assert!(level.abs() < 0.01, "{from} → {to} Hz: {level:.3} dB");
        }
    }

    #[test]
    fn a_tone_above_the_new_nyquist_frequency_does_not_fold_back() {
        // 48 kHz to 22.05: Nyquist 11,025 Hz. A tone 1% above it.
        let input = tone(9600, 48_000.0, 11_025.0 * 1.01, 0.5);
        let out = mono(&input, 48_000, 22_050, false);
        let middle = &out[500..out.len() - 500];
        let level = level_db(middle) - level_db(&input);
        assert!(level < -80.0, "{level:.1} dB");
    }

    #[test]
    fn a_constant_stays_constant_and_channels_stay_apart() {
        let mut stereo = Vec::new();
        for x in tone(2000, 44_100.0, 440.0, 0.9) {
            stereo.extend([x, 0.0, 0.25]);
        }
        let out = resample(&stereo, 3, 44_100, 48_000, true, Limits::default())
            .unwrap_or_else(|error| panic!("{error}"));
        for frame in out.as_chunks::<3>().0 {
            assert_eq!(frame[1], 0.0);
            assert!((frame[2] - 0.25).abs() < 1e-6, "{}", frame[2]);
        }
    }

    #[test]
    fn a_looped_sound_keeps_its_seam() {
        // 441 Hz loops in exactly 10,000 frames at 44.1 kHz: 100 turns.
        let input = tone(10_000, 44_100.0, 441.0, 0.9);
        let out = mono(&input, 44_100, 48_000, true);
        assert_eq!(out.len(), 10_884);
        // The same 100 turns over the output, the seam included.
        let reference: Vec<f64> = (0..out.len())
            .map(|i| (i as f64 * 100.0 / out.len() as f64 * 2.0 * PI).sin() * 0.9)
            .collect();
        let error = error_db(&out, &reference);
        assert!(error < -80.0, "{error:.1} dB");
        // Around the seam alone, as well.
        let seam: Vec<f32> = out[out.len() - 50..]
            .iter()
            .chain(&out[..50])
            .copied()
            .collect();
        let near: Vec<f64> = reference[out.len() - 50..]
            .iter()
            .chain(&reference[..50])
            .copied()
            .collect();
        let error = error_db(&seam, &near);
        assert!(error < -80.0, "{error:.1} dB at the seam");
    }

    #[test]
    fn a_loop_shorter_than_the_kernel_wraps_as_often_as_it_needs() {
        let ratios = [
            (44_100, 48_000),
            (48_000, 44_100),
            (384_000, 8_000),
            (8_000, 384_000),
        ];
        for (from, to) in ratios {
            for frames in [1, 2, 3, 5] {
                let out = mono(&vec![0.25; frames], from, to, true);
                assert!(
                    out.iter().all(|&x| (x - 0.25).abs() < 1e-6),
                    "{frames} frames, {from} → {to}: {out:?}"
                );
            }
        }
        // A single cycle, as a chip tune's waveform, under the new rate's
        // Nyquist frequency: the same cycle over the output.
        for (frames, from, to) in [
            (3, 44_100, 48_000),
            (32, 44_100, 48_000),
            (5, 48_000, 44_100),
            (480, 384_000, 8_000),
            (5, 8_000, 384_000),
        ] {
            let cycle: Vec<f32> = (0..frames)
                .map(|i| (sin_pi(2.0 * i as f64 / frames as f64) * 0.5) as f32)
                .collect();
            let out = mono(&cycle, from, to, true);
            let reference: Vec<f64> = (0..out.len())
                .map(|n| sin_pi(2.0 * n as f64 / out.len() as f64) * 0.5)
                .collect();
            let error = error_db(&out, &reference);
            assert!(
                error < -80.0,
                "{frames} frames, {from} → {to}: {error:.1} dB"
            );
        }
    }

    #[test]
    fn past_either_end_of_a_sound_its_samples_are_0() {
        // Rates 48 times apart put each output frame on an input frame or
        // 48 of them between two, wherever the sound ends: the sound with
        // silence after it gives the same frames, bit for bit, as far as
        // the sound goes. The kernel reaches past both ends of both.
        let short = [0.5, -0.25, 0.75];
        let padded = [0.5, -0.25, 0.75, 0.0, 0.0, 0.0];
        let out = mono(&short, 8_000, 384_000, false);
        let longer = mono(&padded, 8_000, 384_000, false);
        assert_eq!((out.len(), longer.len()), (144, 288));
        assert_eq!(out, longer[..144]);
        let short = tone(96, 384_000.0, 3_000.0, 0.5);
        let mut padded = short.clone();
        padded.extend([0.0; 96]);
        let out = mono(&short, 384_000, 8_000, false);
        let longer = mono(&padded, 384_000, 8_000, false);
        assert_eq!((out.len(), longer.len()), (2, 4));
        assert_eq!(out, longer[..2]);
    }

    #[test]
    fn a_tone_goes_up_48_times_and_back_down_to_itself() {
        let input = tone(800, 8_000.0, 1_000.0, 0.5);
        let up = mono(&input, 8_000, 384_000, false);
        assert_eq!(up.len(), 38_400);
        // Output frame n is at input frame n / 48. Away from the edges.
        let reference: Vec<f64> = (4_800..33_600)
            .map(|n| sin_pi(n as f64 / 48.0 * 1_000.0 / 8_000.0 * 2.0) * 0.5)
            .collect();
        let error = error_db(&up[4_800..33_600], &reference);
        assert!(error < -80.0, "up: {error:.1} dB");
        let down = mono(&up, 384_000, 8_000, false);
        assert_eq!(down.len(), 800);
        let reference: Vec<f64> = input[100..700].iter().map(|&x| f64::from(x)).collect();
        let error = error_db(&down[100..700], &reference);
        assert!(error < -80.0, "back down: {error:.1} dB");
    }

    #[test]
    fn samples_round_to_16_bits_and_a_16_bit_sample_comes_back() {
        for i in -32768..=32767i32 {
            let x = i as f32 / 32768.0;
            assert_eq!(i32::from(to_i16(x)), i);
        }
        assert_eq!(to_i16(1.0), 32767);
        assert_eq!(to_i16(2.5), 32767);
        assert_eq!(to_i16(-1.0), -32768);
        assert_eq!(to_i16(-3.0), -32768);
        // Half a step rounds away from 0.
        assert_eq!(to_i16(0.5 / 32768.0), 1);
        assert_eq!(to_i16(-0.5 / 32768.0), -1);
    }

    #[test]
    fn the_same_samples_resample_to_the_same_bits_on_every_system() {
        let input: Vec<f32> = tone(3000, 44_100.0, 1234.5, 0.7)
            .into_iter()
            .zip(tone(3000, 44_100.0, 98.7, 0.4))
            .flat_map(|(a, b)| [a, b])
            .collect();
        let up = resample(&input, 2, 44_100, 48_000, false, Limits::default())
            .unwrap_or_else(|error| panic!("{error}"));
        let down = resample(&input, 2, 44_100, 22_050, true, Limits::default())
            .unwrap_or_else(|error| panic!("{error}"));
        let bits =
            |samples: &[f32]| -> Vec<u8> { samples.iter().flat_map(|x| x.to_le_bytes()).collect() };
        // Pinned on x86_64 Linux; CI checks them on Windows and on arm64
        // macOS too. The kernel's own bits are pinned below, as a last-bit
        // difference in it may not reach these.
        assert_eq!(
            fnv1a64(&bits(&up)),
            0xb60b_8e00_83c9_29f3,
            "{:#x}",
            fnv1a64(&bits(&up))
        );
        assert_eq!(
            fnv1a64(&bits(&down)),
            0x1ce1_cbfc_4b86_1d50,
            "{:#x}",
            fnv1a64(&bits(&down))
        );
    }

    #[test]
    fn the_kernel_has_the_same_bits_on_every_system() {
        // Its table and the weights between the table's points, at a ratio
        // of the cook's, as `f64`: the output, as `f32`, can hide a
        // difference in their last bits, as a system's `sin` would make.
        let bits =
            |values: &[f64]| -> Vec<u8> { values.iter().flat_map(|x| x.to_le_bytes()).collect() };
        let kernel = Kernel::new(44_100, 22_050);
        assert_eq!((kernel.table.len(), kernel.reach), (16_386, 71));
        let table = fnv1a64(&bits(&kernel.table));
        assert_eq!(table, 0xb2ef_db90_df82_d12e, "{table:#x}");
        let weights: Vec<f64> = (-7_000..=7_000)
            .map(|i| kernel.at(f64::from(i) / 97.0))
            .collect();
        let weights = fnv1a64(&bits(&weights));
        assert_eq!(weights, 0x62ff_cae3_991a_7230, "{weights:#x}");
    }
}
