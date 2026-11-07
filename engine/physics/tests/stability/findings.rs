//! Findings of card 4-17: scenes whose bound, written down before the first measurement, the
//! solver does NOT meet. Each test here asserts the opposite on purpose: that the bound is still
//! exceeded, and by no more than an envelope that was fixed AFTER the measurement (the measured
//! value and the envelope are in `reviews/4-17.md`; each envelope is 10 to 40 % above the measured
//! value, so a finding that gets clearly worse turns its test red, a small change does not). The
//! tests keep the finding visible; when a solver change fixes one, its test fails with the message
//! of `above`, and the scene moves to the settled tests of its topic with the original bound.

use super::chains::{chain_scene, loaded_bridge};
use super::limits::{DRIFT, HINGE_ERROR, W1_STEPS};
use super::measure::{above, below, observe, positions};
use super::scene::{Lie, W1_HALF, column, pyramid};
use physics::d2::{PhysicsError, SleepConfig};

/// The envelope of a finding that exceeds the drift bound by a small amount: a drift below this
/// many metres. Fixed after the measurements (0.0527 and 0.0516).
const SMALL_DRIFT_ENVELOPE: f32 = 0.06;

/// A column or a stack "has come apart" when a box ends further than this from its start, in
/// metres (the findings measured 0.8 to 13 m).
const COLLAPSE: f32 = 0.5;

/// Steps over which a collapsing column is watched: 10 s.
const COLLAPSE_STEPS: u32 = 600;

/// The settled drift of the W1-shaped pyramid: the displacement between step 100 and step 300 must
/// stay below this many metres (measured: 0.0049). It separates the compression of the soft
/// contacts, which happens in the first second, from motion.
const SETTLED_DRIFT: f32 = 0.01;

/// The step at which the settled drift starts to be measured.
const SETTLED_FROM: u32 = 100;

#[test]
fn finding_the_w1_shaped_pyramid_of_twenty_rows_compresses_past_the_drift_bound()
-> Result<(), PhysicsError> {
    let name = "finding: pyramid 20 rows (210 boxes)";
    let mut scene = pyramid(20, W1_HALF, SleepConfig::DISABLED)?;
    let start = positions(&scene)?;
    let mut at_100 = Vec::new();
    for step in 1..=W1_STEPS {
        scene.world.step(super::limits::DT)?;
        if step == SETTLED_FROM {
            at_100 = positions(&scene)?;
        }
    }
    let end = positions(&scene)?;
    let drift = |a: &[base::math::Vec2], b: &[base::math::Vec2]| {
        a.iter()
            .zip(b)
            .map(|(p, q)| (*p - *q).length())
            .fold(0.0, f32::max)
    };
    above(
        name,
        "drift from the start (m), bound exceeded",
        drift(&start, &end),
        DRIFT,
    );
    below(
        name,
        "drift from the start (m), envelope",
        drift(&start, &end),
        SMALL_DRIFT_ENVELOPE,
    );
    below(
        name,
        "drift from step 100 to 300 (m)",
        drift(&at_100, &end),
        SETTLED_DRIFT,
    );
    Ok(())
}

#[test]
fn finding_a_column_of_eleven_boxes_compresses_past_the_drift_bound() -> Result<(), PhysicsError> {
    let name = "finding: column of 11";
    let mut scene = column(&[1.0; 11], W1_HALF, SleepConfig::DISABLED)?;
    let report = observe(&mut scene, W1_STEPS)?;
    above(
        name,
        "max drift (m), bound exceeded",
        report.max_drift,
        DRIFT,
    );
    below(
        name,
        "max drift (m), envelope",
        report.max_drift,
        SMALL_DRIFT_ENVELOPE,
    );
    Ok(())
}

#[test]
fn finding_a_column_of_twenty_boxes_topples() -> Result<(), PhysicsError> {
    let name = "finding: column of 20";
    let mut scene = column(&[1.0; 20], W1_HALF, SleepConfig::DISABLED)?;
    let report = observe(&mut scene, COLLAPSE_STEPS)?;
    above(name, "max drift (m), collapse", report.max_drift, COLLAPSE);
    Ok(())
}

#[test]
fn finding_alternating_boxes_of_mass_ratio_one_hundred_four_high_come_apart()
-> Result<(), PhysicsError> {
    let name = "finding: column 1:100:1:100";
    let mut scene = column(&[1.0, 100.0, 1.0, 100.0], W1_HALF, SleepConfig::DISABLED)?;
    let report = observe(&mut scene, W1_STEPS)?;
    above(name, "max drift (m), collapse", report.max_drift, COLLAPSE);
    Ok(())
}

/// The drift bound is exceeded by the stretch of a 50-link hanging chain: 49 soft hinges that each
/// stretch a few millimetres add up (the largest hinge error stays below this many metres).
const STRETCH_HINGE: f32 = 0.015;

/// The envelope of the stretch of the 50-link chain, in metres (measured 0.318).
const STRETCH_ENVELOPE: f32 = 0.36;

#[test]
fn finding_a_hanging_chain_of_fifty_links_stretches_past_the_drift_bound()
-> Result<(), PhysicsError> {
    let name = "finding: hanging chain 50 links";
    let mut scene = chain_scene(50, Lie::Hanging, 1.0, SleepConfig::DISABLED)?;
    let report = observe(&mut scene, W1_STEPS)?;
    above(
        name,
        "max drift (m), bound exceeded",
        report.max_drift,
        DRIFT,
    );
    below(
        name,
        "max drift (m), envelope",
        report.max_drift,
        STRETCH_ENVELOPE,
    );
    below(
        name,
        "worst hinge error (m)",
        report.worst_hinge,
        STRETCH_HINGE,
    );
    Ok(())
}

/// Steps of the 50-link whip: 10 s, as the whips of the settled tests.
const WHIP_STEPS: u32 = 600;

/// The envelope of the hinge error of the 50-link whip, in metres (measured 0.097).
const WHIP_ENVELOPE: f32 = 0.11;

#[test]
fn finding_a_whip_of_fifty_links_comes_apart_past_the_hinge_bound() -> Result<(), PhysicsError> {
    let name = "finding: whip 50 links";
    let mut scene = chain_scene(50, Lie::Horizontal, 1.0, SleepConfig::DISABLED)?;
    let report = observe(&mut scene, WHIP_STEPS)?;
    above(
        name,
        "worst hinge error (m), bound exceeded",
        report.worst_hinge,
        HINGE_ERROR,
    );
    below(
        name,
        "worst hinge error (m), envelope",
        report.worst_hinge,
        WHIP_ENVELOPE,
    );
    Ok(())
}

/// Steps of the early look at the heavy bridge: 8 s, the first plan of the bridge scenes.
const EARLY_BRIDGE_STEPS: u32 = 480;

/// The load of the heavy bridge is still moving faster than this after `EARLY_BRIDGE_STEPS` (the
/// first plan's bound was 0.5 m/s, measured 0.558) ...
const LOAD_BOUND: f32 = 0.5;

/// ... and slower than this (the envelope; the bridge is at rest 1.1 s later in the settled test).
const LOAD_ENVELOPE: f32 = 0.7;

#[test]
fn finding_the_load_of_the_heavy_bridge_is_still_bouncing_after_eight_seconds()
-> Result<(), PhysicsError> {
    let mut scene = loaded_bridge(10.0, SleepConfig::DISABLED)?;
    observe(&mut scene, EARLY_BRIDGE_STEPS)?;
    let load = *scene.tracked.last().ok_or(PhysicsError::StaleHandle)?;
    let speed = scene.world.body(load)?.linear_velocity().length();
    let name = "finding: heavy bridge";
    above(name, "load speed after 480 steps (m/s)", speed, LOAD_BOUND);
    below(
        name,
        "load speed after 480 steps (m/s), envelope",
        speed,
        LOAD_ENVELOPE,
    );
    Ok(())
}
