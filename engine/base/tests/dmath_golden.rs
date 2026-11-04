//! Pinned bit patterns of the facade functions for a fixed set of inputs.
//!
//! These are test constants, not hash files: they were produced by the implementation and checked
//! against the `f64` functions of `std` (the `oracle` test below, within 0.51 ulp). Because the
//! own functions use only IEEE-exact operations, the same bits must come out on every platform:
//! a difference between Windows and Linux (or x86-64 and `AArch64`) shows up here as a failing test.
//! With the feature `cross-platform-determinism` on, the facade itself is held to the same values.

use base::dmath::{self, portable as own};

/// `[x, sin, cos, tan, exp(x * 0.1), log|x|, asin(clamp(x) * 0.9), acos(clamp(x) * 0.9)]` as bit patterns.
const UNARY: [[u32; 8]; 14] = [
    [
        0x3dcc_cccd,
        0x3dcc_7577,
        0x3f7e_b898,
        0x3dcd_7c44,
        0x3f81_4953,
        0xc013_5d8e,
        0x3db8_91da,
        0x3fbd_86bd,
    ],
    [
        0x3f00_0000,
        0x3ef5_7744,
        0x3f60_a940,
        0x3f0b_da7b,
        0x3f86_900d,
        0xbf31_7218,
        0x3eee_fbdd,
        0x3f8d_50e3,
    ],
    [
        0x3f80_0000,
        0x3f57_6aa4,
        0x3f0a_5140,
        0x3fc7_5923,
        0x3f8d_763e,
        0x0000_0000,
        0x3f8f_549b,
        0x3ee6_ecfe,
    ],
    [
        0x4000_0000,
        0x3f68_c7b7,
        0xbed5_1133,
        0xc00b_d7b1,
        0x3f9c_56ed,
        0x3f31_7218,
        0x3f8f_549b,
        0x3ee6_ecfe,
    ],
    [
        0x4040_0000,
        0x3e10_81c3,
        0xbf7d_7026,
        0xbe11_f7b9,
        0x3fac_c82d,
        0x3f8c_9f54,
        0x3f8f_549b,
        0x3ee6_ecfe,
    ],
    [
        0x4120_0000,
        0xbf0b_44f8,
        0xbf56_cd64,
        0x3f25_fafa,
        0x402d_f854,
        0x4013_5d8e,
        0x3f8f_549b,
        0x3ee6_ecfe,
    ],
    [
        0x42c8_0000,
        0xbf01_a12e,
        0x3f5c_c0ee,
        0xbf16_53a7,
        0x46ac_14ee,
        0x4093_5d8e,
        0x3f8f_549b,
        0x3ee6_ecfe,
    ],
    [
        0x4640_e6b6,
        0xbf34_4b08,
        0x3f35_be20,
        0xbf7d_f549,
        0x7f80_0000,
        0x4116_bcab,
        0x3f8f_549b,
        0x3ee6_ecfe,
    ],
    [
        0xbf33_3333,
        0xbf24_eb73,
        0x3f43_ccb3,
        0xbf57_a036,
        0x3f6e_b15d,
        0xbeb6_9e1a,
        0xbf2e_7a45,
        0x4010_267f,
    ],
    [
        0xc053_3333,
        0x3e21_8813,
        0xbf7c_cb7a,
        0xbe23_9460,
        0x3f38_0b63,
        0x3f98_d273,
        0xbf8f_549b,
        0x402c_323b,
    ],
    [
        0x3727_c5ac,
        0x3727_c5ac,
        0x3f80_0000,
        0x3727_c5ac,
        0x3f80_0008,
        0xc138_34f1,
        0x3716_feb4,
        0x3fc9_0f8f,
    ],
    [
        0x3f7f_be77,
        0x3f57_4735,
        0x3f0a_8861,
        0x3fc6_e911,
        0x3f8d_729f,
        0xba83_22ca,
        0x3f8f_1118,
        0x3ee7_fb0a,
    ],
    [
        0x3fc9_0fdb,
        0x3f80_0000,
        0xb33b_bd2e,
        0xcbae_8a4a,
        0x3f95_c578,
        0x3ee7_35da,
        0x3f8f_549b,
        0x3ee6_ecfe,
    ],
    [
        0x40c9_0fdb,
        0x343b_bd2e,
        0x3f80_0000,
        0x343b_bd2e,
        0x3fef_ee2e,
        0x3feb_3f8e,
        0x3f8f_549b,
        0x3ee6_ecfe,
    ],
];

/// `[y, x, atan2(y, x), pow(|y|, x)]` as bit patterns.
const PAIRS: [[u32; 4]; 8] = [
    [0x3f80_0000, 0x3f80_0000, 0x3f49_0fdb, 0x3f80_0000],
    [0x3f80_0000, 0xbf80_0000, 0x4016_cbe4, 0x3f80_0000],
    [0xc020_0000, 0x3e99_999a, 0xbfb9_c668, 0x3fa8_7f36],
    [0x3e99_999a, 0xc020_0000, 0x4041_6b21, 0x41a2_49c4],
    [0x42c8_0000, 0x3c23_d70a, 0x3fc9_0c94, 0x3f86_084f],
    [0xbc23_d70a, 0xc2c8_0000, 0xc049_0e37, 0x7f80_0000],
    [0x4040_0000, 0x4080_0000, 0x3f24_bc7d, 0x42a2_0000],
    [0x3a83_126f, 0x40e0_0000, 0x3915_cbed, 0x1c97_1da4],
];

/// `(x, n, powi(x, n))` with `x` and the result as bit patterns.
const POWIS: [(u32, i32, u32); 5] = [
    (0x3f8c_cccd, 10, 0x4025_ffe3),
    (0x3f66_6666, -7, 0x4005_cee1),
    (0xc000_0000, 5, 0xc200_0000),
    (0x3f80_0347, 1000, 0x3f8d_76a9),
    (0x4060_0000, 3, 0x422b_8000),
];

fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|v| v.to_bits()).collect()
}

/// The derived inputs of the `UNARY` table.
fn unary_inputs(x: f32) -> [f32; 3] {
    let unit = x.clamp(-1.0, 1.0) * 0.9;
    [x * 0.1, x.abs(), unit]
}

/// The row of `UNARY` for `x`, from the functions of one implementation.
fn unary_row(x: f32, f: &UnaryFunctions) -> [u32; 8] {
    let [scaled, magnitude, unit] = unary_inputs(x);
    let (sine, cosine) = (f.sin_cos)(x);
    [
        x,
        sine,
        cosine,
        (f.tan)(x),
        (f.exp)(scaled),
        (f.log)(magnitude),
        (f.asin)(unit),
        (f.acos)(unit),
    ]
    .map(f32::to_bits)
}

/// The unary functions of one implementation (`portable` or the facade).
struct UnaryFunctions {
    sin_cos: fn(f32) -> (f32, f32),
    tan: fn(f32) -> f32,
    exp: fn(f32) -> f32,
    log: fn(f32) -> f32,
    asin: fn(f32) -> f32,
    acos: fn(f32) -> f32,
}

const OWN: UnaryFunctions = UnaryFunctions {
    sin_cos: own::sin_cos,
    tan: own::tan,
    exp: own::exp,
    log: own::log,
    asin: own::asin,
    acos: own::acos,
};

const FACADE: UnaryFunctions = UnaryFunctions {
    sin_cos: dmath::sin_cos,
    tan: dmath::tan,
    exp: dmath::exp,
    log: dmath::log,
    asin: dmath::asin,
    acos: dmath::acos,
};

#[test]
fn the_own_unary_functions_give_the_pinned_bits() {
    for row in UNARY {
        assert_eq!(
            unary_row(f32::from_bits(row[0]), &OWN),
            row,
            "x = {}",
            f32::from_bits(row[0])
        );
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
/// FNV-1a of the output bits of every own function over 4096 pseudo-random inputs each (a wide
/// net behind the short tables above), taken on Windows x86-64 and Linux x86-64.
const SWEEP_HASH: u64 = 0x24b8_c8a5_acee_1e32;

/// The bit pattern of `value`, with every NaN as the same one.
fn canonical_bits(value: f32) -> u32 {
    if value.is_nan() {
        0x7fc0_0000
    } else {
        value.to_bits()
    }
}

fn fnv(hash: u64, value: u32) -> u64 {
    value.to_le_bytes().into_iter().fold(hash, |h, byte| {
        (h ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
    })
}

/// A seeded linear congruential generator over integers: the same values everywhere.
fn next(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    *state
}

fn sweep_hash() -> u64 {
    let mut state = 12_345_u32;
    let mut hash = FNV_OFFSET;
    for _ in 0..4096 {
        // A value of any exponent for the log, a moderate one for the rest.
        let wide = f32::from_bits(next(&mut state));
        let moderate = f32::from_bits((next(&mut state) & 0x807f_ffff) | 0x4000_0000) * 0.37;
        let unit = f32::from_bits((next(&mut state) & 0x807f_ffff) | 0x3e80_0000);
        let (s, c) = own::sin_cos(moderate * 1000.0);
        for value in [
            s,
            c,
            own::tan(moderate * 1000.0),
            own::atan2(moderate, wide),
            own::asin(unit),
            own::acos(unit),
            own::exp(moderate * 20.0),
            own::log(wide.abs()),
            own::pow(moderate.abs(), unit * 9.0),
            own::powi(moderate, 7),
        ] {
            hash = fnv(hash, canonical_bits(value));
        }
    }
    hash
}

#[test]
fn a_wide_sweep_has_the_pinned_hash() {
    println!("dmath sweep hash {:#018x}", sweep_hash());
    assert_eq!(sweep_hash(), SWEEP_HASH);
}

#[test]
fn the_own_binary_functions_give_the_pinned_bits() {
    for [y, x, atan, power] in PAIRS {
        let (y, x) = (f32::from_bits(y), f32::from_bits(x));
        assert_eq!(
            bits(&[own::atan2(y, x), own::pow(y.abs(), x)]),
            [atan, power],
            "y = {y}, x = {x}"
        );
    }
    for (x, n, expected) in POWIS {
        assert_eq!(
            own::powi(f32::from_bits(x), n).to_bits(),
            expected,
            "x = {}, n = {n}",
            f32::from_bits(x)
        );
    }
}

/// With the feature on, the facade is held to the same bits; with it off the platform functions
/// are allowed to differ and are not pinned.
#[test]
fn the_facade_gives_the_pinned_bits_when_the_feature_is_on() {
    if !dmath::DETERMINISTIC {
        return;
    }
    for row in UNARY {
        assert_eq!(unary_row(f32::from_bits(row[0]), &FACADE), row);
    }
    for [y, x, atan, power] in PAIRS {
        let (y, x) = (f32::from_bits(y), f32::from_bits(x));
        assert_eq!(
            bits(&[dmath::atan2(y, x), dmath::pow(y.abs(), x)]),
            [atan, power]
        );
    }
    for (x, n, expected) in POWIS {
        assert_eq!(dmath::powi(f32::from_bits(x), n).to_bits(), expected);
    }
}

/// The pinned values are cross-checked against the `f64` functions of `std`: each is the `f32`
/// nearest to the exact value, give or take the half ulp that the bound allows.
#[test]
fn the_pinned_bits_agree_with_the_oracle() {
    let close = |bits: u32, exact: f64| {
        let got = f64::from(f32::from_bits(bits));
        if exact.is_infinite() || exact.abs() > f64::from(f32::MAX) {
            assert!(
                got.is_infinite() || got.abs() >= f64::from(f32::MAX) * 0.999,
                "{got} {exact}"
            );
        } else if exact == 0.0 {
            assert_eq!(got.to_bits(), 0.0_f64.to_bits());
        } else {
            let ulp = f64::from(f32::EPSILON) * exact.abs().max(f64::from(f32::MIN_POSITIVE));
            assert!(
                (got - exact).abs() <= 0.51 * ulp,
                "{got:e} against {exact:e}"
            );
        }
    };
    for row in UNARY {
        let x = f64::from(f32::from_bits(row[0]));
        let [scaled, magnitude, unit] = unary_inputs(f32::from_bits(row[0])).map(f64::from);
        close(row[1], x.sin());
        close(row[2], x.cos());
        close(row[3], x.tan());
        close(row[4], scaled.exp());
        close(row[5], magnitude.ln());
        close(row[6], unit.asin());
        close(row[7], unit.acos());
    }
    for [y, x, atan, power] in PAIRS {
        let (y, x) = (f64::from(f32::from_bits(y)), f64::from(f32::from_bits(x)));
        close(atan, y.atan2(x));
        close(power, y.abs().powf(x));
    }
    for (x, n, expected) in POWIS {
        close(expected, f64::from(f32::from_bits(x)).powi(n));
    }
}

/// Arguments above `2^20`, which use the Payne-Hanek reduction: `[x, sin, cos, tan]` as bits.
const LARGE: [[u32; 4]; 8] = [
    [0x4980_0008, 0x3f79_0661, 0x3e6d_67d9, 0x4086_43c1],
    [0x4b18_9680, 0x3ed7_520a, 0xbf68_42df, 0xbeed_53ea],
    [0xd015_02f9, 0x3ef9_9a64, 0x3f5f_84c5, 0x3f0e_f000],
    [0x5e26_8890, 0xbf62_74b7, 0xbeee_c723, 0x3ff2_ca10],
    [0x7149_f2ca, 0xbf4a_89b0, 0xbf1c_9222, 0x3fa5_943b],
    [0x7f7f_ffff, 0xbf05_99b3, 0x3f5a_5f96, 0xbf1c_9eca],
    [0xff7f_ffff, 0x3f05_99b3, 0x3f5a_5f96, 0x3f1c_9eca],
    [0x4b7a_f1ed, 0xbe44_5a17, 0xbf7b_3fe7, 0x3e48_1078],
];
const LARGE_INPUTS: [f32; 8] = [
    1_048_577.0,
    1.0e7,
    -1.0e10,
    3.0e18,
    1.0e30,
    f32::MAX,
    -f32::MAX,
    16_445_933.0,
];
const LARGE_SWEEP_HASH: u64 = 0x27b0_23f1_7e6c_6ab0;

fn large_sweep_hash() -> u64 {
    let mut state = 777_u32;
    let mut hash = FNV_OFFSET;
    for _ in 0..4096 {
        // Any finite value above 2^20: exponent field 147..=254, any sign and mantissa.
        let raw = next(&mut state);
        let field = 147 + (raw >> 24) % 108;
        let x = f32::from_bits((raw & 0x807f_ffff) | (field << 23));
        let (s, c) = own::sin_cos(x);
        for value in [s, c, own::tan(x)] {
            hash = fnv(hash, canonical_bits(value));
        }
    }
    hash
}

#[test]
fn large_arguments_give_the_pinned_bits() {
    let rows = LARGE_INPUTS.map(|x| {
        let (s, c) = own::sin_cos(x);
        [x, s, c, own::tan(x)].map(f32::to_bits)
    });
    for row in rows {
        println!("{row:#010x?}");
    }
    assert_eq!(rows, LARGE);
    // And they are the right numbers: within 1.2e-7 (relative, about two ulps) of the f64 oracle.
    for x in LARGE_INPUTS {
        let (s, c) = own::sin_cos(x);
        let (t, x) = (own::tan(x), f64::from(x));
        for (got, exact) in [(s, x.sin()), (c, x.cos()), (t, x.tan())] {
            assert!(
                (f64::from(got) - exact).abs() <= 1.2e-7 * exact.abs(),
                "{x:e}: {got} against {exact}"
            );
        }
    }
}

#[test]
fn a_large_sweep_has_the_pinned_hash() {
    println!("dmath large sweep hash {:#018x}", large_sweep_hash());
    assert_eq!(large_sweep_hash(), LARGE_SWEEP_HASH);
}
