//! The stiff-contact preset (card 8-54, `SolverConfig::STIFF_CONTACTS`) on the ordinary scenes: it
//! must not be worse than the default where the default is fine. The preset is for stacks that
//! carry a heavy body; these are the pyramids and columns of the stability catalogue run with it.
//! With one relax pass instead of two a column with a heavy body on a light one kept creeping
//! (0.05 to 0.2 m/s after 20 s, drift growing to 0.2 m), which is why the preset has two.

use super::chains::{HEAVY_LOAD_ERROR, check_bridge, check_hanging, check_heavy_end, check_whips};
use super::generator::generate_with_solver;
use super::limits::DT;
use super::limits::{DRIFT, HINGE_ERROR, PENETRATION, REST_SPEED, W1_STEPS};
use super::measure::{
    above, all_finite, below, check_energy, check_resting, deepest, deepest_where, dynamic_count,
    observe, positions,
};
use super::mixed::{MAX_BODY_SPEED, Mixed, Pin, check_resting_mixed, mixed_tuned, mixed_with};
use super::properties::{
    CALM_RUN_STEPS, CALM_SPEED, CALM_STEPS, MIN_CALM_CHECKS, WHIP_APPROACH, calm_contacts, seeds,
};
use super::scene::{Lie, W1_HALF, column_with, pyramid_with};
use super::sleeping::fall_asleep;
use physics::core::SolverConfig;
use physics::d2::{BodyHandle, PhysicsError, SleepConfig};

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

/// The seed of the random bins whose whip folds onto itself fast with the preset (card 8-59).
const DEEP_SEED: u64 = 1147;

#[test]
fn no_contact_of_the_bodies_of_the_random_bins_is_deep_once_calm_with_the_stiff_preset()
-> Result<(), PhysicsError> {
    for seed in seeds() {
        let found = calm_contacts(seed, PRESET)?;
        assert!(
            found.calm_steps >= MIN_CALM_CHECKS,
            "seed {seed}: {} calm steps",
            found.calm_steps
        );
        assert!(
            found.bin_depth < PENETRATION && found.touch_depth < PENETRATION,
            "seed {seed}: a contact is {} m deep (bin) or {} m (whip against the rest)",
            found.bin_depth,
            found.touch_depth
        );
    }
    Ok(())
}

/// The deepest hit of two links of the whip with the preset: 0.0296 m measured (seed 1147); the
/// whip swings, so the bound for bodies at rest does not apply to it.
const WHIP_HIT_DEPTH: f32 = 0.05;

#[test]
fn two_links_of_the_whip_hit_each_other_below_the_bounds_in_the_random_bins_with_the_stiff_preset()
-> Result<(), PhysicsError> {
    for seed in seeds() {
        let found = calm_contacts(seed, PRESET)?;
        assert!(
            found.whip_depth < WHIP_HIT_DEPTH && found.whip_approach < WHIP_APPROACH,
            "seed {seed}: the whip hit itself {} m deep at {} m/s",
            found.whip_depth,
            found.whip_approach
        );
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

// Causes of the three findings above (card 8-59). Each test pins what was found, with the values
// of both solvers, so that a change of either shows.

/// What the random bin of `seed` shows with `solver`: the deepest contact between two links of the
/// whip (with its approach speed) and the deepest contact among bodies that are no link, once the
/// bin has been calm.
struct BinDepths {
    whip_depth: f32,
    whip_approach: f32,
    calm_bin_depth: f32,
    /// The deepest contact of the whole world once calm (what the property took before card 8-69, which measures the bin and the whip apart).
    calm_world_depth: f32,
}

fn bin_depths(seed: u64, solver: SolverConfig) -> Result<BinDepths, PhysicsError> {
    let (mut scene, in_bin) = generate_with_solver(seed, SleepConfig::DISABLED, solver)?;
    let whip: Vec<BodyHandle> = scene.tracked.iter().skip(in_bin).copied().collect();
    let mut found = BinDepths {
        whip_depth: 0.0,
        whip_approach: 0.0,
        calm_bin_depth: 0.0,
        calm_world_depth: 0.0,
    };
    let mut calm = 0_u32;
    for _ in 0..CALM_RUN_STEPS {
        scene.world.step(DT)?;
        let mut fastest = 0.0_f32;
        for &handle in scene.tracked.iter().take(in_bin) {
            fastest = fastest.max(scene.world.body(handle)?.linear_velocity().length());
        }
        calm = if fastest < CALM_SPEED { calm + 1 } else { 0 };
        let (depth, approach) =
            deepest_where(&scene.world, |a, b| whip.contains(&a) && whip.contains(&b))?;
        if depth > found.whip_depth {
            (found.whip_depth, found.whip_approach) = (depth, approach);
        }
        let (bin, _) = deepest_where(&scene.world, |a, b| {
            !whip.contains(&a) && !whip.contains(&b)
        })?;
        if calm >= CALM_STEPS {
            found.calm_bin_depth = found.calm_bin_depth.max(bin);
            found.calm_world_depth = found.calm_world_depth.max(deepest(&scene.world));
        }
    }
    Ok(found)
}

/// Seed 1147 (finding 1): the contact 0.030 m deep is not a calm contact of the bin. It is two
/// links of the whip, which swings all the time (so the bin's calm test does not see it), folding
/// onto each other at step 169: after the solve of that step the two links still approach at
/// 2.2 m/s with the preset, at 1.1 m/s with the default (step 170, 0.010 m deep); the speed before
/// the hit was not measured. The test also pins that this contact is the deepest of the whole world
/// once calm, which is what the property took before card 8-69. Among the bodies of the bin the preset is the shallower
/// (0.001 m against 0.0075 m).
#[test]
fn the_deep_contact_of_seed_1147_is_two_links_of_the_whip_meeting_fast_not_a_calm_contact()
-> Result<(), PhysicsError> {
    let preset = bin_depths(DEEP_SEED, PRESET)?;
    let default = bin_depths(DEEP_SEED, SolverConfig::DEFAULT)?;
    let name = "random bins, seed 1147";
    assert!(
        (preset.calm_world_depth - preset.whip_depth).abs() < 1.0e-6,
        "the deepest calm contact of the world ({}) is not the whip-whip one ({})",
        preset.calm_world_depth,
        preset.whip_depth
    );
    above(
        name,
        "preset whip-whip depth (m)",
        preset.whip_depth,
        PENETRATION,
    );
    above(
        name,
        "preset whip-whip approach (m/s)",
        preset.whip_approach,
        FAST_APPROACH,
    );
    below(
        name,
        "preset calm bin depth (m)",
        preset.calm_bin_depth,
        CALM_BIN_BETWEEN,
    );
    below(
        name,
        "default whip-whip depth (m)",
        default.whip_depth,
        PENETRATION,
    );
    below(
        name,
        "default whip-whip approach (m/s)",
        default.whip_approach,
        FAST_APPROACH,
    );
    above(
        name,
        "default calm bin depth (m)",
        default.calm_bin_depth,
        CALM_BIN_BETWEEN,
    );
    Ok(())
}

/// Approach speed that separates the two whips of seed 1147 (2.2 and 1.1 m/s measured).
const FAST_APPROACH: f32 = 1.5;

/// The deepest calm contact among the bodies of the bin of seed 1147: 0.0010 m with the preset,
/// 0.0075 m with the default; the bound sits between them.
const CALM_BIN_BETWEEN: f32 = 0.004;

/// Steps of the whip scene (7 s); with the default the whip first touches the pyramid at step 181.
const WHIP_RUN: u32 = 420;

/// Runs the mixed scene with a whip pinned as `pin` says and returns the step at which a link first
/// touches a box of the pyramid (if any) and how far the farthest box of the pyramid ended from
/// where it started.
fn whip_hit(solver: SolverConfig, pin: Pin) -> Result<(Option<u32>, f32), PhysicsError> {
    let mut mixed: Mixed = mixed_tuned(Lie::Horizontal, SleepConfig::DISABLED, solver, pin)?;
    let start = positions(&mixed.scene)?;
    let mut first = None;
    for step in 0..WHIP_RUN {
        mixed.scene.world.step(DT)?;
        if first.is_none() && whip_touches_pyramid(&mixed) {
            first = Some(step);
        }
    }
    let mut drift = 0.0_f32;
    for (&handle, &from) in mixed.scene.tracked.iter().zip(&start) {
        drift = drift.max((mixed.scene.world.body(handle)?.position() - from).length());
    }
    Ok((first, drift))
}

/// Whether a link of the whip has a contact manifold with a box of the pyramid at the moment.
fn whip_touches_pyramid(mixed: &Mixed) -> bool {
    let (links, boxes) = (&mixed.links, &mixed.scene.tracked);
    mixed
        .scene
        .world
        .manifolds()
        .as_slice()
        .iter()
        .any(|entry| {
            let (a, b) = (entry.first, entry.second);
            (links.contains(&a) && boxes.contains(&b)) || (links.contains(&b) && boxes.contains(&a))
        })
}

/// The pyramid is only touched or shoved when the whip lands on the apex.
const SHOVED: f32 = 0.05;

/// The pin of a whip whose last link has density `density`.
fn heavy_tip(density: f32) -> Pin {
    Pin {
        last_density: density,
        ..Pin::STANDARD
    }
}

/// The pin of a whip hung `lift` metres higher than the standard one.
fn lifted(lift: f32) -> Pin {
    Pin {
        lift,
        ..Pin::STANDARD
    }
}

/// Finding 2: the whip that scatters the pyramid with the default does not do it with the preset,
/// but the scan does not show that the preset hits softer. On its second swing the whip passes the apex within a
/// few centimetres: with the default it touches it (step 181) when the pin is lifted by 0 to 0.02
/// m (scan: 3 of 21 lifts from 0 to 0.2 m; the test pins 0, 0.01, 0.1, 0.2) and at the standard pin shoves the pyramid 2.5 m; with the
/// preset it cleared the apex at all 21 lifts of the scan. It is a graze of a chaotic swing.
#[test]
fn the_whip_that_shoves_the_pyramid_with_the_default_only_grazes_it_on_its_second_swing()
-> Result<(), PhysicsError> {
    for lift in [0.0, 0.01] {
        let (first, _) = whip_hit(SolverConfig::DEFAULT, lifted(lift))?;
        assert!(first.is_some(), "default, lift {lift}: no touch");
        let step = first.unwrap_or(0);
        assert!(
            (SECOND_SWING.0..SECOND_SWING.1).contains(&step),
            "default, lift {lift}: first touch at step {step}"
        );
    }
    for lift in [0.1, 0.2] {
        let (first, _) = whip_hit(SolverConfig::DEFAULT, lifted(lift))?;
        assert_eq!(first, None, "default, lift {lift}");
    }
    for lift in [0.0, 0.1] {
        let (first, _) = whip_hit(PRESET, lifted(lift))?;
        assert_eq!(first, None, "preset, lift {lift}");
    }
    Ok(())
}

/// Steps between which the whip touches the pyramid on its second swing (181 measured).
const SECOND_SWING: (u32, u32) = (150, 220);

/// A whip with a heavy last link hits the apex on its first swing (steps 63 to 65) with either
/// solver and shoves the pyramid. The shove is smaller with the preset (the farthest box ends
/// from its start, default and preset: density 10 0.42 and 0.13 m, density 20 6.1 and 4.8 m), so a
/// softer hit is not excluded; the test pins that both hit and move the pyramid.
#[test]
fn a_whip_with_a_heavy_last_link_hits_the_pyramid_on_its_first_swing_with_either_solver()
-> Result<(), PhysicsError> {
    for solver in [SolverConfig::DEFAULT, PRESET] {
        for (density, shoved) in [(10.0, SHOVED), (20.0, 1.0)] {
            let (first, drift) = whip_hit(solver, heavy_tip(density))?;
            assert!(first.is_some(), "density {density}: no touch");
            let step = first.unwrap_or(0);
            assert!(
                (FIRST_SWING.0..FIRST_SWING.1).contains(&step),
                "density {density}: first touch at step {step}"
            );
            above("whip, heavy last link", "pyramid drift (m)", drift, shoved);
        }
    }
    Ok(())
}

/// Steps between which a whip with a heavy last link touches the pyramid on its first swing.
const FIRST_SWING: (u32, u32) = (50, 80);

/// What keeps a 44-row pyramid moving at 300 steps with the preset (finding 3, release scan): the
/// contact stiffness of 120 Hz together with the contact damping ratio of 10 that the preset
/// takes from the default. Final speed in m/s at 300 steps, scratch values: default (4 sub-steps,
/// 30 Hz) 0.037; 4 sub-steps at 60 Hz 0.17; 8 sub-steps and 2 relax passes at 30 Hz 0.00, at 60
/// Hz 0.00, at 120 Hz (the preset) 0.19; with a third relax pass at 120 Hz 0.014; with the
/// damping ratio at 5 instead of 10 (120 Hz, two passes) 0.0001 (at 0.2 to 2: 0.013 to 0.0004; at
/// 30 and 100: 0.20 and 0.38). The push speed of the contact and the stiffness of the joints change
/// nothing here (no joint, no deep overlap). Both cures cost something: the third pass 23 % of the
/// step time and a bridge of 6 planks ending lower, the lower damping a stack of 5 boxes under a
/// top 70 times as dense at steps of 30 Hz in 3D (4 of 5 settle, `reviews/8-59.md`). The choice is
/// card 8-68.
#[test]
fn the_contact_stiffness_and_damping_keep_the_44_row_pyramid_moving_and_either_cure_calms_it()
-> Result<(), PhysicsError> {
    let name = "stiff pyramid 44 rows, variants";
    let speed = |solver: SolverConfig| -> Result<f32, PhysicsError> {
        let mut scene = pyramid_with(44, W1_HALF, SleepConfig::DISABLED, solver)?;
        Ok(observe(&mut scene, W1_STEPS)?.final_speed)
    };
    let sixty = SolverConfig {
        contact_hertz: 60.0,
        ..PRESET
    };
    below(
        name,
        "8 sub-steps, 2 passes, 60 Hz (m/s)",
        speed(sixty)?,
        CALM_SPEED_44,
    );
    let damped = SolverConfig {
        contact_damping_ratio: 5.0,
        ..PRESET
    };
    below(
        name,
        "preset with damping 5 (m/s)",
        speed(damped)?,
        CALM_SPEED_44,
    );
    let third_pass = SolverConfig {
        relax_iterations: 3,
        ..PRESET
    };
    below(
        name,
        "preset with a third pass (m/s)",
        speed(third_pass)?,
        THIRD_PASS_SPEED,
    );
    let stiff_default = SolverConfig {
        contact_hertz: 60.0,
        ..SolverConfig::DEFAULT
    };
    above(
        name,
        "default sub-steps at 60 Hz (m/s)",
        speed(stiff_default)?,
        SLOWER_SETTLE_FLOOR,
    );
    Ok(())
}

/// A calm 44-row pyramid at 300 steps (0.00 measured at 60 Hz), and with a third relax pass (0.014).
const CALM_SPEED_44: f32 = 0.02;
const THIRD_PASS_SPEED: f32 = 0.05;
