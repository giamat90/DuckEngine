//! `sin`, `cos`, `sin_cos` and `tan` for `f32`.
//!
//! Range reduction by Cody-Waite in `f64`, then the Taylor kernels of [`super::series`] on
//! `|r| <= pi/4`, then one rounding to `f32` ([`super::convert::to_f32`]). Arguments above `2^20`
//! are reduced exactly by Payne-Hanek ([`super::large`]), so every finite `x` has a result; NaN and
//! the infinities give NaN. The result is within 0.51 ulp of the exact value (nearly always
//! correctly rounded), measured in `tests/dmath_accuracy.rs`.

use super::convert::to_f32;
use super::large;
use super::series::{COS, SIN, horner};
use std::f64::consts::FRAC_2_PI;

/// Largest `|x|` reduced by Cody-Waite: keeps `n` under 2^20, where `n * PIO2_HI` below is exact.
const CODY_WAITE_LIMIT: f32 = 1_048_576.0;
/// `1.5 * 2^52`: adding it rounds a smaller `f64` to an integer (ties to even) without a library
/// call, and leaves that integer in the low bits of the sum.
pub(super) const ROUNDING_MAGIC: f64 = 6_755_399_441_055_744.0;
/// `pi/2` in three pieces of 33 bits each (`PIO2_HI + PIO2_MID + PIO2_LO` plus the tail
/// `PIO2_TAIL`, about 1e-31 off): `n * piece` is exact for `|n| < 2^20`.
const PIO2_HI: f64 = 1.570_796_326_734_125_6;
const PIO2_MID: f64 = 6.077_100_506_303_966e-11;
const PIO2_LO: f64 = 2.022_266_248_711_166_5e-21;
const PIO2_TAIL: f64 = 8.478_427_660_368_9e-32;
/// Mask of the quadrant (the integer `n` modulo 4) in the low bits of the rounded sum.
const QUADRANT_MASK: u64 = 3;

/// `x = r + n * pi/2` with `|r| <= pi/4`, and the quadrant `n mod 4`.
#[derive(Clone, Copy)]
struct Reduced {
    r: f64,
    quadrant: u64,
}

/// The reduction, or `None` when `x` is NaN or infinite.
fn reduce(x: f32) -> Option<Reduced> {
    if !x.is_finite() {
        return None;
    }
    if x.abs() > CODY_WAITE_LIMIT {
        return Some(reduce_large(x));
    }
    let x = f64::from(x);
    let sum = x * FRAC_2_PI + ROUNDING_MAGIC;
    let n = sum - ROUNDING_MAGIC;
    let quadrant = sum.to_bits() & QUADRANT_MASK;
    // Near zero n is 0 and `r = x` keeps the sign of a zero.
    let r = if n == 0.0 {
        x
    } else {
        (((x - n * PIO2_HI) - n * PIO2_MID) - n * PIO2_LO) - n * PIO2_TAIL
    };
    Some(Reduced { r, quadrant })
}

/// The reduction of a finite `x` above the Cody-Waite limit: `-x = -r + (-n) * pi/2`.
fn reduce_large(x: f32) -> Reduced {
    let (r, quadrant) = large::reduce(x.abs().to_bits());
    if x.is_sign_negative() {
        Reduced {
            r: -r,
            quadrant: quadrant.wrapping_neg() & QUADRANT_MASK,
        }
    } else {
        Reduced { r, quadrant }
    }
}

fn sin_kernel(r: f64) -> f64 {
    r * horner(&SIN, r * r)
}

fn cos_kernel(r: f64) -> f64 {
    horner(&COS, r * r)
}

fn sin_of(red: Reduced) -> f64 {
    match red.quadrant {
        0 => sin_kernel(red.r),
        1 => cos_kernel(red.r),
        2 => -sin_kernel(red.r),
        _ => -cos_kernel(red.r),
    }
}

fn cos_of(red: Reduced) -> f64 {
    match red.quadrant {
        0 => cos_kernel(red.r),
        1 => -sin_kernel(red.r),
        2 => -cos_kernel(red.r),
        _ => sin_kernel(red.r),
    }
}

/// Sine of `x` radians. NaN for NaN and infinities; `sin(-0.0)` is `-0.0`.
#[must_use]
pub fn sin(x: f32) -> f32 {
    reduce(x).map_or(f32::NAN, |red| to_f32(sin_of(red)))
}

/// Cosine of `x` radians. NaN for NaN and infinities.
#[must_use]
pub fn cos(x: f32) -> f32 {
    reduce(x).map_or(f32::NAN, |red| to_f32(cos_of(red)))
}

/// Sine and cosine of `x` radians, bit for bit the pair `(sin(x), cos(x))`, with one reduction.
#[must_use]
pub fn sin_cos(x: f32) -> (f32, f32) {
    reduce(x).map_or((f32::NAN, f32::NAN), |red| {
        (to_f32(sin_of(red)), to_f32(cos_of(red)))
    })
}

/// Tangent of `x` radians. NaN for NaN and infinities.
#[must_use]
pub fn tan(x: f32) -> f32 {
    reduce(x).map_or(f32::NAN, |red| {
        let (s, c) = (sin_kernel(red.r), cos_kernel(red.r));
        to_f32(if red.quadrant & 1 == 0 { s / c } else { -c / s })
    })
}
