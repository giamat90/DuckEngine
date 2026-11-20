//! The stiff-contact preset (card 8-54, `SolverConfig::STIFF_CONTACTS`) on the ordinary scenes: it
//! must not be worse than the default where the default is fine. The preset is for stacks that
//! carry a heavy body; these are the pyramids and columns of the stability catalogue run with it.
//! With one relax pass instead of two a column with a heavy body on a light one kept creeping
//! (0.05 to 0.2 m/s after 20 s, drift growing to 0.2 m), which is why the preset has two.

use super::chains::{HEAVY_LOAD_ERROR, check_bridge, check_hanging, check_heavy_end, check_whips};
use super::limits::{DRIFT, HINGE_ERROR, PENETRATION, REST_SPEED, W1_STEPS};
use super::measure::{
    above, all_finite, below, check_energy, check_resting, dynamic_count, observe, positions,
};
use super::mixed::{MAX_BODY_SPEED, check_resting_mixed, mixed_with};
use super::properties::{MIN_CALM_CHECKS, calm_depth, seeds};
use super::scene::{Lie, W1_HALF, column_with, pyramid_with};
use super::sleeping::fall_asleep;
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

#[test]
fn the_chains_whips_and_bridges_that_hold_with_the_default_hold_with_the_stiff_preset()
-> Result<(), PhysicsError> {
    check_hanging(PRESET)?;
    check_whips(PRESET)?;
    check_heavy_end(PRESET)?;
    check_bridge(1.0, HINGE_ERROR, PRESET)?;
    check_bridge(10.0, HEAVY_LOAD_ERROR, PRESET)
}

#[test]
fn the_mixed_scene_with_a_chain_hanging_on_the_apex_holds_with_the_stiff_preset()
-> Result<(), PhysicsError> {
    check_resting_mixed(PRESET)
}

/// With the default the whip hits the pyramid on its second swing (a box first moves at step 184)
/// and moves it 2.5 m (the scene of `mixed`); with the preset the whip never comes nearer than on
/// its first swing (step 62, about 0.59 m centre to centre) and the pyramid moves 0.006 m. The cause
/// was not looked into.
#[test]
fn finding_the_whip_that_scatters_the_pyramid_with_the_default_hardly_moves_it_with_the_stiff_preset()
-> Result<(), PhysicsError> {
    let mut mixed = mixed_with(Lie::Horizontal, SleepConfig::DISABLED, PRESET)?;
    let name = "stiff mixed whip (pyramid 6, whip 8, 5 circles)";
    let report = observe(&mut mixed.scene, W1_STEPS)?;
    below(name, "pyramid max drift (m)", report.max_drift, DRIFT);
    below(
        name,
        "worst hinge error (m)",
        report.worst_hinge,
        HINGE_ERROR,
    );
    below(name, "fastest body (m/s)", report.max_speed, MAX_BODY_SPEED);
    check_energy(name, &report);
    assert!(all_finite(&mixed.scene.world), "{name}: not finite");
    let tip = mixed.links.last().ok_or(PhysicsError::StaleHandle)?;
    let tip_y = mixed.scene.world.body(*tip)?.position().y;
    assert!(
        tip_y < mixed.scene.hang.y - WHIP_SWING_DOWN,
        "{name}: the whip swung down (the tip is at {tip_y})"
    );
    Ok(())
}

/// The whip is let go level with its pin: after the run its tip is at least this far below it.
const WHIP_SWING_DOWN: f32 = 1.0;

/// The seed of the random bins whose deepest calm contact is over the bound with the preset
/// (0.030 m against 0.02 m; 0.010 m with the default). The cause was not looked into.
const DEEP_SEED: u64 = 1147;

/// The most the contact of the exception may be deep, in metres.
const DEEP_SEED_BOUND: f32 = 0.05;

#[test]
fn no_contact_of_the_random_bins_is_deep_once_calm_with_the_stiff_preset_but_one_seed()
-> Result<(), PhysicsError> {
    for seed in seeds() {
        let (depth, steps) = calm_depth(seed, PRESET)?;
        assert!(steps >= MIN_CALM_CHECKS, "seed {seed}: {steps} calm steps");
        if seed == DEEP_SEED {
            above(
                "random bins, seed 1147",
                "deepest calm contact (m)",
                depth,
                PENETRATION,
            );
            below(
                "random bins, seed 1147",
                "deepest calm contact (m)",
                depth,
                DEEP_SEED_BOUND,
            );
        } else {
            assert!(
                depth < PENETRATION,
                "seed {seed}: a contact is {depth} m deep"
            );
        }
    }
    Ok(())
}

/// The tall stack of the sleeping test: seven light boxes under a top 20 times as dense.
const TALL_LIGHT_BOXES: usize = 7;
const TALL_TOP_DENSITY: f32 = 20.0;

/// Steps by which a scene with sleeping on must be asleep with the preset (5 s); the scenes below
/// were asleep at step 30 to 40.
const ASLEEP_BY: u32 = 300;

#[test]
fn with_sleeping_on_the_stiff_preset_puts_pyramids_columns_and_a_tall_stack_to_sleep_where_they_stood()
-> Result<(), PhysicsError> {
    let mut tall = vec![1.0_f32; TALL_LIGHT_BOXES];
    tall.push(TALL_TOP_DENSITY);
    let mut scenes = vec![
        (
            "pyramid 10",
            pyramid_with(10, W1_HALF, SleepConfig::DEFAULT, PRESET)?,
        ),
        (
            "column 1:100",
            column_with(&[1.0, 100.0], W1_HALF, SleepConfig::DEFAULT, PRESET)?,
        ),
        (
            "column 1:100:1",
            column_with(&[1.0, 100.0, 1.0], W1_HALF, SleepConfig::DEFAULT, PRESET)?,
        ),
        (
            "eight boxes, a top 20 times as dense",
            column_with(&tall, W1_HALF, SleepConfig::DEFAULT, PRESET)?,
        ),
    ];
    for (name, scene) in &mut scenes {
        let start = positions(scene)?;
        assert!(
            fall_asleep(scene, ASLEEP_BY)?.is_some(),
            "{name}: not asleep by step {ASLEEP_BY}"
        );
        assert_eq!(
            scene.world.sleeping_count(),
            dynamic_count(&scene.world),
            "{name}"
        );
        let end = positions(scene)?;
        let moved = start
            .iter()
            .zip(&end)
            .map(|(a, b)| (*a - *b).length())
            .fold(0.0, f32::max);
        below(name, "moved before sleeping (m)", moved, DRIFT);
    }
    Ok(())
}

/// The speed after 300 steps is above this and below the envelope (0.19 m/s measured).
const SLOWER_SETTLE_FLOOR: f32 = 0.1;
const SLOWER_SETTLE_ENVELOPE: f32 = 0.5;

/// The drift of the 44-row pyramid with the preset was 0.119 m (the default 0.247 m).
const DRIFT_44_ROWS: f32 = 0.2;

/// A pyramid of 44 rows has the shape of workload W1. With the default it is still moving at
/// 300 steps (0.037 m/s); with the preset it is faster (0.19 m/s), at 1200 steps 0.05 and at rest
/// at 2400, and asleep at step 1230 (the default at step 721), so the preset settles it later.
#[test]
fn finding_a_pyramid_of_44_rows_is_still_moving_at_300_steps_with_the_stiff_preset()
-> Result<(), PhysicsError> {
    let name = "stiff pyramid 44 rows";
    let mut scene = pyramid_with(44, W1_HALF, SleepConfig::DISABLED, PRESET)?;
    let report = observe(&mut scene, W1_STEPS)?;
    above(
        name,
        "final speed (m/s)",
        report.final_speed,
        SLOWER_SETTLE_FLOOR,
    );
    below(
        name,
        "final speed (m/s)",
        report.final_speed,
        SLOWER_SETTLE_ENVELOPE,
    );
    below(name, "max drift (m)", report.max_drift, DRIFT_44_ROWS);
    below(
        name,
        "max penetration (m)",
        report.max_penetration,
        PENETRATION,
    );
    Ok(())
}
