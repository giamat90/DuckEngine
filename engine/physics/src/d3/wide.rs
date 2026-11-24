//! A vector in double precision, for the few steps of GJK where single precision loses all its
//! digits (card 8-75).
//!
//! The nearest point of a triangle to the origin (Ericson 5.1.5) multiplies dot products of the
//! edges. For a prism on a floor 100 m wide the triangles of GJK are slivers 140 m long and under a
//! metre wide: the products are about 1e8 where single precision keeps 8 units, the differences that
//! decide the region are about 1e4, and the point comes out 0.03 m wrong. In double precision the
//! same formulas keep about 1e-8 of that. Only `+ - * /` are used, in a fixed order (no fused
//! multiply-add, no libm), so the result is the same on every platform.

use base::math::Vec3;
use std::ops::{Add, Mul, Sub};

/// A 3D vector of `f64`.
#[derive(Clone, Copy)]
pub(super) struct Wide {
    x: f64,
    y: f64,
    z: f64,
}

impl Wide {
    pub(super) const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub(super) fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub(super) fn dot(self, other: Self) -> f64 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    pub(super) fn cross(self, other: Self) -> Self {
        Self {
            x: self.y * other.z - self.z * other.y,
            y: self.z * other.x - self.x * other.z,
            z: self.x * other.y - self.y * other.x,
        }
    }

    /// The vector rounded to single precision, to nearest, ties to even (the rounding mode the
    /// simulation asserts at the start of each job).
    pub(super) fn narrow(self) -> Vec3 {
        Vec3::new(narrow(self.x), narrow(self.y), narrow(self.z))
    }
}

impl From<Vec3> for Wide {
    fn from(v: Vec3) -> Self {
        Self::new(f64::from(v.x), f64::from(v.y), f64::from(v.z))
    }
}

impl Add for Wide {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }
}

impl Sub for Wide {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }
}

impl Mul<f64> for Wide {
    type Output = Self;
    fn mul(self, factor: f64) -> Self {
        Self::new(self.x * factor, self.y * factor, self.z * factor)
    }
}

/// `value` rounded to single precision.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the one rounding from double to single precision, to nearest"
)]
pub(super) fn narrow(value: f64) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_is_componentwise_and_the_cross_product_is_right_handed() {
        let a = Wide::new(1.0, 2.0, 3.0);
        let b = Wide::new(4.0, 5.0, 6.0);
        assert!((a.dot(b) - 32.0).abs() < 1e-12);
        let c = Wide::new(1.0, 0.0, 0.0).cross(Wide::new(0.0, 1.0, 0.0));
        assert!((c.z - 1.0).abs() < 1e-12 && c.x.abs() < 1e-12 && c.y.abs() < 1e-12);
        let sum = (a + b - a) * 2.0;
        assert!((sum.x - 8.0).abs() < 1e-12 && (sum.z - 12.0).abs() < 1e-12);
    }

    /// A sum that single precision loses (1e8 + 1 - 1e8) is kept, and `narrow` rounds to nearest.
    #[test]
    fn double_precision_keeps_what_single_precision_loses() {
        let big = Wide::from(Vec3::new(1.0e8, 0.0, 0.0));
        let kept = (big + Wide::new(1.0, 0.0, 0.0)) - big;
        assert!((kept.x - 1.0).abs() < 1e-12);
        let narrowed = Wide::new(0.1, -0.1, 1.0e-3).narrow();
        assert!((narrowed.x - 0.1_f32).abs() < f32::EPSILON);
        assert!((narrowed.y + 0.1_f32).abs() < f32::EPSILON);
    }
}
