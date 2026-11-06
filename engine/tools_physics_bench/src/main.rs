//! Dev-time benchmark of the physics crate (card 4-22). Prints the environment and markdown
//! tables: workload W1 (the pyramid, one thread), the work counters, the scaling scene on the
//! work-stealing pool at 1, 2, 4 and 6 workers, and the scheduler overhead. Report only: no
//! threshold is applied to a time, and the exit code is non-zero only when an argument is bad or
//! a measurement could not run.
//!
//! Run: `cargo run --release -p tools_physics_bench` (a full report takes several minutes).
//! Instruction counting helpers: `icount-workload`, `icount-parse`, `icount-spread` (see `USAGE`).
//! Not part of the engine.

use physics::d2::SolverExecution;
use std::process::ExitCode;
use tools_physics_bench::{
    BenchError, Command, GateFeasibility, Measured, Options, USAGE, WorkCounters, counters_header,
    counters_row, gate_feasibility, icount_workload, measure_w1, overhead_table, parse,
    parse_instructions, relative_spread, run_overhead, run_scaling, run_scene, scaling_table,
    stability_table, w1_header, w1_row,
};

fn environment(options: &Options) {
    let cpus = std::thread::available_parallelism().map_or(0, std::num::NonZero::get);
    println!("# tools_physics_bench (card 4-22)\n");
    println!(
        "- machine: {} {}, {cpus} logical CPUs (run with the power plan and load stated by the person who ran it)",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    if cfg!(debug_assertions) {
        println!("- BUILD: DEBUG. The times below are meaningless; build with --release.");
    } else {
        println!("- build: release");
    }
    if options.below_protocol() {
        println!(
            "- WARNING: fewer than 30 timed iterations asked for: below the measurement protocol."
        );
    }
    println!(
        "- solver: default 4 sub-steps, 1 relax pass; sleeping off for the timed rows (a sleeping pyramid stops costing); one warm-up pass discarded"
    );
    println!("- the wall times are report only; the work counters do not vary with the machine\n");
}

fn w1_section(options: &Options) -> Result<(), BenchError> {
    println!("## W1: pyramid, one thread\n");
    let off = measure_w1(
        options.rows,
        options.steps,
        options.iters,
        &SolverExecution::SERIAL,
        false,
    )?;
    let on = measure_w1(
        options.rows,
        options.steps,
        options.iters,
        &SolverExecution::SERIAL,
        true,
    )?;
    println!("{}", w1_header());
    println!(
        "{}",
        w1_row(
            "W1, sleeping off",
            options.rows,
            options.steps,
            options.iters,
            &off
        )
    );
    println!(
        "{}",
        w1_row(
            "W1, sleeping on",
            options.rows,
            options.steps,
            options.iters,
            &on
        )
    );
    println!();
    counters_section(&off, &on);
    Ok(())
}

/// Pyramid sizes of the stability sweep (rows): 55, 210, 465 and 990 boxes.
const SWEEP_ROWS: [u8; 4] = [10, 20, 30, 44];

fn stability_section(options: &Options) -> Result<(), BenchError> {
    println!(
        "## W1 stability by size (one run each, {} steps, sleeping off)
",
        options.steps
    );
    let mut runs = Vec::new();
    for rows in SWEEP_ROWS {
        runs.push((
            rows,
            run_scene(rows, options.steps, &SolverExecution::SERIAL, false)?,
        ));
    }
    println!("{}", stability_table(&runs));
    Ok(())
}

fn counters_section(off: &Measured, on: &Measured) {
    println!("## Work counters (totals over the run, deterministic)\n");
    println!("{}", counters_header());
    println!("{}", counters_row("W1, sleeping off", &off.run.counters));
    println!("{}", counters_row("W1, sleeping on", &on.run.counters));
    println!();
    if !(off.run.complete && on.run.complete) {
        println!("WARNING: the pair or manifold list overflowed, work was dropped.\n");
    }
}

fn scaling_section(options: &Options) -> Result<(), BenchError> {
    println!("## Scaling: the pyramid on the work-stealing pool\n");
    println!(
        "{} rows, {} steps, {} timed iterations per row; efficiency = time at 1 worker / (N x time at N workers), same solver path; chunk 32.\n",
        options.rows, options.scale_steps, options.scale_iters
    );
    let rows = run_scaling(options.rows, options.scale_steps, options.scale_iters)?;
    print!("{}", scaling_table(&rows, rows.first().map(|row| row.hash)));
    println!();
    Ok(())
}

fn overhead_section(options: &Options) -> Result<(), BenchError> {
    println!("## Scheduler overhead: one fork-join call\n");
    println!(
        "{} calls per repetition, {} timed repetitions; inline = no threads (the floor).\n",
        options.calls, options.reps
    );
    print!(
        "{}",
        overhead_table(&run_overhead(options.calls, options.reps)?)
    );
    Ok(())
}

fn report(options: &Options) -> Result<(), BenchError> {
    environment(options);
    w1_section(options)?;
    stability_section(options)?;
    scaling_section(options)?;
    overhead_section(options)
}

fn workload(rows: u8, steps: u32) -> Result<(), BenchError> {
    let (counters, hash) = icount_workload(rows, steps)?;
    let WorkCounters {
        steps,
        pairs,
        manifolds,
        constraints_solved,
        ..
    } = counters;
    println!(
        "icount-workload rows={rows} steps={steps} pairs={pairs} manifolds={manifolds} solved={constraints_solved} hash={hash:016x}"
    );
    Ok(())
}

fn parse_file(tool: tools_physics_bench::Tool, path: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    let count = parse_instructions(tool, &text).map_err(|error| format!("{path}: {error:?}"))?;
    println!("{count}");
    Ok(())
}

fn spread(counts: &[u64]) {
    match relative_spread(counts) {
        Some(spread) => {
            let verdict = match gate_feasibility(spread) {
                GateFeasibility::Feasible => {
                    "feasible (noise <= 0.5%: warn 5% and fail 10% are readable)"
                }
                GateFeasibility::Marginal => {
                    "marginal (noise <= 5%: only the 10% fail band is readable)"
                }
                GateFeasibility::Infeasible => "infeasible (noise > 5%)",
            };
            println!(
                "runs={} spread={:.4}% gate: {verdict}",
                counts.len(),
                spread * 100.0
            );
        }
        None => println!("no spread: need two counts and a smallest above zero"),
    }
}

fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Report(options) => report(&options).map_err(|error| error.to_string()),
        Command::IcountWorkload { rows, steps } => {
            workload(rows, steps).map_err(|error| error.to_string())
        }
        Command::IcountParse { tool, path } => parse_file(tool, &path),
        Command::IcountSpread(counts) => {
            spread(&counts);
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = parse(&args).and_then(run);
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}
