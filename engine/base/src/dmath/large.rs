//! Payne-Hanek range reduction for the large arguments `|x| > 2^20` of `sin`, `cos` and `tan`.
//!
//! `x = m * 2^e` with a 24-bit integer `m`. The product `m * (2/pi)` is formed exactly in integers
//! from the first 256 bits of `2/pi`, and only a window of 66 bits of it is kept: the two bits
//! worth `2^1` and `2^0` (the quadrant, the integer part modulo 4) and the 64 bits below (the
//! fraction of a quadrant). The bits above the window are whole turns, and the bits below it are
//! under `2^-64` quadrants. For every finite `f32` the window lies inside the 256 bits plus the 24
//! of `m`, and the error of the fraction is under `2^-62`, so a result close to a multiple of
//! `pi/2` (the known worst `f32` is about `2^-30` of a quadrant away) still has about 32 correct
//! bits, 8 more than the 24 an `f32` needs. Only integer arithmetic and two `f64` operations are used.

use std::f64::consts::FRAC_PI_2;

/// The first 256 bits of `2/pi` as a binary fraction, most significant limb first.
const TWO_OVER_PI: [u64; 4] = [
    0xa2f9_836e_4e44_1529,
    0xfc27_57d1_f534_ddc0,
    0xdb62_9599_3c43_9041,
    0xfe51_63ab_debb_c561,
];
/// Limbs of the product of the 256 bits and a 24-bit integer (280 bits, padded to six limbs).
const LIMBS: usize = 6;
const LIMB_BITS: u32 = 64;
const MANTISSA_MASK: u32 = 0x007f_ffff;
const IMPLICIT_BIT: u32 = 0x0080_0000;
const EXPONENT_SHIFT: u32 = 23;
const EXPONENT_MASK: u32 = 0xff;
/// The window starts at bit `256 - 64 - e` of the product, and `e = exponent field - 150`.
const WINDOW_BASE: u32 = 256 - 64 + 150;
/// A fraction at or above one half is rounded up to the next quadrant, as a negative fraction.
const HALF: u64 = 1 << 63;
const QUADRANT_MASK: u64 = 3;
/// The 66 bits of the window.
const WINDOW_MASK: u128 = (1 << 66) - 1;
/// `2^32` and `2^-64`, both exact.
const TWO_POW_32: f64 = 4_294_967_296.0;
const TWO_POW_NEG_64: f64 = 1.0 / (TWO_POW_32 * TWO_POW_32);

/// The low and the high 64 bits of `value`.
fn split(value: u128) -> (u64, u64) {
    let [
        a0,
        a1,
        a2,
        a3,
        a4,
        a5,
        a6,
        a7,
        b0,
        b1,
        b2,
        b3,
        b4,
        b5,
        b6,
        b7,
    ] = value.to_le_bytes();
    (
        u64::from_le_bytes([a0, a1, a2, a3, a4, a5, a6, a7]),
        u64::from_le_bytes([b0, b1, b2, b3, b4, b5, b6, b7]),
    )
}

/// `m * 2/pi * 2^256` as little-endian limbs.
fn product(m: u64) -> [u64; LIMBS] {
    let mut limbs = [0_u64; LIMBS];
    let mut carry = 0_u64;
    let fraction = TWO_OVER_PI.iter().rev().chain(std::iter::repeat(&0));
    for (slot, &limb) in limbs.iter_mut().zip(fraction) {
        let (low, high) = split(u128::from(limb) * u128::from(m) + u128::from(carry));
        *slot = low;
        carry = high;
    }
    limbs
}

/// The 66 bits of `limbs` from bit `from` up (`from` is under 256, so three limbs hold them).
fn window(limbs: &[u64; LIMBS], from: u32) -> u128 {
    let skip = (from / LIMB_BITS) as usize;
    let shift = from % LIMB_BITS;
    let mut words = limbs.iter().skip(skip).copied().chain(std::iter::repeat(0));
    let low = u128::from(words.next().unwrap_or(0));
    let middle = u128::from(words.next().unwrap_or(0));
    let high = u128::from(words.next().unwrap_or(0));
    let body = ((middle << LIMB_BITS) | low) >> shift;
    // For a shift of zero the third limb lies beyond the 66 bits.
    let top = high.checked_shl(2 * LIMB_BITS - shift).unwrap_or(0);
    (body | top) & WINDOW_MASK
}

/// A 64-bit integer as `f64` with one rounding (no `as` cast).
fn to_f64(value: u64) -> f64 {
    let [h0, h1, h2, h3, l0, l1, l2, l3] = value.to_be_bytes();
    f64::from(u32::from_be_bytes([h0, h1, h2, h3])) * TWO_POW_32
        + f64::from(u32::from_be_bytes([l0, l1, l2, l3]))
}

/// For the bit pattern of a finite positive `f32` above `2^20`: `r` and the quadrant `n mod 4`
/// with `x = r + n * pi/2` and `|r| <= pi/4`.
pub(super) fn reduce(bits: u32) -> (f64, u64) {
    let field = (bits >> EXPONENT_SHIFT) & EXPONENT_MASK;
    let m = u64::from((bits & MANTISSA_MASK) | IMPLICIT_BIT);
    let (fraction, quadrant) = split(window(&product(m), WINDOW_BASE - field));
    if fraction >= HALF {
        let below = to_f64(fraction.wrapping_neg());
        (
            -below * TWO_POW_NEG_64 * FRAC_PI_2,
            quadrant.wrapping_add(1) & QUADRANT_MASK,
        )
    } else {
        (to_f64(fraction) * TWO_POW_NEG_64 * FRAC_PI_2, quadrant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_separates_the_halves() {
        assert_eq!(split((7_u128 << 64) | 9), (9, 7));
    }

    #[test]
    fn product_of_one_is_the_constant_and_carries_propagate() {
        assert_eq!(
            product(1),
            [
                0xfe51_63ab_debb_c561,
                0xdb62_9599_3c43_9041,
                0xfc27_57d1_f534_ddc0,
                0xa2f9_836e_4e44_1529,
                0,
                0
            ]
        );
        let doubled = product(2);
        assert_eq!(doubled[0], 0xfe51_63ab_debb_c561_u64.wrapping_mul(2));
        assert_eq!(doubled[4], 1);
    }

    /// Two limbs as one number, `high` above `low`.
    fn pair(high: u64, low: u64) -> u128 {
        (u128::from(high) << 64) | u128::from(low)
    }

    #[test]
    fn window_reads_66_bits_across_limb_boundaries() {
        const MASK_66: u128 = WINDOW_MASK;
        let limbs = [
            0x0123_4567_89ab_cdef,
            0xfedc_ba98_7654_3210,
            0x1111_2222_3333_4444,
            0x5555_6666_7777_8888,
            0,
            0,
        ];
        assert_eq!(window(&limbs, 0), pair(limbs[1], limbs[0]) & MASK_66);
        assert_eq!(window(&limbs, 4), (pair(limbs[1], limbs[0]) >> 4) & MASK_66);
        assert_eq!(
            window(&limbs, 63),
            (pair(limbs[1], limbs[0]) >> 63) & MASK_66
        );
        assert_eq!(
            window(&limbs, 68),
            (pair(limbs[2], limbs[1]) >> 4) & MASK_66
        );
        assert_eq!(window(&limbs, 128), pair(limbs[3], limbs[2]) & MASK_66);
        assert_eq!(
            window(&limbs, 190),
            (pair(limbs[3], limbs[2]) >> 62) & MASK_66
        );
    }

    #[test]
    fn to_f64_is_exact_below_two_to_the_53() {
        assert!((to_f64((1 << 52) + 5) - 4_503_599_627_370_501.0).abs() < 0.5);
        assert!((to_f64(u64::from(u32::MAX) + 1) - TWO_POW_32).abs() < 0.5);
    }
}
