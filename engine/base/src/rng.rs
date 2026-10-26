//! Seeded, deterministic random numbers (PCG32, XSH RR 64/32).
//!
//! Integer arithmetic only, so the sequence is identical on every platform.

/// Bit pattern of `1.0_f32`.
const ONE_BITS: u32 = 0x3f80_0000;
/// A 32-bit draw shifted right by this many bits fills the 23 mantissa bits.
const MANTISSA_SHIFT: u32 = 9;
/// PCG XSH RR: the state is xor-shifted right by this many bits first.
const XORSHIFT_BITS: u32 = 18;
/// PCG XSH RR: after the xor-shift the value is shifted right by this many bits.
const XORSHIFT_DISCARD: u32 = 27;
/// PCG XSH RR: the top bits of the old state (from this bit up) pick the rotation.
const ROTATION_SHIFT: u32 = 59;
const MULTIPLIER: u64 = 6_364_136_223_846_793_005;

/// The low 32 bits of `value`; the PCG XSH RR output function discards the rest.
fn low_u32(value: u64) -> u32 {
    let [b0, b1, b2, b3, ..] = value.to_le_bytes();
    u32::from_le_bytes([b0, b1, b2, b3])
}

/// Returned by [`Pcg32::below`] when the bound is zero (no value is below zero).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZeroBound;

/// PCG32 generator: 64 bits of state, 63 bits of stream selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

impl Pcg32 {
    /// Creates a generator from a seed and a stream number.
    ///
    /// Only the low 63 bits of `stream` select the stream: `stream` and
    /// `stream + 2^63` give the same sequence (as in the PCG reference).
    #[must_use]
    pub fn new(seed: u64, stream: u64) -> Self {
        let mut rng = Self {
            state: 0,
            inc: (stream << 1) | 1,
        };
        rng.step();
        rng.state = rng.state.wrapping_add(seed);
        rng.step();
        rng
    }

    fn step(&mut self) {
        self.state = self.state.wrapping_mul(MULTIPLIER).wrapping_add(self.inc);
    }

    /// Next 32 random bits.
    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.step();
        let xorshifted = low_u32(((old >> XORSHIFT_BITS) ^ old) >> XORSHIFT_DISCARD);
        let rotation = low_u32(old >> ROTATION_SHIFT);
        xorshifted.rotate_right(rotation)
    }

    /// Next float in `[0, 1)` with 23 random mantissa bits; integer operations only.
    pub fn next_f32(&mut self) -> f32 {
        // Exponent of 1.0 plus random mantissa gives a value in [1, 2); subtracting 1 is exact.
        f32::from_bits(ONE_BITS | (self.next_u32() >> MANTISSA_SHIFT)) - 1.0
    }

    /// Unbiased value in `0..bound` (rejection sampling).
    ///
    /// # Errors
    /// Returns [`ZeroBound`] when `bound` is zero.
    pub fn below(&mut self, bound: u32) -> Result<u32, ZeroBound> {
        if bound == 0 {
            return Err(ZeroBound);
        }
        // Draws below this threshold would make small remainders more likely.
        let threshold = bound.wrapping_neg() % bound;
        let mut draw = self.next_u32();
        while draw < threshold {
            draw = self.next_u32();
        }
        Ok(draw % bound)
    }
}

#[cfg(test)]
mod tests {
    use super::{Pcg32, ZeroBound};

    fn draw(seed: u64, stream: u64, count: usize) -> Vec<u32> {
        let mut rng = Pcg32::new(seed, stream);
        (0..count).map(|_| rng.next_u32()).collect()
    }

    #[test]
    fn matches_the_published_reference_vector() {
        let mut rng = Pcg32::new(42, 54);
        let got: Vec<u32> = (0..6).map(|_| rng.next_u32()).collect();
        assert_eq!(
            got,
            [
                0xa15c_02b7,
                0x7b47_f409,
                0xba1d_3330,
                0x83d2_f293,
                0xbfa4_784b,
                0xcbed_606e
            ]
        );
    }

    #[test]
    fn other_seed_and_stream_match_an_independent_implementation() {
        assert_eq!(draw(7, 3, 3), [3_682_281_251, 3_185_575_708, 1_655_154_972]);
    }

    #[test]
    fn same_seed_and_stream_repeat_the_sequence() {
        assert_eq!(draw(9, 1, 16), draw(9, 1, 16));
    }

    #[test]
    fn different_streams_give_different_sequences() {
        assert_ne!(draw(9, 1, 8), draw(9, 2, 8));
    }

    #[test]
    fn different_seeds_give_different_sequences() {
        assert_ne!(draw(1, 0, 8), draw(2, 0, 8));
    }

    #[test]
    fn float_uses_the_top_23_bits_of_the_draw() {
        // First draw for seed 42, stream 54 is 0xa15c02b7; 0xa15c02b7 >> 9 = 5_287_425.
        let mut rng = Pcg32::new(42, 54);
        assert_eq!(
            rng.next_f32().to_bits(),
            (5_287_425.0_f32 / 8_388_608.0).to_bits()
        );
    }

    #[test]
    fn floats_stay_in_the_half_open_unit_interval() {
        let mut rng = Pcg32::new(5, 5);
        for _ in 0..10_000 {
            let value = rng.next_f32();
            assert!((0.0..1.0).contains(&value), "{value}");
        }
    }

    #[test]
    fn extreme_bounds_stay_in_range() {
        let mut rng = Pcg32::new(3, 3);
        for _ in 0..1_000 {
            assert!(rng.below(u32::MAX).is_ok_and(|value| value < u32::MAX));
            assert!(rng.below(1 << 31).is_ok_and(|value| value < 1 << 31));
        }
    }

    #[test]
    fn largest_float_is_one_ulp_below_one() {
        // Mantissa all ones is the largest value next_f32 can return.
        let largest = f32::from_bits(0x3f80_0000 | 0x007f_ffff) - 1.0;
        assert!(largest < 1.0);
    }

    #[test]
    fn zero_bound_is_an_error_not_a_panic() {
        assert_eq!(Pcg32::new(1, 1).below(0), Err(ZeroBound));
    }

    #[test]
    fn bound_of_one_always_gives_zero() {
        let mut rng = Pcg32::new(1, 1);
        assert!((0..100).all(|_| rng.below(1) == Ok(0)));
    }

    #[test]
    fn values_stay_below_the_bound_and_cover_it() {
        let mut rng = Pcg32::new(11, 0);
        let mut seen = [false; 6];
        for _ in 0..600 {
            let value = rng.below(6).unwrap_or(u32::MAX);
            assert!(value < 6);
            seen[usize::try_from(value).unwrap_or(0)] = true;
        }
        assert!(seen.iter().all(|hit| *hit));
    }

    #[test]
    fn biased_draws_are_rejected_and_redrawn() {
        // Seed 2 draws 257_813_417 first, below the rejection threshold 1_294_967_296
        // for this bound, so the value must come from a later draw.
        let mut rng = Pcg32::new(2, 0);
        assert_eq!(rng.below(3_000_000_000), Ok(531_328_388));
    }

    #[test]
    fn a_draw_equal_to_the_threshold_is_accepted() {
        // Second draw for seed 42, stream 54 is 2_068_313_097. For bound 2^32 - 2_068_313_097
        // the threshold equals that draw, and a draw at the threshold is unbiased.
        let mut rng = Pcg32::new(42, 54);
        rng.next_u32();
        assert_eq!(rng.below(2_226_654_199), Ok(2_068_313_097));
    }

    #[test]
    fn accepted_draw_is_reduced_modulo_the_bound() {
        // Seed 42, stream 54: first draw 2_707_161_783 is accepted and below the bound.
        let mut rng = Pcg32::new(42, 54);
        assert_eq!(rng.below(3_000_000_000), Ok(2_707_161_783));
        assert_eq!(rng.below(1_000), Ok(2_068_313_097 % 1_000));
    }
}
