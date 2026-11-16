//! Command line: `report` and `icount-workload`, each with `--name value` options.

/// Text printed after a bad command line.
pub const USAGE: &str = "usage: tools_ecs_bench report [--entities N] [--iters N]\n       tools_ecs_bench icount-workload [--entities N] [--frames N]";

/// Entities of the report world.
const DEFAULT_ENTITIES: u32 = 100_000;
/// Timed iterations per row of the report.
const DEFAULT_ITERS: u32 = 30;
/// Entities of the instruction-count workload.
const DEFAULT_ICOUNT_ENTITIES: u32 = 8_192;
/// Frames of the instruction-count workload.
const DEFAULT_ICOUNT_FRAMES: u32 = 8;

/// Options of the `report` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Entities in the world.
    pub entities: u32,
    /// Timed iterations per row.
    pub iters: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            entities: DEFAULT_ENTITIES,
            iters: DEFAULT_ITERS,
        }
    }
}

/// A parsed command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Wall-time tables.
    Report(Options),
    /// The deterministic workload for instruction counting.
    IcountWorkload {
        /// Entities in the world.
        entities: u32,
        /// Frames run.
        frames: u32,
    },
}

/// Reads `--name value` pairs; each value is a positive number.
fn pairs(args: &[String]) -> Result<Vec<(&str, u32)>, String> {
    let mut out = Vec::new();
    let mut rest = args.iter();
    while let Some(name) = rest.next() {
        let value = rest.next().ok_or_else(|| format!("{name} needs a value"))?;
        let number: u32 = value
            .parse()
            .map_err(|_| format!("{name}: `{value}` is not a number"))?;
        if number == 0 {
            return Err(format!("{name} must be at least 1"));
        }
        out.push((name.as_str(), number));
    }
    Ok(out)
}

/// The value of `--name`, or `default`.
fn option(pairs: &[(&str, u32)], name: &str, default: u32) -> u32 {
    pairs
        .iter()
        .rev()
        .find(|(key, _)| *key == name)
        .map_or(default, |(_, value)| *value)
}

fn check_names(pairs: &[(&str, u32)], allowed: &[&str]) -> Result<(), String> {
    match pairs.iter().find(|(key, _)| !allowed.contains(key)) {
        Some((key, _)) => Err(format!("unknown option {key}")),
        None => Ok(()),
    }
}

/// Parses the arguments after the program name.
///
/// # Errors
/// A message for an unknown command or option, a missing or non-numeric value, or a zero.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let (command, rest) = args.split_first().ok_or("no command")?;
    let pairs = pairs(rest)?;
    match command.as_str() {
        "report" => {
            check_names(&pairs, &["--entities", "--iters"])?;
            Ok(Command::Report(Options {
                entities: option(&pairs, "--entities", DEFAULT_ENTITIES),
                iters: option(&pairs, "--iters", DEFAULT_ITERS),
            }))
        }
        "icount-workload" => {
            check_names(&pairs, &["--entities", "--frames"])?;
            Ok(Command::IcountWorkload {
                entities: option(&pairs, "--entities", DEFAULT_ICOUNT_ENTITIES),
                frames: option(&pairs, "--frames", DEFAULT_ICOUNT_FRAMES),
            })
        }
        other => Err(format!("unknown command {other}")),
    }
}
