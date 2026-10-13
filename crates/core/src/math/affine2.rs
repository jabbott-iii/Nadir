//! [`Affine2`]: a compact 2D affine transform.

use std::ops::Mul;

use super::Vec2;

/// A 2D affine transform: a 2×2 linear part plus a translation.
///
/// This is the compact form for CPU-side transforms. Convert it to a
/// [`Mat3`](super::Mat3) when a full matrix is needed, for example for a
/// shader uniform.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine2 {
    /// Where the unit X axis maps to (the first column of the linear part).
    pub x_axis: Vec2,
    /// Where the unit Y axis maps to (the second column of the linear part).
    pub y_axis: Vec2,
    /// The translation, applied after the linear part.
    pub translation: Vec2,
}

impl Affine2 {
    /// The transform that leaves every point unchanged.
    pub const IDENTITY: Self = Self::from_cols(Vec2::X, Vec2::Y, Vec2::ZERO);

    /// Creates a transform from the images of the two axes and a
    /// translation.
    pub const fn from_cols(x_axis: Vec2, y_axis: Vec2, translation: Vec2) -> Self {
        Self {
            x_axis,
            y_axis,
            translation,
        }
    }

    /// Creates a transform that moves points by `translation`.
    pub const fn from_translation(translation: Vec2) -> Self {
        Self::from_cols(Vec2::X, Vec2::Y, translation)
    }

    /// Creates a rotation about the origin by `angle` radians, clockwise on
    /// screen.
    pub fn from_angle(angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::from_cols(Vec2::new(cos, sin), Vec2::new(-sin, cos), Vec2::ZERO)
    }

    /// Creates a scale about the origin.
    pub const fn from_scale(scale: Vec2) -> Self {
        Self::from_cols(Vec2::new(scale.x, 0.0), Vec2::new(0.0, scale.y), Vec2::ZERO)
    }

    /// Creates a transform that scales, then rotates clockwise by `angle`
    /// radians, then translates.
    pub fn from_scale_angle_translation(scale: Vec2, angle: f32, translation: Vec2) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::from_cols(
            Vec2::new(cos, sin) * scale.x,
            Vec2::new(-sin, cos) * scale.y,
            translation,
        )
    }

    /// Transforms a point: applies the linear part, then the translation.
    pub fn transform_point(self, point: Vec2) -> Vec2 {
        self.transform_vector(point) + self.translation
    }

    /// Transforms a direction or offset: applies the linear part only.
    pub fn transform_vector(self, vector: Vec2) -> Vec2 {
        self.x_axis * vector.x + self.y_axis * vector.y
    }

    /// Returns the determinant of the linear part: the signed area scale.
    pub fn determinant(self) -> f32 {
        self.x_axis.perp_dot(self.y_axis)
    }

    /// Returns the inverse transform, or `None` if this transform is singular
    /// or not finite.
    pub fn inverse(self) -> Option<Self> {
        let determinant = self.determinant();
        let inv_det = 1.0 / determinant;
        if !determinant.is_finite() || !inv_det.is_finite() {
            return None;
        }
        let x_axis = Vec2::new(self.y_axis.y, -self.x_axis.y) * inv_det;
        let y_axis = Vec2::new(-self.y_axis.x, self.x_axis.x) * inv_det;
        let linear = Self::from_cols(x_axis, y_axis, Vec2::ZERO);
        Some(Self::from_cols(
            x_axis,
            y_axis,
            -linear.transform_vector(self.translation),
        ))
    }

    /// Returns `true` if every component differs from `rhs` by at most
    /// `epsilon`.
    pub fn approx_eq(self, rhs: Self, epsilon: f32) -> bool {
        self.x_axis.approx_eq(rhs.x_axis, epsilon)
            && self.y_axis.approx_eq(rhs.y_axis, epsilon)
            && self.translation.approx_eq(rhs.translation, epsilon)
    }
}

impl Default for Affine2 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Composition: `a * b` applies `b` first, then `a`.
impl Mul for Affine2 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self::from_cols(
            self.transform_vector(rhs.x_axis),
            self.transform_vector(rhs.y_axis),
            self.transform_point(rhs.translation),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::for_cases;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn identity_changes_nothing() {
        let p = Vec2::new(3.0, -7.0);
        assert_eq!(Affine2::IDENTITY.transform_point(p), p);
        assert_eq!(Affine2::IDENTITY.transform_vector(p), p);
        assert_eq!(Affine2::default(), Affine2::IDENTITY);
        assert_eq!(Affine2::IDENTITY.determinant(), 1.0);
    }

    #[test]
    fn translation_moves_points_but_not_vectors() {
        let t = Affine2::from_translation(Vec2::new(10.0, -5.0));
        assert_eq!(
            t.transform_point(Vec2::new(1.0, 2.0)),
            Vec2::new(11.0, -3.0)
        );
        assert_eq!(t.transform_vector(Vec2::new(1.0, 2.0)), Vec2::new(1.0, 2.0));
    }

    #[test]
    fn positive_rotation_is_clockwise_on_screen() {
        let quarter = Affine2::from_angle(FRAC_PI_2);
        assert!(quarter.transform_vector(Vec2::X).approx_eq(Vec2::Y, 1e-6));
        assert!(quarter.transform_vector(Vec2::Y).approx_eq(-Vec2::X, 1e-6));
    }

    #[test]
    fn scale_and_determinant() {
        let s = Affine2::from_scale(Vec2::new(2.0, -3.0));
        assert_eq!(s.transform_point(Vec2::new(1.0, 1.0)), Vec2::new(2.0, -3.0));
        assert_eq!(s.determinant(), -6.0);
    }

    #[test]
    fn scale_angle_translation_applies_in_that_order() {
        let scale = Vec2::new(2.0, 3.0);
        let translation = Vec2::new(10.0, 20.0);
        let combined = Affine2::from_scale_angle_translation(scale, FRAC_PI_2, translation);
        // (1, 0) scales to (2, 0), turns clockwise to (0, 2), then moves.
        assert!(
            combined
                .transform_point(Vec2::X)
                .approx_eq(Vec2::new(10.0, 22.0), 1e-5)
        );
        let composed = Affine2::from_translation(translation)
            * Affine2::from_angle(FRAC_PI_2)
            * Affine2::from_scale(scale);
        assert!(combined.approx_eq(composed, 1e-5));
    }

    #[test]
    fn singular_transforms_have_no_inverse() {
        assert_eq!(Affine2::from_scale(Vec2::new(0.0, 1.0)).inverse(), None);
        assert_eq!(
            Affine2::from_cols(Vec2::ONE, Vec2::ONE, Vec2::ZERO).inverse(),
            None
        );
        assert_eq!(
            Affine2::from_scale(Vec2::new(f32::NAN, 1.0)).inverse(),
            None
        );
        assert_eq!(
            Affine2::from_scale(Vec2::new(f32::INFINITY, 1.0)).inverse(),
            None
        );
    }

    #[test]
    fn multiplication_composes_right_to_left() {
        for_cases("affine2 composition", 500, |rng| {
            let a = rng.affine2();
            let b = rng.affine2();
            let p = rng.vec2(-50.0, 50.0);
            let composed = (a * b).transform_point(p);
            let stepwise = a.transform_point(b.transform_point(p));
            if !composed.approx_eq(stepwise, 1e-2) {
                return Err(format!(
                    "(a * b)(p) = {composed:?}, a(b(p)) = {stepwise:?}\na = {a:?}\nb = {b:?}"
                ));
            }
            Ok(())
        });
    }

    #[test]
    fn inverse_round_trips() {
        for_cases("affine2 inverse round-trip", 1000, |rng| {
            let m = rng.affine2();
            let inverse = m.inverse().ok_or_else(|| format!("no inverse for {m:?}"))?;
            for product in [m * inverse, inverse * m] {
                if !product.approx_eq(Affine2::IDENTITY, 1e-3) {
                    return Err(format!("{m:?} times its inverse is {product:?}"));
                }
            }
            let p = rng.vec2(-50.0, 50.0);
            let back = inverse.transform_point(m.transform_point(p));
            if !back.approx_eq(p, 1e-2) {
                return Err(format!("{p:?} came back as {back:?} through {m:?}"));
            }
            Ok(())
        });
    }
}
