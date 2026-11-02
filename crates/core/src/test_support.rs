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

//! Generators shared by this crate's property tests.

use std::f32::consts::PI;

use crate::math::{Affine2, Vec2};
use crate::rng::Pcg32;

/// A vector with both components uniform in `[low, high)`.
pub(crate) fn random_vec2(rng: &mut Pcg32, low: f32, high: f32) -> Vec2 {
    Vec2::new(rng.range(low..high), rng.range(low..high))
}

/// A well-conditioned transform: scale magnitudes in `[0.25, 4)` with random
/// signs (so mirrored transforms are covered), any angle, and a translation
/// within ±10.
pub(crate) fn random_affine2(rng: &mut Pcg32) -> Affine2 {
    let mut scale = || {
        let magnitude = rng.range(0.25..4.0f32);
        if rng.next_u32() & 1 == 0 {
            magnitude
        } else {
            -magnitude
        }
    };
    let scale = Vec2::new(scale(), scale());
    Affine2::from_scale_angle_translation(scale, rng.range(-PI..PI), random_vec2(rng, -10.0, 10.0))
}
