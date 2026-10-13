//! [`Mat3`]: a 3×3 matrix for 2D affine transforms in homogeneous form.

use std::ops::Mul;

use super::{Affine2, Vec2};

/// A column-major 3×3 `f32` matrix, used for 2D affine transforms in
/// homogeneous coordinates.
///
/// Points are column vectors `(x, y, 1)`, so the translation lives in the
/// third column. Build transforms with [`Affine2`] and convert with
/// [`Mat3::from`].
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat3 {
    /// The three columns. `cols[c][r]` is the element in column `c`, row `r`.
    pub cols: [[f32; 3]; 3],
}

impl Mat3 {
    /// The identity matrix.
    pub const IDENTITY: Self = Self::from_cols([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);

    /// Creates a matrix from its three columns.
    pub const fn from_cols(x_axis: [f32; 3], y_axis: [f32; 3], z_axis: [f32; 3]) -> Self {
        Self {
            cols: [x_axis, y_axis, z_axis],
        }
    }

    /// Creates a matrix from nine elements in column-major order.
    pub const fn from_cols_array(m: [f32; 9]) -> Self {
        Self::from_cols([m[0], m[1], m[2]], [m[3], m[4], m[5]], [m[6], m[7], m[8]])
    }

    /// Returns the nine elements in column-major order.
    pub const fn to_cols_array(self) -> [f32; 9] {
        let [x, y, z] = self.cols;
        [x[0], x[1], x[2], y[0], y[1], y[2], z[0], z[1], z[2]]
    }

    /// Returns the determinant.
    pub fn determinant(self) -> f32 {
        let [x, y, z] = self.cols;
        dot(x, cross(y, z))
    }

    /// Returns the transpose: rows and columns swapped.
    pub const fn transpose(self) -> Self {
        let [x, y, z] = self.cols;
        Self::from_cols([x[0], y[0], z[0]], [x[1], y[1], z[1]], [x[2], y[2], z[2]])
    }

    /// Returns the inverse matrix, or `None` if this matrix is singular or
    /// not finite.
    pub fn inverse(self) -> Option<Self> {
        let [x, y, z] = self.cols;
        // The rows of the inverse are these cross products over the
        // determinant.
        let row0 = cross(y, z);
        let row1 = cross(z, x);
        let row2 = cross(x, y);
        let determinant = dot(x, row0);
        let inv_det = 1.0 / determinant;
        if !determinant.is_finite() || !inv_det.is_finite() {
            return None;
        }
        Some(Self::from_cols(
            [row0[0] * inv_det, row1[0] * inv_det, row2[0] * inv_det],
            [row0[1] * inv_det, row1[1] * inv_det, row2[1] * inv_det],
            [row0[2] * inv_det, row1[2] * inv_det, row2[2] * inv_det],
        ))
    }

    /// Transforms a point, treating it as `(x, y, 1)`.
    ///
    /// Assumes the matrix is affine (bottom row `0 0 1`): there is no
    /// perspective divide.
    pub fn transform_point(self, point: Vec2) -> Vec2 {
        let [x, y, z] = self.cols;
        Vec2::new(
            x[0] * point.x + y[0] * point.y + z[0],
            x[1] * point.x + y[1] * point.y + z[1],
        )
    }

    /// Transforms a direction or offset, treating it as `(x, y, 0)`, so the
    /// translation is ignored.
    pub fn transform_vector(self, vector: Vec2) -> Vec2 {
        let [x, y, _] = self.cols;
        Vec2::new(
            x[0] * vector.x + y[0] * vector.y,
            x[1] * vector.x + y[1] * vector.y,
        )
    }

    /// Returns `true` if every element differs from `rhs` by at most
    /// `epsilon`.
    pub fn approx_eq(self, rhs: Self, epsilon: f32) -> bool {
        let lhs = self.to_cols_array();
        let rhs = rhs.to_cols_array();
        lhs.iter()
            .zip(&rhs)
            .all(|(&a, &b)| super::approx_eq(a, b, epsilon))
    }

    /// Multiplies this matrix by a column vector.
    fn mul_col(self, v: [f32; 3]) -> [f32; 3] {
        let [x, y, z] = self.cols;
        [
            x[0] * v[0] + y[0] * v[1] + z[0] * v[2],
            x[1] * v[0] + y[1] * v[1] + z[1] * v[2],
            x[2] * v[0] + y[2] * v[1] + z[2] * v[2],
        ]
    }
}

impl Default for Mat3 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// Matrix product: `a * b` applies `b` first, then `a`.
impl Mul for Mat3 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        let [x, y, z] = rhs.cols;
        Self::from_cols(self.mul_col(x), self.mul_col(y), self.mul_col(z))
    }
}

impl From<Affine2> for Mat3 {
    fn from(affine: Affine2) -> Self {
        Self::from_cols(
            [affine.x_axis.x, affine.x_axis.y, 0.0],
            [affine.y_axis.x, affine.y_axis.y, 0.0],
            [affine.translation.x, affine.translation.y, 1.0],
        )
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Pcg32;
    use crate::test_support::{random_affine2, random_vec2};
    use crate::testing::check;

    /// A general (not necessarily affine) matrix with elements in `[-1, 1)`
    /// and a determinant well away from zero.
    fn random_invertible(rng: &mut Pcg32) -> Mat3 {
        loop {
            let m = Mat3::from_cols_array(std::array::from_fn(|_| rng.range(-1.0..1.0f32)));
            if m.determinant().abs() >= 0.1 {
                return m;
            }
        }
    }

    #[test]
    fn identity_changes_nothing() {
        let p = Vec2::new(3.0, -7.0);
        assert_eq!(Mat3::IDENTITY.transform_point(p), p);
        assert_eq!(Mat3::IDENTITY.transform_vector(p), p);
        assert_eq!(Mat3::default(), Mat3::IDENTITY);
        assert_eq!(Mat3::IDENTITY.determinant(), 1.0);
        assert_eq!(Mat3::IDENTITY.inverse(), Some(Mat3::IDENTITY));
        assert_eq!(Mat3::from(Affine2::IDENTITY), Mat3::IDENTITY);
    }

    #[test]
    fn storage_is_column_major() {
        let array = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let m = Mat3::from_cols_array(array);
        assert_eq!(m.cols, [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0], [7.0, 8.0, 9.0]]);
        assert_eq!(m.to_cols_array(), array);
        assert_eq!(
            m.transpose().to_cols_array(),
            [1.0, 4.0, 7.0, 2.0, 5.0, 8.0, 3.0, 6.0, 9.0]
        );
        assert_eq!(m.transpose().transpose(), m);
    }

    #[test]
    fn translation_lives_in_the_third_column() {
        let m = Mat3::from(Affine2::from_translation(Vec2::new(10.0, -5.0)));
        assert_eq!(m.cols[2], [10.0, -5.0, 1.0]);
        assert_eq!(
            m.transform_point(Vec2::new(1.0, 2.0)),
            Vec2::new(11.0, -3.0)
        );
        assert_eq!(m.transform_vector(Vec2::new(1.0, 2.0)), Vec2::new(1.0, 2.0));
    }

    #[test]
    fn determinant_and_inverse_of_a_known_matrix() {
        // Rows: (2 0 0), (0 4 0), (6 8 1).
        let m = Mat3::from_cols([2.0, 0.0, 6.0], [0.0, 4.0, 8.0], [0.0, 0.0, 1.0]);
        assert_eq!(m.determinant(), 8.0);
        let inverse = m.inverse().expect("determinant is 8");
        // Rows: (0.5 0 0), (0 0.25 0), (-3 -2 1).
        assert_eq!(
            inverse,
            Mat3::from_cols([0.5, 0.0, -3.0], [0.0, 0.25, -2.0], [0.0, 0.0, 1.0])
        );
    }

    #[test]
    fn singular_matrices_have_no_inverse() {
        let repeated_column = Mat3::from_cols([1.0, 2.0, 3.0], [1.0, 2.0, 3.0], [0.0, 0.0, 1.0]);
        assert_eq!(repeated_column.determinant(), 0.0);
        assert_eq!(repeated_column.inverse(), None);
        assert_eq!(Mat3::from_cols_array([0.0; 9]).inverse(), None);
        assert_eq!(Mat3::from_cols_array([f32::NAN; 9]).inverse(), None);
        assert_eq!(
            Mat3::from(Affine2::from_scale(Vec2::splat(f32::INFINITY))).inverse(),
            None
        );
    }

    #[test]
    fn matches_affine2() {
        check(
            "mat3 matches affine2",
            500,
            |rng| {
                (
                    random_affine2(rng),
                    random_affine2(rng),
                    random_vec2(rng, -50.0, 50.0),
                )
            },
            |&(a, b, p)| {
                let (ma, mb) = (Mat3::from(a), Mat3::from(b));
                if ma.transform_point(p) != a.transform_point(p)
                    || ma.transform_vector(p) != a.transform_vector(p)
                {
                    return Err("Mat3 and Affine2 transform the point differently".to_string());
                }
                if !(ma * mb).approx_eq(Mat3::from(a * b), 1e-3) {
                    return Err("the products disagree".to_string());
                }
                if !crate::math::approx_eq(ma.determinant(), a.determinant(), 1e-3) {
                    return Err("the determinants disagree".to_string());
                }
                Ok(())
            },
        );
    }

    #[test]
    fn inverse_round_trips_for_affine_matrices() {
        check(
            "mat3 affine inverse round-trip",
            1000,
            |rng| {
                (
                    Mat3::from(random_affine2(rng)),
                    random_vec2(rng, -50.0, 50.0),
                )
            },
            |&(m, p)| {
                let inverse = m.inverse().ok_or("no inverse")?;
                for product in [m * inverse, inverse * m] {
                    if !product.approx_eq(Mat3::IDENTITY, 1e-3) {
                        return Err(format!("the matrix times its inverse is {product:?}"));
                    }
                }
                let back = inverse.transform_point(m.transform_point(p));
                if !back.approx_eq(p, 1e-2) {
                    return Err(format!("the point came back as {back:?}"));
                }
                Ok(())
            },
        );
    }

    #[test]
    fn inverse_round_trips_for_general_matrices() {
        check(
            "mat3 general inverse round-trip",
            1000,
            random_invertible,
            |&m| {
                let inverse = m.inverse().ok_or("no inverse")?;
                for product in [m * inverse, inverse * m] {
                    if !product.approx_eq(Mat3::IDENTITY, 1e-3) {
                        return Err(format!("the matrix times its inverse is {product:?}"));
                    }
                }
                // Inverting twice squares the conditioning, so allow more error.
                let twice = inverse.inverse().ok_or("the inverse has no inverse")?;
                if !twice.approx_eq(m, 1e-2) {
                    return Err(format!("inverting twice gave {twice:?}"));
                }
                Ok(())
            },
        );
    }
}
