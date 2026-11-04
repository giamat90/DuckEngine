//! Accuracy of the own implementations of `base::dmath::portable` against the `f64` functions of
//! `std` (the oracle: about 1e-16 relative, so a hundred million times finer than an `f32` ulp),
//! measured in `f32` ulps over dense sweeps, and the special values.
//!
//! The documented bounds are the `BOUND_*` constants. The measured maxima are printed (run with
//! `--nocapture`) and recorded in `reviews/4-8.md`.

use base::dmath::portable as own;
use std::f32::consts::{FRAC_PI_2, PI};

/// Exact equality of two `f32` by bit pattern (Clippy refuses `==` on floats).
macro_rules! assert_same {
    ($actual:expr, $expected:expr $(, $($message:tt)+)?) => {
        assert_eq!(f32::to_bits($actual), f32::to_bits($expected) $(, $($message)+)?)
    };
}

const BOUND_SIN_COS: f64 = 0.51;
const BOUND_TAN: f64 = 0.51;
const BOUND_ATAN2: f64 = 0.51;
const BOUND_ASIN_ACOS: f64 = 0.51;
const BOUND_EXP: f64 = 0.51;
const BOUND_LOG: f64 = 0.51;
const BOUND_POW: f64 = 0.51;
const BOUND_POWI: f64 = 1.0;
/// Where the reduction of `sin`, `cos` and `tan` changes method (Cody-Waite below, Payne-Hanek above).
const TRIG_SPLIT: f32 = 1_048_576.0;

/// The spacing of `f32` numbers around `y`.
fn spacing(y: f64) -> f64 {
    const SMALLEST_NORMAL_EXPONENT_FIELD: u64 = 1023 - 126;
    const MANTISSA_BITS_DIFFERENCE: u64 = 23;
    let field = (y.abs().to_bits() >> 52) & 0x7ff;
    let field = field.max(SMALLEST_NORMAL_EXPONENT_FIELD) - MANTISSA_BITS_DIFFERENCE;
    f64::from_bits(field << 52)
}

/// The error of `got` against the exact value `exact`, in `f32` ulps.
fn ulps(got: f32, exact: f64) -> f64 {
    const OVERFLOW_EDGE: f64 = 3.402_823_567_797_336_6e38; // the largest f32 plus half an ulp
    if exact.is_nan() {
        assert!(got.is_nan(), "expected NaN, got {got}");
        return 0.0;
    }
    assert!(
        !got.is_nan(),
        "unexpected NaN for the exact value {exact:e}"
    );
    if got.is_infinite() {
        let right = exact.abs() >= OVERFLOW_EDGE && (exact > 0.0) == (got > 0.0);
        return if right { 0.0 } else { f64::INFINITY };
    }
    (f64::from(got) - exact).abs() / spacing(exact)
}

/// Evenly spaced values from `lo` to `hi`.
fn linear(lo: f32, hi: f32, count: u16) -> impl Iterator<Item = f32> {
    let step = (hi - lo) / f32::from(count);
    (0..=count).map(move |i| lo + step * f32::from(i))
}

/// Values whose bit patterns are evenly spaced between those of `lo` and `hi` (both positive):
/// evenly spread over the exponent range.
fn by_bits(lo: f32, hi: f32, count: u32) -> impl Iterator<Item = f32> {
    let (a, b) = (lo.to_bits(), hi.to_bits());
    let step = ((b - a) / count).max(1);
    (a..=b).step_by(step as usize).map(f32::from_bits)
}

/// Both signs of every value.
fn both_signs(values: impl Iterator<Item = f32>) -> impl Iterator<Item = f32> {
    values.flat_map(|v| [v, -v])
}

/// The largest error of `f` against `oracle` over `inputs`, with the input that caused it.
fn worst(
    inputs: impl Iterator<Item = f32>,
    f: impl Fn(f32) -> f32,
    oracle: impl Fn(f64) -> f64,
) -> (f64, f32) {
    let mut worst = (0.0, 0.0);
    for x in inputs {
        let e = ulps(f(x), oracle(f64::from(x)));
        if e > worst.0 {
            worst = (e, x);
        }
    }
    worst
}

/// The largest error over `inputs` of any shape, with the input that caused it.
fn worst_of<T: Copy + std::fmt::Debug>(
    inputs: impl Iterator<Item = T>,
    error: impl Fn(T) -> f64,
) -> (f64, Option<T>) {
    let mut worst = (0.0, None);
    for input in inputs {
        let e = error(input);
        if e > worst.0 {
            worst = (e, Some(input));
        }
    }
    worst
}

fn report(name: &str, bound: f64, worst: &(f64, Option<impl std::fmt::Debug>)) {
    println!("dmath ulp {name}: max {:.4} at {:?}", worst.0, worst.1);
    assert!(worst.0 <= bound, "{name}: {worst:?} exceeds {bound}");
}

fn check(name: &str, bound: f64, measured: (f64, f32)) {
    println!(
        "dmath ulp {name}: max {:.4} at {:e}",
        measured.0, measured.1
    );
    assert!(
        measured.0 <= bound,
        "{name}: {} ulp at {:e} exceeds {bound}",
        measured.0,
        measured.1
    );
}

fn trig_inputs() -> impl Iterator<Item = f32> {
    let tiny_to_large = both_signs(by_bits(1.0e-40, TRIG_SPLIT, 150_000));
    let huge = both_signs(by_bits(TRIG_SPLIT, f32::MAX, 120_000));
    let around_split = (0_u32..=256).flat_map(|d| {
        let bits = TRIG_SPLIT.to_bits();
        [
            f32::from_bits(bits + d),
            f32::from_bits(bits - d),
            -f32::from_bits(bits + d),
            -f32::from_bits(bits - d),
        ]
    });
    let around_pi = linear(-4.0 * PI, 4.0 * PI, 60_000);
    let near_multiples = (0_u16..3000).flat_map(|k| {
        let centre = FRAC_PI_2 * f32::from(k);
        (0_u32..=64).flat_map(move |d| {
            let bits = centre.to_bits();
            [
                f32::from_bits(bits + d),
                f32::from_bits(bits.saturating_sub(d)),
                -f32::from_bits(bits + d),
            ]
        })
    });
    tiny_to_large
        .chain(huge)
        .chain(around_split)
        .chain(around_pi)
        .chain(near_multiples)
}

#[test]
fn sin_and_cos_match_the_oracle() {
    check(
        "sin",
        BOUND_SIN_COS,
        worst(trig_inputs(), own::sin, f64::sin),
    );
    check(
        "cos",
        BOUND_SIN_COS,
        worst(trig_inputs(), own::cos, f64::cos),
    );
}

#[test]
fn sin_cos_returns_the_same_bits_as_sin_and_cos() {
    for x in trig_inputs().step_by(7) {
        let (s, c) = own::sin_cos(x);
        assert_eq!(s.to_bits(), own::sin(x).to_bits(), "{x:e}");
        assert_eq!(c.to_bits(), own::cos(x).to_bits(), "{x:e}");
    }
}

#[test]
fn sin_is_odd_and_cos_is_even_bit_for_bit() {
    for x in trig_inputs().step_by(5) {
        assert_eq!(own::sin(-x).to_bits(), (-own::sin(x)).to_bits(), "{x:e}");
        assert_eq!(own::cos(-x).to_bits(), own::cos(x).to_bits(), "{x:e}");
    }
}

#[test]
fn tan_matches_the_oracle() {
    check("tan", BOUND_TAN, worst(trig_inputs(), own::tan, f64::tan));
}

#[test]
fn trig_special_values() {
    assert_eq!(own::sin(0.0).to_bits(), 0.0_f32.to_bits());
    assert_eq!(own::sin(-0.0).to_bits(), (-0.0_f32).to_bits());
    assert_eq!(own::tan(-0.0).to_bits(), (-0.0_f32).to_bits());
    assert_same!(own::cos(0.0), 1.0);
    assert_same!(own::cos(-0.0), 1.0);
    let (s, c) = own::sin_cos(-0.0);
    assert_eq!((s.to_bits(), c), ((-0.0_f32).to_bits(), 1.0));
    let tiny = f32::from_bits(1);
    assert_same!(own::sin(tiny), tiny);
    assert_same!(own::tan(-tiny), -tiny);
    assert_same!(own::cos(tiny), 1.0);
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(
            own::sin(bad).is_nan() && own::cos(bad).is_nan() && own::tan(bad).is_nan(),
            "{bad}"
        );
        let (s, c) = own::sin_cos(bad);
        assert!(s.is_nan() && c.is_nan());
    }
    // Every finite argument has a finite result, however large.
    for big in [TRIG_SPLIT, 2.0 * TRIG_SPLIT, 1.0e10, 1.0e30, f32::MAX] {
        let (s, c) = own::sin_cos(big);
        assert!(s.is_finite() && c.is_finite() && own::tan(big).is_finite());
        assert_same!(own::sin(-big), -s);
        assert_same!(own::cos(-big), c);
    }
}

#[test]
fn trig_values_at_the_quadrant_points() {
    assert_same!(own::sin(FRAC_PI_2), 1.0);
    assert_same!(own::cos(PI), -1.0);
    assert_same!(own::sin(-FRAC_PI_2), -1.0);
    assert!(own::cos(FRAC_PI_2).abs() < 1.0e-7);
    assert!(own::sin(PI).abs() < 1.0e-6);
    assert!(own::tan(FRAC_PI_2) < -1.0e7);
}

fn pair_inputs() -> impl Iterator<Item = (f32, f32)> {
    let magnitudes: Vec<f32> = by_bits(1.0e-30, 1.0e30, 60).collect();
    let signs = [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)];
    let wide = magnitudes.clone().into_iter().flat_map(move |y| {
        let magnitudes = magnitudes.clone();
        magnitudes
            .into_iter()
            .flat_map(move |x| signs.map(|(sy, sx)| (sy * y, sx * x)))
    });
    wide.chain(linear(-3.0, 3.0, 300).flat_map(|y| linear(-3.0, 3.0, 300).map(move |x| (y, x))))
}

#[test]
fn atan2_matches_the_oracle() {
    let worst = worst_of(pair_inputs(), |(y, x)| {
        ulps(own::atan2(y, x), f64::from(y).atan2(f64::from(x)))
    });
    report("atan2", BOUND_ATAN2, &worst);
}

#[test]
fn atan2_special_values() {
    let pi = PI;
    let cases: [(f32, f32, f32); 24] = [
        (0.0, 1.0, 0.0),
        (-0.0, 1.0, -0.0),
        (0.0, -1.0, pi),
        (-0.0, -1.0, -pi),
        (0.0, 0.0, 0.0),
        (-0.0, 0.0, -0.0),
        (0.0, -0.0, pi),
        (-0.0, -0.0, -pi),
        (1.0, 0.0, FRAC_PI_2),
        (-1.0, -0.0, -FRAC_PI_2),
        (f32::INFINITY, 1.0, FRAC_PI_2),
        (f32::NEG_INFINITY, -1.0, -FRAC_PI_2),
        (1.0, f32::INFINITY, 0.0),
        (-1.0, f32::INFINITY, -0.0),
        (1.0, f32::NEG_INFINITY, pi),
        (-1.0, f32::NEG_INFINITY, -pi),
        (f32::INFINITY, f32::INFINITY, std::f32::consts::FRAC_PI_4),
        (
            f32::INFINITY,
            f32::NEG_INFINITY,
            3.0 * std::f32::consts::FRAC_PI_4,
        ),
        (
            f32::NEG_INFINITY,
            f32::INFINITY,
            -std::f32::consts::FRAC_PI_4,
        ),
        (
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
            -3.0 * std::f32::consts::FRAC_PI_4,
        ),
        (f32::MAX, f32::MIN_POSITIVE, FRAC_PI_2),
        (f32::from_bits(1), f32::MAX, 0.0),
        (f32::MAX, f32::MAX, std::f32::consts::FRAC_PI_4),
        (1.0, 1.0, std::f32::consts::FRAC_PI_4),
    ];
    for (y, x, expected) in cases {
        assert_eq!(
            own::atan2(y, x).to_bits(),
            expected.to_bits(),
            "atan2({y}, {x})"
        );
    }
    for (y, x) in [
        (f32::NAN, 1.0),
        (1.0, f32::NAN),
        (f32::NAN, f32::NAN),
        (f32::NAN, 0.0),
        (f32::INFINITY, f32::NAN),
    ] {
        assert!(own::atan2(y, x).is_nan(), "atan2({y}, {x})");
    }
}

fn unit_inputs() -> impl Iterator<Item = f32> {
    let near_zero = both_signs(by_bits(1.0e-40, 0.5, 40_000));
    let near_one = both_signs(by_bits(0.5, 1.0, 40_000));
    near_zero.chain(near_one).chain(linear(-1.0, 1.0, 40_000))
}

#[test]
fn asin_and_acos_match_the_oracle() {
    check(
        "asin",
        BOUND_ASIN_ACOS,
        worst(unit_inputs(), own::asin, f64::asin),
    );
    check(
        "acos",
        BOUND_ASIN_ACOS,
        worst(unit_inputs(), own::acos, f64::acos),
    );
}

#[test]
fn asin_and_acos_special_values() {
    assert_same!(own::asin(1.0), FRAC_PI_2);
    assert_same!(own::asin(-1.0), -FRAC_PI_2);
    assert_eq!(own::asin(0.0).to_bits(), 0.0_f32.to_bits());
    assert_eq!(own::asin(-0.0).to_bits(), (-0.0_f32).to_bits());
    assert_eq!(own::acos(1.0).to_bits(), 0.0_f32.to_bits());
    assert_same!(own::acos(-1.0), PI);
    assert_same!(own::acos(0.0), FRAC_PI_2);
    assert_same!(own::acos(-0.0), FRAC_PI_2);
    for bad in [
        1.000_000_1,
        -1.000_000_1,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        f32::MAX,
    ] {
        assert!(own::asin(bad).is_nan() && own::acos(bad).is_nan(), "{bad}");
    }
}

#[test]
fn exp_matches_the_oracle() {
    let normal_results = linear(-87.0, 88.7, 60_000);
    let subnormal_results = linear(-104.0, -87.0, 20_000);
    let small = both_signs(by_bits(1.0e-30, 1.0, 40_000));
    let inputs = normal_results.chain(subnormal_results).chain(small);
    check("exp", BOUND_EXP, worst(inputs, own::exp, f64::exp));
}

#[test]
fn exp_special_values() {
    assert_same!(own::exp(0.0), 1.0);
    assert_same!(own::exp(-0.0), 1.0);
    assert_same!(own::exp(f32::NEG_INFINITY), 0.0);
    assert_same!(own::exp(f32::INFINITY), f32::INFINITY);
    assert!(own::exp(f32::NAN).is_nan());
    assert_same!(own::exp(89.0), f32::INFINITY);
    assert_same!(own::exp(f32::MAX), f32::INFINITY);
    assert_same!(own::exp(-104.0), 0.0);
    assert_same!(own::exp(f32::MIN), 0.0);
    assert!(own::exp(88.722_83).is_finite());
    assert_same!(own::exp(88.722_85), f32::INFINITY);
    assert!(own::exp(-103.0) > 0.0);
    assert_same!(own::exp(1.0), std::f32::consts::E);
}

#[test]
fn log_matches_the_oracle() {
    let all_positive = by_bits(f32::from_bits(1), f32::MAX, 200_000);
    let near_one = linear(0.5, 2.0, 60_000);
    let around_one = by_bits(0.99, 1.01, 20_000);
    check(
        "log",
        BOUND_LOG,
        worst(
            all_positive.chain(near_one).chain(around_one),
            own::log,
            f64::ln,
        ),
    );
}

#[test]
fn log_special_values() {
    assert_eq!(own::log(1.0).to_bits(), 0.0_f32.to_bits());
    assert_same!(own::log(0.0), f32::NEG_INFINITY);
    assert_same!(own::log(-0.0), f32::NEG_INFINITY);
    assert_same!(own::log(f32::INFINITY), f32::INFINITY);
    assert!(
        own::log(-1.0).is_nan()
            && own::log(f32::NEG_INFINITY).is_nan()
            && own::log(f32::NAN).is_nan()
    );
    assert!(own::log(-f32::from_bits(1)).is_nan());
    // The f32 nearest e is 4.6e-8 below e, so its logarithm rounds to the f32 below 1.
    assert_same!(own::log(std::f32::consts::E), 0.999_999_94);
    assert!(own::log(f32::from_bits(1)) < -103.0);
}

#[test]
fn pow_matches_the_oracle() {
    let bases: Vec<f32> = by_bits(1.0e-6, 1.0e6, 400).collect();
    let exponents: Vec<f32> = linear(-6.0, 6.0, 120).collect();
    let positive = bases
        .clone()
        .into_iter()
        .flat_map(|x| exponents.clone().into_iter().map(move |y| (x, y)));
    // Negative bases with whole exponents.
    let negative = bases[..200]
        .iter()
        .copied()
        .flat_map(|x| (-9_i8..=9).map(move |y| (-x, f32::from(y))));
    let worst = worst_of(positive.chain(negative), |(x, y)| {
        ulps(own::pow(x, y), f64::from(x).powf(f64::from(y)))
    });
    report("pow", BOUND_POW, &worst);
}

/// The error of `pow(x, y)` against the `f64` oracle.
fn pow_error((x, y): (f32, f32)) -> f64 {
    ulps(own::pow(x, y), f64::from(x).powf(f64::from(y)))
}

/// Exact ties (a result halfway between two `f32`) are decided by the noise of the `f64`
/// evaluation, so the first neighbour may be chosen instead of the even one; the error is then
/// 0.5 ulp plus the noise, which the bound of 0.51 covers.
#[test]
fn pow_matches_the_oracle_near_overflow_underflow_and_one() {
    // Results near the overflow edge and near the underflow edge, including the subnormals.
    let edges = [
        (2.0_f32, 127.9_f32),
        (2.0, 128.0),
        (10.0, 38.5),
        (10.0, 38.53),
        (2.0, -126.5),
        (2.0, -149.0),
        (2.0, -149.5),
        (10.0, -44.9),
        (1.5, 216.0),
        (1.5, -300.0),
    ];
    let near_edges = edges
        .into_iter()
        .flat_map(|(x, y)| (0_u32..200).map(move |d| (x, f32::from_bits(y.to_bits() + d))));
    // A wide range of bases with small exponents.
    let wide =
        by_bits(1.0e-38, 3.0e38, 3_000).flat_map(|x| linear(-3.0, 3.0, 24).map(move |y| (x, y)));
    // Bases close to one with large exponents, where the logarithm is tiny and the product large.
    let near_one = (1_u32..400).flat_map(|d| {
        [
            1.0_f32 + f32::from_bits(0x3400_0000 + d * 997),
            1.0 - f32::from_bits(0x3380_0000 + d * 331),
        ]
        .into_iter()
        .flat_map(|x| [-5000.0_f32, -300.0, 77.7, 1.0e4, 2.5e5, 1.0e7].map(move |y| (x, y)))
    });
    let worst = worst_of(near_edges.chain(wide).chain(near_one), pow_error);
    report("pow (edges, wide, near one)", BOUND_POW, &worst);
}

#[test]
fn pow_exact_cases() {
    assert_same!(own::pow(2.0, 10.0), 1024.0);
    assert_same!(own::pow(2.0, -3.0), 0.125);
    assert_same!(own::pow(9.0, 0.5), 3.0);
    assert_same!(own::pow(-2.0, 3.0), -8.0);
    assert_same!(own::pow(-2.0, 4.0), 16.0);
    assert_same!(own::pow(10.0, 2.0), 100.0);
    assert_same!(own::pow(0.5, 1.0), 0.5);
}

#[test]
fn pow_special_values_follow_c99() {
    let nan = f32::NAN;
    let inf = f32::INFINITY;
    let cases: [(f32, f32, f32); 40] = [
        (nan, 0.0, 1.0),
        (nan, -0.0, 1.0),
        (1.0, nan, 1.0),
        (1.0, inf, 1.0),
        (-1.0, inf, 1.0),
        (-1.0, f32::NEG_INFINITY, 1.0),
        (0.0, 3.0, 0.0),
        (-0.0, 3.0, -0.0),
        (-0.0, 2.0, 0.0),
        (-0.0, 0.5, 0.0),
        (0.0, -3.0, inf),
        (-0.0, -3.0, f32::NEG_INFINITY),
        (-0.0, -2.0, inf),
        (0.0, -0.5, inf),
        (0.0, inf, 0.0),
        (0.0, f32::NEG_INFINITY, inf),
        (2.0, inf, inf),
        (0.5, inf, 0.0),
        (2.0, f32::NEG_INFINITY, 0.0),
        (0.5, f32::NEG_INFINITY, inf),
        (-2.0, inf, inf),
        (-0.5, inf, 0.0),
        (inf, 2.0, inf),
        (inf, -2.0, 0.0),
        (inf, 0.0, 1.0),
        (f32::NEG_INFINITY, 3.0, f32::NEG_INFINITY),
        (f32::NEG_INFINITY, 2.0, inf),
        (f32::NEG_INFINITY, -3.0, -0.0),
        (f32::NEG_INFINITY, -2.0, 0.0),
        (f32::NEG_INFINITY, 0.5, inf),
        (f32::NEG_INFINITY, -0.5, 0.0),
        (-8.0, 16_777_216.0, inf), // beyond 2^24 every f32 is an even integer
        (-0.5, 1.0e30, 0.0),
        (-1.0, 1.0e30, 1.0),
        (-1.0, 3.0, -1.0),
        (-1.0, 2.0, 1.0),
        (1.0e30, 2.0, inf),
        (1.0e-30, 2.0, 0.0),
        (1.0e-30, -2.0, inf),
        (2.0, 0.0, 1.0),
    ];
    for (x, y, expected) in cases {
        let got = own::pow(x, y);
        assert_eq!(got.to_bits(), expected.to_bits(), "pow({x}, {y})");
    }
    for (x, y) in [
        (-2.0, 0.5),
        (-2.0, 1.5),
        (-0.1, -2.5),
        (nan, 1.0),
        (2.0, nan),
        (nan, nan),
        (f32::NEG_INFINITY, nan),
        (-8.0, nan),
    ] {
        assert!(own::pow(x, y).is_nan(), "pow({x}, {y})");
    }
}

#[test]
fn powi_matches_the_oracle() {
    let bases: Vec<f32> = both_signs(by_bits(1.0e-6, 1.0e6, 2_000)).collect();
    let exponents = (-70_i32..=70).chain([100, 1000, -1000, 65_536, -65_536]);
    let pairs = bases
        .into_iter()
        .flat_map(|x| exponents.clone().map(move |n| (x, n)));
    let worst = worst_of(pairs, |(x, n)| {
        ulps(own::powi(x, n), f64::from(x).powf(f64::from(n)))
    });
    report("powi", BOUND_POWI, &worst);
}

#[test]
fn powi_special_values() {
    assert_same!(own::powi(f32::NAN, 0), 1.0);
    assert_same!(own::powi(0.0, 0), 1.0);
    assert!(own::powi(f32::NAN, 2).is_nan());
    assert_same!(own::powi(2.0, 10), 1024.0);
    assert_same!(own::powi(2.0, -2), 0.25);
    assert_same!(own::powi(-2.0, 3), -8.0);
    assert_same!(own::powi(0.0, -1), f32::INFINITY);
    assert_same!(own::powi(-0.0, -1), f32::NEG_INFINITY);
    assert_eq!(own::powi(-0.0, 3).to_bits(), (-0.0_f32).to_bits());
    assert_same!(own::powi(f32::INFINITY, -1), 0.0);
    assert_same!(own::powi(1.0, i32::MIN), 1.0);
    assert_same!(own::powi(-1.0, i32::MAX), -1.0);
    assert_same!(own::powi(2.0, i32::MAX), f32::INFINITY);
    assert_same!(own::powi(2.0, i32::MIN), 0.0);
    assert_same!(own::powi(1.0e30, 3), f32::INFINITY);
    assert_same!(own::powi(1.0e-30, -3), f32::INFINITY);
    assert_same!(own::powi(1.0e30, -3), 0.0);
}
