//! The three loops under test: an 8-lane kernel built from two `F32x4`, a plain scalar loop the
//! compiler may vectorise, and a scalar loop whose loads are hidden from the optimiser.
//!
//! Every function walks whole groups of 8 elements (`chunks_exact(8)`): the length of the three
//! slices must be a multiple of 8 and equal; a shorter output is left unwritten past its end.

use base::simd::F32x4;
use std::hint::black_box;

/// Lanes handled per step: two `F32x4`.
pub const GROUP: usize = 8;
const HALF: usize = GROUP / 2;

fn quad(lanes: &[f32]) -> F32x4 {
    let mut four = [0.0; HALF];
    four.copy_from_slice(lanes);
    F32x4::from_array(four)
}

/// `out = op(a, b)` eight lanes at a time with two `F32x4` per step.
#[inline]
pub fn simd(a: &[f32], b: &[f32], out: &mut [f32], op: impl Fn(F32x4, F32x4) -> F32x4) {
    let groups = a.chunks_exact(GROUP).zip(b.chunks_exact(GROUP));
    for ((x, y), z) in groups.zip(out.chunks_exact_mut(GROUP)) {
        let (x_lo, x_hi) = x.split_at(HALF);
        let (y_lo, y_hi) = y.split_at(HALF);
        let (z_lo, z_hi) = z.split_at_mut(HALF);
        z_lo.copy_from_slice(&op(quad(x_lo), quad(y_lo)).to_array());
        z_hi.copy_from_slice(&op(quad(x_hi), quad(y_hi)).to_array());
    }
}

/// `out = op(a, b)` one lane at a time; the compiler is free to vectorise it.
#[inline]
pub fn scalar(a: &[f32], b: &[f32], out: &mut [f32], op: impl Fn(f32, f32) -> f32) {
    for ((x, y), z) in a.iter().zip(b).zip(out.iter_mut()) {
        *z = op(*x, *y);
    }
}

/// `out = op(a, b)` one lane at a time with every load passed through `black_box`, so the loop
/// cannot be turned into vector code: the "genuinely scalar" baseline.
#[inline]
pub fn scalar_opaque(a: &[f32], b: &[f32], out: &mut [f32], op: impl Fn(f32, f32) -> f32) {
    for ((x, y), z) in a.iter().zip(b).zip(out.iter_mut()) {
        *z = op(black_box(*x), black_box(*y));
    }
}

/// Deterministic test data: element `i` of a series, `(i mod 997) * scale + offset`.
pub fn sample(i: usize, scale: f32, offset: f32) -> f32 {
    f32::from(u16::try_from(i % 997).unwrap_or(0)) * scale + offset
}

/// `a < b ? a : b`, the definition of `F32x4::min` per lane.
pub fn min_lane(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEN: usize = 40;

    fn inputs() -> (Vec<f32>, Vec<f32>) {
        let a = (0..LEN).map(|i| sample(i, 0.37, -5.0)).collect();
        let b = (0..LEN).map(|i| sample(i, -0.21, 9.0)).collect();
        (a, b)
    }

    fn bits(values: &[f32]) -> Vec<u32> {
        values.iter().map(|v| v.to_bits()).collect()
    }

    fn run(kernel: impl Fn(&[f32], &[f32], &mut [f32])) -> Vec<f32> {
        let (a, b) = inputs();
        let mut out = vec![-1.0; LEN];
        kernel(&a, &b, &mut out);
        out
    }

    #[test]
    fn the_simd_add_matches_the_scalar_add_bit_for_bit() {
        let simd_out = run(|a, b, o| simd(a, b, o, |x, y| x + y));
        let scalar_out = run(|a, b, o| scalar(a, b, o, |x, y| x + y));
        assert_eq!(bits(&simd_out), bits(&scalar_out));
        let (a, b) = inputs();
        assert_eq!(simd_out[1].to_bits(), (a[1] + b[1]).to_bits());
    }

    #[test]
    fn the_simd_mul_matches_the_scalar_mul_bit_for_bit() {
        let simd_out = run(|a, b, o| simd(a, b, o, |x, y| x * y));
        let scalar_out = run(|a, b, o| scalar(a, b, o, |x, y| x * y));
        assert_eq!(bits(&simd_out), bits(&scalar_out));
    }

    #[test]
    fn the_simd_min_matches_the_scalar_min_bit_for_bit() {
        let simd_out = run(|a, b, o| simd(a, b, o, F32x4::min));
        let scalar_out = run(|a, b, o| scalar(a, b, o, min_lane));
        assert_eq!(bits(&simd_out), bits(&scalar_out));
    }

    #[test]
    fn the_opaque_loop_gives_the_same_bits_as_the_plain_loop() {
        let plain = run(|a, b, o| scalar(a, b, o, |x, y| x + y));
        let opaque = run(|a, b, o| scalar_opaque(a, b, o, |x, y| x + y));
        assert_eq!(bits(&plain), bits(&opaque));
    }

    #[test]
    fn min_returns_the_second_operand_on_a_tie_like_the_vector_instruction() {
        let (a, b) = ([0.0_f32; 8], [-0.0_f32; 8]);
        let mut simd_out = [1.0_f32; 8];
        let mut scalar_out = [1.0_f32; 8];
        simd(&a, &b, &mut simd_out, F32x4::min);
        scalar(&a, &b, &mut scalar_out, min_lane);
        assert_eq!(bits(&simd_out), bits(&scalar_out));
        assert_eq!(simd_out[0].to_bits(), (-0.0_f32).to_bits());
    }

    #[test]
    fn a_tail_shorter_than_a_group_is_left_unwritten() {
        let (a, b) = (vec![1.0_f32; 11], vec![2.0_f32; 11]);
        let mut out = vec![-1.0_f32; 11];
        simd(&a, &b, &mut out, |x, y| x + y);
        assert_eq!(out[7].to_bits(), 3.0_f32.to_bits());
        assert_eq!(out[8].to_bits(), (-1.0_f32).to_bits());
    }

    #[test]
    fn every_group_of_eight_is_written() {
        let out = run(|a, b, o| simd(a, b, o, |x, y| x + y));
        assert!(out.iter().all(|v| v.to_bits() != (-1.0_f32).to_bits()));
    }
}
