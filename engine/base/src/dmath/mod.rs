//! The deterministic math facade: `sin`, `cos`, `sin_cos`, `tan`, `atan2`, `asin`, `acos`, `exp`,
//! `log`, `pow`, `powi` and `madd` for `f32`, with one signature each and two implementations.
//!
//! * **Feature `cross-platform-determinism` off (the default):** the facade calls the `std` functions
//!   of the platform (libm): fast, the same bits on one machine and build (D0, D1), not the same
//!   bits on Windows and Linux or on x86-64 and `AArch64` (review 3-13: `f32::sin_cos` differs).
//! * **Feature on:** the facade calls the own implementations of [`portable`], which use only
//!   `+ - * /`, comparisons, integer bit operations and `sqrt` (all exact or correctly rounded by
//!   IEEE-754) and no libm, no `mul_add`, no fast-math: the result is bit-identical on every
//!   IEEE-754 platform that evaluates `f32` and `f64` in SSE2/AArch64 style (not x87). This is the
//!   D2 promise for physics (rules-digest section 5, `wiki/architecture/physics.md` section 3).
//!
//! The own functions work in `f64` internally (also strict IEEE, one rounding to `f32` at the end,
//! done in integer arithmetic so that it does not depend on the rounding-mode register) and are
//! within 0.51 `f32` ulp of the exact value, so the accuracy loss against libm is nil. The domain
//! of `sin`, `cos`, `tan` is every finite `f32` (arguments above `2^20` use Payne-Hanek reduction).
//! NaN in gives NaN out everywhere (except where C99 defines a value, as for `pow(x, 0)`);
//! infinities and signed zeros follow C99 Annex F. The measured error of every function is in `reviews/4-8.md`.
//!
//! Game and engine code that must agree between platforms (physics, lockstep or rollback code)
//! calls these functions and never the `std` ones. The physics crate's scan bans the `std` ones.

// Guards. Rust never contracts `a * b + c` into a fused operation by itself and the own functions
// call no `mul_add`, so an FMA-capable target does not change their results. The first guard is a
// conservative policy: the reference results of the D2 job are taken on a build without FMA, and a
// build that enables the feature (for example by `-C target-cpu=native`) is then not the build that
// was compared, so it is refused rather than trusted. The second refuses 32-bit x86 without SSE2,
// where `f32` and `f64` arithmetic runs on the x87 unit with extended intermediates (it is not
// covered by a test: no such target is built here).
#[cfg(all(feature = "cross-platform-determinism", target_feature = "fma"))]
compile_error!(
    "feature `cross-platform-determinism` is on and `target_feature = \"fma\"` is enabled (for example by `-C target-cpu=native`): the build would not be the one that the reference results are taken with; build without `-C target-feature=+fma` / `target-cpu=native`"
);
#[cfg(all(
    feature = "cross-platform-determinism",
    target_arch = "x86",
    not(target_feature = "sse2")
))]
compile_error!(
    "feature `cross-platform-determinism` needs SSE2 floating point on 32-bit x86 (x87 keeps extended intermediates); build with `-C target-feature=+sse2`"
);

mod convert;
mod explog;
mod inverse;
mod large;
mod series;
mod trig;

/// The own implementations, always compiled: the facade uses them with the feature on, and the
/// tests compare them with `std` in both configurations.
pub mod portable {
    pub use super::explog::{exp, log, pow, powi};
    pub use super::inverse::{acos, asin, atan2};
    pub use super::trig::{cos, sin, sin_cos, tan};
}

/// Whether the facade uses the own implementations (the feature `cross-platform-determinism`).
pub const DETERMINISTIC: bool = cfg!(feature = "cross-platform-determinism");

/// Sine of `x` radians.
#[inline]
#[must_use]
pub fn sin(x: f32) -> f32 {
    if DETERMINISTIC {
        portable::sin(x)
    } else {
        x.sin()
    }
}

/// Cosine of `x` radians.
#[inline]
#[must_use]
pub fn cos(x: f32) -> f32 {
    if DETERMINISTIC {
        portable::cos(x)
    } else {
        x.cos()
    }
}

/// Sine and cosine of `x` radians.
#[inline]
#[must_use]
pub fn sin_cos(x: f32) -> (f32, f32) {
    if DETERMINISTIC {
        portable::sin_cos(x)
    } else {
        x.sin_cos()
    }
}

/// Tangent of `x` radians.
#[inline]
#[must_use]
pub fn tan(x: f32) -> f32 {
    if DETERMINISTIC {
        portable::tan(x)
    } else {
        x.tan()
    }
}

/// The angle of the point `(x, y)` from the +X axis in `(-pi, pi]`.
#[inline]
#[must_use]
pub fn atan2(y: f32, x: f32) -> f32 {
    if DETERMINISTIC {
        portable::atan2(y, x)
    } else {
        y.atan2(x)
    }
}

/// Arcsine in `[-pi/2, pi/2]`, NaN for `|x| > 1`.
#[inline]
#[must_use]
pub fn asin(x: f32) -> f32 {
    if DETERMINISTIC {
        portable::asin(x)
    } else {
        x.asin()
    }
}

/// Arccosine in `[0, pi]`, NaN for `|x| > 1`.
#[inline]
#[must_use]
pub fn acos(x: f32) -> f32 {
    if DETERMINISTIC {
        portable::acos(x)
    } else {
        x.acos()
    }
}

/// `e` raised to `x`.
#[inline]
#[must_use]
pub fn exp(x: f32) -> f32 {
    if DETERMINISTIC {
        portable::exp(x)
    } else {
        x.exp()
    }
}

/// Natural logarithm.
#[inline]
#[must_use]
pub fn log(x: f32) -> f32 {
    if DETERMINISTIC {
        portable::log(x)
    } else {
        x.ln()
    }
}

/// `x` raised to `y`.
#[inline]
#[must_use]
pub fn pow(x: f32, y: f32) -> f32 {
    if DETERMINISTIC {
        portable::pow(x, y)
    } else {
        x.powf(y)
    }
}

/// `x` raised to the integer `n`.
#[inline]
#[must_use]
pub fn powi(x: f32, n: i32) -> f32 {
    if DETERMINISTIC {
        portable::powi(x, n)
    } else {
        x.powi(n)
    }
}

/// `a * b + c`. With the feature on: always a rounded product followed by a rounded sum, on every
/// platform. With the feature off: one fused operation when the build targets a CPU with FMA
/// (`target_feature = "fma"`), the same two roundings otherwise, so the result can differ between
/// machines. Rust never fuses `a * b + c` by itself; this is the only place where it is done.
#[inline]
#[must_use]
pub fn madd(a: f32, b: f32, c: f32) -> f32 {
    if !DETERMINISTIC && cfg!(target_feature = "fma") {
        a.mul_add(b, c)
    } else {
        a * b + c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exact equality of two `f32` by bit pattern (Clippy refuses `==` on floats).
    macro_rules! assert_same {
        ($actual:expr, $expected:expr $(, $($message:tt)+)?) => {
            assert_eq!(f32::to_bits($actual), f32::to_bits($expected) $(, $($message)+)?)
        };
    }

    /// Inputs where the platform libm and the own functions are not always the same bits.
    fn samples() -> impl Iterator<Item = f32> {
        (0_u16..6000)
            .map(|i| (f32::from(i) - 3000.0) * 0.013_7 + 0.000_31)
            .chain([0.0, -0.0])
    }

    fn reference(x: f32) -> [f32; 11] {
        let unit = x.rem_euclid(2.0) - 1.0;
        [
            x.sin(),
            x.cos(),
            x.tan(),
            x.atan2(1.5 - x),
            unit.asin(),
            unit.acos(),
            (x * 0.02).exp(),
            x.abs().ln(),
            x.abs().powf(0.37),
            x.powi(5),
            x.sin_cos().0,
        ]
    }

    fn facade(x: f32) -> [f32; 11] {
        let unit = x.rem_euclid(2.0) - 1.0;
        [
            sin(x),
            cos(x),
            tan(x),
            atan2(x, 1.5 - x),
            asin(unit),
            acos(unit),
            exp(x * 0.02),
            log(x.abs()),
            pow(x.abs(), 0.37),
            powi(x, 5),
            sin_cos(x).0,
        ]
    }

    fn own(x: f32) -> [f32; 11] {
        let unit = x.rem_euclid(2.0) - 1.0;
        [
            portable::sin(x),
            portable::cos(x),
            portable::tan(x),
            portable::atan2(x, 1.5 - x),
            portable::asin(unit),
            portable::acos(unit),
            portable::exp(x * 0.02),
            portable::log(x.abs()),
            portable::pow(x.abs(), 0.37),
            portable::powi(x, 5),
            portable::sin_cos(x).0,
        ]
    }

    fn bits(values: [f32; 11]) -> [u32; 11] {
        values.map(f32::to_bits)
    }

    #[test]
    fn the_flag_reports_the_feature() {
        assert_eq!(DETERMINISTIC, cfg!(feature = "cross-platform-determinism"));
    }

    /// The values that the facade must return in the configuration under test.
    fn selected(x: f32) -> [f32; 11] {
        if cfg!(feature = "cross-platform-determinism") {
            own(x)
        } else {
            reference(x)
        }
    }

    /// Whether `a` and `b` are the same number to 1e-5 relative (or both NaN).
    fn close(a: f32, b: f32) -> bool {
        if a.is_finite() && b.is_finite() {
            (a - b).abs() <= 1.0e-5 * b.abs().max(1.0)
        } else {
            a.is_nan() == b.is_nan()
        }
    }

    #[test]
    fn the_facade_is_the_selected_implementation_bit_for_bit() {
        // `reference` is the platform libm; `own` the portable functions.
        for x in samples() {
            assert_eq!(bits(facade(x)), bits(selected(x)), "{x}");
        }
    }

    #[test]
    fn the_own_functions_stay_within_a_hair_of_the_platform_ones() {
        let pairs = samples().flat_map(|x| {
            own(x)
                .into_iter()
                .zip(reference(x))
                .map(move |(a, b)| (x, a, b))
        });
        for (x, a, b) in pairs {
            assert!(close(a, b), "{x}: {a} against {b}");
        }
    }

    #[test]
    fn madd_rounds_the_product_unless_the_build_fuses() {
        let a = 1.0 + 2.0_f32.powi(-12);
        let c = -(1.0 + 2.0_f32.powi(-11));
        let fused = 2.0_f32.powi(-24);
        let result = madd(a, a, c);
        if cfg!(feature = "cross-platform-determinism") || !cfg!(target_feature = "fma") {
            assert_same!(result, 0.0);
        } else {
            assert_same!(result, fused);
        }
        assert_same!(madd(2.0, 3.0, 4.0), 10.0);
        assert!(madd(f32::NAN, 1.0, 1.0).is_nan());
        assert!(madd(f32::INFINITY, 0.0, 1.0).is_nan());
    }
}
