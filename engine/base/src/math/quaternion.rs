//! Unit quaternion for 3D rotation: [`Quat`].
//!
//! Hamilton convention, stored as `x`, `y`, `z` (the vector part) and `w` (the scalar part), so
//! the identity is `(0, 0, 0, 1)`. A rotation is a *unit* quaternion; the methods that need one
//! (`rotate`, `to_mat3`, `to_mat4`, `slerp`) say so and do not check. Rotations are
//! counter-clockwise about the axis in a right-handed system, like [`Mat3::from_rotation_z`].
//!
//! The order of every floating-point operation is fixed and nothing is fused, so results repeat
//! from run to run. Only `from_axis_angle` and `slerp` use `sin`, `cos` or `acos` from `std`,
//! which are the platform libm (the same on one machine and build, not across platforms).

use super::{Mat3, Mat4, Scalar, Vec3};
use std::ops::{Mul, Neg};

/// Two unit quaternions whose dot product is above this are less than about `3.6` degrees of
/// rotation apart (`acos(0.9995)` is `1.8` degrees between the quaternions). `slerp` uses
/// `nlerp` there instead, which is nearly identical, so it never divides by a tiny sine.
const NEARLY_PARALLEL: Scalar = 0.9995;

/// A quaternion, a rotation when it has unit length.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct Quat {
    /// First component of the vector part.
    pub x: Scalar,
    /// Second component of the vector part.
    pub y: Scalar,
    /// Third component of the vector part.
    pub z: Scalar,
    /// The scalar part.
    pub w: Scalar,
}

impl Quat {
    /// No rotation.
    pub const IDENTITY: Self = Self::new(0.0, 0.0, 0.0, 1.0);

    /// From the components in field order. Not normalised.
    #[inline]
    #[must_use]
    pub const fn new(x: Scalar, y: Scalar, z: Scalar, w: Scalar) -> Self {
        Self { x, y, z, w }
    }

    /// From `[x, y, z, w]`.
    #[inline]
    #[must_use]
    pub const fn from_array(components: [Scalar; 4]) -> Self {
        let [x, y, z, w] = components;
        Self { x, y, z, w }
    }

    /// The components as `[x, y, z, w]`.
    #[inline]
    #[must_use]
    pub const fn to_array(self) -> [Scalar; 4] {
        [self.x, self.y, self.z, self.w]
    }

    /// Rotation by `angle` radians counter-clockwise about `axis`, or `None` when the axis has
    /// no direction (zero, NaN or infinite length, or so short that its squared length
    /// underflows) or the angle is NaN or infinite. The axis need not be a unit vector.
    #[inline]
    #[must_use]
    pub fn from_axis_angle(axis: Vec3, angle: Scalar) -> Option<Self> {
        let axis = axis.normalized()?;
        let (sin, cos) = (angle * 0.5).sin_cos();
        Some(Self::new(axis.x * sin, axis.y * sin, axis.z * sin, cos)).filter(|q| q.is_finite())
    }

    /// The opposite rotation, for a unit quaternion: the vector part negated.
    #[inline]
    #[must_use]
    pub fn conjugate(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, self.w)
    }

    /// Sum of the component products, added in the order `x`, `y`, `z`, `w`.
    #[inline]
    #[must_use]
    pub fn dot(self, other: Self) -> Scalar {
        self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w
    }

    /// `self.dot(self)`.
    #[inline]
    #[must_use]
    pub fn length_squared(self) -> Scalar {
        self.dot(self)
    }

    /// The correctly rounded square root of the squared length.
    #[inline]
    #[must_use]
    pub fn length(self) -> Scalar {
        self.length_squared().sqrt()
    }

    /// Divided by its length, or `None` when that length is zero, NaN or infinite. The length
    /// comes from the squared length, so components above about `1e19` overflow and components
    /// below about `1e-19` underflow (both give `None`), like
    /// [`Vec4::normalized`](super::Vec4::normalized).
    #[inline]
    #[must_use]
    pub fn normalized(self) -> Option<Self> {
        let length = self.length();
        if length.is_finite() && length > 0.0 {
            Some(self.divided_by(length))
        } else {
            None
        }
    }

    /// The inverse for any non-zero quaternion: the conjugate over the squared length. `None`
    /// when that is zero, NaN or infinite, or when the result is not finite. Less accurate when
    /// the squared length is subnormal.
    #[inline]
    #[must_use]
    pub fn inverse(self) -> Option<Self> {
        let squared = self.length_squared();
        if !squared.is_finite() {
            return None;
        }
        Some(self.conjugate().divided_by(squared)).filter(|inverse| inverse.is_finite())
    }

    /// `true` when every component is neither infinite nor NaN.
    #[inline]
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite() && self.w.is_finite()
    }

    /// Rotates `vector`, for a unit `self`: `v + w*t + u x t` with `u` the vector part and
    /// `t = 2 * (u x v)`.
    #[inline]
    #[must_use]
    pub fn rotate(self, vector: Vec3) -> Vec3 {
        let u = Vec3::new(self.x, self.y, self.z);
        let t = u.cross(vector) * 2.0;
        vector + t * self.w + u.cross(t)
    }

    /// The rotation matrix, for a unit `self`.
    #[inline]
    #[must_use]
    pub fn to_mat3(self) -> Mat3 {
        let (x2, y2, z2) = (self.x + self.x, self.y + self.y, self.z + self.z);
        let (xx, yy, zz) = (self.x * x2, self.y * y2, self.z * z2);
        let (xy, xz, yz) = (self.x * y2, self.x * z2, self.y * z2);
        let (wx, wy, wz) = (self.w * x2, self.w * y2, self.w * z2);
        Mat3::from_cols(
            Vec3::new(1.0 - (yy + zz), xy + wz, xz - wy),
            Vec3::new(xy - wz, 1.0 - (xx + zz), yz + wx),
            Vec3::new(xz + wy, yz - wx, 1.0 - (xx + yy)),
        )
    }

    /// The rotation as a `Mat4` without translation, for a unit `self`.
    #[inline]
    #[must_use]
    pub fn to_mat4(self) -> Mat4 {
        Mat4::from_mat3(self.to_mat3())
    }

    /// Normalised linear interpolation along the shorter arc: `self` at `t = 0` and `other` at
    /// `t = 1`, with an angular speed that is not constant. `t` is not clamped. `None` when the
    /// blend cannot be normalised (not finite, or zero), for instance for a NaN `t`. Both
    /// quaternions should be unit.
    #[inline]
    #[must_use]
    pub fn nlerp(self, other: Self, t: Scalar) -> Option<Self> {
        let other = self.shorter_arc(other);
        self.scaled(1.0 - t).added(other.scaled(t)).normalized()
    }

    /// Spherical interpolation along the shorter arc at constant angular speed: `self` at
    /// `t = 0` and `other` at `t = 1`. `t` is not clamped: outside `0..=1` it keeps turning at
    /// the same speed. Falls back to [`Self::nlerp`] when the two are nearly the same rotation.
    /// `None` when the result cannot be normalised (non-finite input or `t`). Both quaternions
    /// must be unit.
    #[inline]
    #[must_use]
    pub fn slerp(self, other: Self, t: Scalar) -> Option<Self> {
        let other = self.shorter_arc(other);
        let cos_angle = self.dot(other);
        if cos_angle > NEARLY_PARALLEL {
            return self.nlerp(other, t);
        }
        let angle = cos_angle.acos();
        let sin_angle = angle.sin();
        let from = ((1.0 - t) * angle).sin() / sin_angle;
        let to = (t * angle).sin() / sin_angle;
        self.scaled(from).added(other.scaled(to)).normalized()
    }

    /// `other`, or its negation when that is the same rotation reached by the shorter arc.
    #[inline]
    fn shorter_arc(self, other: Self) -> Self {
        if self.dot(other) < 0.0 { -other } else { other }
    }

    #[inline]
    fn scaled(self, factor: Scalar) -> Self {
        Self::new(
            self.x * factor,
            self.y * factor,
            self.z * factor,
            self.w * factor,
        )
    }

    #[inline]
    fn divided_by(self, divisor: Scalar) -> Self {
        Self::new(
            self.x / divisor,
            self.y / divisor,
            self.z / divisor,
            self.w / divisor,
        )
    }

    #[inline]
    fn added(self, other: Self) -> Self {
        Self::new(
            self.x + other.x,
            self.y + other.y,
            self.z + other.z,
            self.w + other.w,
        )
    }
}

impl Default for Quat {
    /// The identity, so that a default rotation turns nothing.
    #[inline]
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Neg for Quat {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, -self.w)
    }
}

/// The Hamilton product: `(a * b).rotate(v)` is `a.rotate(b.rotate(v))`, so `b` applies first,
/// as for matrices.
impl Mul for Quat {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Self) -> Self {
        Self::new(
            self.w * rhs.x + self.x * rhs.w + self.y * rhs.z - self.z * rhs.y,
            self.w * rhs.y - self.x * rhs.z + self.y * rhs.w + self.z * rhs.x,
            self.w * rhs.z + self.x * rhs.y - self.y * rhs.x + self.z * rhs.w,
            self.w * rhs.w - self.x * rhs.x - self.y * rhs.y - self.z * rhs.z,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Pcg32;
    use core::f32::consts::{FRAC_PI_2, FRAC_PI_4, FRAC_PI_6, PI};

    /// Random cases per property test; Miri is slow, so it gets fewer.
    const CASES: usize = if cfg!(miri) { 12 } else { 1_000 };
    /// 2^24: the first `f32` where adding 1.0 is lost (ties round to even), which makes the
    /// order of a sum visible in the result.
    const TWO_POW_24: Scalar = 16_777_216.0;
    /// Slack for results computed in a different order than the check (a few hundred ulps of
    /// values near 1, far below any sign or index mistake).
    const TOLERANCE: Scalar = 1.0e-5;
    /// Slack for `acos`, which loses precision near 1.
    const LOOSE_TOLERANCE: Scalar = 2.0e-3;
    /// Not a rotation: stands in for "no result" where a test has already asserted `Some`.
    const ZERO: Quat = Quat::new(0.0, 0.0, 0.0, 0.0);

    /// A rotation matrix constructor of `Mat3`.
    type MatrixFn = fn(Scalar) -> Mat3;

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

    /// The same rotation, whichever of `q` and `-q` is given: compares the matrices.
    fn assert_same_rotation(a: Quat, b: Quat, tolerance: Scalar) {
        assert_close(
            &a.to_mat3().to_cols_array(),
            &b.to_mat3().to_cols_array(),
            tolerance,
        );
    }

    fn random_components(rng: &mut Pcg32) -> [Scalar; 4] {
        core::array::from_fn(|_| rng.next_f32() * 16.0 - 8.0)
    }

    fn random_quat(rng: &mut Pcg32) -> Quat {
        Quat::from_array(random_components(rng))
    }

    fn random_vec3(rng: &mut Pcg32) -> Vec3 {
        Vec3::new(
            rng.next_f32() * 16.0 - 8.0,
            rng.next_f32() * 16.0 - 8.0,
            rng.next_f32() * 16.0 - 8.0,
        )
    }

    /// A random rotation: a random axis and an angle in `[-6, 6)`.
    fn random_unit(rng: &mut Pcg32) -> Quat {
        let axis = random_vec3(rng);
        Quat::from_axis_angle(axis, rng.next_f32() * 12.0 - 6.0).unwrap_or(Quat::IDENTITY)
    }

    #[test]
    fn the_identity_is_a_unit_scalar_part_only() {
        assert_same(&Quat::IDENTITY.to_array(), &[0.0, 0.0, 0.0, 1.0]);
        assert_eq!(Quat::default(), Quat::IDENTITY);
    }

    #[test]
    fn components_are_stored_x_y_z_w() {
        let q = Quat::new(1.0, 2.0, 3.0, 4.0);
        assert_same(&q.to_array(), &[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(Quat::from_array([1.0, 2.0, 3.0, 4.0]), q);
        assert_same(&[q.x, q.y, q.z, q.w], &[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(core::mem::size_of::<Quat>(), 16);
        assert_eq!(
            core::mem::align_of::<Quat>(),
            core::mem::align_of::<Scalar>()
        );
    }

    #[test]
    fn the_units_multiply_like_hamilton_said() {
        let i = Quat::new(1.0, 0.0, 0.0, 0.0);
        let j = Quat::new(0.0, 1.0, 0.0, 0.0);
        let k = Quat::new(0.0, 0.0, 1.0, 0.0);
        let minus_one = Quat::new(0.0, 0.0, 0.0, -1.0);
        assert_eq!(i * j, k);
        assert_eq!(j * k, i);
        assert_eq!(k * i, j);
        assert_eq!(j * i, -k);
        assert_eq!(k * j, -i);
        assert_eq!(i * k, -j);
        assert_eq!(i * i, minus_one);
        assert_eq!(j * j, minus_one);
        assert_eq!(k * k, minus_one);
    }

    #[test]
    fn a_product_matches_the_written_out_formula_bit_for_bit() {
        let mut rng = Pcg32::new(101, 0);
        for _ in 0..CASES {
            let (a, b) = (random_quat(&mut rng), random_quat(&mut rng));
            let expected = [
                a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
                a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
                a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
                a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
            ];
            assert_same(&(a * b).to_array(), &expected);
        }
    }

    #[test]
    fn the_identity_is_neutral_in_a_product() {
        let mut rng = Pcg32::new(103, 0);
        for _ in 0..CASES {
            let q = random_quat(&mut rng);
            assert_eq!(Quat::IDENTITY * q, q);
            assert_eq!(q * Quat::IDENTITY, q);
        }
    }

    #[test]
    fn a_product_applies_the_right_factor_first() {
        let mut rng = Pcg32::new(107, 0);
        for _ in 0..CASES {
            let (a, b) = (random_unit(&mut rng), random_unit(&mut rng));
            let v = random_vec3(&mut rng);
            let together = (a * b).rotate(v).to_array();
            let stepwise = a.rotate(b.rotate(v)).to_array();
            assert_close(&together, &stepwise, 1.0e-4);
        }
    }

    #[test]
    fn a_product_of_rotations_is_the_product_of_their_matrices() {
        let mut rng = Pcg32::new(109, 0);
        for _ in 0..CASES {
            let (a, b) = (random_unit(&mut rng), random_unit(&mut rng));
            let expected = (a.to_mat3() * b.to_mat3()).to_cols_array();
            assert_close(&(a * b).to_mat3().to_cols_array(), &expected, TOLERANCE);
        }
    }

    #[test]
    fn a_product_is_associative_up_to_rounding() {
        let mut rng = Pcg32::new(113, 0);
        for _ in 0..CASES {
            let (a, b, c) = (
                random_unit(&mut rng),
                random_unit(&mut rng),
                random_unit(&mut rng),
            );
            assert_close(
                &((a * b) * c).to_array(),
                &(a * (b * c)).to_array(),
                TOLERANCE,
            );
        }
    }

    #[test]
    fn the_conjugate_of_a_product_reverses_the_factors() {
        let mut rng = Pcg32::new(127, 0);
        for _ in 0..CASES {
            let (a, b) = (random_unit(&mut rng), random_unit(&mut rng));
            let left = (a * b).conjugate().to_array();
            assert_close(
                &left,
                &(b.conjugate() * a.conjugate()).to_array(),
                TOLERANCE,
            );
        }
    }

    #[test]
    fn the_conjugate_negates_the_vector_part_only() {
        let q = Quat::new(1.0, -2.0, 3.0, 4.0);
        assert_same(&q.conjugate().to_array(), &[-1.0, 2.0, -3.0, 4.0]);
        assert_eq!(q.conjugate().conjugate(), q);
    }

    #[test]
    fn a_quaternion_times_its_conjugate_is_its_squared_length() {
        let mut rng = Pcg32::new(131, 0);
        for _ in 0..CASES {
            let q = random_quat(&mut rng);
            let expected = [0.0, 0.0, 0.0, q.length_squared()];
            assert_close(&(q * q.conjugate()).to_array(), &expected, 1.0e-3);
        }
    }

    #[test]
    fn negation_flips_every_component() {
        assert_same(
            &(-Quat::new(1.0, -2.0, 0.5, 4.0)).to_array(),
            &[-1.0, 2.0, -0.5, -4.0],
        );
    }

    #[test]
    fn the_dot_product_of_a_known_pair() {
        let (a, b) = (Quat::new(1.0, 2.0, 3.0, 4.0), Quat::new(5.0, 6.0, 7.0, 8.0));
        assert_same(&[a.dot(b)], &[70.0]);
    }

    #[test]
    fn the_dot_product_adds_x_y_z_w_in_that_order() {
        // ((2^24 + 1) + 1) + 1 loses every 1.0; any order that adds the ones first does not.
        let a = Quat::new(TWO_POW_24, 1.0, 1.0, 1.0);
        let b = Quat::new(1.0, 1.0, 1.0, 1.0);
        assert_same(&[a.dot(b)], &[TWO_POW_24]);
    }

    #[test]
    fn the_length_of_a_known_quaternion() {
        let q = Quat::new(1.0, 2.0, 2.0, 4.0);
        assert_same(&[q.length_squared(), q.length()], &[25.0, 5.0]);
    }

    #[test]
    fn normalizing_divides_by_the_length() {
        let unit = Quat::new(1.0, 2.0, 2.0, 4.0).normalized();
        let expected = [1.0 / 5.0, 2.0 / 5.0, 2.0 / 5.0, 4.0 / 5.0];
        assert_eq!(unit.map(|u| bits(&u.to_array())), Some(bits(&expected)));
    }

    #[test]
    fn a_random_quaternion_normalizes_to_length_one() {
        let mut rng = Pcg32::new(137, 0);
        for _ in 0..CASES {
            let unit = random_quat(&mut rng).normalized();
            assert!(unit.is_some());
            assert_close(&[unit.unwrap_or(ZERO).length()], &[1.0], TOLERANCE);
        }
    }

    #[test]
    fn a_quaternion_without_a_length_does_not_normalize() {
        assert_eq!(Quat::new(0.0, 0.0, 0.0, 0.0).normalized(), None);
        assert_eq!(Quat::new(Scalar::NAN, 0.0, 0.0, 1.0).normalized(), None);
        assert_eq!(
            Quat::new(Scalar::INFINITY, 0.0, 0.0, 1.0).normalized(),
            None
        );
        assert_eq!(Quat::new(1.0e20, 1.0e20, 1.0e20, 1.0e20).normalized(), None);
    }

    #[test]
    fn the_inverse_multiplies_back_to_the_identity() {
        let mut rng = Pcg32::new(139, 0);
        for _ in 0..CASES {
            let q = random_quat(&mut rng);
            let inverse = q.inverse();
            assert!(inverse.is_some());
            let inverse = inverse.unwrap_or(q);
            assert_close(
                &(q * inverse).to_array(),
                &Quat::IDENTITY.to_array(),
                TOLERANCE,
            );
            assert_close(
                &(inverse * q).to_array(),
                &Quat::IDENTITY.to_array(),
                TOLERANCE,
            );
        }
    }

    #[test]
    fn the_inverse_of_a_unit_quaternion_is_its_conjugate() {
        let mut rng = Pcg32::new(149, 0);
        for _ in 0..CASES {
            let q = random_unit(&mut rng);
            let inverse = q.inverse().unwrap_or(Quat::IDENTITY);
            assert_close(&inverse.to_array(), &q.conjugate().to_array(), TOLERANCE);
        }
    }

    #[test]
    fn some_quaternions_have_no_inverse() {
        assert_eq!(Quat::new(0.0, 0.0, 0.0, 0.0).inverse(), None);
        assert_eq!(Quat::new(Scalar::NAN, 0.0, 0.0, 1.0).inverse(), None);
        // The squared length overflows: dividing would give an all-zero "inverse".
        assert_eq!(Quat::new(1.0e20, 1.0e20, 1.0e20, 1.0e20).inverse(), None);
        // The squared length underflows to zero.
        assert_eq!(
            Quat::new(1.0e-30, 1.0e-30, 1.0e-30, 1.0e-30).inverse(),
            None
        );
    }

    #[test]
    fn is_finite_looks_at_every_component() {
        assert!(Quat::IDENTITY.is_finite());
        for index in 0..12 {
            let mut components = [0.0, 0.0, 0.0, 1.0];
            components[index % 4] =
                [Scalar::NAN, Scalar::INFINITY, Scalar::NEG_INFINITY][index / 4];
            assert!(!Quat::from_array(components).is_finite());
        }
    }

    #[test]
    fn a_quarter_turn_moves_one_axis_onto_the_next() {
        let about_z = Quat::from_axis_angle(Vec3::Z, FRAC_PI_2).unwrap_or(Quat::IDENTITY);
        let about_x = Quat::from_axis_angle(Vec3::X, FRAC_PI_2).unwrap_or(Quat::IDENTITY);
        let about_y = Quat::from_axis_angle(Vec3::Y, FRAC_PI_2).unwrap_or(Quat::IDENTITY);
        assert_close(
            &about_z.rotate(Vec3::X).to_array(),
            &Vec3::Y.to_array(),
            TOLERANCE,
        );
        assert_close(
            &about_x.rotate(Vec3::Y).to_array(),
            &Vec3::Z.to_array(),
            TOLERANCE,
        );
        assert_close(
            &about_y.rotate(Vec3::Z).to_array(),
            &Vec3::X.to_array(),
            TOLERANCE,
        );
    }

    #[test]
    fn a_half_turn_reverses_the_perpendicular_axes() {
        let q = Quat::from_axis_angle(Vec3::Z, PI).unwrap_or(Quat::IDENTITY);
        assert_close(
            &q.rotate(Vec3::X).to_array(),
            &(-Vec3::X).to_array(),
            TOLERANCE,
        );
        assert_close(
            &q.rotate(Vec3::Z).to_array(),
            &Vec3::Z.to_array(),
            TOLERANCE,
        );
    }

    #[test]
    fn no_turn_is_the_identity() {
        assert_eq!(
            Quat::from_axis_angle(Vec3::new(1.0, 2.0, 3.0), 0.0),
            Some(Quat::IDENTITY)
        );
    }

    #[test]
    fn an_axis_need_not_be_a_unit_vector() {
        // Compared with a tolerance: Miri perturbs `sin` and `cos` on purpose.
        let long = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 5.0), 0.7).unwrap_or(ZERO);
        let unit = Quat::from_axis_angle(Vec3::Z, 0.7).unwrap_or(ZERO);
        assert!(long != ZERO);
        assert_close(&long.to_array(), &unit.to_array(), TOLERANCE);
    }

    #[test]
    fn an_axis_without_a_direction_gives_no_rotation() {
        assert_eq!(Quat::from_axis_angle(Vec3::ZERO, 1.0), None);
        assert_eq!(
            Quat::from_axis_angle(Vec3::new(Scalar::NAN, 0.0, 1.0), 1.0),
            None
        );
        assert_eq!(
            Quat::from_axis_angle(Vec3::new(Scalar::INFINITY, 0.0, 1.0), 1.0),
            None
        );
    }

    #[test]
    fn an_angle_that_is_not_a_number_gives_no_rotation() {
        assert_eq!(Quat::from_axis_angle(Vec3::Z, Scalar::NAN), None);
        assert_eq!(Quat::from_axis_angle(Vec3::Z, Scalar::INFINITY), None);
        assert_eq!(
            Quat::from_axis_angle(Vec3::new(1.0e-30, 0.0, 0.0), 1.0),
            None
        );
    }

    #[test]
    fn a_negative_angle_is_the_opposite_rotation() {
        let mut rng = Pcg32::new(151, 0);
        for _ in 0..CASES {
            let axis = random_vec3(&mut rng);
            let angle = rng.next_f32() * 6.0;
            let forward = Quat::from_axis_angle(axis, angle).unwrap_or(Quat::IDENTITY);
            let backward = Quat::from_axis_angle(axis, -angle).unwrap_or(Quat::IDENTITY);
            assert_close(
                &backward.to_array(),
                &forward.conjugate().to_array(),
                TOLERANCE,
            );
        }
    }

    #[test]
    fn an_axis_angle_rotation_has_length_one() {
        let mut rng = Pcg32::new(157, 0);
        for _ in 0..CASES {
            assert_close(&[random_unit(&mut rng).length()], &[1.0], TOLERANCE);
        }
    }

    #[test]
    fn the_axis_angle_rotations_match_the_rotation_matrices() {
        let cases: [(Vec3, MatrixFn); 3] = [
            (Vec3::X, Mat3::from_rotation_x),
            (Vec3::Y, Mat3::from_rotation_y),
            (Vec3::Z, Mat3::from_rotation_z),
        ];
        let mut rng = Pcg32::new(163, 0);
        for index in 0..CASES * 3 {
            let (about, matrix) = cases[index % 3];
            let angle = rng.next_f32() * 12.0 - 6.0;
            let q = Quat::from_axis_angle(about, angle).unwrap_or(Quat::IDENTITY);
            assert_close(
                &q.to_mat3().to_cols_array(),
                &matrix(angle).to_cols_array(),
                TOLERANCE,
            );
        }
    }

    #[test]
    fn rotating_by_the_identity_changes_nothing() {
        let mut rng = Pcg32::new(167, 0);
        for _ in 0..CASES {
            let v = random_vec3(&mut rng);
            assert_eq!(Quat::IDENTITY.rotate(v), v);
        }
    }

    #[test]
    fn rotating_matches_the_matrix() {
        let mut rng = Pcg32::new(173, 0);
        for _ in 0..CASES {
            let (q, v) = (random_unit(&mut rng), random_vec3(&mut rng));
            assert_close(
                &q.rotate(v).to_array(),
                &(q.to_mat3() * v).to_array(),
                1.0e-4,
            );
        }
    }

    #[test]
    fn rotating_matches_the_sandwich_product() {
        let mut rng = Pcg32::new(179, 0);
        for _ in 0..CASES {
            let (q, v) = (random_unit(&mut rng), random_vec3(&mut rng));
            let sandwich = q * Quat::new(v.x, v.y, v.z, 0.0) * q.conjugate();
            assert_close(
                &q.rotate(v).to_array(),
                &[sandwich.x, sandwich.y, sandwich.z],
                1.0e-4,
            );
        }
    }

    #[test]
    fn rotating_keeps_the_length() {
        let mut rng = Pcg32::new(181, 0);
        for _ in 0..CASES {
            let (q, v) = (random_unit(&mut rng), random_vec3(&mut rng));
            assert_close(&[q.rotate(v).length()], &[v.length()], 1.0e-4);
        }
    }

    #[test]
    fn the_matrix_of_the_identity_is_the_identity() {
        assert_eq!(Quat::IDENTITY.to_mat3(), Mat3::IDENTITY);
        assert_eq!(Quat::IDENTITY.to_mat4(), Mat4::IDENTITY);
    }

    #[test]
    fn the_matrix_of_a_rotation_is_orthonormal_with_determinant_one() {
        let mut rng = Pcg32::new(191, 0);
        for _ in 0..CASES {
            let matrix = random_unit(&mut rng).to_mat3();
            let identity = Mat3::IDENTITY.to_cols_array();
            assert_close(
                &(matrix.transpose() * matrix).to_cols_array(),
                &identity,
                TOLERANCE,
            );
            assert_close(&[matrix.determinant()], &[1.0], TOLERANCE);
        }
    }

    #[test]
    fn the_four_by_four_matrix_is_the_three_by_three_one_without_translation() {
        let q = Quat::from_axis_angle(Vec3::new(1.0, 2.0, 3.0), 0.9).unwrap_or(Quat::IDENTITY);
        assert_eq!(q.to_mat4(), Mat4::from_mat3(q.to_mat3()));
    }

    #[test]
    fn a_blend_at_the_ends_is_one_of_the_rotations() {
        let mut rng = Pcg32::new(193, 0);
        for _ in 0..CASES {
            let (a, b) = (random_unit(&mut rng), random_unit(&mut rng));
            let (start, end) = (a.nlerp(b, 0.0), a.nlerp(b, 1.0));
            assert_same_rotation(start.unwrap_or(ZERO), a, TOLERANCE);
            assert_same_rotation(end.unwrap_or(ZERO), b, TOLERANCE);
            let (start, end) = (a.slerp(b, 0.0), a.slerp(b, 1.0));
            assert_same_rotation(start.unwrap_or(ZERO), a, TOLERANCE);
            // The weights at t = 1 differ from 1 by a few ulps (and Miri perturbs `sin`).
            assert_same_rotation(end.unwrap_or(ZERO), b, 1.0e-4);
        }
    }

    #[test]
    fn a_blend_has_length_one() {
        let mut rng = Pcg32::new(197, 0);
        for _ in 0..CASES {
            let (a, b, t) = (random_unit(&mut rng), random_unit(&mut rng), rng.next_f32());
            let near = a.nlerp(b, t).unwrap_or(ZERO);
            let far = a.slerp(b, t).unwrap_or(ZERO);
            assert_close(&[near.length(), far.length()], &[1.0, 1.0], TOLERANCE);
        }
    }

    #[test]
    fn a_blend_takes_the_shorter_arc() {
        let mut rng = Pcg32::new(199, 0);
        for _ in 0..CASES {
            let (a, b, t) = (random_unit(&mut rng), random_unit(&mut rng), rng.next_f32());
            let (direct, flipped) = (a.nlerp(b, t), a.nlerp(-b, t));
            assert_same_rotation(direct.unwrap_or(ZERO), flipped.unwrap_or(ZERO), TOLERANCE);
            let (direct, flipped) = (a.slerp(b, t), a.slerp(-b, t));
            assert_same_rotation(direct.unwrap_or(ZERO), flipped.unwrap_or(ZERO), TOLERANCE);
        }
    }

    #[test]
    fn halfway_between_two_turns_about_one_axis_is_the_middle_angle() {
        let a = Quat::IDENTITY;
        let b = Quat::from_axis_angle(Vec3::Z, FRAC_PI_2).unwrap_or(Quat::IDENTITY);
        let expected = [FRAC_PI_4.cos(), FRAC_PI_4.sin(), 0.0];
        let near = a.nlerp(b, 0.5).unwrap_or(ZERO);
        let far = a.slerp(b, 0.5).unwrap_or(ZERO);
        assert_close(&near.rotate(Vec3::X).to_array(), &expected, TOLERANCE);
        assert_close(&far.rotate(Vec3::X).to_array(), &expected, TOLERANCE);
    }

    #[test]
    fn slerp_turns_at_a_constant_speed_and_nlerp_does_not() {
        let a = Quat::IDENTITY;
        let b = Quat::from_axis_angle(Vec3::Z, FRAC_PI_2).unwrap_or(Quat::IDENTITY);
        let third = 1.0 / 3.0;
        let thirty_degrees = [FRAC_PI_6.cos(), FRAC_PI_6.sin(), 0.0];
        let far = a.slerp(b, third).unwrap_or(ZERO);
        assert_close(&far.rotate(Vec3::X).to_array(), &thirty_degrees, TOLERANCE);
        let near = a.nlerp(b, third).unwrap_or(ZERO);
        let gap = (near.rotate(Vec3::X).y - thirty_degrees[1]).abs();
        assert!(
            gap > 1.0e-3,
            "nlerp should lag behind slerp by about 0.7 degrees, gap {gap}"
        );
    }

    #[test]
    fn slerp_moves_by_the_fraction_of_the_angle() {
        let mut rng = Pcg32::new(211, 0);
        for _ in 0..CASES {
            let (a, b, t) = (random_unit(&mut rng), random_unit(&mut rng), rng.next_f32());
            let angle = a.dot(b).abs().min(1.0).acos();
            let moved = a.slerp(b, t).unwrap_or(ZERO);
            assert_close(&[a.dot(moved)], &[(t * angle).cos()], LOOSE_TOLERANCE);
        }
    }

    #[test]
    fn slerp_keeps_turning_beyond_the_end() {
        // Twice a quarter turn is a half turn: +X ends up at -X.
        let b = Quat::from_axis_angle(Vec3::Z, FRAC_PI_2).unwrap_or(Quat::IDENTITY);
        let beyond = Quat::IDENTITY.slerp(b, 2.0).unwrap_or(ZERO);
        assert_close(
            &beyond.rotate(Vec3::X).to_array(),
            &(-Vec3::X).to_array(),
            1.0e-4,
        );
    }

    #[test]
    fn slerp_of_nearly_equal_rotations_is_nlerp() {
        let a = Quat::from_axis_angle(Vec3::Y, 0.3).unwrap_or(Quat::IDENTITY);
        let b = Quat::from_axis_angle(Vec3::Y, 0.31).unwrap_or(Quat::IDENTITY);
        assert!(a.dot(b) > NEARLY_PARALLEL);
        assert_eq!(a.slerp(b, 0.4), a.nlerp(b, 0.4));
    }

    #[test]
    fn a_dot_product_of_exactly_the_threshold_still_uses_the_sine_formula() {
        let a = Quat::IDENTITY;
        let z = (1.0 - NEARLY_PARALLEL * NEARLY_PARALLEL).sqrt();
        let b = Quat::new(0.0, 0.0, z, NEARLY_PARALLEL);
        assert_same(&[a.dot(b)], &[NEARLY_PARALLEL]);
        let (sine, linear) = (
            a.slerp(b, 0.2).unwrap_or(ZERO),
            a.nlerp(b, 0.2).unwrap_or(ZERO),
        );
        assert!((sine.z - linear.z).abs() > 1.0e-8, "{sine:?} vs {linear:?}");
    }

    #[test]
    fn perpendicular_quaternions_are_not_flipped() {
        // A dot product of exactly zero: both arcs are equally short, `other` is used as given.
        let a = Quat::IDENTITY;
        let b = Quat::new(1.0, 0.0, 0.0, 0.0);
        let blend = a.nlerp(b, 0.25).unwrap_or(ZERO);
        assert!(blend.x > 0.0, "{blend:?}");
        let blend = a.slerp(b, 0.25).unwrap_or(ZERO);
        assert!(blend.x > 0.0, "{blend:?}");
    }

    #[test]
    fn a_blend_of_something_that_is_not_a_rotation_is_refused() {
        let nan = Quat::new(Scalar::NAN, 0.0, 0.0, 1.0);
        assert_eq!(Quat::IDENTITY.nlerp(nan, 0.5), None);
        assert_eq!(Quat::IDENTITY.slerp(nan, 0.5), None);
        let b = Quat::from_axis_angle(Vec3::Z, 1.0).unwrap_or(Quat::IDENTITY);
        assert_eq!(Quat::IDENTITY.nlerp(b, Scalar::NAN), None);
        assert_eq!(Quat::IDENTITY.slerp(b, Scalar::NAN), None);
        let zero = Quat::new(0.0, 0.0, 0.0, 0.0);
        assert_eq!(zero.nlerp(zero, 0.5), None);
    }
}
