//! Dev-time benchmark of the global allocator (card 1-27, evidence for ADR-0022).
//!
//! `alloc-bench [--reps N]` runs the workloads with the allocator this binary was built with
//! (the default one, or mimalloc with `--features mimalloc-allocator`) and prints `ENV` and
//! `RESULT` lines. `alloc-bench --table DEFAULT.txt CANDIDATE.txt` joins two such outputs into a
//! markdown table. Not part of the engine; no engine crate depends on it.

mod report;
mod scenarios;
mod stats;

use report::{Row, compare, format_row, parse_row};
use stats::summarize;
use std::hint::black_box;
use std::time::Instant;

#[cfg(feature = "mimalloc-allocator")]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

const DEFAULT_REPS: usize = 9;

#[derive(Debug, PartialEq)]
enum Command {
    Run { reps: usize },
    Table { default: String, candidate: String },
}

fn parse_args(args: &[String]) -> Result<Command, String> {
    match args {
        [] => Ok(Command::Run { reps: DEFAULT_REPS }),
        [flag, n] if flag == "--reps" => match n.parse::<usize>() {
            Ok(reps) if reps > 0 => Ok(Command::Run { reps }),
            _ => Err(format!("--reps needs a positive number, got {n}")),
        },
        [flag, default, candidate] if flag == "--table" => Ok(Command::Table {
            default: default.clone(),
            candidate: candidate.clone(),
        }),
        _ => Err("usage: alloc-bench [--reps N] | --table DEFAULT.txt CANDIDATE.txt".to_owned()),
    }
}

/// Times `work` (one warm-up call, then `reps` timed calls) and returns the row for `units` of
/// work per call, in nanoseconds per unit.
fn measure(case: &str, param: &str, reps: usize, units: f64, work: &dyn Fn() -> u64) -> Row {
    black_box(work());
    let samples: Vec<f64> = (0..reps)
        .map(|_| {
            let start = Instant::now();
            black_box(work());
            start.elapsed().as_secs_f64() * 1e9 / units
        })
        .collect();
    let summary = summarize(&samples).unwrap_or(stats::Summary {
        median: f64::NAN,
        min: f64::NAN,
        max: f64::NAN,
    });
    Row {
        case: case.to_owned(),
        param: param.to_owned(),
        summary,
    }
}

/// A count of work units as the divisor of a timing.
fn units(count: usize) -> f64 {
    f64::from(u32::try_from(count).unwrap_or(u32::MAX))
}

fn allocator_name() -> &'static str {
    if cfg!(feature = "mimalloc-allocator") {
        "mimalloc 0.1.52"
    } else {
        "default (std System)"
    }
}

fn print_environment() {
    let cpus = std::thread::available_parallelism().map_or(0, std::num::NonZeroUsize::get);
    println!("ENV|allocator|{}", allocator_name());
    println!(
        "ENV|target|{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    println!("ENV|logical cpus|{cpus}");
}

fn run(reps: usize) {
    print_environment();
    for row in single_thread_rows(reps)
        .into_iter()
        .chain(multi_thread_rows(reps))
    {
        println!("{}", format_row(&row));
    }
}

fn single_thread_rows(reps: usize) -> Vec<Row> {
    vec![
        measure("churn 16 B", "x 16384", reps, 16384.0, &|| {
            scenarios::churn::<16>(16384)
        }),
        measure("churn 64 B", "x 4096", reps, 4096.0, &|| {
            scenarios::churn::<64>(4096)
        }),
        measure("churn 64 B", "x 262144", reps, 262_144.0, &|| {
            scenarios::churn::<64>(262_144)
        }),
        measure("churn 512 B", "x 16384", reps, 16384.0, &|| {
            scenarios::churn::<512>(16384)
        }),
        measure("lists", "2000 lists of 1..8 u32", reps, 2000.0, &|| {
            scenarios::lists(2000, 7)
        }),
        measure("tree build+free", "131071 nodes", reps, 131_071.0, &|| {
            scenarios::tree(17)
        }),
        measure("Vec growth", "1M u32 pushes", reps, 1_000_000.0, &|| {
            scenarios::grow_vec(1_000_000)
        }),
        measure(
            "map churn",
            "100000 live, 100000 ops",
            reps,
            100_000.0,
            &|| scenarios::map_churn(100_000, 100_000),
        ),
    ]
}

fn multi_thread_rows(reps: usize) -> Vec<Row> {
    const PAIRS: usize = 1_000_000;
    const BATCHES: usize = 200;
    const BATCH: usize = 5000;
    let mut rows = Vec::new();
    for threads in [1, 2, 4, 8] {
        rows.push(measure(
            "threads, own free",
            &format!("{threads} threads"),
            reps,
            units(PAIRS),
            &|| scenarios::threads_same(threads, PAIRS),
        ));
    }
    for threads in [4, 8] {
        rows.push(measure(
            "threads, freed by another",
            &format!("{threads} threads, batches of {BATCH}"),
            reps,
            units(BATCHES * BATCH),
            &|| scenarios::threads_cross(threads, BATCHES, BATCH),
        ));
    }
    rows
}

fn read_rows(path: &str) -> Result<Vec<Row>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    Ok(text.lines().filter_map(parse_row).collect())
}

fn table(default: &str, candidate: &str) -> Result<(), String> {
    let (default, candidate) = (read_rows(default)?, read_rows(candidate)?);
    print!("{}", compare(&default, &candidate));
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match parse_args(&args) {
        Ok(Command::Run { reps }) => {
            run(reps);
            Ok(())
        }
        Ok(Command::Table { default, candidate }) => table(&default, &candidate),
        Err(message) => Err(message),
    };
    if let Err(message) = outcome {
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
    fn no_arguments_run_with_the_default_repetitions() {
        assert_eq!(
            parse_args(&args(&[])),
            Ok(Command::Run { reps: DEFAULT_REPS })
        );
    }

    #[test]
    fn reps_takes_a_positive_number() {
        assert_eq!(
            parse_args(&args(&["--reps", "3"])),
            Ok(Command::Run { reps: 3 })
        );
        assert!(parse_args(&args(&["--reps", "0"])).is_err());
        assert!(parse_args(&args(&["--reps", "x"])).is_err());
    }

    #[test]
    fn table_takes_two_files() {
        assert_eq!(
            parse_args(&args(&["--table", "a.txt", "b.txt"])),
            Ok(Command::Table {
                default: "a.txt".to_owned(),
                candidate: "b.txt".to_owned()
            })
        );
        assert!(parse_args(&args(&["--table", "a.txt"])).is_err());
        assert!(parse_args(&args(&["--bogus"])).is_err());
    }

    #[test]
    fn measuring_divides_by_the_units_of_work() {
        let one = measure("w", "1", 3, 1.0, &|| 0);
        let two = measure("w", "2", 3, 2.0, &|| 0);
        assert!(one.summary.max > 0.0 && two.summary.max > 0.0);
        assert!(units(7) > 6.9 && units(7) < 7.1);
    }

    #[test]
    fn measuring_reports_nanoseconds_per_unit_of_a_known_wait() {
        let row = measure("wait", "1 ms", 3, 1.0, &|| {
            std::thread::sleep(std::time::Duration::from_millis(1));
            0
        });
        assert!(row.summary.median >= 1.0e6, "{:?}", row.summary);
        assert!(row.summary.min <= row.summary.median);
        assert!(row.summary.median <= row.summary.max);
    }
}
