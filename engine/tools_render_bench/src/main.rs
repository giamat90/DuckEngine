//! Dev-time CPU benchmark of one 2D frame (card 3-13). Prints the environment, the size of the
//! frame and a markdown table: nanoseconds per frame of each stage (minimum, median and 90th
//! percentile over the repetitions of one process run) and the allocator calls each stage makes
//! once warm. Report only: no threshold is applied here, and GPU time is not measured (the device
//! is the Null device). Each stage is timed on its own, so the stages do not add up to the composed
//! cpu frame row.
//!
//! `tools_render_bench [--reps N] [--frames N]`. Build with `--release`. Not part of the engine.
//! The exit code is non-zero when an argument is bad or a stage failed.

use base::alloc::{CountingAllocator, count_allocations};
use std::alloc::System;
use std::hint::black_box;
use std::process::Command;
use std::time::Instant;
use tools_render_bench::{FrameScene, Summary, summarize};

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const DEFAULT_REPS: usize = 15;
const DEFAULT_FRAMES: usize = 200;
const NANOS_PER_SECOND: f64 = 1e9;
/// Largest `--frames` accepted: keeps the frame count exact as a float and a run short.
const MAX_FRAMES: usize = 1_000_000;

/// Repetitions and frames per repetition.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    reps: usize,
    frames: usize,
}

/// One line of the report: a stage, its timing (`None` when the stage failed) and its allocations.
#[derive(Debug, PartialEq)]
struct Row {
    name: &'static str,
    times: Option<Summary>,
    calls: u64,
}

/// A count as `f64`; exact because the counts passed are at most `MAX_FRAMES`.
fn as_f64(count: usize) -> f64 {
    f64::from(u32::try_from(count).unwrap_or(u32::MAX))
}

fn positive(flag: &str, value: &str, max: usize) -> Result<usize, String> {
    match value.parse::<usize>() {
        Ok(n) if n > max => Err(format!("{flag} is at most {max}, got {value}")),
        Ok(n) if n > 0 => Ok(n),
        _ => Err(format!("{flag} needs a positive number, got {value}")),
    }
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        reps: DEFAULT_REPS,
        frames: DEFAULT_FRAMES,
    };
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or("a flag needs a value")?;
        match flag.as_str() {
            "--reps" => options.reps = positive(flag, value, MAX_FRAMES)?,
            "--frames" => options.frames = positive(flag, value, MAX_FRAMES)?,
            _ => return Err("usage: tools_render_bench [--reps N] [--frames N]".to_owned()),
        }
    }
    Ok(options)
}

/// Nanoseconds per frame for `seconds` spent on `frames` frames.
fn per_frame_ns(seconds: f64, frames: usize) -> f64 {
    seconds * NANOS_PER_SECOND / as_f64(frames)
}

/// Calls `stage` `frames` times; false as soon as one call fails.
fn run_frames(frames: usize, stage: &mut impl FnMut() -> bool) -> bool {
    (0..frames).all(|_| black_box(stage()))
}

/// Nanoseconds per call of `stage` over `reps` repetitions of `frames` calls each (one extra
/// repetition first as warm-up, not sampled). `None` when the stage fails.
fn time_stage(options: &Options, mut stage: impl FnMut() -> bool) -> Option<Summary> {
    let mut samples = Vec::with_capacity(options.reps);
    for rep in 0..=options.reps {
        let start = Instant::now();
        if !run_frames(options.frames, &mut stage) {
            return None;
        }
        if rep > 0 {
            samples.push(per_frame_ns(start.elapsed().as_secs_f64(), options.frames));
        }
    }
    summarize(&mut samples)
}

/// Allocator calls (allocations plus frees) one warm call of `stage` makes.
fn alloc_calls(mut stage: impl FnMut() -> bool) -> u64 {
    stage();
    count_allocations(stage).1.calls()
}

fn measure(options: &Options, name: &'static str, stage: &dyn Fn() -> bool) -> Row {
    Row {
        name,
        times: time_stage(options, stage),
        calls: alloc_calls(stage),
    }
}

/// True when any stage failed.
fn any_failed(rows: &[Row]) -> bool {
    rows.iter().any(|row| row.times.is_none())
}

fn format_row(row: &Row) -> String {
    match row.times {
        Some(t) => format!(
            "| {} | {:.0} | {:.0} | {:.0} | {} |",
            row.name, t.min, t.median, t.p90, row.calls
        ),
        None => format!(
            "| {} | FAILED | FAILED | FAILED | {} |",
            row.name, row.calls
        ),
    }
}

/// The build profile, with a loud warning when assertions (so no optimisation) are on.
fn profile_text(debug: bool) -> &'static str {
    if debug {
        "debug (WARNING: built without optimisation, the numbers mean nothing; use --release)"
    } else {
        "release"
    }
}

/// The compiler version the PATH offers, as a hint of what built this binary.
fn rustc_version() -> String {
    Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map_or_else(|| "unknown".to_owned(), |text| text.trim().to_owned())
}

/// The model name of the first CPU, from the environment (Windows) or `/proc/cpuinfo` (Linux).
fn cpu_name() -> String {
    if let Ok(name) = std::env::var("PROCESSOR_IDENTIFIER") {
        return name;
    }
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|info| {
            let line = info.lines().find(|line| line.starts_with("model name"))?;
            line.split_once(':').map(|(_, name)| name.trim().to_owned())
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

fn print_environment(scene: &FrameScene) {
    println!(
        "ENV|target|{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!("ENV|profile|{}", profile_text(cfg!(debug_assertions)));
    println!("ENV|rustc|{}", rustc_version());
    println!("ENV|cpu|{}", cpu_name());
    println!(
        "ENV|scene|{} sprites, {} batches, {} stream bytes, {} vertex bytes",
        scene.sprite_count(),
        scene.batch_count(),
        scene.stream_bytes(),
        scene.vertex_upload_bytes()
    );
}

fn report(options: &Options) -> Result<(), String> {
    let mut scene = FrameScene::new()?;
    scene.cpu_frame()?;
    print_environment(&scene);
    let s = std::cell::RefCell::new(scene);
    let rows = [
        measure(options, "prepare (sort, stage, batch)", &|| {
            s.borrow_mut().prepare().is_ok()
        }),
        measure(options, "encode (vertex bytes)", &|| {
            s.borrow_mut().encode().is_ok()
        }),
        measure(options, "record (graph + stream)", &|| {
            s.borrow_mut().record().is_ok()
        }),
        measure(options, "cpu frame (composed, not the sum)", &|| {
            s.borrow_mut().cpu_frame().is_ok()
        }),
        measure(options, "submit (Null device)", &|| {
            s.borrow_mut().submit().is_ok()
        }),
    ];
    println!("| stage | min ns | median ns | p90 ns | allocator calls |\n|---|---|---|---|---|");
    for row in &rows {
        println!("{}", format_row(row));
    }
    if any_failed(&rows) {
        return Err("a stage failed".to_owned());
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = parse(&args).and_then(|options| report(&options));
    if let Err(message) = result {
        eprintln!("{message}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn summary(value: f64) -> Summary {
        Summary {
            min: value,
            median: value,
            p90: value,
        }
    }

    #[test]
    fn no_arguments_give_the_defaults() {
        let options = parse(&[]);
        assert_eq!(
            options,
            Ok(Options {
                reps: DEFAULT_REPS,
                frames: DEFAULT_FRAMES
            })
        );
    }

    #[test]
    fn reps_and_frames_take_positive_numbers() {
        let options = parse(&args(&["--reps", "3", "--frames", "7"]));
        assert_eq!(options, Ok(Options { reps: 3, frames: 7 }));
    }

    #[test]
    fn zero_unknown_flags_and_missing_values_are_refused() {
        assert!(parse(&args(&["--reps", "0"])).is_err());
        assert!(parse(&args(&["--frames", "x"])).is_err());
        assert!(parse(&args(&["--bogus", "1"])).is_err());
        assert!(parse(&args(&["--reps"])).is_err());
    }

    #[test]
    fn frames_above_the_limit_are_refused_and_the_limit_is_accepted() {
        let over = (MAX_FRAMES + 1).to_string();
        assert!(parse(&args(&["--frames", &over])).is_err());
        let at = MAX_FRAMES.to_string();
        assert_eq!(
            parse(&args(&["--frames", &at])).map(|o| o.frames),
            Ok(MAX_FRAMES)
        );
    }

    #[test]
    fn the_per_frame_time_is_nanoseconds_divided_by_the_frames() {
        assert_eq!(as_f64(250).to_bits(), 250.0_f64.to_bits());
        assert_eq!(per_frame_ns(0.5, 250).to_bits(), 2.0e6_f64.to_bits());
    }

    #[test]
    fn the_warm_up_repetition_runs_but_is_not_sampled() {
        let options = Options { reps: 3, frames: 5 };
        let mut calls = 0_usize;
        let times = time_stage(&options, || {
            calls += 1;
            true
        });
        assert!(times.is_some());
        assert_eq!(calls, (3 + 1) * 5);
    }

    #[test]
    fn a_stage_that_fails_is_reported_as_none() {
        let options = Options { reps: 2, frames: 2 };
        assert_eq!(time_stage(&options, || false), None);
    }

    #[test]
    fn a_working_stage_has_ordered_positive_times_and_counts_its_allocations() {
        let options = Options { reps: 4, frames: 2 };
        let times = time_stage(&options, || black_box(vec![1_u8; 64]).len() == 64);
        assert!(times.is_some_and(|t| t.min > 0.0 && t.min <= t.median && t.median <= t.p90));
        assert_eq!(alloc_calls(|| black_box(vec![1_u8; 64]).len() == 64), 2);
        assert_eq!(alloc_calls(|| black_box(7_u8) == 7), 0);
    }

    #[test]
    fn a_failed_row_makes_the_run_fail_and_prints_failed() {
        let ok = Row {
            name: "a",
            times: Some(summary(1.0)),
            calls: 0,
        };
        let bad = Row {
            name: "b",
            times: None,
            calls: 3,
        };
        assert!(!any_failed(&[]));
        assert!(!any_failed(std::slice::from_ref(&ok)));
        assert!(any_failed(&[ok, bad]));
        let failed = Row {
            name: "b",
            times: None,
            calls: 3,
        };
        assert_eq!(format_row(&failed), "| b | FAILED | FAILED | FAILED | 3 |");
    }

    #[test]
    fn a_row_shows_min_median_p90_and_calls() {
        let row = Row {
            name: "x",
            times: Some(Summary {
                min: 1.0,
                median: 2.0,
                p90: 3.0,
            }),
            calls: 4,
        };
        assert_eq!(format_row(&row), "| x | 1 | 2 | 3 | 4 |");
    }

    #[test]
    fn a_debug_build_is_flagged_loudly() {
        assert!(profile_text(true).contains("WARNING"));
        assert_eq!(profile_text(false), "release");
    }

    #[test]
    fn the_environment_helpers_return_text() {
        assert!(!cpu_name().is_empty());
        assert!(!rustc_version().is_empty());
    }
}
