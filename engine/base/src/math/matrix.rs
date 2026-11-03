//! Square matrices: [`Mat3`] and [`Mat4`].
//!
//! Column-major storage, column vectors, `M * v` (see `wiki/architecture/conventions.md`).
//! Element `(row, column)` of an `N`x`N` matrix is at flat position `column * N + row`. A
//! matrix is stored as its columns (`cols`), each a vector; the types are plain `repr(C)` data,
//! so the order in memory is the one `to_cols_array` returns.
//!
//! The order of every floating-point operation is fixed and nothing is fused: each element of a
//! product is the products of a row and a column added left to right (tested bit for bit), and
//! the determinant and the inverse use the expansions written in their docs (tested against an
//! `f64` reference within a tolerance, not bit for bit). Results repeat from run to run, and a
//! SIMD path can be checked against these.

use super::{Scalar, Vec2, Vec3, Vec4};
use std::ops::Mul;

/// The operations every size shares. `$n` is the dimension, `$len` is `$n * $n`.
macro_rules! matrix_common {
    ($name:ident, $vec:ident, $n:literal, $len:literal) => {
        impl $name {
            /// Every element `0.0`.
            pub const ZERO: Self = Self {
                cols: [$vec::ZERO; $n],
            };

            /// From the elements in column-major order: the first `N` are the first column.
            #[inline]
            #[must_use]
            pub fn from_cols_array(elements: [Scalar; $len]) -> Self {
                Self {
                    cols: core::array::from_fn(|column| {
                        $vec::from_array(core::array::from_fn(|row| elements[column * $n + row]))
                    }),
                }
            }

            /// The elements in column-major order.
            #[inline]
            #[must_use]
            pub fn to_cols_array(self) -> [Scalar; $len] {
                let cols = self.cols.map($vec::to_array);
                core::array::from_fn(|i| cols[i / $n][i % $n])
            }

            /// Rows and columns swapped.
            #[inline]
            #[must_use]
            pub fn transpose(self) -> Self {
                let elements = self.to_cols_array();
                Self::from_cols_array(core::array::from_fn(|i| elements[(i % $n) * $n + i / $n]))
            }

            /// `true` when every element is neither infinite nor NaN.
            #[inline]
            #[must_use]
            pub fn is_finite(self) -> bool {
                self.to_cols_array()
                    .iter()
                    .all(|element| element.is_finite())
            }

            /// Every element divided by `divisor`.
            #[inline]
            fn divided_by(self, divisor: Scalar) -> Self {
                Self {
                    cols: self.cols.map(|column| column / divisor),
                }
            }
        }

        impl Default for $name {
            /// The identity, so that a default transform leaves things where they are.
            #[inline]
            fn default() -> Self {
                Self::IDENTITY
            }
        }

        /// Each component of the result is the products of a row and the vector, added left to
        /// right.
        impl Mul<$vec> for $name {
            type Output = $vec;
            #[inline]
            fn mul(self, rhs: $vec) -> $vec {
                let a = self.to_cols_array();
                let v = rhs.to_array();
                $vec::from_array(core::array::from_fn(|row| {
                    (1..$n).fold(a[row] * v[0], |sum, k| sum + a[k * $n + row] * v[k])
                }))
            }
        }

        /// Each element is a row of `self` times a column of `rhs`, added left to right, so
        /// `(a * b) * v` applies `b` first.
        impl Mul for $name {
            type Output = Self;
            #[inline]
            fn mul(self, rhs: Self) -> Self {
                let a = self.to_cols_array();
                let b = rhs.to_cols_array();
                Self::from_cols_array(core::array::from_fn(|i| {
                    let (column, row) = (i / $n, i % $n);
                    (1..$n).fold(a[row] * b[column * $n], |sum, k| {
                        sum + a[k * $n + row] * b[column * $n + k]
                    })
                }))
            }
        }
    };
}

/// Three-by-three matrix: a rotation or a scale in 3D, or a 2D transform in homogeneous form.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Mat3 {
    /// The columns, in order. The first is the image of `Vec3::X`.
    pub cols: [Vec3; 3],
}

impl Mat3 {
    /// Ones on the diagonal, zero elsewhere.
    pub const IDENTITY: Self = Self::from_cols(Vec3::X, Vec3::Y, Vec3::Z);

    /// From its three columns.
    #[inline]
    #[must_use]
    pub const fn from_cols(x_axis: Vec3, y_axis: Vec3, z_axis: Vec3) -> Self {
        Self {
            cols: [x_axis, y_axis, z_axis],
        }
    }

    /// Scales each axis by the matching component of `scale`.
    #[inline]
    #[must_use]
    pub const fn from_scale(scale: Vec3) -> Self {
        Self::from_cols(
            Vec3::new(scale.x, 0.0, 0.0),
            Vec3::new(0.0, scale.y, 0.0),
            Vec3::new(0.0, 0.0, scale.z),
        )
    }

    /// Counter-clockwise rotation about +X by `angle` radians: +Y turns towards +Z.
    #[inline]
    #[must_use]
    pub fn from_rotation_x(angle: Scalar) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::from_cols(Vec3::X, Vec3::new(0.0, cos, sin), Vec3::new(0.0, -sin, cos))
    }

    /// Counter-clockwise rotation about +Y by `angle` radians: +Z turns towards +X.
    #[inline]
    #[must_use]
    pub fn from_rotation_y(angle: Scalar) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::from_cols(Vec3::new(cos, 0.0, -sin), Vec3::Y, Vec3::new(sin, 0.0, cos))
    }

    /// Counter-clockwise rotation about +Z by `angle` radians: +X turns towards +Y.
    #[inline]
    #[must_use]
    pub fn from_rotation_z(angle: Scalar) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::from_cols(Vec3::new(cos, sin, 0.0), Vec3::new(-sin, cos, 0.0), Vec3::Z)
    }

    /// `x.dot(y.cross(z))` of the columns `x`, `y`, `z`.
    #[inline]
    #[must_use]
    pub fn determinant(self) -> Scalar {
        let [x, y, z] = self.cols;
        x.dot(y.cross(z))
    }

    /// The inverse, or `None` when the determinant is zero, NaN or infinite, or when the
    /// result would not be finite. A zero determinant gives infinite or NaN quotients, which
    /// the finiteness check rejects. A matrix that is invertible but whose determinant
    /// underflows to zero (a scale of about `1e-15` or smaller) is refused too.
    ///
    /// The rows of the inverse are the cross products `y x z`, `z x x` and `x x y` of the
    /// columns `x`, `y`, `z`, each element divided by the determinant.
    #[must_use]
    pub fn inverse(self) -> Option<Self> {
        let [x, y, z] = self.cols;
        let determinant = self.determinant();
        if !determinant.is_finite() {
            return None;
        }
        let rows = Self::from_cols(y.cross(z), z.cross(x), x.cross(y));
        Some(rows.transpose().divided_by(determinant)).filter(|inverse| inverse.is_finite())
    }
}

matrix_common!(Mat3, Vec3, 3, 9);

/// Four-by-four matrix: a 3D transform in homogeneous form, or a projection.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Mat4 {
    /// The columns, in order. The last holds the translation of an affine transform.
    pub cols: [Vec4; 4],
}

impl Mat4 {
    /// Ones on the diagonal, zero elsewhere.
    pub const IDENTITY: Self = Self::from_cols(Vec4::X, Vec4::Y, Vec4::Z, Vec4::W);

    /// From its four columns.
    #[inline]
    #[must_use]
    pub const fn from_cols(x_axis: Vec4, y_axis: Vec4, z_axis: Vec4, w_axis: Vec4) -> Self {
        Self {
            cols: [x_axis, y_axis, z_axis, w_axis],
        }
    }

    /// Moves points by `translation`; directions are unchanged.
    #[inline]
    #[must_use]
    pub const fn from_translation(translation: Vec3) -> Self {
        Self::from_cols(
            Vec4::X,
            Vec4::Y,
            Vec4::Z,
            Vec4::new(translation.x, translation.y, translation.z, 1.0),
        )
    }

    /// Scales each axis by the matching component of `scale`.
    #[inline]
    #[must_use]
    pub const fn from_scale(scale: Vec3) -> Self {
        Self::from_cols(
            Vec4::new(scale.x, 0.0, 0.0, 0.0),
            Vec4::new(0.0, scale.y, 0.0, 0.0),
            Vec4::new(0.0, 0.0, scale.z, 0.0),
            Vec4::W,
        )
    }

    /// The matrix with `rotation` in the upper left and the identity elsewhere.
    #[inline]
    #[must_use]
    pub const fn from_mat3(rotation: Mat3) -> Self {
        let [x, y, z] = rotation.cols;
        Self::from_cols(x.extend(0.0), y.extend(0.0), z.extend(0.0), Vec4::W)
    }

    /// Orthographic projection of the box from `min` to `max` in x and y and from `near` to `far`
    /// in front of the camera (the camera looks down -Z), right-handed, Y up, depth 0 to 1:
    /// `z = -near` maps to 0 and `z = -far` to 1. `None` when a value is not finite or an
    /// extent is zero or so small that the matrix would overflow.
    #[must_use]
    pub fn orthographic_rh(min: Vec2, max: Vec2, near: Scalar, far: Scalar) -> Option<Self> {
        let (width, height, depth) = (max.x - min.x, max.y - min.y, near - far);
        let finite = [width, height, depth, min.x, min.y, near]
            .iter()
            .all(|v| v.is_finite());
        if !finite {
            return None;
        }
        let matrix = Self::from_cols(
            Vec4::new(2.0 / width, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 2.0 / height, 0.0, 0.0),
            Vec4::new(0.0, 0.0, 1.0 / depth, 0.0),
            Vec4::new(
                -(max.x + min.x) / width,
                -(max.y + min.y) / height,
                near / depth,
                1.0,
            ),
        );
        matrix.is_finite().then_some(matrix)
    }

    /// Counter-clockwise rotation about +X, see [`Mat3::from_rotation_x`].
    #[inline]
    #[must_use]
    pub fn from_rotation_x(angle: Scalar) -> Self {
        Self::from_mat3(Mat3::from_rotation_x(angle))
    }

    /// Counter-clockwise rotation about +Y, see [`Mat3::from_rotation_y`].
    #[inline]
    #[must_use]
    pub fn from_rotation_y(angle: Scalar) -> Self {
        Self::from_mat3(Mat3::from_rotation_y(angle))
    }

    /// Counter-clockwise rotation about +Z, see [`Mat3::from_rotation_z`].
    #[inline]
    #[must_use]
    pub fn from_rotation_z(angle: Scalar) -> Self {
        Self::from_mat3(Mat3::from_rotation_z(angle))
    }

    /// Transforms a point (`w = 1`): the translation applies. The result is not divided by
    /// `w`, so use this for affine transforms only.
    #[inline]
    #[must_use]
    pub fn transform_point3(self, point: Vec3) -> Vec3 {
        (self * point.extend(1.0)).truncate()
    }

    /// Transforms a direction (`w = 0`): the translation does not apply.
    #[inline]
    #[must_use]
    pub fn transform_vector3(self, vector: Vec3) -> Vec3 {
        (self * vector.extend(0.0)).truncate()
    }

    /// The determinant: `s0*c5 - s1*c4 + s2*c3 + s3*c2 - s4*c1 + s5*c0` over the 2x2 minors
    /// `s` of the first two rows and `c` of the last two rows (see [`Self::inverse`]).
    #[inline]
    #[must_use]
    pub fn determinant(self) -> Scalar {
        let (s, c) = minors(&self.to_cols_array());
        determinant_from_minors(&s, &c)
    }

    /// The inverse, or `None` when the determinant is zero, NaN or infinite, or when the
    /// result would not be finite. A zero determinant gives infinite or NaN quotients, which
    /// the finiteness check rejects. A matrix that is invertible but whose determinant
    /// underflows to zero is refused too.
    ///
    /// With the 2x2 minors `s` of the first two rows and `c` of the last two rows (both over
    /// the column pairs 01, 02, 03, 12, 13, 23), each element of the adjugate is three
    /// products of an element and a minor, and is divided by the determinant.
    #[must_use]
    pub fn inverse(self) -> Option<Self> {
        let m = self.to_cols_array();
        let (s, c) = minors(&m);
        let determinant = determinant_from_minors(&s, &c);
        if !determinant.is_finite() {
            return None;
        }
        let a = |row: usize, column: usize| m[column * 4 + row];
        // The adjugate in row-major order; read as column-major it is the transposed one.
        let adjugate = [
            a(1, 1) * c[5] - a(1, 2) * c[4] + a(1, 3) * c[3],
            -a(0, 1) * c[5] + a(0, 2) * c[4] - a(0, 3) * c[3],
            a(3, 1) * s[5] - a(3, 2) * s[4] + a(3, 3) * s[3],
            -a(2, 1) * s[5] + a(2, 2) * s[4] - a(2, 3) * s[3],
            -a(1, 0) * c[5] + a(1, 2) * c[2] - a(1, 3) * c[1],
            a(0, 0) * c[5] - a(0, 2) * c[2] + a(0, 3) * c[1],
            -a(3, 0) * s[5] + a(3, 2) * s[2] - a(3, 3) * s[1],
            a(2, 0) * s[5] - a(2, 2) * s[2] + a(2, 3) * s[1],
            a(1, 0) * c[4] - a(1, 1) * c[2] + a(1, 3) * c[0],
            -a(0, 0) * c[4] + a(0, 1) * c[2] - a(0, 3) * c[0],
            a(3, 0) * s[4] - a(3, 1) * s[2] + a(3, 3) * s[0],
            -a(2, 0) * s[4] + a(2, 1) * s[2] - a(2, 3) * s[0],
            -a(1, 0) * c[3] + a(1, 1) * c[1] - a(1, 2) * c[0],
            a(0, 0) * c[3] - a(0, 1) * c[1] + a(0, 2) * c[0],
            -a(3, 0) * s[3] + a(3, 1) * s[1] - a(3, 2) * s[0],
            a(2, 0) * s[3] - a(2, 1) * s[1] + a(2, 2) * s[0],
        ];
        Some(
            Self::from_cols_array(adjugate)
                .transpose()
                .divided_by(determinant),
        )
        .filter(|inverse| inverse.is_finite())
    }
}

matrix_common!(Mat4, Vec4, 4, 16);

/// The 2x2 minors of a column-major 4x4, over the column pairs 01, 02, 03, 12, 13, 23 in this
/// order: `s` from rows 0 and 1, `c` from rows 2 and 3.
fn minors(m: &[Scalar; 16]) -> ([Scalar; 6], [Scalar; 6]) {
    let a = |row: usize, column: usize| m[column * 4 + row];
    let s = [
        a(0, 0) * a(1, 1) - a(1, 0) * a(0, 1),
        a(0, 0) * a(1, 2) - a(1, 0) * a(0, 2),
        a(0, 0) * a(1, 3) - a(1, 0) * a(0, 3),
        a(0, 1) * a(1, 2) - a(1, 1) * a(0, 2),
        a(0, 1) * a(1, 3) - a(1, 1) * a(0, 3),
        a(0, 2) * a(1, 3) - a(1, 2) * a(0, 3),
    ];
    let c = [
        a(2, 0) * a(3, 1) - a(3, 0) * a(2, 1),
        a(2, 0) * a(3, 2) - a(3, 0) * a(2, 2),
        a(2, 0) * a(3, 3) - a(3, 0) * a(2, 3),
        a(2, 1) * a(3, 2) - a(3, 1) * a(2, 2),
        a(2, 1) * a(3, 3) - a(3, 1) * a(2, 3),
        a(2, 2) * a(3, 3) - a(3, 2) * a(2, 3),
    ];
    (s, c)
}

/// Laplace expansion along the first two rows.
fn determinant_from_minors(s: &[Scalar; 6], c: &[Scalar; 6]) -> Scalar {
    s[0] * c[5] - s[1] * c[4] + s[2] * c[3] + s[3] * c[2] - s[4] * c[1] + s[5] * c[0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Pcg32;
    use core::f32::consts::FRAC_PI_2;

    /// Random cases per property test; Miri is slow, so it gets fewer.
    const CASES: usize = if cfg!(miri) { 12 } else { 1_000 };
    /// 2^24: the first `f32` where adding 1.0 is lost (ties round to even), which makes the
    /// order of a sum visible in the result.
    const TWO_POW_24: Scalar = 16_777_216.0;
    /// Slack for results that are computed in a different order than the check (a few hundred
    /// ulps of the values involved, far below any sign or index mistake).
    const TOLERANCE: Scalar = 1.0e-3;
    /// Slack for the sine and cosine of a quarter turn.
    const ANGLE_TOLERANCE: Scalar = 1.0e-6;

    fn bits(values: &[Scalar]) -> Vec<u32> {
        values.iter().map(|value| value.to_bits()).collect()
    }

    fn assert_same(actual: &[Scalar], expected: &[Scalar]) {
        assert_eq!(bits(actual), bits(expected), "{actual:?} != {expected:?}");
    }

    fn assert_close(actual: &[Scalar], expected: &[Scalar], tolerance: Scalar) {
        assert_eq!(actual.len(), expected.len());
        for (a, e) in actual.iter().zip(expected) {
            assert!((a - e).abs() <= tolerance, "{actual:?} !~ {expected:?}");
        }
    }

    /// `first, first + 1, ...`: every element differs, all exactly representable.
    fn counting<const N: usize>(first: Scalar) -> [Scalar; N] {
        let mut next = first;
        core::array::from_fn(|_| {
            let value = next;
            next += 1.0;
            value
        })
    }

    /// A random `n`x`n` matrix, column-major, whose diagonal outweighs the rest of its row and
    /// column: invertible, and far from singular.
    fn dominant<const LEN: usize>(rng: &mut Pcg32, n: usize) -> [Scalar; LEN] {
        let weight = scalar(n) + 1.0;
        core::array::from_fn(|i| {
            let off_diagonal = rng.next_f32() * 2.0 - 1.0;
            let on_diagonal = Scalar::from(u8::from(i / n == i % n));
            off_diagonal * (1.0 - on_diagonal) + weight.copysign(off_diagonal) * on_diagonal
        })
    }

    /// A small count as a scalar.
    fn scalar(count: usize) -> Scalar {
        Scalar::from(u8::try_from(count).unwrap_or(u8::MAX))
    }

    /// Row times column, added left to right.
    fn reference_product(n: usize, a: &[Scalar], b: &[Scalar]) -> Vec<Scalar> {
        (0..n * n)
            .map(|i| row_times_column(n, a, b, i % n, i / n))
            .collect()
    }

    fn row_times_column(n: usize, a: &[Scalar], b: &[Scalar], row: usize, column: usize) -> Scalar {
        let mut sum = a[row] * b[column * n];
        for k in 1..n {
            sum += a[k * n + row] * b[column * n + k];
        }
        sum
    }

    /// Laplace expansion along the first column, in `f64`.
    fn reference_determinant(n: usize, m: &[f64]) -> f64 {
        if n == 1 {
            return m[0];
        }
        let mut sum = 0.0;
        for skip in 0..n {
            let minor: Vec<f64> = (1..n)
                .flat_map(|c| (0..n).filter(move |r| *r != skip).map(move |r| (c, r)))
                .map(|(c, r)| m[c * n + r])
                .collect();
            let sign = [1.0, -1.0][skip % 2];
            sum += sign * m[skip] * reference_determinant(n - 1, &minor);
        }
        sum
    }

    /// The same checks for every size; `$vec` is the column type, `$n` the dimension and `$len`
    /// the element count.
    macro_rules! suite {
        ($module:ident, $mat:ident, $vec:ident, $n:literal, $len:literal) => {
            mod $module {
                use super::*;

                const N: usize = $n;
                const LEN: usize = $len;

                fn m(elements: [Scalar; LEN]) -> $mat {
                    $mat::from_cols_array(elements)
                }

                fn v(components: [Scalar; N]) -> $vec {
                    $vec::from_array(components)
                }

                fn random_vector(rng: &mut Pcg32) -> [Scalar; N] {
                    core::array::from_fn(|_| rng.next_f32() * 16.0 - 8.0)
                }

                #[test]
                fn identity_has_ones_on_the_diagonal_only() {
                    let expected: [Scalar; LEN] =
                        core::array::from_fn(|i| if i % (N + 1) == 0 { 1.0 } else { 0.0 });
                    assert_same(&$mat::IDENTITY.to_cols_array(), &expected);
                }

                #[test]
                fn zero_and_default_are_zero_and_identity() {
                    assert_same(&$mat::ZERO.to_cols_array(), &[0.0; LEN]);
                    assert_eq!($mat::default(), $mat::IDENTITY);
                }

                #[test]
                fn elements_are_stored_column_major() {
                    let matrix = m(counting::<LEN>(1.0));
                    assert_same(&matrix.cols[0].to_array(), &counting::<N>(1.0));
                    assert_same(
                        &matrix.cols[N - 1].to_array(),
                        &counting::<N>(scalar(LEN - N + 1)),
                    );
                    assert_same(&matrix.to_cols_array(), &counting::<LEN>(1.0));
                }

                #[test]
                fn size_and_alignment_are_those_of_the_elements() {
                    assert_eq!(
                        core::mem::size_of::<$mat>(),
                        LEN * core::mem::size_of::<Scalar>()
                    );
                    assert_eq!(
                        core::mem::align_of::<$mat>(),
                        core::mem::align_of::<Scalar>()
                    );
                }

                #[test]
                fn transpose_swaps_rows_and_columns() {
                    let elements = counting::<LEN>(1.0);
                    let transposed = m(elements).transpose().to_cols_array();
                    for i in 0..LEN {
                        let (column, row) = (i / N, i % N);
                        assert!(
                            transposed[column * N + row].to_bits()
                                == elements[row * N + column].to_bits()
                        );
                    }
                }

                #[test]
                fn transposing_twice_gives_the_matrix_back() {
                    let matrix = m(counting::<LEN>(-4.0));
                    assert_eq!(matrix.transpose().transpose(), matrix);
                }

                #[test]
                fn a_matrix_times_a_vector_matches_the_row_by_row_sum() {
                    let mut rng = Pcg32::new(31, N as u64);
                    for _ in 0..CASES {
                        let a: [Scalar; LEN] =
                            core::array::from_fn(|_| rng.next_f32() * 16.0 - 8.0);
                        let x = random_vector(&mut rng);
                        let expected = reference_product(
                            N,
                            &a,
                            &core::array::from_fn::<Scalar, LEN, _>(
                                |i| if i < N { x[i] } else { 0.0 },
                            ),
                        );
                        assert_same(&(m(a) * v(x)).to_array(), &expected[..N]);
                    }
                }

                #[test]
                fn a_matrix_times_a_vector_adds_left_to_right() {
                    // Row 0 is (2^24, 1, 1, ..): left to right every 1.0 is lost, in any other
                    // order the ones are added to each other first.
                    let mut elements = [0.0; LEN];
                    elements[0] = TWO_POW_24;
                    for column in 1..N {
                        elements[column * N] = 1.0;
                    }
                    let result = m(elements) * v([1.0; N]);
                    assert_same(&[result.to_array()[0]], &[TWO_POW_24]);
                }

                #[test]
                fn a_row_of_negative_zeros_gives_a_negative_zero() {
                    // The sum starts from the first product, not from +0.0.
                    let mut elements = [0.0; LEN];
                    for column in 0..N {
                        elements[column * N] = -0.0;
                    }
                    let result = m(elements) * v([1.0; N]);
                    assert_same(&[result.to_array()[0]], &[-0.0]);
                }

                #[test]
                fn the_identity_leaves_a_vector_alone() {
                    let mut rng = Pcg32::new(37, N as u64);
                    for _ in 0..CASES {
                        let x = v(random_vector(&mut rng));
                        assert_eq!($mat::IDENTITY * x, x);
                    }
                }

                #[test]
                fn a_product_matches_the_row_by_column_sums() {
                    let mut rng = Pcg32::new(41, N as u64);
                    for _ in 0..CASES {
                        let a: [Scalar; LEN] =
                            core::array::from_fn(|_| rng.next_f32() * 16.0 - 8.0);
                        let b: [Scalar; LEN] =
                            core::array::from_fn(|_| rng.next_f32() * 16.0 - 8.0);
                        let expected = reference_product(N, &a, &b);
                        assert_same(&(m(a) * m(b)).to_cols_array(), &expected);
                    }
                }

                #[test]
                fn a_product_adds_left_to_right() {
                    // (2^24, 1, ..) times a column of ones: the first element of the product.
                    let mut a = [0.0; LEN];
                    a[0] = TWO_POW_24;
                    for column in 1..N {
                        a[column * N] = 1.0;
                    }
                    let product = m(a) * m([1.0; LEN]);
                    assert_same(&[product.to_cols_array()[0]], &[TWO_POW_24]);
                }

                #[test]
                fn the_identity_is_neutral_in_a_product() {
                    let mut rng = Pcg32::new(43, N as u64);
                    for _ in 0..CASES {
                        let a = m(core::array::from_fn(|_| rng.next_f32() * 16.0 - 8.0));
                        assert_eq!($mat::IDENTITY * a, a);
                        assert_eq!(a * $mat::IDENTITY, a);
                    }
                }

                #[test]
                fn the_transpose_of_a_product_is_the_product_of_the_transposes_reversed() {
                    let mut rng = Pcg32::new(47, N as u64);
                    for _ in 0..CASES {
                        let a = m(core::array::from_fn(|_| rng.next_f32() * 16.0 - 8.0));
                        let b = m(core::array::from_fn(|_| rng.next_f32() * 16.0 - 8.0));
                        let left = (a * b).transpose().to_cols_array();
                        let right = (b.transpose() * a.transpose()).to_cols_array();
                        assert_same(&left, &right);
                    }
                }

                #[test]
                fn a_product_applies_the_right_factor_first() {
                    let mut rng = Pcg32::new(53, N as u64);
                    for _ in 0..CASES {
                        let a = m(dominant::<LEN>(&mut rng, N));
                        let b = m(dominant::<LEN>(&mut rng, N));
                        let x = v(random_vector(&mut rng));
                        let together = ((a * b) * x).to_array();
                        let stepwise = (a * (b * x)).to_array();
                        let scale = 1.0 + stepwise.iter().fold(0.0, |s: Scalar, c| s.max(c.abs()));
                        assert_close(&together, &stepwise, TOLERANCE * scale);
                    }
                }

                #[test]
                fn the_determinant_of_the_identity_is_one() {
                    assert_same(&[$mat::IDENTITY.determinant()], &[1.0]);
                }

                #[test]
                fn the_determinant_matches_an_f64_expansion() {
                    let mut rng = Pcg32::new(59, N as u64);
                    for _ in 0..CASES {
                        let a = dominant::<LEN>(&mut rng, N);
                        let wide: Vec<f64> = a.iter().map(|e| f64::from(*e)).collect();
                        let expected = reference_determinant(N, &wide);
                        let actual = f64::from(m(a).determinant());
                        assert!(
                            (actual - expected).abs() <= 1.0e-4 * (1.0 + expected.abs()),
                            "{actual} vs {expected}"
                        );
                    }
                }

                #[test]
                fn the_determinant_of_a_product_is_the_product_of_the_determinants() {
                    let mut rng = Pcg32::new(61, N as u64);
                    for _ in 0..CASES {
                        let a = m(dominant::<LEN>(&mut rng, N));
                        let b = m(dominant::<LEN>(&mut rng, N));
                        let product = f64::from((a * b).determinant());
                        let expected = f64::from(a.determinant()) * f64::from(b.determinant());
                        assert!((product - expected).abs() <= 1.0e-3 * (1.0 + expected.abs()));
                    }
                }

                #[test]
                fn the_determinant_of_the_transpose_is_the_same() {
                    let mut rng = Pcg32::new(67, N as u64);
                    for _ in 0..CASES {
                        let a = m(dominant::<LEN>(&mut rng, N));
                        let (d, dt) = (
                            f64::from(a.determinant()),
                            f64::from(a.transpose().determinant()),
                        );
                        assert!((d - dt).abs() <= 1.0e-4 * (1.0 + d.abs()));
                    }
                }

                #[test]
                fn the_inverse_undoes_the_matrix_on_both_sides() {
                    let mut rng = Pcg32::new(71, N as u64);
                    for _ in 0..CASES {
                        let a = m(dominant::<LEN>(&mut rng, N));
                        let inverse = a.inverse();
                        assert!(inverse.is_some());
                        let inverse = inverse.unwrap_or(a);
                        let identity = $mat::IDENTITY.to_cols_array();
                        assert_close(&(a * inverse).to_cols_array(), &identity, TOLERANCE);
                        assert_close(&(inverse * a).to_cols_array(), &identity, TOLERANCE);
                    }
                }

                #[test]
                fn a_singular_matrix_has_no_inverse() {
                    // Every column is (1, 2, ..): rank one, small integers, so the determinant
                    // comes out as exactly zero.
                    let same_columns: [Scalar; LEN] = core::array::from_fn(|i| scalar(i % N) + 1.0);
                    assert_eq!(m(same_columns).inverse(), None);
                    assert_eq!($mat::ZERO.inverse(), None);
                }

                #[test]
                fn a_matrix_with_nan_or_infinity_has_no_inverse() {
                    for bad in [Scalar::NAN, Scalar::INFINITY] {
                        let mut elements = $mat::IDENTITY.to_cols_array();
                        elements[N + 1] = bad;
                        assert_eq!(m(elements).inverse(), None);
                    }
                }

                #[test]
                fn an_inverse_that_overflows_is_refused() {
                    // The determinant is a subnormal 1e-40, the quotients are infinite.
                    let mut elements = $mat::IDENTITY.to_cols_array();
                    elements[0] = 1.0e-40;
                    assert_eq!(m(elements).inverse(), None);
                }

                #[test]
                fn an_infinite_determinant_leaves_no_inverse() {
                    // Finite elements whose determinant overflows: dividing by it would give
                    // an all-zero "inverse".
                    let big: Scalar = if N == 3 { 1.0e13 } else { 1.0e10 };
                    let elements: [Scalar; LEN] =
                        core::array::from_fn(|i| if i / N == i % N { big } else { 0.0 });
                    let matrix = m(elements);
                    assert!(matrix.is_finite());
                    assert_eq!(matrix.inverse(), None);
                }

                #[test]
                fn is_finite_looks_at_every_element() {
                    assert!($mat::IDENTITY.is_finite());
                    for i in 0..LEN {
                        for bad in [Scalar::NAN, Scalar::INFINITY, Scalar::NEG_INFINITY] {
                            let mut elements = $mat::IDENTITY.to_cols_array();
                            elements[i] = bad;
                            assert!(!m(elements).is_finite());
                        }
                    }
                }
            }
        };
    }

    suite!(mat3, Mat3, Vec3, 3, 9);
    suite!(mat4, Mat4, Vec4, 4, 16);

    #[test]
    fn mat3_from_cols_puts_each_vector_in_its_column() {
        let matrix = Mat3::from_cols(
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::new(4.0, 5.0, 6.0),
            Vec3::new(7.0, 8.0, 9.0),
        );
        assert_same(&matrix.to_cols_array(), &counting::<9>(1.0));
    }

    #[test]
    fn mat3_from_scale_fills_the_diagonal() {
        let matrix = Mat3::from_scale(Vec3::new(2.0, 3.0, 4.0));
        assert_same(
            &matrix.to_cols_array(),
            &[2.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 4.0],
        );
        assert_eq!(matrix * Vec3::new(1.0, 1.0, 1.0), Vec3::new(2.0, 3.0, 4.0));
    }

    #[test]
    fn mat3_the_determinant_of_a_known_matrix() {
        let matrix = Mat3::from_cols_array([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 10.0]);
        assert_same(&[matrix.determinant()], &[-3.0]);
    }

    #[test]
    fn mat3_the_inverse_of_a_scale_is_the_reciprocal_scale() {
        let inverse = Mat3::from_scale(Vec3::new(2.0, 4.0, 8.0)).inverse();
        assert_eq!(inverse, Some(Mat3::from_scale(Vec3::new(0.5, 0.25, 0.125))));
    }

    #[test]
    fn mat3_the_inverse_of_a_known_matrix() {
        // Rows (2,0,0), (1,1,0), (0,3,4): inverse rows (0.5,0,0), (-0.5,1,0), (0.375,-0.75,0.25).
        let matrix = Mat3::from_cols_array([2.0, 1.0, 0.0, 0.0, 1.0, 3.0, 0.0, 0.0, 4.0]);
        let expected = Mat3::from_cols_array([0.5, -0.5, 0.375, 0.0, 1.0, -0.75, 0.0, 0.0, 0.25]);
        assert_eq!(matrix.inverse(), Some(expected));
    }

    #[test]
    fn mat3_a_quarter_turn_moves_one_axis_onto_the_next() {
        assert_close(
            &(Mat3::from_rotation_z(FRAC_PI_2) * Vec3::X).to_array(),
            &Vec3::Y.to_array(),
            ANGLE_TOLERANCE,
        );
        assert_close(
            &(Mat3::from_rotation_x(FRAC_PI_2) * Vec3::Y).to_array(),
            &Vec3::Z.to_array(),
            ANGLE_TOLERANCE,
        );
        assert_close(
            &(Mat3::from_rotation_y(FRAC_PI_2) * Vec3::Z).to_array(),
            &Vec3::X.to_array(),
            ANGLE_TOLERANCE,
        );
    }

    #[test]
    fn mat3_a_quarter_turn_leaves_its_own_axis_alone() {
        assert_close(
            &(Mat3::from_rotation_z(FRAC_PI_2) * Vec3::Z).to_array(),
            &Vec3::Z.to_array(),
            ANGLE_TOLERANCE,
        );
        assert_close(
            &(Mat3::from_rotation_x(FRAC_PI_2) * Vec3::X).to_array(),
            &Vec3::X.to_array(),
            ANGLE_TOLERANCE,
        );
        assert_close(
            &(Mat3::from_rotation_y(FRAC_PI_2) * Vec3::Y).to_array(),
            &Vec3::Y.to_array(),
            ANGLE_TOLERANCE,
        );
    }

    #[test]
    fn mat3_no_turn_is_the_identity() {
        assert_eq!(Mat3::from_rotation_x(0.0), Mat3::IDENTITY);
        assert_eq!(Mat3::from_rotation_y(0.0), Mat3::IDENTITY);
        assert_eq!(Mat3::from_rotation_z(0.0), Mat3::IDENTITY);
    }

    #[test]
    fn mat3_a_rotation_is_orthonormal_with_determinant_one() {
        let axes: [fn(Scalar) -> Mat3; 3] = [
            Mat3::from_rotation_x,
            Mat3::from_rotation_y,
            Mat3::from_rotation_z,
        ];
        let mut rng = Pcg32::new(73, 0);
        for index in 0..CASES * 3 {
            let rotation = axes[index % 3](rng.next_f32() * 12.0 - 6.0);
            assert_close(
                &(rotation.transpose() * rotation).to_cols_array(),
                &Mat3::IDENTITY.to_cols_array(),
                ANGLE_TOLERANCE,
            );
            assert_close(&[rotation.determinant()], &[1.0], ANGLE_TOLERANCE);
        }
    }

    #[test]
    fn mat3_the_inverse_of_a_rotation_is_its_transpose() {
        let rotation = Mat3::from_rotation_y(0.7) * Mat3::from_rotation_x(-1.3);
        let inverse = rotation.inverse().unwrap_or(Mat3::ZERO);
        assert_close(
            &inverse.to_cols_array(),
            &rotation.transpose().to_cols_array(),
            ANGLE_TOLERANCE,
        );
    }

    #[test]
    fn mat4_from_cols_puts_each_vector_in_its_column() {
        let matrix = Mat4::from_cols(
            Vec4::new(1.0, 2.0, 3.0, 4.0),
            Vec4::new(5.0, 6.0, 7.0, 8.0),
            Vec4::new(9.0, 10.0, 11.0, 12.0),
            Vec4::new(13.0, 14.0, 15.0, 16.0),
        );
        assert_same(&matrix.to_cols_array(), &counting::<16>(1.0));
    }

    #[test]
    fn mat4_a_translation_sits_in_the_last_column() {
        let matrix = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(matrix.cols[3], Vec4::new(1.0, 2.0, 3.0, 1.0));
        assert_eq!(matrix.cols[0], Vec4::X);
    }

    #[test]
    fn mat4_a_translation_moves_points_and_not_directions() {
        let matrix = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(
            matrix.transform_point3(Vec3::new(10.0, 20.0, 30.0)),
            Vec3::new(11.0, 22.0, 33.0)
        );
        assert_eq!(
            matrix.transform_vector3(Vec3::new(10.0, 20.0, 30.0)),
            Vec3::new(10.0, 20.0, 30.0)
        );
        assert_eq!(
            matrix * Vec4::new(1.0, 1.0, 1.0, 2.0),
            Vec4::new(3.0, 5.0, 7.0, 2.0)
        );
    }

    #[test]
    fn mat4_a_scale_multiplies_each_axis() {
        let matrix = Mat4::from_scale(Vec3::new(2.0, 3.0, 4.0));
        assert_same(
            &matrix.to_cols_array(),
            &[
                2.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ],
        );
        assert_eq!(
            matrix.transform_point3(Vec3::new(1.0, 1.0, 1.0)),
            Vec3::new(2.0, 3.0, 4.0)
        );
    }

    #[test]
    fn mat4_from_mat3_keeps_the_rotation_and_adds_the_identity() {
        let rotation = Mat3::from_rotation_z(0.5);
        let matrix = Mat4::from_mat3(rotation);
        assert_eq!(matrix.cols[0].truncate(), rotation.cols[0]);
        assert_eq!(matrix.cols[2].truncate(), rotation.cols[2]);
        assert_eq!(matrix.cols[3], Vec4::W);
        assert_same(
            &[matrix.cols[0].w, matrix.cols[1].w, matrix.cols[2].w],
            &[0.0, 0.0, 0.0],
        );
    }

    #[test]
    fn mat4_the_rotations_turn_like_the_three_by_three_ones() {
        // Compared with a tolerance: Miri perturbs `sin` and `cos` on purpose.
        assert_close(
            &Mat4::from_rotation_x(0.4).to_cols_array(),
            &Mat4::from_mat3(Mat3::from_rotation_x(0.4)).to_cols_array(),
            ANGLE_TOLERANCE,
        );
        assert_close(
            &Mat4::from_rotation_y(0.4).to_cols_array(),
            &Mat4::from_mat3(Mat3::from_rotation_y(0.4)).to_cols_array(),
            ANGLE_TOLERANCE,
        );
        assert_close(
            &Mat4::from_rotation_z(0.4).to_cols_array(),
            &Mat4::from_mat3(Mat3::from_rotation_z(0.4)).to_cols_array(),
            ANGLE_TOLERANCE,
        );
    }

    #[test]
    fn mat4_a_point_is_not_divided_by_w() {
        let matrix = Mat4::from_cols(Vec4::X, Vec4::Y, Vec4::Z, Vec4::new(0.0, 0.0, 0.0, 2.0));
        assert_eq!(
            matrix.transform_point3(Vec3::new(1.0, 2.0, 3.0)),
            Vec3::new(1.0, 2.0, 3.0)
        );
    }

    #[test]
    fn mat4_the_inverse_of_a_translation_is_the_opposite_translation() {
        let inverse = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0)).inverse();
        assert_eq!(
            inverse,
            Some(Mat4::from_translation(Vec3::new(-1.0, -2.0, -3.0)))
        );
    }

    #[test]
    fn mat4_the_inverse_of_a_scale_is_the_reciprocal_scale() {
        let inverse = Mat4::from_scale(Vec3::new(2.0, 4.0, 8.0)).inverse();
        assert_eq!(inverse, Some(Mat4::from_scale(Vec3::new(0.5, 0.25, 0.125))));
    }

    #[test]
    fn mat4_the_determinant_of_a_known_matrix() {
        // Upper triangular, so the product of the diagonal.
        let matrix = Mat4::from_cols_array([
            2.0, 0.0, 0.0, 0.0, 5.0, 3.0, 0.0, 0.0, 7.0, 11.0, 4.0, 0.0, 13.0, 17.0, 19.0, 5.0,
        ]);
        assert_same(&[matrix.determinant()], &[120.0]);
    }

    #[test]
    fn mat4_translate_rotate_scale_can_be_undone() {
        let mut rng = Pcg32::new(79, 0);
        for _ in 0..CASES {
            let translation = Vec3::new(
                rng.next_f32() * 20.0 - 10.0,
                rng.next_f32() * 20.0 - 10.0,
                rng.next_f32() * 20.0 - 10.0,
            );
            let scale = Vec3::new(
                rng.next_f32() + 0.5,
                rng.next_f32() + 0.5,
                rng.next_f32() + 0.5,
            );
            let angle = rng.next_f32() * 12.0 - 6.0;
            let transform = Mat4::from_translation(translation)
                * Mat4::from_rotation_y(angle)
                * Mat4::from_scale(scale);
            let point = Vec3::new(
                rng.next_f32() * 8.0 - 4.0,
                rng.next_f32() * 8.0 - 4.0,
                rng.next_f32() * 8.0 - 4.0,
            );
            let back = transform
                .inverse()
                .map(|inverse| inverse.transform_point3(transform.transform_point3(point)));
            assert!(back.is_some());
            assert_close(
                &back.unwrap_or(Vec3::ZERO).to_array(),
                &point.to_array(),
                TOLERANCE,
            );
            let expected = scale.x * scale.y * scale.z;
            assert_close(&[transform.determinant()], &[expected], TOLERANCE);
        }
    }

    fn ortho(extent: [Scalar; 6]) -> Option<Mat4> {
        let [left, right, bottom, top, near, far] = extent;
        Mat4::orthographic_rh(Vec2::new(left, bottom), Vec2::new(right, top), near, far)
    }

    /// The clip-space position of `point` (w stays 1 for an orthographic matrix).
    fn project(m: Mat4, point: Vec3) -> [Scalar; 3] {
        let p = m.transform_point3(point);
        [p.x, p.y, p.z]
    }

    #[test]
    fn orthographic_maps_the_view_box_to_the_clip_cube_with_depth_zero_to_one() {
        let m = ortho([-4.0, 4.0, -3.0, 3.0, 1.0, 11.0]).unwrap_or(Mat4::IDENTITY);
        assert_close(
            &project(m, Vec3::new(-4.0, -3.0, -1.0)),
            &[-1.0, -1.0, 0.0],
            1e-6,
        );
        assert_close(
            &project(m, Vec3::new(4.0, 3.0, -11.0)),
            &[1.0, 1.0, 1.0],
            1e-6,
        );
        assert_close(
            &project(m, Vec3::new(0.0, 0.0, -6.0)),
            &[0.0, 0.0, 0.5],
            1e-6,
        );
    }

    #[test]
    fn orthographic_is_off_centre_when_the_box_is() {
        let m = ortho([10.0, 30.0, 0.0, 10.0, 0.0, 1.0]).unwrap_or(Mat4::IDENTITY);
        assert_close(
            &project(m, Vec3::new(20.0, 5.0, -0.5)),
            &[0.0, 0.0, 0.5],
            1e-6,
        );
        assert_close(
            &project(m, Vec3::new(10.0, 10.0, 0.0)),
            &[-1.0, 1.0, 0.0],
            1e-6,
        );
    }

    #[test]
    fn orthographic_has_no_perspective_row_and_is_column_major() {
        let m = ortho([0.0, 2.0, 0.0, 2.0, 0.0, 1.0]).unwrap_or(Mat4::IDENTITY);
        let e = m.to_cols_array();
        assert_same(&[e[3], e[7], e[11], e[15]], &[0.0, 0.0, 0.0, 1.0]);
        assert_close(&[e[12], e[13], e[14]], &[-1.0, -1.0, 0.0], 0.0);
    }

    #[test]
    fn orthographic_refuses_extents_so_small_that_the_matrix_overflows() {
        assert_eq!(ortho([0.0, 1.0e-39, 0.0, 1.0, 0.0, 1.0]), None);
    }

    #[test]
    fn orthographic_refuses_an_empty_or_non_finite_box() {
        for (l, r, b, t, n, f) in [
            (1.0, 1.0, 0.0, 1.0, 0.0, 1.0),
            (0.0, 1.0, 2.0, 2.0, 0.0, 1.0),
            (0.0, 1.0, 0.0, 1.0, 3.0, 3.0),
            (0.0, Scalar::NAN, 0.0, 1.0, 0.0, 1.0),
            (0.0, Scalar::INFINITY, 0.0, 1.0, 0.0, 1.0),
            (0.0, 1.0, 0.0, 1.0, 0.0, Scalar::INFINITY),
        ] {
            assert_eq!(ortho([l, r, b, t, n, f]), None);
        }
    }
}
