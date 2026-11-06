//! The golden recorder (card 4-20): runs the scenes of the harness on the scalar serial reference and
//! prints the state hash after chosen steps as machine-readable `GOLDEN|...` lines.
//! `gates/golden/physics-replay.toml` stores the expected lines (taken with the feature
//! `cross-platform-determinism` on); the tool `tools/golden-check` compares a run with the file on
//! Windows and Linux (D2). The target compares nothing itself: a hash that differs is a finding of
//! the tool, which also refuses a run in which a line is missing. Run it with
//! `cargo test -p physics --features cross-platform-determinism --test determinism -- golden:: --nocapture`.

use super::recorder::{Trace, record};
use super::scenes::{Scene, long_runs, matrix};
use physics::d2::{PhysicsError, SolverExecution};

/// The format of the lines; the tool refuses another number.
const FORMAT: u32 = 1;

/// Checkpoint stride of the matrix scenes and of the long scenes, in steps.
const MATRIX_STRIDE: u32 = 5;
const LONG_STRIDE: u32 = 25;

/// Whether the hash after `step` (counted from 1) is a checkpoint: the first step, every
/// `stride`-th step and the last one.
fn is_checkpoint(step: u32, stride: u32, steps: u32) -> bool {
    step == 1 || step.is_multiple_of(stride) || step == steps
}

/// The checkpoint lines of one scene.
fn lines(scene: &Scene, stride: u32, trace: &Trace) -> Vec<String> {
    let mut out = vec![format!(
        "GOLDEN|scene|{}|{}|{stride}",
        scene.name, scene.steps
    )];
    for (index, sample) in trace.samples.iter().enumerate() {
        let step = u32::try_from(index + 1).unwrap_or(u32::MAX);
        if is_checkpoint(step, stride, scene.steps) {
            out.push(format!(
                "GOLDEN|hash|{}|{step}|{:016x}",
                scene.name, sample.hash
            ));
        }
    }
    out
}

/// The operating system of the build, from `cfg` (the harness may not read the environment):
/// informational for people, gated by the tool.
fn platform_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    }
}

/// The CPU family of the build.
fn platform_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "other"
    }
}

/// The header: the format, the feature, the platform and whether the build fuses operations.
fn header() -> Vec<String> {
    vec![
        format!("GOLDEN|format|{FORMAT}"),
        format!(
            "GOLDEN|feature|cross-platform-determinism|{}",
            if cfg!(feature = "cross-platform-determinism") {
                "on"
            } else {
                "off"
            }
        ),
        format!("GOLDEN|platform|{}|{}", platform_os(), platform_arch()),
        format!("GOLDEN|fma|{}", cfg!(target_feature = "fma")),
    ]
}

/// Every golden line of a run, in file order, ending with the count of the lines before it.
fn golden_lines() -> Result<Vec<String>, PhysicsError> {
    let mut out = header();
    let groups = [(matrix(), MATRIX_STRIDE), (long_runs(), LONG_STRIDE)];
    for (scenes, stride) in groups {
        for scene in &scenes {
            let trace = record(scene, &SolverExecution::SERIAL)?;
            out.extend(lines(scene, stride, &trace));
        }
    }
    out.push(format!("GOLDEN|end|{}", out.len()));
    Ok(out)
}

/// The test the CI job runs with `--nocapture`: prints the lines for the tool.
#[test]
fn print_the_golden_lines() -> Result<(), PhysicsError> {
    // The test harness prints "test <name> ... " without a newline: start the lines on a new one.
    println!();
    for line in golden_lines()? {
        println!("{line}");
    }
    Ok(())
}

/// The same lines twice in one process: a recorder that printed something unstable (a clock, an
/// address, a thread count) would never match the stored file.
#[test]
fn the_golden_lines_are_the_same_twice() -> Result<(), PhysicsError> {
    assert_eq!(golden_lines()?, golden_lines()?);
    Ok(())
}

#[test]
fn the_golden_lines_cover_every_scene_with_the_first_and_the_last_step() -> Result<(), PhysicsError>
{
    let all = golden_lines()?;
    let scenes = matrix().len() + long_runs().len();
    let count = |kind: &str| {
        let prefix = format!("GOLDEN|{kind}|");
        all.iter().filter(|line| line.starts_with(&prefix)).count()
    };
    assert_eq!(count("scene"), scenes);
    assert_eq!(count("end"), 1);
    let expected_end = format!("GOLDEN|end|{}", all.len() - 1);
    assert_eq!(all.last(), Some(&expected_end));
    for scene in matrix().into_iter().chain(long_runs()) {
        let first = format!("GOLDEN|hash|{}|1|", scene.name);
        let last = format!("GOLDEN|hash|{}|{}|", scene.name, scene.steps);
        assert!(
            all.iter().any(|line| line.starts_with(&first)),
            "{}",
            scene.name
        );
        assert!(
            all.iter().any(|line| line.starts_with(&last)),
            "{}",
            scene.name
        );
    }
    Ok(())
}

#[test]
fn golden_checkpoints_are_the_first_step_every_stride_and_the_last() {
    let picked: Vec<u32> = (1..=12).filter(|&s| is_checkpoint(s, 5, 12)).collect();
    assert_eq!(picked, [1, 5, 10, 12]);
    let picked: Vec<u32> = (1..=10).filter(|&s| is_checkpoint(s, 5, 10)).collect();
    assert_eq!(picked, [1, 5, 10]);
}
