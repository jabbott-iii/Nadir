//! 2D math: vectors, matrices, rectangles and scalar helpers.
//!
//! World space is +X right and +Y down. Angles are radians, and a positive
//! rotation is clockwise on screen. All math is `f32`.

mod affine2;
mod mat3;
mod rect;
mod vec2;

pub use affine2::Affine2;
pub use mat3::Mat3;
pub use rect::{IRect, Rect};
pub use vec2::{IVec2, UVec2, Vec2};

use std::f32::consts::{PI, TAU};

/// Linearly interpolates from `a` (at `t = 0`) to `b` (at `t = 1`).
///
/// `t` is not clamped, so values outside `[0, 1]` extrapolate.
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Returns `true` if `a` and `b` differ by at most `epsilon`.
pub fn approx_eq(a: f32, b: f32, epsilon: f32) -> bool {
    (a - b).abs() <= epsilon
}

/// Clamps `value` to `[min, max]`.
///
/// Unlike [`f32::clamp`], this never panics: if `min > max` it returns `max`,
/// and a NaN `value` yields `min`.
pub fn clamp(value: f32, min: f32, max: f32) -> f32 {
    value.max(min).min(max)
}

/// Wraps an angle in radians into `[-π, π]`.
pub fn wrap_angle(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

/// Returns the shortest signed rotation from `from` to `to`, in `[-π, π]`.
///
/// Positive means clockwise on screen.
pub fn angle_delta(from: f32, to: f32) -> f32 {
    wrap_angle(to - from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn lerp_hits_endpoints_and_extrapolates() {
        assert_eq!(lerp(2.0, 6.0, 0.0), 2.0);
        assert_eq!(lerp(2.0, 6.0, 1.0), 6.0);
        assert_eq!(lerp(2.0, 6.0, 0.25), 3.0);
        assert_eq!(lerp(2.0, 6.0, 1.5), 8.0);
    }

    #[test]
    fn approx_eq_is_inclusive() {
        assert!(approx_eq(1.0, 1.25, 0.25));
        assert!(!approx_eq(1.0, 1.5, 0.25));
        assert!(approx_eq(-1.0, -1.0, 0.0));
    }

    #[test]
    fn clamp_never_panics() {
        assert_eq!(clamp(5.0, 0.0, 1.0), 1.0);
        assert_eq!(clamp(-5.0, 0.0, 1.0), 0.0);
        assert_eq!(clamp(0.5, 0.0, 1.0), 0.5);
        assert_eq!(clamp(0.5, 1.0, 0.0), 0.0);
        assert_eq!(clamp(f32::NAN, 0.25, 1.0), 0.25);
    }

    #[test]
    fn wrap_angle_stays_in_range() {
        assert!(approx_eq(wrap_angle(0.0), 0.0, 1e-6));
        assert!(approx_eq(wrap_angle(TAU + 0.5), 0.5, 1e-5));
        assert!(approx_eq(wrap_angle(-TAU - 0.5), -0.5, 1e-5));
        assert!(approx_eq(wrap_angle(PI + 0.25), -PI + 0.25, 1e-5));
        for step in -100..=100 {
            let wrapped = wrap_angle(step as f32 * 0.37);
            assert!((-PI..=PI).contains(&wrapped), "{wrapped}");
        }
    }

    #[test]
    fn angle_delta_takes_the_short_way() {
        assert!(approx_eq(angle_delta(0.0, FRAC_PI_2), FRAC_PI_2, 1e-6));
        assert!(approx_eq(angle_delta(FRAC_PI_2, 0.0), -FRAC_PI_2, 1e-6));
        assert!(approx_eq(angle_delta(-PI + 0.1, PI - 0.1), -0.2, 1e-5));
    }
}
