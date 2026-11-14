//! Report of the ADR-0003 spike (card 7-2): markdown tables of the six workloads on the four
//! storages at three world sizes, the memory of each storage, and the allocation counts of the
//! steady state. Wall-clock, median of the timed runs on this machine; report only.
//!
//! `tools_ecs_spike [--reps 40] [--sizes 1000,10000,100000]`. Build with `--release`.

use base::alloc::{CountingAllocator, count_allocations};
use std::alloc::System;
use std::process::ExitCode;
use tools_ecs_spike::workloads::{Workload, mark_half, populate, run_once, time_workload};
use tools_ecs_spike::{ArchetypeRaw, ArchetypeSafe, Hybrid, SparseWorld, Store};

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const DEFAULT_REPS: usize = 40;
const DEFAULT_SIZES: [usize; 3] = [1_000, 10_000, 100_000];

struct Options {
    reps: usize,
    sizes: Vec<usize>,
}

fn number<T: std::str::FromStr>(text: &str, flag: &str) -> Result<T, String> {
    text.trim()
        .parse()
        .map_err(|_| format!("bad value for {flag}"))
}

fn apply(options: &mut Options, flag: &str, value: &str) -> Result<(), String> {
    match flag {
        "--reps" => options.reps = number(value, flag)?,
        "--sizes" => {
            options.sizes = value
                .split(',')
                .map(|part| number(part, flag))
                .collect::<Result<_, _>>()?;
        }
        other => return Err(format!("unknown option {other}")),
    }
    Ok(())
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        reps: DEFAULT_REPS,
        sizes: DEFAULT_SIZES.to_vec(),
    };
    for pair in args.chunks(2) {
        let [flag, value] = pair else {
            return Err("every option needs a value".to_string());
        };
        apply(&mut options, flag, value)?;
    }
    Ok(options)
}

/// A count as `f64` (the sizes of the report are far below 2^32).
fn count(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

fn cell<S: Store>(n: usize, workload: Workload, reps: usize) -> String {
    let (median, min) = time_workload::<S>(n, workload, reps);
    let per = count(workload.touched(n).max(1));
    format!("{:.1} ({:.1})", median / per, min / per)
}

fn timing_table(n: usize, reps: usize) {
    println!("### {n} entities: ns per entity touched, median (min) of {reps} runs\n");
    println!("| workload | archetype-safe | archetype-raw | sparse-set | hybrid |");
    println!("|---|---|---|---|---|");
    for workload in Workload::ALL {
        println!(
            "| {} | {} | {} | {} | {} |",
            workload.name(),
            cell::<ArchetypeSafe>(n, workload, reps),
            cell::<ArchetypeRaw>(n, workload, reps),
            cell::<SparseWorld>(n, workload, reps),
            cell::<Hybrid>(n, workload, reps),
        );
    }
    println!();
}

fn bytes_row<S: Store>(n: usize) -> String {
    let (world, _) = populate::<S>(n);
    format!("{:.1}", count(world.data_bytes()) / count(n))
}

fn memory_table(sizes: &[usize]) {
    println!("### Bytes per entity (component data and bookkeeping, by length)\n");
    println!("| entities | archetype-safe | archetype-raw | sparse-set | hybrid |");
    println!("|---|---|---|---|---|");
    for &n in sizes {
        println!(
            "| {n} | {} | {} | {} | {} |",
            bytes_row::<ArchetypeSafe>(n),
            bytes_row::<ArchetypeRaw>(n),
            bytes_row::<SparseWorld>(n),
            bytes_row::<Hybrid>(n),
        );
    }
    println!();
}

fn allocations<S: Store>(n: usize) -> String {
    let (mut world, ids) = populate::<S>(n);
    mark_half(&mut world, &ids);
    let mut scratch = Vec::with_capacity(n / 10 + 1);
    let mut out = Vec::new();
    for workload in Workload::ALL {
        run_once(&mut world, &ids, workload, &mut scratch);
        let (_, counts) = count_allocations(|| run_once(&mut world, &ids, workload, &mut scratch));
        out.push(counts.calls().to_string());
    }
    out.join(" / ")
}

fn allocation_table(n: usize) {
    println!("### Allocator calls in one steady-state run ({n} entities, after a warm-up run)\n");
    println!("Order: iterate 1, 2, 3, marker, churn, toggle.\n");
    println!("| storage | calls |");
    println!("|---|---|");
    println!("| archetype-safe | {} |", allocations::<ArchetypeSafe>(n));
    println!("| archetype-raw | {} |", allocations::<ArchetypeRaw>(n));
    println!("| sparse-set | {} |", allocations::<SparseWorld>(n));
    println!("| hybrid | {} |", allocations::<Hybrid>(n));
    println!();
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse(&args) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    println!("# tools_ecs_spike (card 7-2)\n");
    println!(
        "- machine: {} {}, {} logical CPUs",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().map_or(0, std::num::NonZero::get)
    );
    println!(
        "- build: {}",
        if cfg!(debug_assertions) {
            "DEBUG (times meaningless, build with --release)"
        } else {
            "release"
        }
    );
    println!(
        "- one thread; wall-clock only, no instruction counts (no valgrind on this machine)\n"
    );
    for &n in &options.sizes {
        timing_table(n, options.reps);
    }
    memory_table(&options.sizes);
    let steady = options.sizes.get(1).copied().unwrap_or(10_000);
    allocation_table(steady);
    ExitCode::SUCCESS
}
