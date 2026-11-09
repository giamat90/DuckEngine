//! Dev-time CPU benchmark of the audio mixer by voice count (card 5-8). Prints the environment and
//! a markdown table: nanoseconds per 256-frame stereo block (minimum, median and 90th percentile
//! over the repetitions of one process run), the median as a share of one core (the block lasts
//! 5.33 ms at 48 kHz) and the allocator calls one warm block makes. Report only: no threshold is
//! applied here; registering a gate is a governance step. Latency is not measured.
//!
//! `tools_audio_bench [--reps N] [--blocks N]`. Build with `--release`. Not part of the engine.
//! `tools_audio_bench icount-workload [--voices N] [--blocks N]` renders without timing for an
//! instruction counter and prints one line with a checksum (card 5-15).
//! The exit code is non-zero when an argument is bad or a scene cannot be built.

use base::alloc::{CountingAllocator, count_allocations};
use std::alloc::System;
use std::hint::black_box;
use std::process::Command;
use std::time::Instant;
use tools_audio_bench::{
    BLOCK_FRAMES, SAMPLE_RATE, Scene, Summary, block_percent, icount_workload, summarize,
};

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const VOICE_COUNTS: [u32; 3] = [64, 256, 1024];
const DEFAULT_REPS: usize = 15;
const DEFAULT_BLOCKS: usize = 500;
const NANOS_PER_SECOND: f64 = 1e9;
/// Largest count accepted: keeps the block count exact as a float and a run short.
const MAX_COUNT: usize = 1_000_000;

/// Repetitions and blocks per repetition.
#[derive(Debug, PartialEq, Eq)]
struct Options {
    reps: usize,
    blocks: usize,
}

fn positive(flag: &str, value: &str) -> Result<usize, String> {
    match value.parse::<usize>() {
        Ok(n) if n > MAX_COUNT => Err(format!("{flag} is at most {MAX_COUNT}, got {value}")),
        Ok(n) if n > 0 => Ok(n),
        _ => Err(format!("{flag} needs a positive number, got {value}")),
    }
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        reps: DEFAULT_REPS,
        blocks: DEFAULT_BLOCKS,
    };
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or("a flag needs a value")?;
        match flag.as_str() {
            "--reps" => options.reps = positive(flag, value)?,
            "--blocks" => options.blocks = positive(flag, value)?,
            _ => return Err("usage: tools_audio_bench [--reps N] [--blocks N]".to_owned()),
        }
    }
    Ok(options)
}

/// A count as `f64`; exact because the counts passed are at most `MAX_COUNT`.
fn as_f64(count: usize) -> f64 {
    f64::from(u32::try_from(count).unwrap_or(u32::MAX))
}

/// Nanoseconds per block for `seconds` spent on `blocks` blocks.
fn per_block_ns(seconds: f64, blocks: usize) -> f64 {
    seconds * NANOS_PER_SECOND / as_f64(blocks)
}

/// Nanoseconds per call of `run` over `reps` repetitions of `blocks` calls each (one extra
/// repetition first as warm-up, not sampled).
fn time_calls(options: &Options, mut run: impl FnMut()) -> Option<Summary> {
    let mut samples = Vec::with_capacity(options.reps);
    for rep in 0..=options.reps {
        let start = Instant::now();
        for _ in 0..options.blocks {
            run();
        }
        if rep > 0 {
            samples.push(per_block_ns(start.elapsed().as_secs_f64(), options.blocks));
        }
    }
    summarize(&mut samples)
}

/// Nanoseconds per block of the scene.
fn time_blocks(scene: &mut Scene, options: &Options) -> Option<Summary> {
    time_calls(options, || {
        scene.render_block();
        black_box(scene.output());
    })
}

/// Allocator calls (allocations plus frees) one warm call of `run` makes.
fn calls_of(mut run: impl FnMut()) -> u64 {
    run();
    count_allocations(run).1.calls()
}

/// Allocator calls one warm block makes.
fn alloc_calls(scene: &mut Scene) -> u64 {
    calls_of(|| scene.render_block())
}

/// One report line.
fn format_row(voices: u32, times: &Summary, calls: u64) -> String {
    format!(
        "| {voices} | {:.0} | {:.0} | {:.0} | {:.2} | {calls} |",
        times.min,
        times.median,
        times.p90,
        block_percent(times.median, BLOCK_FRAMES, SAMPLE_RATE)
    )
}

fn profile_text(debug: bool) -> &'static str {
    if debug {
        "debug (WARNING: built without optimisation, the numbers mean nothing; use --release)"
    } else {
        "release"
    }
}

fn rustc_version() -> String {
    Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map_or_else(|| "unknown".to_owned(), |text| text.trim().to_owned())
}

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

fn report(options: &Options) -> Result<(), String> {
    println!(
        "ENV|target|{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!("ENV|profile|{}", profile_text(cfg!(debug_assertions)));
    println!("ENV|rustc|{}", rustc_version());
    println!("ENV|cpu|{}", cpu_name());
    println!("ENV|stream|stereo, {SAMPLE_RATE} Hz, {BLOCK_FRAMES}-frame blocks, looping voices");
    println!(
        "| voices | min ns/block | median ns/block | p90 ns/block | median % of one core | allocator calls/block |\n|---|---|---|---|---|---|"
    );
    for voices in VOICE_COUNTS {
        let mut scene = Scene::new(voices).map_err(|e| format!("{voices} voices: {e}"))?;
        let times = time_blocks(&mut scene, options).ok_or("no samples")?;
        println!("{}", format_row(voices, &times, alloc_calls(&mut scene)));
    }
    Ok(())
}

/// Voices of the instruction-count workload: the largest scene of the report.
const WORKLOAD_VOICES: u32 = 1024;
/// Blocks of the instruction-count workload: enough to dominate the start-up cost.
const WORKLOAD_BLOCKS: u32 = 200;

/// `icount-workload [--voices N] [--blocks N]`: the sizes of the workload.
fn parse_workload(args: &[String]) -> Result<(u32, u32), String> {
    let (mut voices, mut blocks) = (WORKLOAD_VOICES, WORKLOAD_BLOCKS);
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or("a flag needs a value")?;
        let number = positive(flag, value)?;
        // Cannot fail (the cap of `positive` is far below `u32::MAX`); kept so no cast is needed.
        let number = u32::try_from(number).map_err(|_| format!("{flag} is too large"))?;
        match flag.as_str() {
            "--voices" => voices = number,
            "--blocks" => blocks = number,
            _ => {
                return Err(
                    "usage: tools_audio_bench icount-workload [--voices N] [--blocks N]".to_owned(),
                );
            }
        }
    }
    Ok((voices, blocks))
}

/// The one line `icount-workload` prints.
fn workload_line(args: &[String]) -> Result<String, String> {
    let (voices, blocks) = parse_workload(args)?;
    icount_workload(voices, blocks).map_err(|e| e.to_string())
}

fn run(args: &[String]) -> Result<(), String> {
    match args.split_first() {
        Some((word, rest)) if word == "icount-workload" => {
            println!("{}", workload_line(rest)?);
            Ok(())
        }
        _ => parse(args).and_then(|options| report(&options)),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(message) = run(&args) {
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

    #[test]
    fn defaults_and_explicit_values() {
        assert_eq!(
            parse(&[]),
            Ok(Options {
                reps: DEFAULT_REPS,
                blocks: DEFAULT_BLOCKS
            })
        );
        assert_eq!(
            parse(&args(&["--reps", "3", "--blocks", "7"])),
            Ok(Options { reps: 3, blocks: 7 })
        );
    }

    #[test]
    fn zero_unknown_flags_missing_values_and_huge_counts_are_refused() {
        assert!(parse(&args(&["--reps", "0"])).is_err());
        assert!(parse(&args(&["--blocks", "x"])).is_err());
        assert!(parse(&args(&["--bogus", "1"])).is_err());
        assert!(parse(&args(&["--reps"])).is_err());
        let over = (MAX_COUNT + 1).to_string();
        assert!(parse(&args(&["--blocks", &over])).is_err());
        let at = MAX_COUNT.to_string();
        assert_eq!(
            parse(&args(&["--blocks", &at])).map(|o| o.blocks),
            Ok(MAX_COUNT)
        );
    }

    #[test]
    fn the_workload_has_defaults_and_takes_voices_and_blocks() {
        assert_eq!(parse_workload(&[]), Ok((WORKLOAD_VOICES, WORKLOAD_BLOCKS)));
        assert_eq!(
            parse_workload(&args(&["--voices", "8", "--blocks", "3"])),
            Ok((8, 3))
        );
    }

    #[test]
    fn the_workload_refuses_zero_unknown_flags_and_huge_counts() {
        assert!(parse_workload(&args(&["--voices", "0"])).is_err());
        assert!(parse_workload(&args(&["--blocks", "x"])).is_err());
        assert!(parse_workload(&args(&["--reps", "3"])).is_err());
        assert!(parse_workload(&args(&["--voices"])).is_err());
        let over = (MAX_COUNT + 1).to_string();
        assert!(parse_workload(&args(&["--voices", &over])).is_err());
    }

    #[test]
    fn the_workload_accepts_the_cap_and_the_last_duplicate_flag_wins() {
        let at = MAX_COUNT.to_string();
        assert_eq!(
            parse_workload(&args(&["--blocks", &at])).map(|sizes| sizes.1),
            u32::try_from(MAX_COUNT).map_err(|e| e.to_string())
        );
        assert_eq!(
            parse_workload(&args(&["--voices", "4", "--voices", "9"])),
            Ok((9, WORKLOAD_BLOCKS))
        );
    }

    #[test]
    fn the_workload_line_is_the_library_line_for_the_parsed_sizes() {
        assert_eq!(
            workload_line(&args(&["--voices", "4", "--blocks", "2"])),
            icount_workload(4, 2).map_err(|e| e.to_string())
        );
        assert!(workload_line(&args(&["--voices", "0"])).is_err());
    }

    #[test]
    fn a_scene_renders_with_samples_and_no_allocation() {
        let Ok(mut scene) = Scene::new(8) else {
            panic!("a scene of 8 voices is built");
        };
        let options = Options { reps: 3, blocks: 4 };
        let times = time_blocks(&mut scene, &options);
        assert!(times.is_some_and(|t| t.min > 0.0 && t.min <= t.median && t.median <= t.p90));
        assert_eq!(alloc_calls(&mut scene), 0);
    }

    #[test]
    fn a_row_shows_the_voices_the_times_the_share_and_the_calls() {
        let t = Summary {
            min: 1.0,
            median: 2.0,
            p90: 3.0,
        };
        assert_eq!(format_row(64, &t, 5), "| 64 | 1 | 2 | 3 | 0.00 | 5 |");
    }

    #[test]
    fn the_time_per_block_is_nanoseconds_divided_by_the_blocks() {
        assert_eq!(per_block_ns(0.5, 250).to_bits(), 2.0e6_f64.to_bits());
    }

    #[test]
    fn the_warm_up_repetition_runs_but_is_not_sampled() {
        let options = Options { reps: 3, blocks: 5 };
        let mut calls = 0_usize;
        let times = time_calls(&options, || calls += 1);
        assert!(times.is_some());
        assert_eq!(calls, (3 + 1) * 5);
    }

    #[test]
    fn allocator_calls_are_counted_after_one_warm_call() {
        assert_eq!(calls_of(|| drop(black_box(vec![1_u8; 64]))), 2);
        assert_eq!(
            calls_of(|| {
                black_box(7_u8);
            }),
            0
        );
    }

    #[test]
    fn the_compiler_version_is_the_compilers_own_text() {
        assert!(rustc_version().starts_with("rustc"));
    }

    #[test]
    fn the_counts_and_helpers_are_exact() {
        assert_eq!(as_f64(250).to_bits(), 250.0_f64.to_bits());
        assert!(profile_text(true).contains("WARNING"));
        assert_eq!(profile_text(false), "release");
        assert!(!cpu_name().is_empty() && !rustc_version().is_empty());
    }
}
