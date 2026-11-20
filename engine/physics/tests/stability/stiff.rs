//! The stiff-contact preset (card 8-54, `SolverConfig::STIFF_CONTACTS`) on the ordinary scenes: it
//! must not be worse than the default where the default is fine. The preset is for stacks that
//! carry a heavy body; these are the pyramids and columns of the stability catalogue run with it.
//! With one relax pass instead of two a column with a heavy body on a light one kept creeping
//! (0.05 to 0.2 m/s after 20 s, drift growing to 0.2 m), which is why the preset has two.

use super::limits::{DRIFT, REST_SPEED, W1_STEPS};
use super::measure::{below, check_resting, observe};
use super::scene::{W1_HALF, column_with, pyramid_with};
use physics::core::SolverConfig;
use physics::d2::{PhysicsError, SleepConfig};

const PRESET: SolverConfig = SolverConfig::STIFF_CONTACTS;

/// Columns of ratio 10 and of ratio 100, lowest box first (those of `mass_ratio`).
const COLUMNS: [&[f32]; 7] = [
    &[1.0, 10.0],
    &[10.0, 1.0],
    &[1.0, 10.0, 1.0],
    &[10.0, 1.0, 10.0],
    &[1.0, 100.0],
    &[100.0, 1.0],
    &[1.0, 100.0, 1.0],
];

/// Steps of the long run: 40 s, eight times the 5 s of the settled tests.
const LONG_STEPS: u32 = 2400;

#[test]
fn pyramids_of_five_and_ten_rows_stand_with_the_stiff_preset() -> Result<(), PhysicsError> {
    for rows in [5_u8, 10] {
        let mut scene = pyramid_with(rows, W1_HALF, SleepConfig::DISABLED, PRESET)?;
        let report = observe(&mut scene, W1_STEPS)?;
        check_resting(&format!("stiff pyramid {rows} rows"), &report);
    }
    Ok(())
}

#[test]
fn columns_of_ratio_ten_and_a_hundred_come_to_rest_with_the_stiff_preset()
-> Result<(), PhysicsError> {
    for densities in COLUMNS {
        let mut scene = column_with(densities, W1_HALF, SleepConfig::DISABLED, PRESET)?;
        let report = observe(&mut scene, W1_STEPS)?;
        check_resting(&format!("stiff column {densities:?}"), &report);
    }
    Ok(())
}

#[test]
fn columns_with_a_heavy_body_on_a_light_one_do_not_creep_over_40_seconds_with_the_stiff_preset()
-> Result<(), PhysicsError> {
    for densities in [&[1.0_f32, 100.0][..], &[1.0, 100.0, 1.0]] {
        let name = format!("stiff column {densities:?}, 40 s");
        let mut scene = column_with(densities, W1_HALF, SleepConfig::DISABLED, PRESET)?;
        let report = observe(&mut scene, LONG_STEPS)?;
        below(&name, "max drift (m)", report.max_drift, DRIFT);
        below(&name, "final speed (m/s)", report.final_speed, REST_SPEED);
    }
    Ok(())
}

#[test]
fn a_pyramid_of_twenty_rows_stays_within_the_drift_bound_with_the_stiff_preset()
-> Result<(), PhysicsError> {
    // The default moves 0.053 m (a finding of the catalogue); the preset 0.029 m, with a quarter of
    // the penetration.
    let mut scene = pyramid_with(20, W1_HALF, SleepConfig::DISABLED, PRESET)?;
    let report = observe(&mut scene, W1_STEPS)?;
    check_resting("stiff pyramid 20 rows", &report);
    Ok(())
}
