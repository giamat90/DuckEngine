//! Dev-time benchmark for gate G4 (card 1-33): is the 8-lane `F32x4` kernel faster than scalar
//! loops? Prints the environment and a markdown table (nanoseconds per element, and ratios of the
//! scalar loops to the kernel). Report only: no threshold is applied here.
//!
//! `tools_simd_bench [--reps N]`. Build with `--release`. Not part of the engine.

mod kernels;

use base::simd::F32x4;
use kernels::{min_lane, sample, scalar, scalar_opaque, simd};
use std::hint::black_box;
use std::time::Instant;

const DEFAULT_REPS: usize = 15;
/// Elements per array: 4 KiB (fits L1) and 1 MiB (L2/L3). Multiples of 8.
const LENGTHS: [usize; 2] = [1024, 262_144];
/// Elements processed per timed repetition, whatever the array length.
const ELEMENTS_PER_REP: usize = 1 << 24;

type Kernel<'a> = dyn Fn(&[f32], &[f32], &mut [f32]) + 'a;

struct Row {
    op: &'static str,
    len: usize,
    simd_ns: f64,
    plain_ns: f64,
    opaque_ns: f64,
}

/// A count as `f64` (exact below 2^32).
fn as_f64(count: usize) -> f64 {
    f64::from(u32::try_from(count).unwrap_or(u32::MAX))
}

/// Median of the samples; `None` when there are none.
fn median(samples: &mut [f64]) -> Option<f64> {
    samples.sort_by(f64::total_cmp);
    let middle = samples.len() / 2;
    if samples.len() % 2 == 1 {
        samples.get(middle).copied()
    } else {
        let (low, high) = (samples.get(middle.checked_sub(1)?)?, samples.get(middle)?);
        Some((low + high) / 2.0)
    }
}

/// Median nanoseconds per element of `kernel` run over arrays of `len` elements.
fn time_kernel(len: usize, reps: usize, kernel: &Kernel<'_>) -> f64 {
    let a: Vec<f32> = (0..len).map(|i| sample(i, 0.5, -100.0)).collect();
    let b: Vec<f32> = (0..len).map(|i| sample(i, 0.25, 1.0)).collect();
    let mut out = vec![0.0_f32; len];
    let passes = (ELEMENTS_PER_REP / len).max(1);
    let mut samples: Vec<f64> = (0..=reps)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..passes {
                kernel(black_box(&a), black_box(&b), black_box(&mut out));
            }
            start.elapsed().as_secs_f64() * 1e9 / as_f64(passes * len)
        })
        .skip(1) // the first repetition is the warm-up
        .collect();
    black_box(&out);
    median(&mut samples).unwrap_or(f64::NAN)
}

fn row(
    op: &'static str,
    len: usize,
    reps: usize,
    on_simd: impl Fn(F32x4, F32x4) -> F32x4 + Copy,
    on_lane: impl Fn(f32, f32) -> f32 + Copy,
) -> Row {
    Row {
        op,
        len,
        simd_ns: time_kernel(len, reps, &|a, b, o| simd(a, b, o, on_simd)),
        plain_ns: time_kernel(len, reps, &|a, b, o| scalar(a, b, o, on_lane)),
        opaque_ns: time_kernel(len, reps, &|a, b, o| scalar_opaque(a, b, o, on_lane)),
    }
}

fn rows(reps: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    for len in LENGTHS {
        rows.push(row("add", len, reps, |x, y| x + y, |x, y| x + y));
        rows.push(row("mul", len, reps, |x, y| x * y, |x, y| x * y));
        rows.push(row("min", len, reps, F32x4::min, min_lane));
    }
    rows
}

fn print_environment() {
    println!(
        "ENV|target|{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!("ENV|simd backend|{}", simd_backend());
}

fn simd_backend() -> &'static str {
    if cfg!(all(target_arch = "x86_64", target_feature = "sse2")) {
        "sse2"
    } else {
        "scalar"
    }
}

fn print_table(rows: &[Row]) {
    println!(
        "| op | elements | kernel ns/el | plain loop ns/el | opaque loop ns/el | plain / kernel | opaque / kernel |"
    );
    println!("|---|---|---|---|---|---|---|");
    for r in rows {
        println!(
            "| {} | {} | {:.3} | {:.3} | {:.3} | {:.2} | {:.2} |",
            r.op,
            r.len,
            r.simd_ns,
            r.plain_ns,
            r.opaque_ns,
            r.plain_ns / r.simd_ns,
            r.opaque_ns / r.simd_ns
        );
    }
}

fn parse_reps(args: &[String]) -> Result<usize, String> {
    match args {
        [] => Ok(DEFAULT_REPS),
        [flag, n] if flag == "--reps" => match n.parse::<usize>() {
            Ok(reps) if reps > 0 => Ok(reps),
            _ => Err(format!("--reps needs a positive number, got {n}")),
        },
        _ => Err("usage: tools_simd_bench [--reps N]".to_owned()),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_reps(&args) {
        Ok(reps) => {
            print_environment();
            print_table(&rows(reps));
        }
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_of_an_odd_count_is_the_middle_value() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), Some(2.0));
    }

    #[test]
    fn the_median_of_an_even_count_is_the_mean_of_the_middle_two() {
        assert_eq!(median(&mut [4.0, 1.0, 2.0, 3.0]), Some(2.5));
    }

    #[test]
    fn the_median_of_nothing_is_none() {
        assert_eq!(median(&mut []), None);
    }

    #[test]
    fn reps_defaults_and_takes_a_positive_number() {
        assert_eq!(parse_reps(&[]), Ok(DEFAULT_REPS));
        assert_eq!(parse_reps(&["--reps".to_owned(), "3".to_owned()]), Ok(3));
        assert!(parse_reps(&["--reps".to_owned(), "0".to_owned()]).is_err());
        assert!(parse_reps(&["--bogus".to_owned()]).is_err());
    }

    #[test]
    fn timing_a_kernel_reports_a_positive_cost_per_element() {
        let ns = time_kernel(64, 2, &|a, b, o| simd(a, b, o, |x, y| x + y));
        assert!(ns > 0.0 && ns.is_finite());
    }
}
