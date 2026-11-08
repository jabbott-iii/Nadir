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

//! The mixer's arithmetic: the pan law, positional attenuation, linear
//! ramps and the master limiter.

use std::f32::consts::{FRAC_PI_4, SQRT_2};

/// Where the limiter starts to bend the signal: −1 dBFS.
pub(crate) const KNEE: f32 = 0.891_250_9;

/// The pan law: the gains of the left and right outputs for a source of
/// `channels` channels at `pan`, from −1 (left) to 1 (right), with
/// `θ = (pan + 1) · π/4`.
///
/// - A mono source follows the constant-power law: `(cos θ, sin θ)`, so
///   `l² + r² = 1` at every pan, and each side is at −3 dB at the centre.
/// - A stereo source follows a balance law: each channel at
///   `√2 · (cos θ, sin θ)`, capped at 1. The centre leaves both as they
///   are, the near side stays at 1, and the far side fades; no gain passes
///   1, so a stereo sound never goes past full scale on its own (its
///   samples above −1 dBFS still meet the limiter's bend).
pub(crate) fn pan_gains(pan: f32, channels: u8) -> [f32; 2] {
    let (sin, cos) = ((pan + 1.0) * FRAC_PI_4).sin_cos();
    if channels == 1 {
        [cos, sin]
    } else {
        [
            if pan <= 0.0 { 1.0 } else { cos * SQRT_2 },
            if pan >= 0.0 { 1.0 } else { sin * SQRT_2 },
        ]
    }
}

/// The gain of a sound `distance` away from the listener: 1 up to `min`,
/// falling linearly to 0 at `max`, and 0 from there.
pub(crate) fn attenuation(distance: f32, min: f32, max: f32) -> f32 {
    if distance <= min {
        1.0
    } else if distance >= max {
        0.0
    } else {
        (max - distance) / (max - min)
    }
}

/// The most the limiter gives out: the largest `f32` below 1.
pub(crate) const CEILING: f32 = 1.0 - f32::EPSILON / 2.0;

/// The master limiter: the signal as it is up to [`KNEE`], and above it a
/// soft clip shaped as `u / (1 + u)`, which meets the line with its slope
/// and approaches full scale without reaching it: never past
/// [`CEILING`]. Not a number gives 0.
pub(crate) fn limit(sample: f32) -> f32 {
    let magnitude = sample.abs();
    if magnitude <= KNEE {
        sample
    } else if magnitude.is_nan() {
        0.0
    } else {
        let over = (magnitude - KNEE) / (1.0 - KNEE);
        // `1 − 1 / (1 + u)` is `u / (1 + u)`, and stays 1 for an infinite
        // `u`. In `f32`, a large `u` rounds it to 1, hence the ceiling.
        let soft = KNEE + (1.0 - KNEE) * (1.0 - 1.0 / (1.0 + over));
        soft.min(CEILING).copysign(sample)
    }
}

/// A value that moves linearly to a target, a sample at a time: after a
/// ramp of `n` samples, the `k`-th sample is `start + (target − start) ·
/// k/n`, and the `n`-th the target itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ramp {
    start: f32,
    target: f32,
    /// The samples the ramp takes, 0 once it has reached its target.
    len: u32,
    /// The samples it has taken.
    done: u32,
}

impl Ramp {
    /// A ramp resting at `value`.
    pub(crate) const fn at(value: f32) -> Self {
        Self {
            start: value,
            target: value,
            len: 0,
            done: 0,
        }
    }

    /// The value now: of the last sample [`Ramp::next`] gave.
    pub(crate) fn value(&self) -> f32 {
        if self.len == 0 {
            self.target
        } else {
            self.start + (self.target - self.start) * (self.done as f32 / self.len as f32)
        }
    }

    /// The value the ramp is going to.
    pub(crate) fn target(&self) -> f32 {
        self.target
    }

    /// The samples left before it reaches its target.
    pub(crate) fn remaining(&self) -> u32 {
        self.len - self.done
    }

    /// Goes from where it is now to `target` over `samples` samples; at
    /// once for 0.
    pub(crate) fn to(&mut self, target: f32, samples: u32) {
        let start = self.value();
        *self = if samples == 0 {
            Self::at(target)
        } else {
            Self {
                start,
                target,
                len: samples,
                done: 0,
            }
        };
    }

    /// Takes a sample: the value it moves to.
    pub(crate) fn next(&mut self) -> f32 {
        if self.len != 0 {
            self.done += 1;
            if self.done == self.len {
                *self = Self::at(self.target);
            }
        }
        self.value()
    }
}
