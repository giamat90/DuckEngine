//! Stacking pyramids (exit criterion: "stacking/chain stability scenes pass"; workload W1: "the
//! pyramid is still standing after 300 steps (no box moved more than 0.05 m from its rest
//! position)"). W1 itself has about 1,000 boxes of 0.5 m; the scenes here are 5 and 10 rows of the
//! same boxes. A pyramid of 20 rows (210 boxes) moves 0.0527 m from its start and is a finding
//! (`findings`).

use super::limits::W1_STEPS;
use super::measure::{check_resting, observe};
use super::scene::{Scene, W1_HALF, pyramid};
use physics::d2::{PhysicsError, SleepConfig};

/// Rows of the two pyramids that hold their bounds (15 and 55 boxes; W1 has about 1,000).
const SIZES: [u8; 2] = [5, 10];

/// A run is not vacuous when some box moves faster than this, in metres per second (the boxes
/// start a hair above their neighbours and settle).
const MIN_MOVING_SPEED: f32 = 0.01;

/// A pyramid of `rows` rows of 0.5 m boxes with sleeping off (so the solver alone holds it up).
pub fn w1_pyramid(rows: u8) -> Result<Scene, PhysicsError> {
    pyramid(rows, W1_HALF, SleepConfig::DISABLED)
}

fn settles(rows: u8) -> Result<(), PhysicsError> {
    let mut scene = w1_pyramid(rows)?;
    let boxes = usize::from(rows) * (usize::from(rows) + 1) / 2;
    assert_eq!(
        scene.tracked.len(),
        boxes,
        "the pyramid has the boxes it should"
    );
    let report = observe(&mut scene, W1_STEPS)?;
    assert!(
        report.max_speed > MIN_MOVING_SPEED,
        "the run is not vacuous: boxes do move"
    );
    check_resting(&format!("pyramid {rows} rows ({boxes} boxes)"), &report);
    Ok(())
}

#[test]
fn a_pyramid_of_five_rows_is_still_standing_after_300_steps() -> Result<(), PhysicsError> {
    settles(SIZES[0])
}

#[test]
fn a_pyramid_of_ten_rows_is_still_standing_after_300_steps() -> Result<(), PhysicsError> {
    settles(SIZES[1])
}
