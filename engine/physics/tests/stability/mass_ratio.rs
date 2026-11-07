//! High mass ratio stacks (exit criterion: "stacking/chain stability scenes pass"; the Soft Step
//! solver targets heavy-on-light stacks). A column of 0.5 m boxes whose densities differ by 10
//! (0.25 kg against 2.5 kg) must settle like any other column; the 1:100 stress columns (0.25 kg
//! against 25 kg) are held to the same drift and speed bounds and a looser penetration bound.

use super::limits::W1_STEPS;
use super::measure::{below, check_energy, check_resting, observe};
use super::scene::{W1_HALF, column};
use physics::d2::{PhysicsError, SleepConfig};

/// The deepest contact allowed in a 1:100 stress column, in metres: a quarter of a box side
/// would be 0.125 m; 0.05 m is one tenth of a box side. Stated before the first measurement.
const STRESS_PENETRATION: f32 = 0.05;

/// Columns of ratio 10, lowest box first: heavy on light, light on heavy and two sandwiches.
const RATIO_10: [&[f32]; 4] = [
    &[1.0, 10.0],
    &[10.0, 1.0],
    &[1.0, 10.0, 1.0],
    &[10.0, 1.0, 10.0],
];

/// Columns of ratio 100.
const RATIO_100: [&[f32]; 3] = [&[1.0, 100.0], &[100.0, 1.0], &[1.0, 100.0, 1.0]];

fn run(densities: &[f32]) -> Result<super::measure::Report, PhysicsError> {
    let mut scene = column(densities, W1_HALF, SleepConfig::DISABLED)?;
    observe(&mut scene, W1_STEPS)
}

#[test]
fn columns_with_a_mass_ratio_of_ten_settle_like_any_other() -> Result<(), PhysicsError> {
    for densities in RATIO_10 {
        let report = run(densities)?;
        check_resting(&format!("mass ratio 1:10 {densities:?}"), &report);
    }
    Ok(())
}

#[test]
fn columns_with_a_mass_ratio_of_one_hundred_stay_put_and_do_not_sink_far()
-> Result<(), PhysicsError> {
    for densities in RATIO_100 {
        let name = format!("mass ratio 1:100 {densities:?}");
        let report = run(densities)?;
        below(
            &name,
            "max drift (m)",
            report.max_drift,
            super::limits::DRIFT,
        );
        below(
            &name,
            "final speed (m/s)",
            report.final_speed,
            super::limits::REST_SPEED,
        );
        below(
            &name,
            "max penetration (m)",
            report.max_penetration,
            STRESS_PENETRATION,
        );
        check_energy(&name, &report);
    }
    Ok(())
}
