//! `exp`, `log`, `pow` and `powi` for `f32`.
//!
//! `exp` and `log` are computed in `f64` (Cody-Waite reduction with `ln 2` in two pieces, series of
//! [`super::series`]) and rounded once to `f32`; `pow` is `exp(y * log |x|)` in `f64` after the
//! special cases of C99 Annex F; `powi` multiplies in `f64`. Results are within 0.51 ulp of the
//! exact value for `exp`, `log` and `pow` (nearly always correctly rounded) and within 1 ulp for
//! `powi` with `|n| <= 2^16`, measured in `tests/dmath_accuracy.rs`.

use super::convert::to_f32;
use super::series::{ATANH, EXP, horner};
use super::trig::ROUNDING_MAGIC;
use std::f64::consts::LOG2_E;

/// `ln 2` in two pieces: `LN2_HI` has 32 significant bits, so `k * LN2_HI` is exact for `|k| < 2^21`.
const LN2_HI: f64 = 0.693_147_180_369_123_8;
const LN2_LO: f64 = 1.908_214_929_270_587_7e-10;
/// `exp` of an argument outside this range is beyond the `f32` range (infinity or zero) and would
/// leave the `f64` exponent range after scaling; the argument is clamped to it.
const EXP_CLAMP_LOW: f64 = -110.0;
const EXP_CLAMP_HIGH: f64 = 100.0;
/// Exponent bias of an `f64` and the width of its mantissa field.
const F64_EXPONENT_BIAS: u32 = 1023;
const F64_MANTISSA_BITS: u32 = 52;
const F64_MANTISSA_MASK: u64 = (1 << F64_MANTISSA_BITS) - 1;
/// `sqrt(2)`: the mantissa of `log` is folded into `[1/sqrt(2), sqrt(2))`.
const SQRT_2: f64 = std::f64::consts::SQRT_2;
/// Above this magnitude every `f32` is an even integer; below `2^24` the odd ones exist.
const ODD_INTEGER_LIMIT: f32 = 16_777_216.0;

/// `2^k` for the integer `k` held in the low 32 bits of the rounded sum `sum` (see
/// [`ROUNDING_MAGIC`]), with `-1022 <= k <= 1023`.
fn two_pow(sum: f64) -> f64 {
    let [b0, b1, b2, b3, ..] = sum.to_bits().to_le_bytes();
    let k = u32::from_le_bytes([b0, b1, b2, b3]);
    f64::from_bits(u64::from(k.wrapping_add(F64_EXPONENT_BIAS)) << F64_MANTISSA_BITS)
}

/// `e^t` in `f64`, for `t` clamped to the range that matters for an `f32` result.
fn exp_f64(t: f64) -> f64 {
    let t = t.clamp(EXP_CLAMP_LOW, EXP_CLAMP_HIGH);
    let sum = t * LOG2_E + ROUNDING_MAGIC;
    let k = sum - ROUNDING_MAGIC;
    let r = (t - k * LN2_HI) - k * LN2_LO;
    horner(&EXP, r) * two_pow(sum)
}

/// Natural logarithm of a positive, finite `x` (an `f32` converted to `f64` is always normal).
fn log_f64(x: f64) -> f64 {
    let bits = x.to_bits();
    let field = u32::try_from(bits >> F64_MANTISSA_BITS).unwrap_or(F64_EXPONENT_BIAS);
    let mut exponent = f64::from(field) - f64::from(F64_EXPONENT_BIAS);
    let mut m = f64::from_bits(
        (bits & F64_MANTISSA_MASK) | (u64::from(F64_EXPONENT_BIAS) << F64_MANTISSA_BITS),
    );
    if m > SQRT_2 {
        m *= 0.5;
        exponent += 1.0;
    }
    let s = (m - 1.0) / (m + 1.0);
    let series = 2.0 * s * horner(&ATANH, s * s);
    exponent * LN2_HI + (exponent * LN2_LO + series)
}

/// `e^x`. Overflows to infinity above about 88.72 and underflows through the subnormals to zero
/// below about -103.97; NaN stays NaN.
#[must_use]
pub fn exp(x: f32) -> f32 {
    if x.is_nan() {
        return f32::NAN;
    }
    to_f32(exp_f64(f64::from(x)))
}

/// Natural logarithm: `-infinity` for zero (either sign), NaN for a negative `x` and for NaN.
#[must_use]
pub fn log(x: f32) -> f32 {
    if x.is_nan() || x < 0.0 {
        f32::NAN
    } else if x == 0.0 {
        f32::NEG_INFINITY
    } else if x.is_infinite() {
        f32::INFINITY
    } else {
        to_f32(log_f64(f64::from(x)))
    }
}

/// Whether `x` is exactly 1.
fn is_one(x: f32) -> bool {
    x.total_cmp(&1.0).is_eq()
}

/// Whether the finite `y` is a whole number.
fn is_integer(y: f32) -> bool {
    y.trunc().total_cmp(&y).is_eq()
}

/// Whether the finite `y` is an odd whole number.
fn is_odd_integer(y: f32) -> bool {
    let half = y * 0.5;
    is_integer(y) && y.abs() < ODD_INTEGER_LIMIT && half.trunc().total_cmp(&half).is_ne()
}

/// `pow` for an infinite `y` (`x` is not NaN and not 1).
fn pow_infinite_exponent(x: f32, y: f32) -> f32 {
    let magnitude = x.abs();
    if is_one(magnitude) {
        1.0
    } else if (magnitude < 1.0) == (y > 0.0) {
        0.0
    } else {
        f32::INFINITY
    }
}

/// `pow` for a zero or infinite `x` and a finite non-zero `y`.
fn pow_zero_or_infinite_base(x: f32, y: f32) -> f32 {
    let negative = x.is_sign_negative() && is_odd_integer(y);
    let magnitude = if (x == 0.0) == (y < 0.0) {
        f32::INFINITY
    } else {
        0.0
    };
    if negative { -magnitude } else { magnitude }
}

/// The cases of C99 Annex F that the general formula does not cover.
fn pow_special(x: f32, y: f32) -> Option<f32> {
    if y == 0.0 || is_one(x) {
        Some(1.0)
    } else if x.is_nan() || y.is_nan() {
        Some(f32::NAN)
    } else if y.is_infinite() {
        Some(pow_infinite_exponent(x, y))
    } else if x == 0.0 || x.is_infinite() {
        Some(pow_zero_or_infinite_base(x, y))
    } else if x < 0.0 && !is_integer(y) {
        Some(f32::NAN)
    } else {
        None
    }
}

/// `x` raised to `y`, with the special cases of C99 Annex F (`pow(x, 0)` and `pow(1, y)` are 1
/// even for NaN; a negative `x` needs a whole `y`, otherwise NaN).
#[must_use]
pub fn pow(x: f32, y: f32) -> f32 {
    if let Some(special) = pow_special(x, y) {
        return special;
    }
    let magnitude = exp_f64(f64::from(y) * log_f64(f64::from(x.abs())));
    to_f32(if x < 0.0 && is_odd_integer(y) {
        -magnitude
    } else {
        magnitude
    })
}

/// `x` raised to the integer `n` by square-and-multiply in `f64` (each multiplication rounds once,
/// then one rounding to `f32` at the end); a negative `n` takes the reciprocal. `powi(x, 0)` is 1 for every `x`, NaN included. Within 1 ulp
/// for `|n| <= 2^16`; the error grows in proportion to `|n|` beyond.
#[must_use]
pub fn powi(x: f32, n: i32) -> f32 {
    let mut base = f64::from(x);
    let mut rest = n.unsigned_abs();
    let mut product = 1.0_f64;
    while rest != 0 {
        if rest & 1 == 1 {
            product *= base;
        }
        base *= base;
        rest >>= 1;
    }
    to_f32(if n < 0 { 1.0 / product } else { product })
}
