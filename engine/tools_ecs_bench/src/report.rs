//! The wall-time report: nanoseconds per entity of each workload, serial and on the pool.

use crate::cli::Options;
use crate::workload::{Kind, build, frames_parallel, frames_serial};
use ecs::EcsError;
use jobs::pool::WorkStealingPool;
use jobs::scheduler::IScheduler;
use platform::threads::StdThreads;
use std::time::Instant;

/// Pool sizes of the parallel rows (the calling thread included).
const WORKERS: [u32; 3] = [2, 4, 6];

/// Rows per chunk of the parallel rows.
const GRAIN: u32 = 2048;

/// Frames per timed iteration.
const FRAMES: u32 = 4;

/// Nanoseconds in a second, as a float.
const NS: f64 = 1.0e9;

/// Median of the samples (the slice is sorted); 0 when empty.
fn median(samples: &mut [f64]) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples.get(samples.len() / 2).copied().unwrap_or(0.0)
}

/// Median nanoseconds per entity per frame of `run` over `iters` iterations.
fn time(
    entities: u32,
    iters: u32,
    mut run: impl FnMut() -> Result<(), EcsError>,
) -> Result<f64, EcsError> {
    run()?; // warm-up, discarded
    let mut samples = Vec::new();
    for _ in 0..iters {
        let start = Instant::now();
        run()?;
        let per = start.elapsed().as_secs_f64() * NS / f64::from(entities) / f64::from(FRAMES);
        samples.push(per);
    }
    Ok(median(&mut samples))
}

fn serial_row(options: Options, kind: Kind) -> Result<f64, EcsError> {
    let mut world = build(options.entities)?;
    time(options.entities, options.iters, || {
        frames_serial(&mut world, kind, FRAMES)
    })
}

fn pool_row(options: Options, kind: Kind, pool: &dyn IScheduler) -> Result<f64, EcsError> {
    let mut world = build(options.entities)?;
    time(options.entities, options.iters, || {
        frames_parallel(&mut world, kind, FRAMES, pool, GRAIN)
    })
}

/// Prints the report tables.
///
/// # Errors
/// A refusal of the world or the scheduler.
pub fn report(options: &Options) -> Result<(), EcsError> {
    let cpus = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
    println!("# tools_ecs_bench (card 7-5)\n");
    println!(
        "- machine: {} {}, {cpus} logical CPUs; {} entities, {} timed iterations of {FRAMES} frames, median; one warm-up discarded",
        std::env::consts::OS,
        std::env::consts::ARCH,
        options.entities,
        options.iters
    );
    if cfg!(debug_assertions) {
        println!("- BUILD: DEBUG. The times below are meaningless; build with --release.");
    }
    println!("- wall times are report only; chunk {GRAIN} rows; ns per entity per frame\n");
    let threads = StdThreads::new();
    let pools: Vec<(u32, WorkStealingPool)> = WORKERS
        .iter()
        .filter_map(|n| Some((*n, WorkStealingPool::new(*n, &threads).ok()?)))
        .collect();
    print!("| workload | serial |");
    for (n, _) in &pools {
        print!(" pool {n} |");
    }
    println!();
    print!("|---|---:|");
    for _ in &pools {
        print!("---:|");
    }
    println!();
    for kind in Kind::ALL {
        print!("| {} | {:.2} |", kind.label(), serial_row(*options, kind)?);
        for (_, pool) in &pools {
            print!(" {:.2} |", pool_row(*options, kind, pool)?);
        }
        println!();
    }
    Ok(())
}
