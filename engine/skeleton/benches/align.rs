//! Std-only benchmark harness for the walking skeleton.
//!
//! Warm-up, then timed samples; prints the median and 95th percentile in
//! nanoseconds per call as JSON on stdout. A library benchmark tool replaces
//! this when the PR gate on instruction counts is built.

use std::hint::black_box;
use std::time::Instant;

use skeleton::align::align_up;

const WARM_UP_CALLS: u32 = 100_000;
const SAMPLES: usize = 30;
const CALLS_PER_SAMPLE: u32 = 200_000;
const NANOS_PER_SECOND: f64 = 1e9;
const MEDIAN_PERCENT: usize = 50;
const P95_PERCENT: usize = 95;
const PERCENT: usize = 100;
const VALUE: u64 = 12_345;
const ALIGNMENT: u64 = 64;

fn time_one_sample() -> f64 {
    let start = Instant::now();
    for _ in 0..CALLS_PER_SAMPLE {
        black_box(align_up(black_box(VALUE), black_box(ALIGNMENT)));
    }
    start.elapsed().as_secs_f64() * NANOS_PER_SECOND / f64::from(CALLS_PER_SAMPLE)
}

fn percentile(sorted: &[f64], percent: usize) -> f64 {
    let index = (sorted.len() * percent / PERCENT).min(sorted.len().saturating_sub(1));
    sorted.get(index).copied().unwrap_or(0.0)
}

fn main() {
    for _ in 0..WARM_UP_CALLS {
        black_box(align_up(black_box(VALUE), black_box(ALIGNMENT)));
    }
    let mut samples: Vec<f64> = (0..SAMPLES).map(|_| time_one_sample()).collect();
    samples.sort_by(f64::total_cmp);
    println!(
        "{{\"benchmarks\":[{{\"name\":\"align_up\",\"median_ns\":{:.3},\"p95_ns\":{:.3}}}]}}",
        percentile(&samples, MEDIAN_PERCENT),
        percentile(&samples, P95_PERCENT),
    );
}
