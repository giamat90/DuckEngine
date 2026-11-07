//! The libm users of `base::math` (the `Mat3` rotation constructors, `Quat::from_axis_angle` and
//! `Quat::slerp`) call the facade `base::dmath`: with the feature `cross-platform-determinism` on
//! that means the own implementations, off the platform ones. Both configurations run these tests.

use base::dmath;
use base::math::{Mat3, Quat, Vec3};

/// Largest deviation of a column of a rotation matrix from unit length, and of two columns from
/// orthogonality: the rounding of `sin` and `cos` (0.5 ulp each, so below 1.3e-7 for `s^2 + c^2`)
/// and of one product and one sum.
const ORTHONORMAL_BOUND: f32 = 3.0e-7;

fn angles() -> impl Iterator<Item = f32> {
    (0_u16..4000).map(|i| (f32::from(i) - 2000.0) * 0.031_7 + 0.001_3)
}

fn assert_orthonormal(m: Mat3, angle: f32) {
    let product = m.transpose() * m;
    for (i, column) in product.cols.iter().enumerate() {
        let expected = [Vec3::X, Vec3::Y, Vec3::Z][i];
        let diff = *column - expected;
        assert!(
            diff.x.abs().max(diff.y.abs()).max(diff.z.abs()) <= ORTHONORMAL_BOUND,
            "{angle}: {product:?}"
        );
    }
}

#[test]
fn the_rotation_matrices_use_the_facade_bit_for_bit() {
    for angle in angles() {
        let (s, c) = dmath::sin_cos(angle);
        assert_eq!(
            Mat3::from_rotation_x(angle).cols,
            [Vec3::X, Vec3::new(0.0, c, s), Vec3::new(0.0, -s, c)],
            "x {angle}"
        );
        assert_eq!(
            Mat3::from_rotation_y(angle).cols,
            [Vec3::new(c, 0.0, -s), Vec3::Y, Vec3::new(s, 0.0, c)],
            "y {angle}"
        );
        assert_eq!(
            Mat3::from_rotation_z(angle).cols,
            [Vec3::new(c, s, 0.0), Vec3::new(-s, c, 0.0), Vec3::Z],
            "z {angle}"
        );
    }
}

#[test]
fn the_rotation_matrices_are_orthonormal_within_the_bound() {
    for angle in angles() {
        assert_orthonormal(Mat3::from_rotation_x(angle), angle);
        assert_orthonormal(Mat3::from_rotation_y(angle), angle);
        assert_orthonormal(Mat3::from_rotation_z(angle), angle);
    }
}

#[test]
fn the_axis_angle_quaternion_uses_the_facade_bit_for_bit() {
    let axis = Vec3::new(1.0, 2.0, 3.0);
    let unit = axis.normalized();
    for angle in angles() {
        let (s, c) = dmath::sin_cos(angle * 0.5);
        let n = unit.unwrap_or(Vec3::X);
        let expected = Quat::new(n.x * s, n.y * s, n.z * s, c);
        assert_eq!(
            Quat::from_axis_angle(axis, angle),
            Some(expected),
            "{angle}"
        );
    }
}

#[test]
fn the_axis_angle_quaternion_has_unit_length_within_the_bound() {
    for angle in angles() {
        let q = Quat::from_axis_angle(Vec3::new(-0.3, 0.8, 0.5), angle).unwrap_or(Quat::IDENTITY);
        assert!(
            (q.length() - 1.0).abs() <= ORTHONORMAL_BOUND,
            "{angle}: {}",
            q.length()
        );
    }
}

#[test]
fn slerp_uses_the_facade_bit_for_bit() {
    let a = Quat::IDENTITY;
    let b = Quat::from_axis_angle(Vec3::new(0.2, 1.0, -0.4), 1.2).unwrap_or(Quat::IDENTITY);
    let angle = dmath::acos(a.dot(b));
    let sin_angle = dmath::sin(angle);
    for i in 0_u16..=400 {
        let t = (f32::from(i) - 100.0) * 0.005;
        let from = dmath::sin((1.0 - t) * angle) / sin_angle;
        let to = dmath::sin(t * angle) / sin_angle;
        let blend = Quat::new(
            a.x * from + b.x * to,
            a.y * from + b.y * to,
            a.z * from + b.z * to,
            a.w * from + b.w * to,
        );
        assert_eq!(a.slerp(b, t), blend.normalized(), "{t}");
    }
}
