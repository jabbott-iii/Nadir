//! 2D vector types: [`Vec2`] (`f32`), [`IVec2`] (`i32`) and [`UVec2`] (`u32`).

use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

/// Implements the operators and array conversions shared by all three vector
/// types.
macro_rules! impl_vec2_ops {
    ($vec:ident, $scalar:ty) => {
        impl Add for $vec {
            type Output = Self;
            fn add(self, rhs: Self) -> Self {
                Self::new(self.x + rhs.x, self.y + rhs.y)
            }
        }

        impl Sub for $vec {
            type Output = Self;
            fn sub(self, rhs: Self) -> Self {
                Self::new(self.x - rhs.x, self.y - rhs.y)
            }
        }

        impl Mul<$scalar> for $vec {
            type Output = Self;
            fn mul(self, rhs: $scalar) -> Self {
                Self::new(self.x * rhs, self.y * rhs)
            }
        }

        impl Mul<$vec> for $scalar {
            type Output = $vec;
            fn mul(self, rhs: $vec) -> $vec {
                $vec::new(self * rhs.x, self * rhs.y)
            }
        }

        impl AddAssign for $vec {
            fn add_assign(&mut self, rhs: Self) {
                self.x += rhs.x;
                self.y += rhs.y;
            }
        }

        impl SubAssign for $vec {
            fn sub_assign(&mut self, rhs: Self) {
                self.x -= rhs.x;
                self.y -= rhs.y;
            }
        }

        impl MulAssign<$scalar> for $vec {
            fn mul_assign(&mut self, rhs: $scalar) {
                self.x *= rhs;
                self.y *= rhs;
            }
        }

        impl From<[$scalar; 2]> for $vec {
            fn from([x, y]: [$scalar; 2]) -> Self {
                Self::new(x, y)
            }
        }

        impl From<$vec> for [$scalar; 2] {
            fn from(v: $vec) -> Self {
                [v.x, v.y]
            }
        }
    };
}

/// Implements the constants and methods shared by the integer vector types.
macro_rules! impl_int_vec2 {
    ($vec:ident, $scalar:ty) => {
        impl $vec {
            /// `(0, 0)`.
            pub const ZERO: Self = Self::splat(0);
            /// `(1, 1)`.
            pub const ONE: Self = Self::splat(1);
            /// `(1, 0)`.
            pub const X: Self = Self::new(1, 0);
            /// `(0, 1)`.
            pub const Y: Self = Self::new(0, 1);

            /// Creates a vector from its components.
            pub const fn new(x: $scalar, y: $scalar) -> Self {
                Self { x, y }
            }

            /// Creates a vector with both components set to `v`.
            pub const fn splat(v: $scalar) -> Self {
                Self::new(v, v)
            }

            /// Returns the component-wise minimum.
            pub fn min(self, rhs: Self) -> Self {
                Self::new(self.x.min(rhs.x), self.y.min(rhs.y))
            }

            /// Returns the component-wise maximum.
            pub fn max(self, rhs: Self) -> Self {
                Self::new(self.x.max(rhs.x), self.y.max(rhs.y))
            }

            /// Converts to a [`Vec2`]. Components beyond ±2²⁴ lose precision.
            pub fn as_vec2(&self) -> Vec2 {
                Vec2::new(self.x as f32, self.y as f32)
            }
        }
    };
}

/// A 2D vector of `f32`: a position, offset, size or direction.
///
/// World space is +X right and +Y down.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    /// Horizontal component; positive is right.
    pub x: f32,
    /// Vertical component; positive is down.
    pub y: f32,
}

impl Vec2 {
    /// `(0, 0)`.
    pub const ZERO: Self = Self::splat(0.0);
    /// `(1, 1)`.
    pub const ONE: Self = Self::splat(1.0);
    /// `(1, 0)`: the unit vector pointing right.
    pub const X: Self = Self::new(1.0, 0.0);
    /// `(0, 1)`: the unit vector pointing down.
    pub const Y: Self = Self::new(0.0, 1.0);

    /// Creates a vector from its components.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Creates a vector with both components set to `v`.
    pub const fn splat(v: f32) -> Self {
        Self::new(v, v)
    }

    /// Returns the unit vector at `angle` radians, measured clockwise on
    /// screen from +X.
    pub fn from_angle(angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::new(cos, sin)
    }

    /// Returns the dot product.
    pub fn dot(self, rhs: Self) -> f32 {
        self.x * rhs.x + self.y * rhs.y
    }

    /// Returns the 2D cross product, `self.x * rhs.y - self.y * rhs.x`.
    ///
    /// It is positive when `rhs` is clockwise (on screen) from `self`.
    pub fn perp_dot(self, rhs: Self) -> f32 {
        self.x * rhs.y - self.y * rhs.x
    }

    /// Returns the squared length. Cheaper than [`Vec2::length`].
    pub fn length_squared(self) -> f32 {
        self.dot(self)
    }

    /// Returns the length.
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Returns the squared distance to `rhs`.
    pub fn distance_squared(self, rhs: Self) -> f32 {
        (rhs - self).length_squared()
    }

    /// Returns the distance to `rhs`.
    pub fn distance(self, rhs: Self) -> f32 {
        (rhs - self).length()
    }

    /// Returns the vector scaled to length 1, or [`Vec2::ZERO`] if its length
    /// is zero or not finite.
    pub fn normalize_or_zero(self) -> Self {
        let length = self.length();
        if length > 0.0 && length.is_finite() {
            self / length
        } else {
            Self::ZERO
        }
    }

    /// Returns the vector rotated a quarter turn clockwise on screen.
    pub fn perp(self) -> Self {
        Self::new(-self.y, self.x)
    }

    /// Returns the angle of the vector in radians, in `[-π, π]`, measured
    /// clockwise on screen from +X.
    pub fn angle(self) -> f32 {
        self.y.atan2(self.x)
    }

    /// Returns the vector rotated by `angle` radians, clockwise on screen.
    pub fn rotate(self, angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::new(self.x * cos - self.y * sin, self.x * sin + self.y * cos)
    }

    /// Linearly interpolates from `self` (at `t = 0`) to `rhs` (at `t = 1`).
    pub fn lerp(self, rhs: Self, t: f32) -> Self {
        self + (rhs - self) * t
    }

    /// Returns the component-wise minimum.
    pub fn min(self, rhs: Self) -> Self {
        Self::new(self.x.min(rhs.x), self.y.min(rhs.y))
    }

    /// Returns the component-wise maximum.
    pub fn max(self, rhs: Self) -> Self {
        Self::new(self.x.max(rhs.x), self.y.max(rhs.y))
    }

    /// Clamps each component to `[min, max]`. Never panics.
    pub fn clamp(self, min: Self, max: Self) -> Self {
        self.max(min).min(max)
    }

    /// Returns the component-wise absolute value.
    pub fn abs(self) -> Self {
        Self::new(self.x.abs(), self.y.abs())
    }

    /// Rounds each component down.
    pub fn floor(self) -> Self {
        Self::new(self.x.floor(), self.y.floor())
    }

    /// Rounds each component up.
    pub fn ceil(self) -> Self {
        Self::new(self.x.ceil(), self.y.ceil())
    }

    /// Rounds each component to the nearest integer, with ties away from
    /// zero.
    pub fn round(self) -> Self {
        Self::new(self.x.round(), self.y.round())
    }

    /// Returns `true` if both components differ from `rhs` by at most
    /// `epsilon`.
    pub fn approx_eq(self, rhs: Self, epsilon: f32) -> bool {
        super::approx_eq(self.x, rhs.x, epsilon) && super::approx_eq(self.y, rhs.y, epsilon)
    }

    /// Converts to an [`IVec2`], truncating toward zero and saturating at the
    /// `i32` range. NaN becomes 0.
    ///
    /// For tile coordinates, call [`Vec2::floor`] first.
    pub fn as_ivec2(&self) -> IVec2 {
        IVec2::new(self.x as i32, self.y as i32)
    }

    /// Converts to a [`UVec2`], truncating toward zero and saturating at the
    /// `u32` range. Negative values and NaN become 0.
    pub fn as_uvec2(&self) -> UVec2 {
        UVec2::new(self.x as u32, self.y as u32)
    }
}

impl_vec2_ops!(Vec2, f32);

impl Neg for Vec2 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

/// Component-wise multiplication.
impl Mul for Vec2 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self::new(self.x * rhs.x, self.y * rhs.y)
    }
}

impl Div<f32> for Vec2 {
    type Output = Self;
    fn div(self, rhs: f32) -> Self {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

/// Component-wise division.
impl Div for Vec2 {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        Self::new(self.x / rhs.x, self.y / rhs.y)
    }
}

impl DivAssign<f32> for Vec2 {
    fn div_assign(&mut self, rhs: f32) {
        self.x /= rhs;
        self.y /= rhs;
    }
}

/// A 2D vector of `i32`: a tile coordinate, pixel position or integer offset.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct IVec2 {
    /// Horizontal component; positive is right.
    pub x: i32,
    /// Vertical component; positive is down.
    pub y: i32,
}

impl_int_vec2!(IVec2, i32);
impl_vec2_ops!(IVec2, i32);

impl Neg for IVec2 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

/// A 2D vector of `u32`: a size in pixels or an unsigned grid coordinate.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct UVec2 {
    /// Horizontal component.
    pub x: u32,
    /// Vertical component.
    pub y: u32,
}

impl_int_vec2!(UVec2, u32);
impl_vec2_ops!(UVec2, u32);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::for_cases;
    use std::f32::consts::{FRAC_PI_2, PI};

    #[test]
    fn constants() {
        assert_eq!(Vec2::ZERO, Vec2::new(0.0, 0.0));
        assert_eq!(Vec2::ONE, Vec2::new(1.0, 1.0));
        assert_eq!(Vec2::X, Vec2::new(1.0, 0.0));
        assert_eq!(Vec2::Y, Vec2::new(0.0, 1.0));
        assert_eq!(Vec2::splat(3.0), Vec2::new(3.0, 3.0));
        assert_eq!(Vec2::default(), Vec2::ZERO);
    }

    #[test]
    fn operators() {
        let a = Vec2::new(1.0, 2.0);
        let b = Vec2::new(3.0, 5.0);
        assert_eq!(a + b, Vec2::new(4.0, 7.0));
        assert_eq!(b - a, Vec2::new(2.0, 3.0));
        assert_eq!(-a, Vec2::new(-1.0, -2.0));
        assert_eq!(a * 2.0, Vec2::new(2.0, 4.0));
        assert_eq!(2.0 * a, Vec2::new(2.0, 4.0));
        assert_eq!(a * b, Vec2::new(3.0, 10.0));
        assert_eq!(b / 2.0, Vec2::new(1.5, 2.5));
        assert_eq!(b / a, Vec2::new(3.0, 2.5));

        let mut c = a;
        c += b;
        assert_eq!(c, Vec2::new(4.0, 7.0));
        c -= a;
        assert_eq!(c, b);
        c *= 2.0;
        assert_eq!(c, Vec2::new(6.0, 10.0));
        c /= 4.0;
        assert_eq!(c, Vec2::new(1.5, 2.5));
    }

    #[test]
    fn products_and_lengths() {
        let a = Vec2::new(3.0, 4.0);
        assert_eq!(a.dot(Vec2::new(2.0, -1.0)), 2.0);
        assert_eq!(a.length_squared(), 25.0);
        assert_eq!(a.length(), 5.0);
        assert_eq!(Vec2::ZERO.distance(a), 5.0);
        assert_eq!(Vec2::ONE.distance_squared(a), 13.0);
        // +Y is clockwise from +X on screen.
        assert_eq!(Vec2::X.perp_dot(Vec2::Y), 1.0);
        assert_eq!(Vec2::Y.perp_dot(Vec2::X), -1.0);
    }

    #[test]
    fn normalize_handles_degenerate_input() {
        assert!(
            Vec2::new(3.0, 4.0)
                .normalize_or_zero()
                .approx_eq(Vec2::new(0.6, 0.8), 1e-6)
        );
        assert_eq!(Vec2::ZERO.normalize_or_zero(), Vec2::ZERO);
        assert_eq!(Vec2::new(f32::NAN, 1.0).normalize_or_zero(), Vec2::ZERO);
        assert_eq!(
            Vec2::new(f32::INFINITY, 1.0).normalize_or_zero(),
            Vec2::ZERO
        );
    }

    #[test]
    fn positive_rotation_is_clockwise_on_screen() {
        // Right turns to down, because +Y is down.
        assert!(Vec2::X.rotate(FRAC_PI_2).approx_eq(Vec2::Y, 1e-6));
        assert!(Vec2::Y.rotate(FRAC_PI_2).approx_eq(-Vec2::X, 1e-6));
        assert_eq!(Vec2::X.perp(), Vec2::Y);
        assert!(Vec2::from_angle(FRAC_PI_2).approx_eq(Vec2::Y, 1e-6));
        assert!(crate::math::approx_eq(Vec2::Y.angle(), FRAC_PI_2, 1e-6));
        assert!(crate::math::approx_eq((-Vec2::Y).angle(), -FRAC_PI_2, 1e-6));
    }

    #[test]
    fn interpolation_and_component_wise_helpers() {
        let a = Vec2::new(-1.5, 2.5);
        let b = Vec2::new(4.0, -2.0);
        assert_eq!(a.lerp(b, 0.0), a);
        assert_eq!(a.lerp(b, 1.0), b);
        assert_eq!(
            Vec2::ZERO.lerp(Vec2::new(4.0, 8.0), 0.25),
            Vec2::new(1.0, 2.0)
        );
        assert_eq!(a.min(b), Vec2::new(-1.5, -2.0));
        assert_eq!(a.max(b), Vec2::new(4.0, 2.5));
        assert_eq!(a.abs(), Vec2::new(1.5, 2.5));
        assert_eq!(a.floor(), Vec2::new(-2.0, 2.0));
        assert_eq!(a.ceil(), Vec2::new(-1.0, 3.0));
        assert_eq!(a.round(), Vec2::new(-2.0, 3.0));
        assert_eq!(
            Vec2::new(5.0, -5.0).clamp(Vec2::ZERO, Vec2::ONE),
            Vec2::new(1.0, 0.0)
        );
        assert!(a.approx_eq(Vec2::new(-1.4, 2.6), 0.11));
        assert!(!a.approx_eq(Vec2::new(-1.4, 2.8), 0.11));
    }

    #[test]
    fn float_to_integer_conversions_truncate_and_saturate() {
        assert_eq!(Vec2::new(1.9, -1.9).as_ivec2(), IVec2::new(1, -1));
        assert_eq!(Vec2::new(-1.9, 2.0).floor().as_ivec2(), IVec2::new(-2, 2));
        assert_eq!(
            Vec2::new(1e12, f32::NAN).as_ivec2(),
            IVec2::new(i32::MAX, 0)
        );
        assert_eq!(Vec2::new(1.9, -3.0).as_uvec2(), UVec2::new(1, 0));
        assert_eq!(
            Vec2::new(1e12, f32::NAN).as_uvec2(),
            UVec2::new(u32::MAX, 0)
        );
    }

    #[test]
    fn array_conversions() {
        assert_eq!(Vec2::from([1.0, 2.0]), Vec2::new(1.0, 2.0));
        assert_eq!(<[f32; 2]>::from(Vec2::new(1.0, 2.0)), [1.0, 2.0]);
        assert_eq!(IVec2::from([-1, 2]), IVec2::new(-1, 2));
        assert_eq!(<[i32; 2]>::from(IVec2::new(-1, 2)), [-1, 2]);
        assert_eq!(UVec2::from([1, 2]), UVec2::new(1, 2));
        assert_eq!(<[u32; 2]>::from(UVec2::new(1, 2)), [1, 2]);
    }

    #[test]
    fn ivec2_basics() {
        let a = IVec2::new(1, -2);
        let b = IVec2::new(3, 5);
        assert_eq!(IVec2::ZERO, IVec2::new(0, 0));
        assert_eq!(IVec2::ONE, IVec2::splat(1));
        assert_eq!(IVec2::X + IVec2::Y, IVec2::ONE);
        assert_eq!(IVec2::default(), IVec2::ZERO);
        assert_eq!(a + b, IVec2::new(4, 3));
        assert_eq!(a - b, IVec2::new(-2, -7));
        assert_eq!(-a, IVec2::new(-1, 2));
        assert_eq!(a * 3, IVec2::new(3, -6));
        assert_eq!(3 * a, IVec2::new(3, -6));
        assert_eq!(a.min(b), IVec2::new(1, -2));
        assert_eq!(a.max(b), IVec2::new(3, 5));
        assert_eq!(a.as_vec2(), Vec2::new(1.0, -2.0));

        let mut c = a;
        c += b;
        assert_eq!(c, IVec2::new(4, 3));
        c -= a;
        assert_eq!(c, b);
        c *= 2;
        assert_eq!(c, IVec2::new(6, 10));
    }

    #[test]
    fn uvec2_basics() {
        let a = UVec2::new(480, 270);
        let b = UVec2::new(20, 300);
        assert_eq!(UVec2::ZERO, UVec2::new(0, 0));
        assert_eq!(UVec2::ONE, UVec2::splat(1));
        assert_eq!(UVec2::X + UVec2::Y, UVec2::ONE);
        assert_eq!(UVec2::default(), UVec2::ZERO);
        assert_eq!(a + b, UVec2::new(500, 570));
        assert_eq!(a - UVec2::new(80, 70), UVec2::new(400, 200));
        assert_eq!(a * 2, UVec2::new(960, 540));
        assert_eq!(2 * a, UVec2::new(960, 540));
        assert_eq!(a.min(b), UVec2::new(20, 270));
        assert_eq!(a.max(b), UVec2::new(480, 300));
        assert_eq!(a.as_vec2(), Vec2::new(480.0, 270.0));

        let mut c = a;
        c += b;
        assert_eq!(c, UVec2::new(500, 570));
        c -= b;
        assert_eq!(c, a);
        c *= 2;
        assert_eq!(c, UVec2::new(960, 540));
    }

    #[test]
    fn rotation_preserves_length_and_round_trips() {
        for_cases("vec2 rotation round-trip", 500, |rng| {
            let v = rng.vec2(-100.0, 100.0);
            let angle = rng.range_f32(-4.0 * PI, 4.0 * PI);
            let rotated = v.rotate(angle);
            if !crate::math::approx_eq(rotated.length(), v.length(), 1e-3) {
                return Err(format!(
                    "{v:?} rotated by {angle} changed length to {}",
                    rotated.length()
                ));
            }
            let back = rotated.rotate(-angle);
            if !back.approx_eq(v, 1e-3) {
                return Err(format!("{v:?} rotated by ±{angle} came back as {back:?}"));
            }
            Ok(())
        });
    }
}
