//! Scalar reference for four `f32` lanes: the oracle every ISA backend is tested against.
//!
//! Every operation is one IEEE-754 operation per lane, with no fusing and no
//! estimate instructions. NaN payloads and signs are unspecified: simulation
//! code must not produce NaN, and the backend tests treat any two NaNs as equal.

use std::ops::{Add, Div, Mul, Neg, Sub};

/// Four `f32` lanes, lane 0 first.
#[derive(Debug, Clone, Copy)]
pub struct F32x4([f32; 4]);

impl F32x4 {
    #[inline]
    #[must_use]
    pub fn splat(value: f32) -> Self {
        Self([value; 4])
    }

    #[inline]
    #[must_use]
    pub fn from_array(lanes: [f32; 4]) -> Self {
        Self(lanes)
    }

    #[inline]
    #[must_use]
    pub fn to_array(self) -> [f32; 4] {
        self.0
    }

    /// Correctly rounded square root per lane (`sqrt` is exact in IEEE-754).
    #[inline]
    #[must_use]
    pub fn sqrt(self) -> Self {
        Self(self.0.map(f32::sqrt))
    }

    /// Per lane `a < b ? a : b`: returns `other` when the lanes are equal or either is NaN.
    #[inline]
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        self.zip(other, |a, b| if a < b { a } else { b })
    }

    /// Per lane `a > b ? a : b`: returns `other` when the lanes are equal or either is NaN.
    #[inline]
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        self.zip(other, |a, b| if a > b { a } else { b })
    }

    /// Clears the sign bit of every lane.
    #[inline]
    #[must_use]
    pub fn abs(self) -> Self {
        Self(self.0.map(f32::abs))
    }

    /// Sum of the lanes in the fixed order `(l0 + l1) + (l2 + l3)`.
    #[inline]
    #[must_use]
    pub fn horizontal_sum(self) -> f32 {
        let [l0, l1, l2, l3] = self.0;
        (l0 + l1) + (l2 + l3)
    }

    fn zip(self, other: Self, op: fn(f32, f32) -> f32) -> Self {
        let [a0, a1, a2, a3] = self.0;
        let [b0, b1, b2, b3] = other.0;
        Self([op(a0, b0), op(a1, b1), op(a2, b2), op(a3, b3)])
    }
}

impl Add for F32x4 {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        self.zip(rhs, |a, b| a + b)
    }
}

impl Sub for F32x4 {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        self.zip(rhs, |a, b| a - b)
    }
}

impl Mul for F32x4 {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Self) -> Self {
        self.zip(rhs, |a, b| a * b)
    }
}

impl Div for F32x4 {
    type Output = Self;
    #[inline]
    fn div(self, rhs: Self) -> Self {
        self.zip(rhs, |a, b| a / b)
    }
}

impl Neg for F32x4 {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Self(self.0.map(|lane| -lane))
    }
}

#[cfg(test)]
mod tests {
    use super::F32x4;

    fn lanes(a: [f32; 4]) -> F32x4 {
        F32x4::from_array(a)
    }

    fn bits(v: [f32; 4]) -> [u32; 4] {
        v.map(f32::to_bits)
    }

    #[test]
    fn splat_fills_every_lane() {
        assert_eq!(bits(F32x4::splat(2.5).to_array()), bits([2.5; 4]));
    }

    #[test]
    fn from_array_keeps_lane_order() {
        let a = [1.0, 2.0, 3.0, 4.0];
        assert_eq!(bits(lanes(a).to_array()), bits(a));
    }

    #[test]
    fn add_sub_mul_div_work_lane_by_lane() {
        let a = lanes([8.0, 6.0, 4.0, 2.0]);
        let b = lanes([2.0, 3.0, 4.0, 5.0]);
        assert_eq!(bits((a + b).to_array()), bits([10.0, 9.0, 8.0, 7.0]));
        assert_eq!(bits((a - b).to_array()), bits([6.0, 3.0, 0.0, -3.0]));
        assert_eq!(bits((a * b).to_array()), bits([16.0, 18.0, 16.0, 10.0]));
        assert_eq!(bits((a / b).to_array()), bits([4.0, 2.0, 1.0, 0.4]));
    }

    #[test]
    fn sqrt_is_exact_on_squares_and_nan_on_negatives() {
        let r = lanes([4.0, 9.0, 0.0, -1.0]).sqrt().to_array();
        assert_eq!(bits([r[0], r[1], r[2], 0.0]), bits([2.0, 3.0, 0.0, 0.0]));
        assert!(r[3].is_nan());
    }

    #[test]
    fn neg_and_abs_flip_and_clear_the_sign_bit() {
        let v = lanes([1.0, -2.0, -0.0, 0.0]);
        assert_eq!(bits((-v).to_array()), bits([-1.0, 2.0, 0.0, -0.0]));
        assert_eq!(bits(v.abs().to_array()), bits([1.0, 2.0, 0.0, 0.0]));
    }

    #[test]
    fn min_and_max_return_the_second_operand_when_unordered_or_equal() {
        // Same rule as the x86 instructions: min = a < b ? a : b, max = a > b ? a : b.
        let a = lanes([1.0, f32::NAN, 0.0, 5.0]);
        let b = lanes([2.0, 3.0, -0.0, f32::NAN]);
        let lo = a.min(b).to_array();
        let hi = a.max(b).to_array();
        assert_eq!(
            bits([lo[0], lo[1], lo[2], 0.0]),
            bits([1.0, 3.0, -0.0, 0.0])
        );
        assert!(lo[3].is_nan());
        assert_eq!(
            bits([hi[0], hi[1], hi[2], 0.0]),
            bits([2.0, 3.0, -0.0, 0.0])
        );
        assert!(hi[3].is_nan());
    }

    #[test]
    fn horizontal_sum_adds_in_pairs_lane0_lane1_then_lane2_lane3() {
        // Pairwise: (1e8 + 1) + (-1e8 + 5) = 1e8 + -99999992 = 8. Left to right would give 5.
        let v = lanes([1.0e8, 1.0, -1.0e8, 5.0]);
        assert_eq!(v.horizontal_sum().to_bits(), 8.0_f32.to_bits());
    }
}
