//! `atan2`, `asin` and `acos` for `f32`.
//!
//! The arctangent is computed in `f64`: the argument is folded into `[0, 1]`, halved twice with
//! `a / (1 + sqrt(1 + a^2))` (exact `sqrt`), and summed with the series of [`super::series::ATAN`].
//! `asin` and `acos` are `atan2` of the sine and cosine sides of the unit triangle. Results are
//! within 0.51 ulp of the exact value (nearly always correctly rounded), measured in
//! `tests/dmath_accuracy.rs`. NaN in gives NaN out; the signs of zeros and the infinities follow
//! the C99 rules for `atan2`.

use super::convert::to_f32;
use super::series::{ATAN, horner};
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

/// Each halving maps `a` to `a / (1 + sqrt(1 + a^2))`, which is `tan(atan(a) / 2)`.
const HALVINGS: u32 = 2;
/// The result of the halved argument is multiplied back by `2^HALVINGS`.
const HALVING_FACTOR: f64 = 4.0;

fn halve(a: f64) -> f64 {
    a / (1.0 + (1.0 + a * a).sqrt())
}

/// Arctangent of a finite or infinite, not NaN, `z`.
fn atan_f64(z: f64) -> f64 {
    let a = z.abs();
    let inverted = a > 1.0;
    let folded = if inverted { 1.0 / a } else { a };
    let mut h = folded;
    for _ in 0..HALVINGS {
        h = halve(h);
    }
    let y = HALVING_FACTOR * h * horner(&ATAN, h * h);
    (if inverted { FRAC_PI_2 - y } else { y }).copysign(z)
}

/// `atan2` when `y` or `x` is infinite (neither is NaN).
fn atan2_infinite(y: f64, x: f64) -> f64 {
    if y.is_infinite() && x.is_infinite() {
        let quarter = if x > 0.0 { FRAC_PI_4 } else { PI - FRAC_PI_4 };
        quarter.copysign(y)
    } else if x.is_infinite() {
        if x > 0.0 {
            0.0_f64.copysign(y)
        } else {
            PI.copysign(y)
        }
    } else {
        FRAC_PI_2.copysign(y)
    }
}

/// `atan2` when `x` is a zero (either sign) and `y` is finite.
fn atan2_zero_x(y: f64, x: f64) -> f64 {
    if y != 0.0 {
        FRAC_PI_2.copysign(y)
    } else if x.is_sign_positive() {
        y
    } else {
        PI.copysign(y)
    }
}

/// `atan2` for neither NaN.
fn atan2_f64(y: f64, x: f64) -> f64 {
    if y.is_infinite() || x.is_infinite() {
        atan2_infinite(y, x)
    } else if x == 0.0 {
        atan2_zero_x(y, x)
    } else if x > 0.0 {
        atan_f64(y / x)
    } else {
        PI.copysign(y) + atan_f64(y / x)
    }
}

/// The angle of the point `(x, y)` from the +X axis, in `(-pi, pi]`, counter-clockwise.
#[must_use]
pub fn atan2(y: f32, x: f32) -> f32 {
    if y.is_nan() || x.is_nan() {
        return f32::NAN;
    }
    to_f32(atan2_f64(f64::from(y), f64::from(x)))
}

/// `sqrt(1 - x^2)` for `|x| <= 1`, as the square root of `(1 - x)(1 + x)`: each factor is rounded
/// at most once in `f64` (exact for `|x| >= 2^-29`, and for smaller `x` the result is 1 to well
/// below an `f32` ulp), which keeps the result accurate also close to `|x| = 1`.
fn complement(x: f64) -> f64 {
    ((1.0 - x) * (1.0 + x)).sqrt()
}

/// Arcsine in `[-pi/2, pi/2]`; NaN for `|x| > 1` and for NaN.
#[must_use]
pub fn asin(x: f32) -> f32 {
    if x.is_nan() || x.abs() > 1.0 {
        return f32::NAN;
    }
    let x = f64::from(x);
    to_f32(atan2_f64(x, complement(x)))
}

/// Arccosine in `[0, pi]`; NaN for `|x| > 1` and for NaN.
#[must_use]
pub fn acos(x: f32) -> f32 {
    if x.is_nan() || x.abs() > 1.0 {
        return f32::NAN;
    }
    let x = f64::from(x);
    to_f32(atan2_f64(complement(x), x))
}
