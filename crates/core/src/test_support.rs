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
