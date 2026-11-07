//! Instruction counting: reading the count out of the text a counting tool prints, and deciding
//! from repeated counts whether a 5 % / 10 % gate on them is feasible.
//!
//! The tools themselves (`valgrind --tool=callgrind`, `valgrind --tool=cachegrind`,
//! `perf stat -e instructions:u`) are run by the caller, on Linux; this module only reads what
//! they print, so it is testable on any machine. The findings about which of them work on a
//! GitHub-hosted runner are in `docs/budgets/physics-benchmark-4-22.md`.

use std::str::FromStr;

/// The gate bands of `gates/gates.toml [benchmark]`, as fractions: warn at 5 %.
pub const WARN_FRACTION: f64 = 0.05;

/// The noise of a measure must be at most this share of the warn band, or a change inside the
/// band cannot be told from noise.
pub const NOISE_SHARE: f64 = 0.1;

/// Largest spread that is feasible: a tenth of the warn band, written out so no float product
/// decides the boundary (`WARN_FRACTION * NOISE_SHARE`).
pub const FEASIBLE_MAX: f64 = 0.005;

/// A tool whose output carries an instruction count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// `valgrind --tool=callgrind`: prints `Collected : N`.
    Callgrind,
    /// `valgrind --tool=cachegrind`: prints `I   refs: N`.
    Cachegrind,
    /// `perf stat -e instructions:u`: a line with the event name (default or `-x,` output); on a hybrid CPU `perf` prints one line per core type and the first wins.
    PerfStat,
}

/// Why a count could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcountError {
    /// The text has no line with the count, or the number does not parse.
    Malformed,
    /// The tool ran but the machine has no such counter (`<not supported>`, `<not counted>`),
    /// as a virtual machine without a performance-monitoring unit reports.
    CounterUnavailable,
    /// The tool name is not one of `callgrind`, `cachegrind`, `perf`.
    UnknownTool,
}

impl FromStr for Tool {
    type Err = IcountError;

    fn from_str(name: &str) -> Result<Self, IcountError> {
        match name {
            "callgrind" => Ok(Self::Callgrind),
            "cachegrind" => Ok(Self::Cachegrind),
            "perf" => Ok(Self::PerfStat),
            _ => Err(IcountError::UnknownTool),
        }
    }
}

/// A number with thousands separators, `1,234` or `1234`.
fn number(token: &str) -> Result<u64, IcountError> {
    let digits: String = token.chars().filter(|c| *c != ',').collect();
    digits.parse().map_err(|_| IcountError::Malformed)
}

/// The text after a valgrind `==pid==` prefix.
fn without_pid(line: &str) -> &str {
    let line = line.trim_start();
    match line
        .strip_prefix("==")
        .and_then(|rest| rest.split_once("=="))
    {
        Some((_, rest)) => rest.trim_start(),
        None => line,
    }
}

fn callgrind(text: &str) -> Result<u64, IcountError> {
    let line = text
        .lines()
        .map(without_pid)
        .find_map(|line| line.strip_prefix("Collected :"))
        .ok_or(IcountError::Malformed)?;
    number(
        line.split_whitespace()
            .next()
            .ok_or(IcountError::Malformed)?,
    )
}

fn cachegrind(text: &str) -> Result<u64, IcountError> {
    for line in text.lines().map(without_pid) {
        let mut tokens = line.split_whitespace();
        if let (Some("I"), Some("refs:"), Some(count)) =
            (tokens.next(), tokens.next(), tokens.next())
        {
            return number(count);
        }
    }
    Err(IcountError::Malformed)
}

fn perf(text: &str) -> Result<u64, IcountError> {
    let line = text
        .lines()
        .find(|line| line.contains("instructions"))
        .ok_or(IcountError::Malformed)?;
    if line.contains("<not supported>") || line.contains("<not counted>") {
        return Err(IcountError::CounterUnavailable);
    }
    let csv = line.split(',').nth(1).is_some_and(str::is_empty);
    let token = if csv {
        line.split(',').next()
    } else {
        line.split_whitespace().next()
    };
    number(token.ok_or(IcountError::Malformed)?)
}

/// The instruction count in the output of `tool`.
///
/// # Errors
/// [`IcountError::Malformed`] when the line is missing, [`IcountError::CounterUnavailable`] when
/// `perf` reports the counter as not supported.
pub fn parse_instructions(tool: Tool, text: &str) -> Result<u64, IcountError> {
    match tool {
        Tool::Callgrind => callgrind(text),
        Tool::Cachegrind => cachegrind(text),
        Tool::PerfStat => perf(text),
    }
}

/// A count as an `f64` from its two 32-bit halves: exact below 2^53, far above any count of a run.
fn as_f64(count: u64) -> f64 {
    let two_pow_32 = f64::from(u32::MAX) + 1.0;
    let high = u32::try_from(count >> 32).unwrap_or(u32::MAX);
    let low = u32::try_from(count & u64::from(u32::MAX)).unwrap_or(u32::MAX);
    f64::from(high) * two_pow_32 + f64::from(low)
}

/// The range of the counts over the smallest, for example 0.01 for a spread of 1 %; `None` for
/// fewer than two counts or a smallest count of zero.
#[must_use]
pub fn relative_spread(counts: &[u64]) -> Option<f64> {
    if counts.len() < 2 {
        return None;
    }
    let (low, high) = (*counts.iter().min()?, *counts.iter().max()?);
    if low == 0 {
        return None;
    }
    Some(as_f64(high - low) / as_f64(low))
}

/// Whether a gate on a count can work, from the spread measured over repeated runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateFeasibility {
    /// Noise at most a tenth of the 5 % warn band: both bands (5 % and 10 %) are readable.
    Feasible,
    /// Noise inside the warn band but above a tenth of it: the 10 % fail band is only coarsely readable, the
    /// 5 % warn band is not.
    Marginal,
    /// Noise above the warn band: no band is readable.
    Infeasible,
}

/// Classifies a spread against the bands.
#[must_use]
pub fn gate_feasibility(spread: f64) -> GateFeasibility {
    if spread <= FEASIBLE_MAX {
        GateFeasibility::Feasible
    } else if spread <= WARN_FRACTION {
        GateFeasibility::Marginal
    } else {
        GateFeasibility::Infeasible
    }
}
