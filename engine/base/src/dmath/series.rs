//! Taylor-type series coefficients computed at compile time, and their evaluation.
//!
//! The coefficients come from `+ * /` on `f64` in constant evaluation (soft float, IEEE-754
//! binary64 on every host), so no decimal literal can be mistyped and every platform gets the same
//! bits. Each series is used only on a small reduced argument, where the first few terms give an
//! error far below half an `f32` ulp.

/// `sin(r) = r * sum(SIN[k] * r^(2k))`, terms up to `r^15` (first omitted term `r^17/17!`, under 6e-17 for `|r| <= pi/4`).
pub(super) const SIN: [f64; 8] = factorial_series(1.0, 2.0, true);
/// `cos(r) = sum(COS[k] * r^(2k))`, terms up to `r^14` (first omitted term `r^16/16!`, under 1.1e-15
/// for `|r| <= pi/4`: ten `f64` ulps, a hundred-millionth of an `f32` ulp).
pub(super) const COS: [f64; 8] = factorial_series(0.0, 2.0, true);
/// `exp(r) = sum(EXP[k] * r^k)`, terms up to `r^13` (first omitted term `r^14/14!`, under 5e-18 for `|r| <= 0.35`).
pub(super) const EXP: [f64; 14] = factorial_series(0.0, 1.0, false);
/// `atan(h) = h * sum(ATAN[k] * h^(2k))`, terms up to `h^19` (error under 1e-14 relative for `|h| <= 0.2`).
pub(super) const ATAN: [f64; 10] = reciprocal_series(true);
/// `atanh(s) = s * sum(ATANH[k] * s^(2k))`, terms up to `s^23` (error under 1e-17 for `|s| <= 0.18`).
pub(super) const ATANH: [f64; 12] = reciprocal_series(false);

/// Coefficients `1 / n!` for the powers `n = first, first + step, ...`, with alternating signs when
/// `alternate` is set (the sign of the first term is positive).
const fn factorial_series<const N: usize>(first: f64, step: f64, alternate: bool) -> [f64; N] {
    let mut out = [0.0; N];
    let mut factorial = 1.0;
    let mut power = first;
    let mut factor = 2.0; // the next integer to multiply into the factorial
    let mut sign = 1.0;
    let mut index = 0;
    while index < N {
        while factor <= power {
            factorial *= factor;
            factor += 1.0;
        }
        out[index] = sign / factorial;
        if alternate {
            sign = -sign;
        }
        power += step;
        index += 1;
    }
    out
}

/// Coefficients `1 / (2k + 1)` for `k = 0, 1, ...`, with alternating signs when `alternate` is set.
const fn reciprocal_series<const N: usize>(alternate: bool) -> [f64; N] {
    let mut out = [0.0; N];
    let mut denominator = 1.0;
    let mut sign = 1.0;
    let mut index = 0;
    while index < N {
        out[index] = sign / denominator;
        if alternate {
            sign = -sign;
        }
        denominator += 2.0;
        index += 1;
    }
    out
}

/// `coefficients[0] + t * (coefficients[1] + t * (...))`, evaluated from the last term. The order
/// of the operations is fixed and nothing is fused.
pub(super) fn horner(coefficients: &[f64], t: f64) -> f64 {
    coefficients.iter().rev().fold(0.0, |acc, &c| acc * t + c)
}

#[cfg(test)]
mod tests {
    use super::{ATAN, ATANH, COS, EXP, SIN, horner};

    fn bits(values: &[f64]) -> Vec<u64> {
        values.iter().map(|value| value.to_bits()).collect()
    }

    /// Exact equality by bit pattern (Clippy refuses `==` on floats).
    macro_rules! assert_same {
        ($actual:expr, $expected:expr) => {
            assert_eq!(bits(&$actual), bits(&$expected))
        };
    }

    #[test]
    fn sine_coefficients_are_alternating_inverse_odd_factorials() {
        let expected = [1.0, -1.0 / 6.0, 1.0 / 120.0, -1.0 / 5040.0, 1.0 / 362_880.0];
        assert_same!(SIN[..5], expected);
        assert_same!([SIN[5]], [-1.0 / 39_916_800.0]);
        assert_same!([SIN[6]], [1.0 / 6_227_020_800.0]);
        assert_same!([SIN[7]], [-1.0 / 1_307_674_368_000.0]);
    }

    #[test]
    fn cosine_coefficients_are_alternating_inverse_even_factorials() {
        let expected = [1.0, -0.5, 1.0 / 24.0, -1.0 / 720.0, 1.0 / 40_320.0];
        assert_same!(COS[..5], expected);
        assert_same!([COS[5]], [-1.0 / 3_628_800.0]);
        assert_same!([COS[7]], [-1.0 / 87_178_291_200.0]);
    }

    #[test]
    fn exponential_coefficients_are_inverse_factorials() {
        let expected = [
            1.0,
            1.0,
            0.5,
            1.0 / 6.0,
            1.0 / 24.0,
            1.0 / 120.0,
            1.0 / 720.0,
        ];
        assert_same!(EXP[..7], expected);
        assert_same!([EXP[13]], [1.0 / 6_227_020_800.0]);
    }

    #[test]
    fn arc_series_coefficients_are_reciprocals_of_odd_numbers() {
        assert_same!(ATAN[..4], [1.0, -1.0 / 3.0, 1.0 / 5.0, -1.0 / 7.0]);
        assert_same!([ATAN[9]], [-1.0 / 19.0]);
        assert_same!(ATANH[..4], [1.0, 1.0 / 3.0, 1.0 / 5.0, 1.0 / 7.0]);
        assert_same!([ATANH[11]], [1.0 / 23.0]);
    }

    #[test]
    fn horner_evaluates_a_polynomial_and_the_empty_one() {
        assert_same!([horner(&[1.0, 2.0, 3.0], 2.0)], [17.0]);
        assert_same!([horner(&[5.0], 123.0)], [5.0]);
        assert_same!([horner(&[], 3.0)], [0.0]);
    }
}
