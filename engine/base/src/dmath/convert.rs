//! `f64` to `f32` rounding in integer arithmetic.

/// Mantissa bits of an `f64` (52) and of an `f32` (23); the difference is dropped when rounding.
const F64_MANTISSA_BITS: u32 = 52;
const F32_MANTISSA_BITS: u32 = 23;
const DROPPED_BITS: u32 = F64_MANTISSA_BITS - F32_MANTISSA_BITS;
const F64_MANTISSA_MASK: u64 = (1 << F64_MANTISSA_BITS) - 1;
/// The implicit leading one of a normal `f64` mantissa.
const F64_IMPLICIT_ONE: u64 = 1 << F64_MANTISSA_BITS;
const F64_EXPONENT_MASK: u64 = 0x7ff;
/// Biased `f64` exponent of non-finite values.
const F64_EXPONENT_NON_FINITE: u64 = 0x7ff;
/// Biased exponent of an `f64` minus the biased exponent of the same `f32` value (1023 - 127).
const EXPONENT_REBIAS: u64 = 896;
/// Biased `f64` exponent from which a value is beyond the `f32` range before rounding (f32 exponent 255).
const F64_EXPONENT_OVERFLOW: u64 = EXPONENT_REBIAS + 255;
/// The shift that aligns the 53-bit significand of an `f64` to units of 2^-149 (the smallest
/// subnormal `f32`) is `SUBNORMAL_SHIFT_BASE - exponent`: 29 dropped bits, one for the leading one,
/// and the exponent offset.
const SUBNORMAL_SHIFT_BASE: u64 = EXPONENT_REBIAS + 30;
/// A shift of at least this many bits moves the whole 53-bit significand below half the smallest subnormal.
const SHIFT_TO_ZERO: u64 = 54;
const SIGN_SHIFT: u32 = 31;
const F32_EXPONENT_ALL_ONES: u32 = 0x7f80_0000;
/// Canonical quiet NaN of an `f32` (payloads are not preserved).
const F32_QUIET_NAN: u32 = 0x7fc0_0000;

/// A `u64` known to fit in 32 bits (by the masks and ranges at the call sites).
fn narrow(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Whether dropping the low `shift` bits of `value` rounds up: round to nearest, ties to even.
fn rounds_up(value: u64, shift: u32) -> bool {
    let rest = value & ((1 << shift) - 1);
    let half = 1 << (shift - 1);
    rest > half || (rest == half && (value >> shift) & 1 == 1)
}

/// Magnitude bits of a finite `f64` of biased exponent `exponent` and mantissa field `mantissa`.
fn finite_magnitude(exponent: u64, mantissa: u64) -> u32 {
    if exponent >= F64_EXPONENT_OVERFLOW {
        return F32_EXPONENT_ALL_ONES;
    }
    if exponent > EXPONENT_REBIAS {
        // Normal result. A carry out of the mantissa bumps the exponent, up to infinity.
        let unrounded = (narrow(exponent - EXPONENT_REBIAS) << F32_MANTISSA_BITS)
            + narrow(mantissa >> DROPPED_BITS);
        return unrounded + u32::from(rounds_up(mantissa, DROPPED_BITS));
    }
    // Subnormal or zero result. A carry out of the mantissa gives the smallest normal number.
    let shift = SUBNORMAL_SHIFT_BASE - exponent;
    if shift >= SHIFT_TO_ZERO {
        return 0;
    }
    let significand = F64_IMPLICIT_ONE | mantissa;
    let shift = narrow(shift);
    narrow(significand >> shift) + u32::from(rounds_up(significand, shift))
}

/// `x` rounded to the nearest `f32`, ties to even, in integer arithmetic.
///
/// The conversion instruction (`as f32`) rounds as the floating-point control register says and
/// Clippy refuses the cast; this version depends on neither. NaN becomes the canonical quiet NaN (positive, no payload).
/// The integer path does not depend on the denormals-are-zero / flush-to-zero flags either.
pub(super) fn to_f32(x: f64) -> f32 {
    let bits = x.to_bits();
    let sign = narrow(bits >> 63) << SIGN_SHIFT;
    let exponent = (bits >> F64_MANTISSA_BITS) & F64_EXPONENT_MASK;
    let mantissa = bits & F64_MANTISSA_MASK;
    if exponent == F64_EXPONENT_NON_FINITE && mantissa != 0 {
        return f32::from_bits(F32_QUIET_NAN);
    }
    let magnitude = if exponent == F64_EXPONENT_NON_FINITE {
        F32_EXPONENT_ALL_ONES
    } else {
        finite_magnitude(exponent, mantissa)
    };
    f32::from_bits(sign | magnitude)
}

#[cfg(test)]
mod tests {
    use super::to_f32;

    /// Exact equality of two `f32` by bit pattern (Clippy refuses `==` on floats).
    macro_rules! assert_same {
        ($actual:expr, $expected:expr $(, $($message:tt)+)?) => {
            assert_eq!(f32::to_bits($actual), f32::to_bits($expected) $(, $($message)+)?)
        };
    }

    /// Correctly rounded `f32` of the exact value of `x`, via the standard library's decimal parser.
    fn oracle(x: f64) -> f32 {
        format!("{x:.800e}").parse::<f32>().unwrap_or(f32::NAN)
    }

    fn same(a: f32, b: f32) -> bool {
        a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
    }

    #[test]
    fn exact_values_and_signed_zero_pass_through() {
        for x in [
            0.0,
            -0.0,
            1.0,
            -1.5,
            f64::from(0.1_f32),
            f64::from(f32::MAX),
            f64::from(f32::MIN_POSITIVE),
            f64::from(1.0e-45_f32),
        ] {
            assert!(same(to_f32(x), oracle(x)), "{x:e}");
        }
        assert_eq!(to_f32(-0.0).to_bits(), (-0.0_f32).to_bits());
    }

    #[test]
    fn specials_map_to_specials() {
        assert_same!(to_f32(f64::INFINITY), f32::INFINITY);
        assert_same!(to_f32(f64::NEG_INFINITY), f32::NEG_INFINITY);
        assert_eq!(to_f32(f64::NAN).to_bits(), super::F32_QUIET_NAN);
        assert_eq!(to_f32(-f64::NAN).to_bits(), super::F32_QUIET_NAN);
        assert_eq!(
            to_f32(f64::from_bits(0xfff0_0000_0000_0001)).to_bits(),
            super::F32_QUIET_NAN
        );
    }

    #[test]
    fn overflow_goes_to_infinity_at_the_rounding_boundary() {
        let max = f64::from(f32::MAX);
        let tie = max + 2.0_f64.powi(103); // half an ulp above the largest: the tie rounds to even, which is infinity
        let below = max + 2.0_f64.powi(102);
        assert_same!(to_f32(below), f32::MAX);
        assert_same!(to_f32(tie), f32::INFINITY);
        assert_same!(to_f32(-tie), f32::NEG_INFINITY);
        assert_same!(to_f32(1.0e300), f32::INFINITY);
    }

    #[test]
    fn ties_round_to_even() {
        let half_ulp = 2.0_f64.powi(-24);
        assert_same!(to_f32(1.0 + half_ulp), 1.0); // tie, even is 1.0
        assert_same!(
            to_f32(1.0 + 3.0 * half_ulp),
            f32::from_bits(1.0_f32.to_bits() + 2)
        ); // tie, even is up
        assert_same!(
            to_f32(1.0 + half_ulp + 2.0_f64.powi(-40)),
            f32::from_bits(1.0_f32.to_bits() + 1)
        );
    }

    #[test]
    fn subnormal_results_are_rounded_like_the_oracle() {
        let tiny = 2.0_f64.powi(-149);
        for k in 0..64 {
            let x = tiny * (f64::from(k) * 0.25 + 0.125);
            assert!(same(to_f32(x), oracle(x)), "{x:e}");
            assert!(same(to_f32(-x), oracle(-x)), "{x:e}");
        }
        assert_same!(to_f32(tiny * 0.5), 0.0); // tie between 0 and the smallest subnormal: even is 0
        assert_eq!(to_f32(tiny * 1.5).to_bits(), 2); // tie between 1 and 2: even is 2
        assert_eq!(
            to_f32(2.0_f64.powi(-126) - tiny * 0.25).to_bits(),
            0x0080_0000
        ); // carries into the smallest normal
        assert_same!(to_f32(1.0e-300), 0.0);
        assert_same!(to_f32(5.0e-324), 0.0);
    }

    #[test]
    fn a_dense_sweep_of_bit_patterns_matches_the_oracle() {
        // From below the smallest subnormal (2^-150 and under) to above the largest normal.
        let mut bits: u64 = 0x3600_0000_0000_0000;
        while bits < 0x47f8_0000_0000_0000 {
            let x = f64::from_bits(bits);
            assert!(same(to_f32(x), oracle(x)), "{x:e}");
            bits += 0x0000_1234_5678_9abc;
        }
    }
}
