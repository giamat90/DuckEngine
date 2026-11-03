//! Throw-away harness of the ADR-0004 spike (card 4-5). Prints one tab-separated line per
//! (workload, candidate, worker count) cell: median wall time of a run on this machine, the
//! minimum and maximum, and whether the output hash equals the serial reference (D1).
//!
//! `tools_scheduler_spike bench [--workers 1,2,4,6,20] [--reps 15] [--spin 0] [--only w1-tiny,...]
//! [--kinds a-shared-queue,...]`; `tools_scheduler_spike idle <kind> <workers> <spin> <seconds>`
//! builds a pool, makes one call and sleeps (to read the CPU time of the idle pool from outside).
//! Build with `--release`.

use jobs::scheduler::IScheduler;
use platform::threads::{IThreads, StdThreads};
use std::process::ExitCode;
use std::time::{Duration, Instant};
use tools_scheduler_spike::workloads::{Bench, Sizes, Workload};
use tools_scheduler_spike::{Kind, build};

const DEFAULT_REPS: usize = 15;
const WARM_UP_RUNS: usize = 2;

struct Options {
    workers: Vec<u32>,
    reps: usize,
    spin: u32,
    workloads: Vec<Workload>,
    kinds: Vec<Kind>,
}

fn list<T>(text: &str, parse: impl Fn(&str) -> Option<T>) -> Option<Vec<T>> {
    text.split(',').map(|part| parse(part.trim())).collect()
}

fn parse_options(args: &[String], parallelism: u32) -> Result<Options, String> {
    let mut options = Options {
        workers: vec![1, 2, 4, 6],
        reps: DEFAULT_REPS,
        spin: 0,
        workloads: Workload::ALL.to_vec(),
        kinds: Kind::ALL.to_vec(),
    };
    if !options.workers.contains(&parallelism) {
        options.workers.push(parallelism);
    }
    let mut pairs = args.chunks(2);
    while let Some([flag, value]) = pairs.next() {
        let bad = || format!("bad value {value} for {flag}");
        match flag.as_str() {
            "--workers" => options.workers = list(value, |t| t.parse().ok()).ok_or_else(bad)?,
            "--reps" => options.reps = value.parse().map_err(|_| bad())?,
            "--spin" => options.spin = value.parse().map_err(|_| bad())?,
            "--only" => options.workloads = list(value, Workload::parse).ok_or_else(bad)?,
            "--kinds" => options.kinds = list(value, Kind::parse).ok_or_else(bad)?,
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    Ok(options)
}

fn median(sorted: &[Duration]) -> Duration {
    sorted.get(sorted.len() / 2).copied().unwrap_or_default()
}

/// One cell: warm-up, then `reps` timed runs. Returns the sorted durations and how many runs
/// gave a hash other than `reference`.
fn measure(
    bench: &Bench,
    scheduler: &dyn IScheduler,
    reps: usize,
    reference: u64,
) -> Result<(Vec<Duration>, usize), String> {
    let mut times = Vec::with_capacity(reps);
    let mut mismatches = 0;
    for run in 0..WARM_UP_RUNS + reps {
        let start = Instant::now();
        let hash = bench.run(scheduler).map_err(|error| error.to_string())?;
        let elapsed = start.elapsed();
        mismatches += usize::from(hash != reference);
        if run >= WARM_UP_RUNS {
            times.push(elapsed);
        }
    }
    times.sort_unstable();
    Ok((times, mismatches))
}

/// One cell of the report: builds the scheduler, measures, prints the line; returns whether
/// any run gave a hash other than the reference.
fn run_cell(
    bench: &Bench,
    cell: (Workload, Kind, u32),
    options: &Options,
    reference: u64,
) -> Result<bool, String> {
    let (workload, kind, workers) = cell;
    let scheduler = build(kind, workers, options.spin, &StdThreads::new())
        .map_err(|error| format!("{}: {error}", kind.name()))?;
    let (times, mismatches) = measure(bench, &*scheduler, options.reps, reference)?;
    let units = workload.units_per_run(&Sizes::FULL);
    println!(
        "cell	{}	{}	{workers}	{}	{}	{}	{}	{units}	{reference:016x}	{mismatches}",
        workload.name(),
        kind.name(),
        options.spin,
        median(&times).as_nanos(),
        times.first().map_or(0, Duration::as_nanos),
        times.last().map_or(0, Duration::as_nanos),
    );
    Ok(mismatches > 0)
}

fn run_workload(workload: Workload, options: &Options) -> Result<usize, String> {
    let bench = Bench::new(workload, Sizes::FULL);
    let reference = bench
        .run(&jobs::inline::InlineScheduler)
        .map_err(|error| error.to_string())?;
    let cells: Vec<_> = options
        .kinds
        .iter()
        .flat_map(|kind| options.workers.iter().map(move |workers| (*kind, *workers)))
        .filter(|(kind, workers)| *kind != Kind::Inline || *workers == 1)
        .collect();
    let mut bad_cells = 0;
    for (kind, workers) in cells {
        let bad = run_cell(&bench, (workload, kind, workers), options, reference)?;
        bad_cells += usize::from(bad);
    }
    Ok(bad_cells)
}

fn bench(args: &[String]) -> Result<bool, String> {
    let threads = StdThreads::new();
    let options = parse_options(args, threads.parallelism())?;
    println!(
        "# machine parallelism {}, reps {}, spin {}, wall time",
        threads.parallelism(),
        options.reps,
        options.spin
    );
    println!(
        "# cell\tworkload\tcandidate\tworkers\tspin\tmedian_ns\tmin_ns\tmax_ns\tunits\tref_hash\tbad_runs"
    );
    let mut bad_cells = 0;
    for workload in &options.workloads {
        bad_cells += run_workload(*workload, &options)?;
    }
    println!("d1\tcells_with_a_different_hash\t{bad_cells}");
    Ok(bad_cells == 0)
}

fn idle(args: &[String]) -> Result<bool, String> {
    let [kind, workers, spin, seconds] = args else {
        return Err("idle <kind> <workers> <spin> <seconds>".to_owned());
    };
    let bad = |what: &str| format!("bad {what}");
    let kind = Kind::parse(kind).ok_or_else(|| bad("kind"))?;
    let workers = workers.parse().map_err(|_| bad("workers"))?;
    let spin = spin.parse().map_err(|_| bad("spin"))?;
    let seconds: u64 = seconds.parse().map_err(|_| bad("seconds"))?;
    let scheduler =
        build(kind, workers, spin, &StdThreads::new()).map_err(|error| error.to_string())?;
    Bench::new(Workload::Dispatch, Sizes::SMALL)
        .run(&*scheduler)
        .map_err(|error| error.to_string())?;
    std::thread::sleep(Duration::from_secs(seconds));
    Ok(true)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((mode, rest)) if mode == "idle" => idle(rest),
        Some((mode, rest)) if mode == "bench" => bench(rest),
        None => bench(&[]),
        Some(_) => {
            Err("usage: bench [options] | idle <kind> <workers> <spin> <seconds>".to_owned())
        }
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(2),
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}
