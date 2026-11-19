//! The 3D workload of the instruction-count gate (card 8-29). The gate runs
//! `tools_physics3d_bench icount-workload --layers N --steps N` under a counting tool and reads
//! the count; the one line printed carries the box count and the state hash, so a change of the
//! workload is seen next to the count. Dev-time only; the engine does not depend on it.

use tools_physics_bench::scene3d::icount_workload3;

/// Largest pyramid the tool builds: the bottom layer is this many boxes on a side, and the
/// pyramid has the sum of the squares of 1 to this many boxes (the world holds at most `u16`).
const MAX_LAYERS: u8 = 40;

/// Most steps one run takes, the bound the gate's own workload keys use.
const MAX_STEPS: u32 = 1_000_000;

/// The usage text.
pub const USAGE: &str = "usage: tools_physics3d_bench icount-workload --layers N --steps N";

/// What to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Workload {
    /// Layers of the square pyramid (the bottom layer is this many boxes on a side).
    pub layers: u8,
    /// Steps of the world.
    pub steps: u32,
}

/// One flag value, read as a number.
fn number<T: std::str::FromStr>(flag: &str, value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("{flag} needs a number\n{USAGE}"))
}

/// Reads the command line (without the program name).
///
/// # Errors
/// A message when the command, a flag or a value is not what the tool takes.
pub fn parse(args: &[String]) -> Result<Workload, String> {
    let Some((command, flags)) = args.split_first() else {
        return Err(USAGE.to_owned());
    };
    if command != "icount-workload" {
        return Err(USAGE.to_owned());
    }
    let (mut layers, mut steps) = (None, None);
    let mut rest = flags.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or_else(|| USAGE.to_owned())?;
        match flag.as_str() {
            "--layers" => layers = Some(number::<u8>(flag, value)?),
            "--steps" => steps = Some(number::<u32>(flag, value)?),
            _ => return Err(USAGE.to_owned()),
        }
    }
    match (layers, steps) {
        (Some(layers), Some(steps)) if layers <= MAX_LAYERS && steps <= MAX_STEPS => {
            Ok(Workload { layers, steps })
        }
        _ => Err(USAGE.to_owned()),
    }
}

/// Runs the workload and returns the line to print.
///
/// # Errors
/// The physics error of the build or of a step, as text.
pub fn run(workload: Workload) -> Result<String, String> {
    let Workload { layers, steps } = workload;
    let (bodies, hash) = icount_workload3(layers, steps).map_err(|error| format!("{error:?}"))?;
    Ok(format!(
        "icount-workload layers={layers} steps={steps} bodies={bodies} hash={hash:016x}"
    ))
}
