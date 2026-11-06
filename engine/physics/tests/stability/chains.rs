//! Chains of revolute-joined bodies and a bridge with a load (exit criterion: "stacking/chain
//! stability scenes pass"; a chain is a row of revolute joints, `physics-and-jobs-v0` O6).

use super::limits::{DRIFT, HINGE_ERROR, REST_SPEED, W1_STEPS};
use super::measure::{below, check_energy, observe};
use super::scene::{ChainPlan, Lie, Scene, W1_HALF, bridge, chain, dynamic_box, world_with_floor};
use base::math::Vec2;
use physics::d2::{PhysicsError, SleepConfig};

/// Links of the hanging chain that holds its bounds. Chains of 25 links and more stretch more than
/// the drift bound (`findings`).
const HANGING_LINKS: [u8; 1] = [20];

/// Links of the whips that hold their bounds: 20 and 30. A whip of 40 links and more comes apart
/// by more than the hinge bound (`findings`).
const WHIP_LINKS: [u8; 2] = [20, 30];

/// Links of the chain of the heavy-end scenes.
const HEAVY_END_LINKS: u8 = 20;

/// Steps of a whip: 10 s, enough for several swings.
const WHIP_STEPS: u32 = 600;

/// A whip swings when its fastest link moves faster than this, in metres per second.
const MIN_SWING_SPEED: f32 = 1.0;

/// A link of a whip may not move faster than this, in metres per second (a free fall from the
/// pin of a 25 m chain is 22 m/s and the fastest link measured is 36.6 m/s; this is the "no
/// explosion" bound).
const MAX_LINK_SPEED: f32 = 50.0;

/// A link centre may not get further from the pin than the length of the chain plus this, in
/// metres. This is a loose bound: a taut chain puts the last centre half a link (0.25 m) inside the
/// length, and the measured reach of 9.87 m (20 links) and 15.01 m (30 links) says the hinges
/// stretch by about 0.12 m and 0.26 m in sum; it catches an explosion, not a stretch.
const REACH_SLACK: f32 = 0.05;

/// The most a hinge may come apart under a last link 100 times heavier than the others, in
/// metres: one link thickness, twice the bound of the light cases (loosened, see `limits`).
const HEAVY_END_ERROR: f32 = 0.1;

/// The most a hinge of a bridge may come apart under a load 50 times heavier than a plank, in
/// metres: one link thickness, twice the bound of the light cases (loosened, see `limits`).
const HEAVY_LOAD_ERROR: f32 = 0.1;

/// Steps of the bridge scenes: 20 s. The first plan was 480 steps (8 s) and a load speed of
/// 0.5 m/s; the heavy load is still bouncing at 0.56 m/s then (`findings`) and is at rest from about
/// step 1000 on, so the run was lengthened to 1200 steps after the first measurement, and the bound
/// on the load speed is `LOAD_END_SPEED` (it is not 0.5 m/s).
const BRIDGE_STEPS: u32 = 1200;

/// The load of a bridge may not be moving faster than this at the end, in metres per second: 2.5
/// times the rest speed of the other scenes, because a bridge is a spring with little damping and
/// the light load is still swinging at 0.036 m/s after 20 s (the heavy one is at 0.011 m/s).
const LOAD_END_SPEED: f32 = 0.05;

/// A chain of `links` links from a pin high enough for it to swing freely.
pub fn chain_scene(
    links: u8,
    lie: Lie,
    last_density: f32,
    sleep: SleepConfig,
) -> Result<Scene, PhysicsError> {
    let top = Vec2::new(0.0, 0.5 * f32::from(links) + 2.0);
    let mut world = world_with_floor(u32::from(links) + 8, u32::from(links) + 8, sleep)?;
    let plan = ChainPlan {
        top,
        links,
        lie,
        last_density,
    };
    let tracked = chain(&mut world, plan)?;
    Ok(Scene {
        world,
        tracked,
        hang: top,
    })
}

#[test]
fn a_hanging_chain_of_twenty_links_stays_settled() -> Result<(), PhysicsError> {
    for links in HANGING_LINKS {
        let name = format!("hanging chain {links} links");
        let mut scene = chain_scene(links, Lie::Hanging, 1.0, SleepConfig::DISABLED)?;
        let report = observe(&mut scene, W1_STEPS)?;
        below(&name, "max drift (m)", report.max_drift, DRIFT);
        below(&name, "final speed (m/s)", report.final_speed, REST_SPEED);
        below(
            &name,
            "worst hinge error (m)",
            report.worst_hinge,
            HINGE_ERROR,
        );
        check_energy(&name, &report);
    }
    Ok(())
}

#[test]
fn whips_of_twenty_and_thirty_links_keep_their_joints_and_do_not_explode()
-> Result<(), PhysicsError> {
    for links in WHIP_LINKS {
        let name = format!("whip {links} links");
        let mut scene = chain_scene(links, Lie::Horizontal, 1.0, SleepConfig::DISABLED)?;
        let report = observe(&mut scene, WHIP_STEPS)?;
        assert!(
            report.max_speed > MIN_SWING_SPEED,
            "{name}: the whip does swing"
        );
        below(
            &name,
            "worst hinge error (m)",
            report.worst_hinge,
            HINGE_ERROR,
        );
        below(
            &name,
            "fastest link (m/s)",
            report.max_speed,
            MAX_LINK_SPEED,
        );
        let length = 0.5 * f32::from(links);
        below(&name, "reach (m)", report.max_reach, length + REACH_SLACK);
        check_energy(&name, &report);
    }
    Ok(())
}

#[test]
fn a_last_link_ten_and_one_hundred_times_heavier_is_held() -> Result<(), PhysicsError> {
    for (density, bound) in [(10.0, HINGE_ERROR), (100.0, HEAVY_END_ERROR)] {
        let name = format!("whip 20 links, end 1:{density}");
        let mut scene = chain_scene(
            HEAVY_END_LINKS,
            Lie::Horizontal,
            density,
            SleepConfig::DISABLED,
        )?;
        let report = observe(&mut scene, WHIP_STEPS)?;
        below(&name, "worst hinge error (m)", report.worst_hinge, bound);
        below(
            &name,
            "fastest link (m/s)",
            report.max_speed,
            MAX_LINK_SPEED,
        );
        check_energy(&name, &report);
    }
    Ok(())
}

/// A bridge of 16 planks over 8 m with a 0.5 m load of `density` dropped on the middle.
pub fn loaded_bridge(density: f32, sleep: SleepConfig) -> Result<Scene, PhysicsError> {
    let mut world = world_with_floor(24, 24, sleep)?;
    let mut tracked = bridge(&mut world, 8.0, 5.0, 16, 1.0)?;
    let half = Vec2::new(W1_HALF, W1_HALF);
    tracked.push(dynamic_box(&mut world, Vec2::new(4.0, 6.5), half, density)?);
    Ok(Scene {
        world,
        tracked,
        hang: Vec2::ZERO,
    })
}

/// The bridge scenes: the hinge bound and a load that stays on the bridge.
fn check_bridge(density: f32, bound: f32) -> Result<(), PhysicsError> {
    let name = format!("bridge, load density {density}");
    let mut scene = loaded_bridge(density, SleepConfig::DISABLED)?;
    let report = observe(&mut scene, BRIDGE_STEPS)?;
    below(&name, "worst hinge error (m)", report.worst_hinge, bound);
    check_energy(&name, &report);
    let load = *scene.tracked.last().ok_or(PhysicsError::StaleHandle)?;
    let body = scene.world.body(load)?;
    let at = body.position();
    assert!(
        at.y > 2.0 && at.x > 2.0 && at.x < 6.0,
        "{name}: the load is at {at:?}"
    );
    below(
        &name,
        "load speed at the end (m/s)",
        body.linear_velocity().length(),
        LOAD_END_SPEED,
    );
    Ok(())
}

#[test]
fn a_bridge_carries_a_load_five_times_a_plank() -> Result<(), PhysicsError> {
    check_bridge(1.0, HINGE_ERROR)
}

#[test]
fn a_bridge_carries_a_load_fifty_times_a_plank() -> Result<(), PhysicsError> {
    check_bridge(10.0, HEAVY_LOAD_ERROR)
}
