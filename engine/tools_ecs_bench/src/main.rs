//! `tools_ecs_bench report` prints the wall-time tables; `tools_ecs_bench icount-workload` runs the
//! deterministic serial workload (for `valgrind --tool=callgrind`) and prints its checksum.
//!
//! Run: `cargo run --release -p tools_ecs_bench -- report`. Not part of the engine.

use std::process::ExitCode;
use tools_ecs_bench::report::report;
use tools_ecs_bench::{Command, Kind, USAGE, build, parse, run_serial};

fn icount(entities: u32, frames: u32) -> Result<(), String> {
    let mut world = build(entities).map_err(|e| e.to_string())?;
    let mut checksum = 0_u64;
    for kind in Kind::ALL {
        let part = run_serial(&mut world, kind, frames).map_err(|e| e.to_string())?;
        checksum = checksum.wrapping_mul(31).wrapping_add(part);
    }
    println!("icount-workload entities={entities} frames={frames} checksum={checksum:016x}");
    Ok(())
}

fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Report(options) => report(&options).map_err(|e| e.to_string()),
        Command::IcountWorkload { entities, frames } => icount(entities, frames),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args).and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}
