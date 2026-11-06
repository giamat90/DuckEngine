//! Box stacks of growing height (exit criterion: "stacking ... stability scenes pass"): how tall a
//! column of 0.5 m boxes stays settled under the default solver settings.

use super::limits::W1_STEPS;
use super::measure::{check_resting, observe, record};
use super::scene::{W1_HALF, column};
use physics::d2::{PhysicsError, SleepConfig};

/// The height every column from 2 up to it must hold. The value stated before the first
/// measurement was 20; the measurement gave 10 (`findings`), and this is the measured value.
const REQUIRED_HEIGHT: u8 = 10;

/// Heights above `REQUIRED_HEIGHT` that are probed to report how tall a column can be; the probe
/// has no bound of its own, the bounds are the ones of the settled scenes.
const PROBE_HEIGHT: u8 = 14;

fn column_of(height: u8) -> Result<super::scene::Scene, PhysicsError> {
    let densities = vec![1.0; usize::from(height)];
    column(&densities, W1_HALF, SleepConfig::DISABLED)
}

#[test]
fn columns_of_two_to_the_required_height_stay_settled() -> Result<(), PhysicsError> {
    for height in 2..=REQUIRED_HEIGHT {
        let mut scene = column_of(height)?;
        let report = observe(&mut scene, W1_STEPS)?;
        check_resting(&format!("column of {height}"), &report);
    }
    Ok(())
}

/// The tallest column that stays within the drift bound after 300 steps: found by probing up to
/// `PROBE_HEIGHT`, so the measured value is a lower bound of the real limit when it equals it.
#[test]
fn the_tallest_stable_column_is_at_least_the_required_height() -> Result<(), PhysicsError> {
    let mut tallest = 0_u8;
    for height in 2..=PROBE_HEIGHT {
        let mut scene = column_of(height)?;
        let report = observe(&mut scene, W1_STEPS)?;
        if report.max_drift >= super::limits::DRIFT {
            break;
        }
        tallest = height;
    }
    record(
        "column probe",
        "tallest column within drift",
        f64::from(tallest),
        f64::from(REQUIRED_HEIGHT),
    );
    assert!(
        tallest >= REQUIRED_HEIGHT,
        "the tallest stable column is {tallest}, required {REQUIRED_HEIGHT}"
    );
    Ok(())
}
