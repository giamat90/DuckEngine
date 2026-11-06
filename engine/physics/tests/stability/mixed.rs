//! The mixed scene (exit criterion: "stacking/chain stability scenes pass"): a pyramid, a chain and
//! a bin of circles in one world, and the three touch: the chain hangs from a pin just above the
//! apex of the pyramid (the end of the hanging chain touches the apex, or the whip hits it), and the
//! first circle of the bin starts 0.05 m from the foot of the pyramid. Contacts of several kinds, joints
//! and the colouring share one solve and one island.

use super::limits::{DRIFT, HINGE_ERROR, PENETRATION, W1_STEPS};
use super::measure::{all_finite, below, check_energy, observe};
use super::scene::{
    ChainPlan, Lie, Scene, W1_HALF, chain, dynamic_ball, place_pyramid, static_box,
    world_with_floor,
};
use base::math::Vec2;
use physics::d2::{BodyHandle, PhysicsError, SleepConfig};

/// The pyramid, the chain and the circles of the mixed scene.
pub struct Mixed {
    /// The world and the boxes of the pyramid (the tracked bodies).
    pub scene: Scene,
    pub links: Vec<BodyHandle>,
    pub circles: Vec<BodyHandle>,
}

/// Rows of the pyramid.
const ROWS: u8 = 6;

/// Links of the chain.
const LINKS: u8 = 8;

/// No body of the whip scene may move faster than this, in metres per second (an explosion bound;
/// the fastest body measured is at 5.2 m/s when the whip hits the apex).
const MAX_BODY_SPEED: f32 = 15.0;

/// Radius of a circle in the bin, in metres.
const RADIUS: f32 = 0.3;

/// The foot of the pyramid reaches this far to the right (6 boxes of 0.5 m, centred on x = 0), and
/// the apex is this high above the floor, in metres.
const PYRAMID_FOOT: f32 = 1.5;
const PYRAMID_TOP: f32 = 3.0;

/// The bin of circles has a wall on its right only (the pyramid is its left side): the inside ends
/// at `BIN_RIGHT` on x and `BIN_TOP` on y.
const BIN_RIGHT: f32 = 3.5;
const BIN_TOP: f32 = 2.0;

/// Where the circles start: a lower row resting almost on the floor, the first 0.05 m from the foot
/// of the pyramid, and an upper row in the gaps.
const CIRCLES: [(f32, f32); 5] = [
    (1.85, 0.5),
    (2.45, 0.5),
    (3.05, 0.5),
    (2.15, 1.2),
    (2.75, 1.2),
];

/// A pyramid centred on x = 0, a chain hung from just above its apex and five circles in a bin on
/// its right.
pub fn mixed(lie: Lie, sleep: SleepConfig) -> Result<Mixed, PhysicsError> {
    let mut world = world_with_floor(40, 16, sleep)?;
    let mut boxes = Vec::new();
    place_pyramid(&mut world, &mut boxes, ROWS, W1_HALF, 0.0)?;
    let top = Vec2::new(0.0, PYRAMID_TOP + 0.5 * f32::from(LINKS));
    let plan = ChainPlan {
        top,
        links: LINKS,
        lie,
        last_density: 5.0,
    };
    let links = chain(&mut world, plan)?;
    let wall = Vec2::new(0.25, 1.0);
    static_box(&mut world, Vec2::new(BIN_RIGHT + 0.25, 1.0), wall)?;
    let mut circles = Vec::new();
    for (x, y) in CIRCLES {
        circles.push(dynamic_ball(&mut world, Vec2::new(x, y), RADIUS, 1.0)?);
    }
    let scene = Scene {
        world,
        tracked: boxes,
        hang: top,
    };
    Ok(Mixed {
        scene,
        links,
        circles,
    })
}

/// Asserts that every circle is still in the bin.
fn assert_circles_in_bin(mixed: &Mixed) -> Result<(), PhysicsError> {
    for &circle in &mixed.circles {
        let at = mixed.scene.world.body(circle)?.position();
        assert!(
            at.x > PYRAMID_FOOT - RADIUS && at.x < BIN_RIGHT && at.y > 0.0 && at.y < BIN_TOP,
            "a circle left the bin: {at:?}"
        );
    }
    Ok(())
}

#[test]
fn a_chain_hanging_on_the_apex_and_a_bin_of_circles_beside_the_pyramid_all_stay_in_order()
-> Result<(), PhysicsError> {
    let mut mixed = mixed(Lie::Hanging, SleepConfig::DISABLED)?;
    let name = "mixed (pyramid 6, chain 8 on the apex, 5 circles)";
    let report = observe(&mut mixed.scene, W1_STEPS)?;
    below(name, "pyramid max drift (m)", report.max_drift, DRIFT);
    below(
        name,
        "worst hinge error (m)",
        report.worst_hinge,
        HINGE_ERROR,
    );
    below(
        name,
        "max penetration (m)",
        report.max_penetration,
        PENETRATION,
    );
    check_energy(name, &report);
    assert_circles_in_bin(&mixed)?;
    let tip = mixed.links.last().ok_or(PhysicsError::StaleHandle)?;
    let at = mixed.scene.world.body(*tip)?.position();
    assert!(
        at.y > PYRAMID_TOP && at.y < mixed.scene.hang.y - 1.0,
        "the chain rests on the apex: the tip is at {at:?}"
    );
    Ok(())
}

/// The whip hits the apex of the pyramid at several metres per second and scatters boxes: the
/// pyramid is not held to its drift bound here, the joints and the world are.
#[test]
fn a_whip_that_hits_the_apex_keeps_its_hinges_and_the_world_finite() -> Result<(), PhysicsError> {
    let mut mixed = mixed(Lie::Horizontal, SleepConfig::DISABLED)?;
    let name = "mixed whip (pyramid 6, whip 8, 5 circles)";
    let report = observe(&mut mixed.scene, W1_STEPS)?;
    assert!(
        report.max_drift > DRIFT,
        "the whip does hit the pyramid ({} m)",
        report.max_drift
    );
    below(
        name,
        "worst hinge error (m)",
        report.worst_hinge,
        HINGE_ERROR,
    );
    below(name, "fastest body (m/s)", report.max_speed, MAX_BODY_SPEED);
    check_energy(name, &report);
    assert!(all_finite(&mixed.scene.world), "{name}: not finite");
    Ok(())
}
