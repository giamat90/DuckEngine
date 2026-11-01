//! NEON backend for four `f32` lanes (`AArch64` baseline: no runtime detection needed).
//!
//! Each operation is one IEEE-754 operation per lane (or a sign-bit operation, a compare-and-select
//! with the scalar reference's rule, or the fixed add order of `horizontal_sum`); no fused
//! multiply-add (`vfmaq`, `vmlaq`), no reciprocal or square-root estimate (`vrecpe`, `vrsqrte`),
//! and no `vminq`/`vmaxq`, whose NaN and zero rules differ from the reference. Tests compare every
//! operation with [`super::scalar`] bit for bit; they need an `AArch64` CPU (or an emulator) to run.

#![expect(
    unsafe_code,
    reason = "intrinsics are unsafe fns even where the feature is in the target baseline; every block has a SAFETY comment"
)]

use std::arch::aarch64::{
    float32x4_t, vabsq_f32, vaddq_f32, vbslq_f32, vcgtq_f32, vcltq_f32, vdivq_f32, vdupq_n_f32,
    vgetq_lane_f32, vld1q_f32, vmulq_f32, vnegq_f32, vpaddq_f32, vsqrtq_f32, vst1q_f32, vsubq_f32,
};
use std::ops::{Add, Div, Mul, Neg, Sub};

/// Four `f32` lanes in one NEON register, lane 0 first.
#[derive(Clone, Copy)]
pub struct F32x4(float32x4_t);

impl std::fmt::Debug for F32x4 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("F32x4").field(&self.to_array()).finish()
    }
}

impl F32x4 {
    #[inline]
    #[must_use]
    pub fn splat(value: f32) -> Self {
        // SAFETY: NEON is enabled for this build (see the `cfg` on this module).
        Self(unsafe { vdupq_n_f32(value) })
    }

    #[inline]
    #[must_use]
    pub fn from_array(lanes: [f32; 4]) -> Self {
        // SAFETY: NEON is enabled for this build; `lanes` is valid for four `f32` reads and
        // `vld1q_f32` has no alignment requirement beyond that of `f32`.
        Self(unsafe { vld1q_f32(lanes.as_ptr()) })
    }

    #[inline]
    #[must_use]
    pub fn to_array(self) -> [f32; 4] {
        let mut lanes = [0.0_f32; 4];
        // SAFETY: NEON is enabled for this build; `lanes` is valid for four `f32` writes.
        unsafe { vst1q_f32(lanes.as_mut_ptr(), self.0) };
        lanes
    }

    /// Correctly rounded square root per lane (`fsqrt`).
    #[inline]
    #[must_use]
    pub fn sqrt(self) -> Self {
        // SAFETY: NEON is enabled for this build (see the `cfg` on this module).
        Self(unsafe { vsqrtq_f32(self.0) })
    }

    /// Per lane `a < b ? a : b`: returns `other` when equal or unordered. A compare and a select,
    /// because `fmin` returns NaN for a NaN operand and orders the zeros.
    #[inline]
    #[must_use]
    pub fn min(self, other: Self) -> Self {
        // SAFETY: NEON is enabled for this build (see the `cfg` on this module).
        Self(unsafe { vbslq_f32(vcltq_f32(self.0, other.0), self.0, other.0) })
    }

    /// Per lane `a > b ? a : b`: returns `other` when equal or unordered (see [`Self::min`]).
    #[inline]
    #[must_use]
    pub fn max(self, other: Self) -> Self {
        // SAFETY: NEON is enabled for this build (see the `cfg` on this module).
        Self(unsafe { vbslq_f32(vcgtq_f32(self.0, other.0), self.0, other.0) })
    }

    /// Clears the sign bit of every lane.
    #[inline]
    #[must_use]
    pub fn abs(self) -> Self {
        // SAFETY: NEON is enabled for this build (see the `cfg` on this module).
        Self(unsafe { vabsq_f32(self.0) })
    }

    /// Sum of the lanes in the fixed order `(l0 + l1) + (l2 + l3)`.
    #[inline]
    #[must_use]
    pub fn horizontal_sum(self) -> f32 {
        // SAFETY: NEON is enabled for this build (see the `cfg` on this module).
        unsafe {
            // Lane 0 holds l0 + l1 and lane 1 holds l2 + l3; the last add is an ordinary f32 add.
            let pairs = vpaddq_f32(self.0, self.0);
            vgetq_lane_f32::<0>(pairs) + vgetq_lane_f32::<1>(pairs)
        }
    }
}

impl Add for F32x4 {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        // SAFETY: NEON is enabled for this build (see the `cfg` on this module).
        Self(unsafe { vaddq_f32(self.0, rhs.0) })
    }
}

impl Sub for F32x4 {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: Self) -> Self {
        // SAFETY: NEON is enabled for this build (see the `cfg` on this module).
        Self(unsafe { vsubq_f32(self.0, rhs.0) })
    }
}

impl Mul for F32x4 {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Self) -> Self {
        // SAFETY: NEON is enabled for this build (see the `cfg` on this module).
        Self(unsafe { vmulq_f32(self.0, rhs.0) })
    }
}

impl Div for F32x4 {
    type Output = Self;
    #[inline]
    fn div(self, rhs: Self) -> Self {
        // SAFETY: NEON is enabled for this build (see the `cfg` on this module).
        Self(unsafe { vdivq_f32(self.0, rhs.0) })
    }
}

impl Neg for F32x4 {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        // SAFETY: NEON is enabled for this build (see the `cfg` on this module).
        Self(unsafe { vnegq_f32(self.0) })
    }
}

#[cfg(test)]
mod tests {
    use super::F32x4;
    use crate::rng::Pcg32;
    use crate::simd::scalar::F32x4 as Reference;

    /// Miri interprets every instruction, so it gets a short run.
    const CASES: usize = if cfg!(miri) { 40 } else { 20_000 };
    /// Values where backends tend to disagree: zeros, infinities, NaN, subnormal, extremes.
    const SPECIAL: [f32; 9] = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        1.0e-45,
        f32::MAX,
    ];

    fn lane(rng: &mut Pcg32) -> f32 {
        let draw = rng.next_u32();
        let pick = usize::try_from(draw >> 8).unwrap_or(0) % SPECIAL.len();
        match draw % 4 {
            0 => SPECIAL[pick],
            1 => f32::from_bits(rng.next_u32()),
            _ => rng.next_f32() * 200.0 - 100.0,
        }
    }

    fn lanes(rng: &mut Pcg32) -> [f32; 4] {
        [lane(rng), lane(rng), lane(rng), lane(rng)]
    }

    /// Same bits, or both NaN (the payload of a NaN is unspecified).
    fn same_lane(a: f32, b: f32) -> bool {
        a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
    }

    fn same(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| same_lane(*x, y))
    }

    fn check_binary(
        seed: u64,
        reference: fn(Reference, Reference) -> Reference,
        backend: fn(F32x4, F32x4) -> F32x4,
    ) {
        let mut rng = Pcg32::new(seed, 1);
        for _ in 0..CASES {
            let (a, b) = (lanes(&mut rng), lanes(&mut rng));
            let want = reference(Reference::from_array(a), Reference::from_array(b)).to_array();
            let got = backend(F32x4::from_array(a), F32x4::from_array(b)).to_array();
            assert!(same(want, got), "a={a:?} b={b:?} want={want:?} got={got:?}");
        }
    }

    fn same_bits(a: [f32; 4], b: [f32; 4]) -> bool {
        a.map(f32::to_bits) == b.map(f32::to_bits)
    }

    fn check_unary(
        seed: u64,
        reference: fn(Reference) -> Reference,
        backend: fn(F32x4) -> F32x4,
        equal: fn([f32; 4], [f32; 4]) -> bool,
    ) {
        let mut rng = Pcg32::new(seed, 2);
        for _ in 0..CASES {
            let a = lanes(&mut rng);
            let want = reference(Reference::from_array(a)).to_array();
            let got = backend(F32x4::from_array(a)).to_array();
            assert!(equal(want, got), "a={a:?} want={want:?} got={got:?}");
        }
    }

    #[test]
    fn splat_and_from_array_keep_lane_order() {
        let v = F32x4::from_array([1.0, 2.0, 3.0, 4.0]).to_array();
        assert!(same(v, [1.0, 2.0, 3.0, 4.0]));
        assert!(same(F32x4::splat(2.5).to_array(), [2.5; 4]));
    }

    #[test]
    fn add_matches_the_reference_bit_for_bit() {
        check_binary(1, |a, b| a + b, |a, b| a + b);
    }

    #[test]
    fn sub_matches_the_reference_bit_for_bit() {
        check_binary(2, |a, b| a - b, |a, b| a - b);
    }

    #[test]
    fn mul_matches_the_reference_bit_for_bit() {
        check_binary(3, |a, b| a * b, |a, b| a * b);
    }

    #[test]
    fn div_matches_the_reference_bit_for_bit() {
        check_binary(4, |a, b| a / b, |a, b| a / b);
    }

    #[test]
    fn min_matches_the_reference_bit_for_bit() {
        check_binary(5, Reference::min, F32x4::min);
    }

    #[test]
    fn max_matches_the_reference_bit_for_bit() {
        check_binary(6, Reference::max, F32x4::max);
    }

    #[test]
    fn sqrt_matches_the_reference_bit_for_bit() {
        check_unary(7, Reference::sqrt, F32x4::sqrt, same);
    }

    #[test]
    fn abs_matches_the_reference_bit_for_bit() {
        // abs and neg only touch the sign bit, so even NaN lanes must match exactly.
        check_unary(8, Reference::abs, F32x4::abs, same_bits);
    }

    #[test]
    fn neg_matches_the_reference_bit_for_bit() {
        check_unary(9, |a| -a, |a| -a, same_bits);
    }

    #[test]
    fn horizontal_sum_matches_the_reference_bit_for_bit() {
        let mut rng = Pcg32::new(10, 3);
        for _ in 0..CASES {
            let a = lanes(&mut rng);
            let want = Reference::from_array(a).horizontal_sum();
            let got = F32x4::from_array(a).horizontal_sum();
            assert!(same_lane(want, got), "a={a:?} want={want:?} got={got:?}");
        }
    }

    #[test]
    fn horizontal_sum_uses_the_documented_pair_order() {
        let v = F32x4::from_array([1.0e8, 1.0, -1.0e8, 5.0]);
        assert_eq!(v.horizontal_sum().to_bits(), 8.0_f32.to_bits());
    }

    #[test]
    fn min_and_max_return_the_second_operand_when_equal_or_unordered() {
        // NEON `fmin`/`fmax` would return NaN and order the zeros; the reference returns `other`.
        let a = F32x4::from_array([0.0, -0.0, f32::NAN, 1.0]);
        let b = F32x4::from_array([-0.0, 0.0, 2.0, f32::NAN]);
        let lo = a.min(b).to_array();
        let hi = a.max(b).to_array();
        assert_eq!(lo[0].to_bits(), (-0.0_f32).to_bits());
        assert_eq!(lo[1].to_bits(), 0.0_f32.to_bits());
        assert_eq!(lo[2].to_bits(), 2.0_f32.to_bits());
        assert!(lo[3].is_nan());
        assert_eq!(hi[0].to_bits(), (-0.0_f32).to_bits());
        assert_eq!(hi[1].to_bits(), 0.0_f32.to_bits());
        assert_eq!(hi[2].to_bits(), 2.0_f32.to_bits());
        assert!(hi[3].is_nan());
    }

    #[test]
    fn a_multiply_and_an_add_round_twice() {
        // (1 + e)^2 = 1 + 2e + e^2. Rounded on its own the product is 1 + 2e, so adding -(1 + 2e) gives
        // exactly 0; a fused multiply-add would keep the e^2 term and give a non-zero result.
        let a = F32x4::splat(1.0 + f32::EPSILON);
        let c = F32x4::splat(-(1.0 + 2.0 * f32::EPSILON));
        let lanes = (a * a + c).to_array();
        assert!(
            lanes.iter().all(|lane| lane.to_bits() == 0.0_f32.to_bits()),
            "{lanes:?}"
        );
    }

    #[test]
    fn debug_prints_the_lanes_in_order() {
        let v = F32x4::from_array([1.0, 2.0, 3.0, 4.0]);
        assert_eq!(format!("{v:?}"), "F32x4([1.0, 2.0, 3.0, 4.0])");
    }
}
