//! Report only (card 4-7, ADR-0004 point 3): how the wait-policy time budget changes the join
//! latency of the work-stealing pool. It is a measurement aid, not a gate.
//!
//! Workloads are the join-latency ones of the scheduler spike: `dispatch`, many fork-joins over
//! 24 trivial items (the caller waits `gap` microseconds between calls, as a frame does between
//! two phases), and `phases`, 100 dependent phases of 512 indices at grain 8. A budget of 0 is
//! "no scan before parking" (the ablation of the spike). The cell is the median over
//! repetitions, in nanoseconds per call (gap excluded) or per phase.
//!
//! Run: `cargo run --release -p tools_scheduler_spike --example wait_budget -- [workers] [repetitions]`.

use jobs::pool::{WaitPolicy, WorkStealingPool};
use jobs::scheduler::IScheduler;
use platform::threads::StdThreads;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Budgets compared, in microseconds (0: park at once).
const BUDGETS_US: [u64; 6] = [0, 50, 100, 200, 400, 1000];
/// Time the caller spends between two calls, in microseconds.
const GAPS_US: [u64; 6] = [0, 20, 50, 100, 200, 500];
/// Fork-joins per repetition of `dispatch`.
const CALLS: u32 = 1000;
/// Items of one `dispatch` call.
const DISPATCH_ITEMS: u32 = 24;
/// Phases per repetition of `phases`, indices per phase, chunk size, burn per index.
const PHASES: u32 = 100;
const PHASE_INDICES: u32 = 512;
const PHASE_GRAIN: u32 = 8;
const PHASE_BURN: u32 = 12;

/// A dependent chain of `iters` multiply-xorshift steps: cannot be folded away.
fn burn(seed: u64, iters: u32) -> u64 {
    (0..iters).fold(seed | 1, |x, _| {
        let y = x.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        y ^ (y >> 29)
    })
}

fn wait_for(gap: Duration) {
    let until = Instant::now() + gap;
    while Instant::now() < until {
        std::hint::spin_loop();
    }
}

/// Nanoseconds per call of `CALLS` fork-joins with `gap` between them (the gap is subtracted).
fn dispatch(pool: &dyn IScheduler, gap: Duration, sink: &AtomicU64) -> f64 {
    let begun = Instant::now();
    for _ in 0..CALLS {
        let result = pool.for_each(DISPATCH_ITEMS, 1, &|i| {
            sink.fetch_add(u64::from(i), Ordering::Relaxed);
        });
        assert_eq!(result, Ok(()));
        wait_for(gap);
    }
    let total = begun.elapsed().as_secs_f64() * 1e9;
    total / f64::from(CALLS) - gap.as_secs_f64() * 1e9
}

/// One chunk of a phase: each slot becomes a function of its right neighbour in the previous
/// phase's buffer.
fn phase_chunk(source: &[AtomicU64], target: &[AtomicU64], range: std::ops::Range<u32>) {
    for i in range {
        let next = source[((i + 1) % PHASE_INDICES) as usize].load(Ordering::Relaxed);
        target[i as usize].store(burn(next, PHASE_BURN), Ordering::Relaxed);
    }
}

/// Nanoseconds per phase of `PHASES` dependent phases.
fn phases(pool: &dyn IScheduler, buffers: &[Vec<AtomicU64>; 2]) -> f64 {
    let begun = Instant::now();
    for phase in 0..PHASES {
        let (source, target) = if phase % 2 == 0 {
            (&buffers[0], &buffers[1])
        } else {
            (&buffers[1], &buffers[0])
        };
        let result = pool.parallel_for(PHASE_INDICES, PHASE_GRAIN, &|range| {
            phase_chunk(source, target, range);
        });
        assert_eq!(result, Ok(()));
    }
    begun.elapsed().as_secs_f64() * 1e9 / f64::from(PHASES)
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn cells(values: &[f64]) -> String {
    let text: Vec<String> = values.iter().map(|v| format!("{v:.0}")).collect();
    text.join(" | ")
}

fn build(workers: u32, budget_us: u64) -> WorkStealingPool {
    WorkStealingPool::with_policy(
        workers,
        WaitPolicy::bounded_micros(budget_us),
        &StdThreads::new(),
    )
    .unwrap_or_else(|e| panic!("a pool of {workers} workers: {e}"))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let workers: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(4);
    let reps: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(7);
    let sink = AtomicU64::new(0);
    let buffers: [Vec<AtomicU64>; 2] = std::array::from_fn(|_| {
        (0..PHASE_INDICES)
            .map(|i| AtomicU64::new(u64::from(i)))
            .collect()
    });
    println!("workers {workers}, repetitions {reps}, median ns (dispatch: per call, gap excluded)");
    let header: Vec<String> = BUDGETS_US.iter().map(|b| format!("{b} us")).collect();
    println!("| workload | {} |", header.join(" | "));
    println!("|---|{}", "---|".repeat(BUDGETS_US.len()));
    // One pool at a time, so idle pools do not disturb the measured one.
    let mut table: Vec<Vec<f64>> = vec![Vec::new(); GAPS_US.len() + 1];
    for budget_us in BUDGETS_US {
        let pool = build(workers, budget_us);
        for (row, gap_us) in GAPS_US.iter().enumerate() {
            let gap = Duration::from_micros(*gap_us);
            let _ = dispatch(&pool, gap, &sink); // warm-up
            let runs = (0..reps).map(|_| dispatch(&pool, gap, &sink)).collect();
            table[row].push(median(runs));
        }
        let _ = phases(&pool, &buffers);
        let runs = (0..reps).map(|_| phases(&pool, &buffers)).collect();
        table[GAPS_US.len()].push(median(runs));
    }
    for (row, gap_us) in GAPS_US.iter().enumerate() {
        println!("| dispatch, gap {gap_us} us | {} |", cells(&table[row]));
    }
    println!("| phases (per phase) | {} |", cells(&table[GAPS_US.len()]));
    println!("sink {}", sink.load(Ordering::Relaxed));
}
