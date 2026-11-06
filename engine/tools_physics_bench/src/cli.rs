//! The command line of the binary, parsed into a value so it can be tested.
//!
//! ```text
//! tools_physics_bench [report] [--rows N] [--steps N] [--iters N] [--scale-steps N]
//!                     [--scale-iters N] [--calls N] [--reps N]
//! tools_physics_bench icount-workload [--rows N] [--steps N]
//! tools_physics_bench icount-parse <callgrind|cachegrind|perf> <file>
//! tools_physics_bench icount-spread <count> <count> ...
//! ```

use crate::icount::Tool;
use crate::measure::MIN_ITERATIONS;
use crate::scene::{W1_ROWS, W1_STEPS};

/// The sizes of a report run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Rows of the pyramid.
    pub rows: u8,
    /// Steps per W1 iteration.
    pub steps: u32,
    /// Timed W1 iterations.
    pub iters: u32,
    /// Steps per iteration of the scaling scene.
    pub scale_steps: u32,
    /// Timed iterations per row of the scaling scene.
    pub scale_iters: u32,
    /// Calls per repetition of the overhead measure.
    pub calls: u32,
    /// Timed repetitions of the overhead measure.
    pub reps: u32,
}

impl Options {
    /// The defaults: the full W1 pyramid, 300 steps, 30 iterations; the scaling scene at 60 steps.
    pub const DEFAULT: Self = Self {
        rows: W1_ROWS,
        steps: W1_STEPS,
        iters: MIN_ITERATIONS,
        scale_steps: 60,
        scale_iters: MIN_ITERATIONS,
        calls: 1000,
        reps: MIN_ITERATIONS,
    };

    /// True when a count is below what the measurement protocol asks for.
    #[must_use]
    pub fn below_protocol(&self) -> bool {
        self.iters < MIN_ITERATIONS
            || self.scale_iters < MIN_ITERATIONS
            || self.reps < MIN_ITERATIONS
    }
}

/// What the binary was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Print the full report.
    Report(Options),
    /// Run the pyramid once without timing, for an instruction counter to watch.
    IcountWorkload {
        /// Rows of the pyramid.
        rows: u8,
        /// Steps to take.
        steps: u32,
    },
    /// Print the instruction count in the saved output of a tool.
    IcountParse {
        /// The tool that wrote the file.
        tool: Tool,
        /// The file.
        path: String,
    },
    /// Print the spread of repeated counts and whether a gate on them is feasible.
    IcountSpread(Vec<u64>),
}

/// The usage text.
pub const USAGE: &str = "usage: tools_physics_bench [report] [--rows N] [--steps N] [--iters N] [--scale-steps N] [--scale-iters N] [--calls N] [--reps N]\n       tools_physics_bench icount-workload [--rows N] [--steps N]\n       tools_physics_bench icount-parse <callgrind|cachegrind|perf> <file>\n       tools_physics_bench icount-spread <count> <count> ...";

/// Largest value any count flag accepts: keeps a run short and a count exact as a float.
const MAX_COUNT: u32 = 1_000_000;

fn positive(flag: &str, value: &str) -> Result<u32, String> {
    match value.parse::<u32>() {
        Ok(n) if n > MAX_COUNT => Err(format!("{flag} is at most {MAX_COUNT}, got {value}")),
        Ok(n) if n > 0 => Ok(n),
        _ => Err(format!("{flag} needs a positive number, got {value}")),
    }
}

fn set_flag(options: &mut Options, flag: &str, value: &str) -> Result<(), String> {
    let n = positive(flag, value)?;
    match flag {
        "--rows" => options.rows = u8::try_from(n).map_err(|_| "--rows is at most 255")?,
        "--steps" => options.steps = n,
        "--iters" => options.iters = n,
        "--scale-steps" => options.scale_steps = n,
        "--scale-iters" => options.scale_iters = n,
        "--calls" => options.calls = n,
        "--reps" => options.reps = n,
        _ => return Err(USAGE.to_owned()),
    }
    Ok(())
}

fn options(args: &[String]) -> Result<Options, String> {
    let mut options = Options::DEFAULT;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or("a flag needs a value")?;
        set_flag(&mut options, flag, value)?;
    }
    Ok(options)
}

fn workload(args: &[String]) -> Result<Command, String> {
    let all = options(args)?;
    Ok(Command::IcountWorkload {
        rows: all.rows,
        steps: all.steps,
    })
}

fn parse_tool(args: &[String]) -> Result<Command, String> {
    match args {
        [tool, path] => Ok(Command::IcountParse {
            tool: tool.parse().map_err(|_| USAGE.to_owned())?,
            path: path.clone(),
        }),
        _ => Err(USAGE.to_owned()),
    }
}

fn spread(args: &[String]) -> Result<Command, String> {
    let counts: Result<Vec<u64>, _> = args.iter().map(|arg| arg.parse::<u64>()).collect();
    match counts {
        Ok(counts) if counts.len() >= 2 => Ok(Command::IcountSpread(counts)),
        _ => Err(USAGE.to_owned()),
    }
}

/// Parses the arguments after the program name.
///
/// # Errors
/// The message to print (the usage, or what is wrong with a flag).
pub fn parse(args: &[String]) -> Result<Command, String> {
    match args.split_first() {
        None => Ok(Command::Report(Options::DEFAULT)),
        Some((word, rest)) if word == "report" => options(rest).map(Command::Report),
        Some((word, rest)) if word == "icount-workload" => workload(rest),
        Some((word, rest)) if word == "icount-parse" => parse_tool(rest),
        Some((word, rest)) if word == "icount-spread" => spread(rest),
        Some(_) => options(args).map(Command::Report),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn no_arguments_run_the_full_report() {
        assert_eq!(parse(&[]), Ok(Command::Report(Options::DEFAULT)));
        assert_eq!(Options::DEFAULT.rows, 44);
        assert_eq!(Options::DEFAULT.steps, 300);
        assert!(!Options::DEFAULT.below_protocol());
    }

    #[test]
    fn flags_change_the_sizes_with_or_without_the_report_word() {
        let wanted = Options {
            rows: 10,
            iters: 5,
            ..Options::DEFAULT
        };
        let flags = ["--rows", "10", "--iters", "5"];
        assert_eq!(parse(&args(&flags)), Ok(Command::Report(wanted)));
        let with_word = ["report", "--rows", "10", "--iters", "5"];
        assert_eq!(parse(&args(&with_word)), Ok(Command::Report(wanted)));
        assert!(wanted.below_protocol());
    }

    #[test]
    fn every_size_flag_is_read() {
        let all = [
            "--rows",
            "9",
            "--steps",
            "8",
            "--iters",
            "7",
            "--scale-steps",
            "6",
            "--scale-iters",
            "5",
            "--calls",
            "4",
            "--reps",
            "3",
        ];
        let wanted = Options {
            rows: 9,
            steps: 8,
            iters: 7,
            scale_steps: 6,
            scale_iters: 5,
            calls: 4,
            reps: 3,
        };
        assert_eq!(parse(&args(&all)), Ok(Command::Report(wanted)));
    }

    #[test]
    fn any_one_count_below_the_protocol_is_reported() {
        let base = Options::DEFAULT;
        let few = MIN_ITERATIONS - 1;
        assert!(Options { iters: few, ..base }.below_protocol());
        assert!(
            Options {
                scale_iters: few,
                ..base
            }
            .below_protocol()
        );
        assert!(Options { reps: few, ..base }.below_protocol());
        let exact = Options {
            iters: MIN_ITERATIONS,
            scale_iters: MIN_ITERATIONS,
            reps: MIN_ITERATIONS,
            ..base
        };
        assert!(!exact.below_protocol());
    }

    #[test]
    fn the_largest_count_is_accepted_and_one_more_is_not() {
        assert!(parse(&args(&["--steps", "1000000"])).is_ok());
        assert!(parse(&args(&["--steps", "1000001"])).is_err());
    }

    #[test]
    fn bad_flags_and_numbers_are_refused() {
        assert!(parse(&args(&["--rows"])).is_err());
        assert!(parse(&args(&["--rows", "0"])).is_err());
        assert!(parse(&args(&["--rows", "256"])).is_err());
        assert!(parse(&args(&["--steps", "x"])).is_err());
        assert!(parse(&args(&["--steps", "1000001"])).is_err());
        assert!(parse(&args(&["--nope", "1"])).is_err());
    }

    #[test]
    fn the_icount_commands_are_read() {
        assert_eq!(
            parse(&args(&["icount-workload", "--rows", "12", "--steps", "40"])),
            Ok(Command::IcountWorkload {
                rows: 12,
                steps: 40
            })
        );
        assert_eq!(
            parse(&args(&["icount-parse", "callgrind", "out.txt"])),
            Ok(Command::IcountParse {
                tool: Tool::Callgrind,
                path: "out.txt".to_owned()
            })
        );
        assert_eq!(
            parse(&args(&["icount-spread", "100", "101"])),
            Ok(Command::IcountSpread(vec![100, 101]))
        );
    }

    #[test]
    fn the_icount_commands_refuse_bad_arguments() {
        assert!(parse(&args(&["icount-parse", "gdb", "out.txt"])).is_err());
        assert!(parse(&args(&["icount-parse", "perf"])).is_err());
        assert!(parse(&args(&["icount-spread", "100"])).is_err());
        assert!(parse(&args(&["icount-spread", "100", "x"])).is_err());
    }
}
