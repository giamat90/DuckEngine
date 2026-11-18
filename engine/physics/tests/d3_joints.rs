//! Card 8-7: the 3D joints. A ball-and-socket joint, a hinge (with limits) and a distance joint
//! (a rod, or a rope) hold bodies together while the world steps. The expected values come from the
//! physics (pendulum energy, a hinge that keeps its axis, a rope that stops at its length).

use base::dmath::{atan2, cos, sin};
use base::math::{Quat, Vec3};
use physics::core::PhysicsError;
use physics::d3::{
    AngleLimits, BallJoint3, BodyDesc, BodyHandle, BodyKind, DEFAULT_BROADPHASE_MARGIN,
    DEFAULT_SPECULATIVE_MARGIN, DistanceJoint3, HingeJoint3, Joint3, JointDesc, LengthLimits,
    STANDARD_GRAVITY, Shape3, SleepConfig, SolverConfig, World, WorldConfig,
};

/// The fixed step of every scene: 60 Hz.
const DT: f32 = 1.0 / 60.0;
const PIVOT_HEIGHT: f32 = 10.0;

fn world_with(gravity: Vec3, sleep: SleepConfig) -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: 64,
        gravity,
        max_pairs: 1024,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 1024,
        max_joints: 32,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep,
    })
}

fn world(gravity: Vec3) -> Result<World, PhysicsError> {
    world_with(gravity, SleepConfig::DISABLED)
}

fn ball(radius: f32) -> Shape3 {
    Shape3::Sphere { radius }
}

/// A static body that is the pivot of a scene, at height `PIVOT_HEIGHT`.
fn pivot(world: &mut World) -> Result<BodyHandle, PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, PIVOT_HEIGHT, 0.0),
        ball(0.1),
    ))
}

fn bob(
    world: &mut World,
    position: Vec3,
    configure: impl FnOnce(&mut BodyDesc),
) -> Result<BodyHandle, PhysicsError> {
    let mut desc = BodyDesc::new(BodyKind::Dynamic, position, ball(0.1));
    configure(&mut desc);
    world.create_body(&desc)
}

fn join(
    world: &mut World,
    first: BodyHandle,
    second: BodyHandle,
    shape: Joint3,
) -> Result<(), PhysicsError> {
    world
        .create_joint(&JointDesc::new(first, second, shape))
        .map(|_| ())
}

fn position(world: &World, handle: BodyHandle) -> Result<Vec3, PhysicsError> {
    Ok(world.body(handle)?.position())
}

fn speed(world: &World, handle: BodyHandle) -> Result<f32, PhysicsError> {
    Ok(world.body(handle)?.linear_velocity().length())
}

fn pivot_point() -> Vec3 {
    Vec3::new(0.0, PIVOT_HEIGHT, 0.0)
}

/// A bob 1 m to the side of the pivot, held to it by a ball-and-socket joint at the pivot.
fn pendulum(world: &mut World, length: f32) -> Result<BodyHandle, PhysicsError> {
    let pin = pivot(world)?;
    let weight = bob(world, pivot_point() + Vec3::new(length, 0.0, 0.0), |_| {})?;
    let joint = BallJoint3::new(Vec3::ZERO, Vec3::new(-length, 0.0, 0.0));
    join(world, pin, weight, Joint3::Ball(joint))?;
    Ok(weight)
}

#[test]
fn a_pendulum_swings_down_with_the_speed_energy_gives_and_keeps_its_length()
-> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    let weight = pendulum(&mut w, 1.0)?;
    let mut fastest = 0.0_f32;
    let mut longest = 0.0_f32;
    let mut shortest = f32::MAX;
    for _ in 0..90 {
        w.step(DT)?;
        let arm = (position(&w, weight)? - pivot_point()).length();
        longest = longest.max(arm);
        shortest = shortest.min(arm);
        fastest = fastest.max(speed(&w, weight)?);
    }
    // A quarter swing from rest: v = sqrt(2 g L) at the bottom (the bob's own spin takes 0.4%).
    let expected = (2.0 * 9.81_f32).sqrt();
    assert!(
        (fastest - expected).abs() < 0.02 * expected,
        "fastest {fastest}, expected {expected}"
    );
    assert!(
        longest < 1.005 && shortest > 0.995,
        "arm {shortest}..{longest}"
    );
    Ok(())
}

#[test]
fn a_pendulum_swings_with_the_period_of_a_pendulum() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    // A small angle: 0.1 rad from hanging straight down, length 1 m.
    let pin = pivot(&mut w)?;
    let start = pivot_point() + Vec3::new(sin(0.1), -cos(0.1), 0.0);
    let weight = bob(&mut w, start, |_| {})?;
    let joint = BallJoint3::new(Vec3::ZERO, pivot_point() - start);
    join(&mut w, pin, weight, Joint3::Ball(joint))?;
    // Count the steps until the bob crosses the vertical a second time: one full period.
    let mut crossings = 0;
    let mut steps = 0;
    let mut before = position(&w, weight)?.x;
    while crossings < 2 && steps < 600 {
        w.step(DT)?;
        steps += 1;
        let now = position(&w, weight)?.x;
        if before * now < 0.0 {
            crossings += 1;
        }
        before = now;
    }
    let period = 2.0 * std::f32::consts::PI * (1.0_f32 / 9.81).sqrt();
    let measured = f32::from(u16::try_from(steps).unwrap_or(u16::MAX)) * DT;
    // The first crossing is a quarter period away and the second three quarters: 2 crossings = 3/4 T.
    assert!(
        (measured - 0.75 * period).abs() < 0.1 * period,
        "measured {measured}, expected {}",
        0.75 * period
    );
    Ok(())
}

#[test]
fn a_ball_joint_with_the_anchors_apart_pulls_the_bodies_together() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let first = bob(&mut w, Vec3::new(-2.0, 0.0, 0.0), |_| {})?;
    let second = bob(&mut w, Vec3::new(2.0, 0.0, 0.0), |_| {})?;
    // Anchors on the two bodies that start 4 m apart in the world must be brought to one place.
    join(
        &mut w,
        first,
        second,
        Joint3::Ball(BallJoint3::new(Vec3::ZERO, Vec3::ZERO)),
    )?;
    for _ in 0..240 {
        w.step(DT)?;
    }
    let gap = (position(&w, first)? - position(&w, second)?).length();
    assert!(gap < 0.01, "the anchors are still {gap} m apart");
    Ok(())
}

#[test]
fn a_hinge_keeps_its_axis_whatever_the_bodies_are_pushed_to_do() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    let pin = pivot(&mut w)?;
    // A swinging arm hinged about the z axis, started with a push along z and a spin about x.
    let weight = bob(&mut w, pivot_point() + Vec3::new(1.0, 0.0, 0.0), |desc| {
        desc.linear_velocity = Vec3::new(0.0, 0.0, 2.0);
        desc.angular_velocity = Vec3::new(3.0, 0.0, 0.0);
    })?;
    let hinge = HingeJoint3::new(Vec3::ZERO, Vec3::new(-1.0, 0.0, 0.0), Vec3::Z, Vec3::Z);
    join(&mut w, pin, weight, Joint3::Hinge(hinge))?;
    let mut drift = 0.0_f32;
    let mut tilt = 0.0_f32;
    for _ in 0..180 {
        w.step(DT)?;
        let at = position(&w, weight)?;
        drift = drift.max(at.z.abs());
        let axis = w.body(weight)?.rotation().rotate(Vec3::Z);
        tilt = tilt.max(axis.cross(Vec3::Z).length());
    }
    assert!(drift < 0.005, "the arm left its plane by {drift} m");
    assert!(tilt < 0.005, "the hinge axis tilted by {tilt}");
    Ok(())
}

#[test]
fn a_hinge_turns_freely_about_its_axis() -> Result<(), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let pin = pivot(&mut w)?;
    let wheel = bob(&mut w, pivot_point(), |desc| {
        desc.angular_velocity = Vec3::new(0.0, 0.0, 4.0);
    })?;
    let hinge = HingeJoint3::new(Vec3::ZERO, Vec3::ZERO, Vec3::Z, Vec3::Z);
    join(&mut w, pin, wheel, Joint3::Hinge(hinge))?;
    for _ in 0..60 {
        w.step(DT)?;
    }
    let spin = w.body(wheel)?.angular_velocity();
    assert!(
        (spin.z - 4.0).abs() < 0.1,
        "the spin about the axis became {}",
        spin.z
    );
    Ok(())
}

/// The angle of a bob that hangs from a hinge about z: 0 straight down, positive counter-clockwise.
fn swing_angle(at: Vec3) -> f32 {
    let arm = at - pivot_point();
    atan2(arm.x, -arm.y)
}

#[test]
fn hinge_limits_stop_the_swing() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    let pin = pivot(&mut w)?;
    // Hanging 1 m under the pivot; the arm is the body's y axis, the hinge about z, angle 0 = down.
    let weight = bob(&mut w, pivot_point() + Vec3::new(0.0, -1.0, 0.0), |desc| {
        desc.linear_velocity = Vec3::new(4.0, 0.0, 0.0);
    })?;
    let hinge = HingeJoint3 {
        limits: Some(AngleLimits {
            lower: -0.5,
            upper: 0.5,
        }),
        ..HingeJoint3::new(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), Vec3::Z, Vec3::Z)
    };
    join(&mut w, pin, weight, Joint3::Hinge(hinge))?;
    let mut widest = 0.0_f32;
    for _ in 0..180 {
        w.step(DT)?;
        widest = widest.max(swing_angle(position(&w, weight)?).abs());
    }
    // Without limits the push would carry the bob far past 0.5 rad; it is held at the limit.
    assert!(widest > 0.45, "the bob never reached the limit: {widest}");
    assert!(
        widest < 0.52,
        "the bob went {widest} rad, past the limit of 0.5"
    );
    Ok(())
}

#[test]
fn a_reference_angle_moves_the_zero_of_the_hinge() -> Result<(), PhysicsError> {
    // The same hinge at rest, once with the limits around 0 and once around the reference angle:
    // the bodies start at the relative angle 0, so with a reference of 1 the joint angle starts at -1,
    // outside the limits of plus and minus 0.25, and the hinge is pushed into the range.
    let mut w = world(Vec3::ZERO)?;
    let pin = pivot(&mut w)?;
    let wheel = bob(&mut w, pivot_point(), |_| {})?;
    let hinge = HingeJoint3 {
        reference_angle: 1.0,
        limits: Some(AngleLimits {
            lower: -0.25,
            upper: 0.25,
        }),
        ..HingeJoint3::new(Vec3::ZERO, Vec3::ZERO, Vec3::Z, Vec3::Z)
    };
    join(&mut w, pin, wheel, Joint3::Hinge(hinge))?;
    for _ in 0..240 {
        w.step(DT)?;
    }
    // The joint angle is (relative angle - reference): the wheel starts at -1 and is pushed up to the
    // lower limit -0.25, which is a relative angle of 0.75 about z.
    let rotation = w.body(wheel)?.rotation();
    let turned = rotation.rotate(Vec3::X);
    let angle = atan2(turned.y, turned.x);
    assert!(
        (angle - 0.75).abs() < 0.08,
        "the wheel turned {angle} rad, expected 0.75"
    );
    Ok(())
}

#[test]
fn a_rod_keeps_its_length_while_the_bodies_swing() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    let pin = pivot(&mut w)?;
    let weight = bob(&mut w, pivot_point() + Vec3::new(2.0, 0.0, 0.0), |desc| {
        desc.linear_velocity = Vec3::new(0.0, 0.0, 1.0);
    })?;
    let rod = DistanceJoint3::new(Vec3::ZERO, Vec3::ZERO, 2.0);
    join(&mut w, pin, weight, Joint3::Distance(rod))?;
    let mut shortest = f32::MAX;
    let mut longest = 0.0_f32;
    for _ in 0..180 {
        w.step(DT)?;
        let length = (position(&w, weight)? - pivot_point()).length();
        shortest = shortest.min(length);
        longest = longest.max(length);
    }
    assert!(
        shortest > 1.99 && longest < 2.01,
        "length {shortest}..{longest}"
    );
    // Unlike a ball joint a rod does not forbid the bob from rising above the pivot's plane.
    Ok(())
}

#[test]
fn a_rope_is_slack_between_its_lengths_and_stops_at_the_longest() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    let pin = pivot(&mut w)?;
    let weight = bob(&mut w, pivot_point() + Vec3::new(0.0, -0.5, 0.0), |_| {})?;
    let rope = DistanceJoint3 {
        limits: Some(LengthLimits { min: 0.5, max: 2.0 }),
        ..DistanceJoint3::new(Vec3::ZERO, Vec3::ZERO, 1.0)
    };
    join(&mut w, pin, weight, Joint3::Distance(rope))?;
    // Slack: it falls freely for a while (a rod would hold it at 0.5 m).
    for _ in 0..12 {
        w.step(DT)?;
    }
    let fallen = pivot_point().y - position(&w, weight)?.y;
    assert!(
        fallen > 0.6,
        "the rope held the body at {fallen} m instead of letting it fall"
    );
    for _ in 0..180 {
        w.step(DT)?;
    }
    let length = (position(&w, weight)? - pivot_point()).length();
    assert!(
        length > 1.98 && length < 2.02,
        "the rope ended {length} m long"
    );
    Ok(())
}

#[test]
fn bodies_of_a_joint_make_no_contact_unless_the_joint_says_so() -> Result<(), PhysicsError> {
    for collide in [false, true] {
        let mut w = world(Vec3::ZERO)?;
        let first = bob(&mut w, Vec3::new(0.0, 0.0, 0.0), |_| {})?;
        let second = bob(&mut w, Vec3::new(0.05, 0.0, 0.0), |_| {})?;
        let mut desc = JointDesc::new(
            first,
            second,
            Joint3::Ball(BallJoint3::new(Vec3::ZERO, Vec3::new(-0.05, 0.0, 0.0))),
        );
        desc.collide_connected = collide;
        w.create_joint(&desc)?;
        w.step(DT)?;
        // The two spheres overlap by 0.15 m: only a contact between them makes a manifold.
        let manifolds = w.solver_stats().solved + w.solver_stats().frozen;
        assert_eq!(
            manifolds,
            usize::from(collide),
            "collide_connected {collide}"
        );
    }
    Ok(())
}

#[test]
fn a_chain_of_ten_hangs_still_and_falls_asleep() -> Result<(), PhysicsError> {
    let mut w = world_with(STANDARD_GRAVITY, SleepConfig::DEFAULT)?;
    let mut previous = pivot(&mut w)?;
    let mut last = previous;
    for link in 0..10_u8 {
        let y = PIVOT_HEIGHT - 0.5 - f32::from(link);
        let next = bob(&mut w, Vec3::new(0.0, y, 0.0), |_| {})?;
        let anchor_a = if link == 0 {
            Vec3::ZERO
        } else {
            Vec3::new(0.0, -0.5, 0.0)
        };
        let joint = BallJoint3::new(anchor_a, Vec3::new(0.0, 0.5, 0.0));
        join(&mut w, previous, next, Joint3::Ball(joint))?;
        previous = next;
        last = next;
    }
    for _ in 0..900 {
        w.step(DT)?;
    }
    let end = position(&w, last)?;
    assert!(
        end.y < PIVOT_HEIGHT - 9.0 && end.y > PIVOT_HEIGHT - 10.6,
        "the chain end is at {end:?}"
    );
    assert!(
        end.x.abs() < 0.05 && end.z.abs() < 0.05,
        "the chain hangs off the vertical: {end:?}"
    );
    assert!(w.is_asleep(last)?);
    Ok(())
}

#[test]
fn a_chain_thrown_sideways_stays_finite_and_attached() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    let mut previous = pivot(&mut w)?;
    let mut bodies = Vec::new();
    for link in 0..8_u8 {
        let x = 0.5 + f32::from(link);
        let next = bob(&mut w, pivot_point() + Vec3::new(x, 0.0, 0.0), |desc| {
            desc.angular_velocity = Vec3::new(5.0, 3.0, 8.0);
        })?;
        let anchor_a = if link == 0 {
            Vec3::ZERO
        } else {
            Vec3::new(0.5, 0.0, 0.0)
        };
        let joint = BallJoint3::new(anchor_a, Vec3::new(-0.5, 0.0, 0.0));
        join(&mut w, previous, next, Joint3::Ball(joint))?;
        previous = next;
        bodies.push(next);
    }
    for _ in 0..300 {
        w.step(DT)?;
    }
    let mut start = pivot_point();
    for body in bodies {
        let at = position(&w, body)?;
        assert!(at.is_finite_all(), "a link is not finite: {at:?}");
        assert!(
            (at - start).length() < 1.5,
            "a link came apart: {} m",
            (at - start).length()
        );
        start = at;
    }
    Ok(())
}

trait Finite {
    fn is_finite_all(&self) -> bool;
}

impl Finite for Vec3 {
    fn is_finite_all(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

#[test]
fn a_joint_to_a_body_that_is_destroyed_goes_with_it() -> Result<(), PhysicsError> {
    let mut w = world(STANDARD_GRAVITY)?;
    let weight = pendulum(&mut w, 1.0)?;
    assert_eq!(w.joint_count(), 1);
    w.destroy_body(weight)?;
    assert_eq!(w.joint_count(), 0);
    w.step(DT)?;
    Ok(())
}

#[test]
fn the_same_scene_run_twice_has_the_same_state_hash() -> Result<(), PhysicsError> {
    let mut hashes = Vec::new();
    for _ in 0..2 {
        let mut w = world(STANDARD_GRAVITY)?;
        let weight = pendulum(&mut w, 1.0)?;
        let other = bob(&mut w, Vec3::new(3.0, 8.0, 0.0), |desc| {
            desc.rotation =
                Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), 0.4).unwrap_or(Quat::IDENTITY);
        })?;
        let hinge = HingeJoint3::new(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), Vec3::Z, Vec3::Z);
        join(&mut w, weight, other, Joint3::Hinge(hinge))?;
        for _ in 0..120 {
            w.step(DT)?;
        }
        hashes.push(w.state_hash());
    }
    assert_eq!(hashes.first(), hashes.get(1));
    Ok(())
}

#[test]
fn a_joint_to_a_sleeping_body_wakes_nothing_by_itself_but_a_hit_on_one_wakes_both()
-> Result<(), PhysicsError> {
    let mut w = world_with(Vec3::ZERO, SleepConfig::DEFAULT)?;
    let first = bob(&mut w, Vec3::new(0.0, 0.0, 0.0), |_| {})?;
    let second = bob(&mut w, Vec3::new(1.0, 0.0, 0.0), |_| {})?;
    join(
        &mut w,
        first,
        second,
        Joint3::Distance(DistanceJoint3::new(Vec3::ZERO, Vec3::ZERO, 1.0)),
    )?;
    for _ in 0..120 {
        w.step(DT)?;
    }
    assert!(
        w.is_asleep(first)? && w.is_asleep(second)?,
        "the pair should sleep as one island"
    );
    bob(&mut w, Vec3::new(-1.0, 0.0, 0.0), |desc| {
        desc.linear_velocity = Vec3::new(3.0, 0.0, 0.0);
    })?;
    let mut woke = false;
    for _ in 0..60 {
        w.step(DT)?;
        woke |= !w.is_asleep(second)?;
    }
    assert!(
        woke,
        "the far end of the island stayed asleep after a hit on the near one"
    );
    Ok(())
}

#[test]
fn a_joint_between_awake_bodies_is_solved_and_one_between_sleepers_is_kept_as_it_is()
-> Result<(), PhysicsError> {
    let mut w = world_with(Vec3::ZERO, SleepConfig::DEFAULT)?;
    let first = bob(&mut w, Vec3::new(0.0, 0.0, 0.0), |_| {})?;
    let second = bob(&mut w, Vec3::new(1.0, 0.0, 0.0), |desc| {
        desc.linear_velocity = Vec3::new(0.0, 0.0, 1.0);
    })?;
    join(
        &mut w,
        first,
        second,
        Joint3::Distance(DistanceJoint3::new(Vec3::ZERO, Vec3::ZERO, 1.0)),
    )?;
    w.step(DT)?;
    let stats = w.solver_stats();
    assert_eq!((stats.joints_solved, stats.joints_frozen), (1, 0));
    // A rod between two bodies in free space keeps turning for ever: nothing sleeps. Stop it.
    w.destroy_body(second)?;
    let third = bob(&mut w, Vec3::new(1.0, 0.0, 0.0), |_| {})?;
    join(
        &mut w,
        first,
        third,
        Joint3::Distance(DistanceJoint3::new(Vec3::ZERO, Vec3::ZERO, 1.0)),
    )?;
    for _ in 0..120 {
        w.step(DT)?;
    }
    assert!(w.is_asleep(first)? && w.is_asleep(third)?);
    let stats = w.solver_stats();
    assert_eq!((stats.joints_solved, stats.joints_frozen), (0, 1));
    Ok(())
}

/// A body hinged to a post about z by an anchor 1 m from its centre, started `turn` radians from the
/// angle zero with its anchor on the post, in zero gravity, limits plus and minus 0.5.
fn hinged_at(turn: f32) -> Result<(World, BodyHandle), PhysicsError> {
    let mut w = world(Vec3::ZERO)?;
    let pin = pivot(&mut w)?;
    let rotation = Quat::from_axis_angle(Vec3::Z, turn).unwrap_or(Quat::IDENTITY);
    let arm = rotation.rotate(Vec3::X);
    let door = bob(&mut w, pivot_point() + arm, |desc| desc.rotation = rotation)?;
    let hinge = HingeJoint3 {
        limits: Some(AngleLimits {
            lower: -0.5,
            upper: 0.5,
        }),
        ..HingeJoint3::new(Vec3::ZERO, Vec3::new(-1.0, 0.0, 0.0), Vec3::Z, Vec3::Z)
    };
    join(&mut w, pin, door, Joint3::Hinge(hinge))?;
    Ok((w, door))
}

fn turn_about_z(w: &World, body: BodyHandle) -> Result<f32, PhysicsError> {
    let direction = w.body(body)?.rotation().rotate(Vec3::X);
    Ok(atan2(direction.y, direction.x))
}

#[test]
fn a_hinge_far_outside_its_limits_comes_back_to_them_and_stays_finite() -> Result<(), PhysicsError>
{
    for turn in [1.5, 2.0, 3.0, -2.0] {
        let (mut w, door) = hinged_at(turn)?;
        for _ in 0..300 {
            w.step(DT)?;
        }
        let angle = turn_about_z(&w, door)?;
        assert!(
            angle.abs() < 0.55 && angle.signum() == turn.signum(),
            "started at {turn}, ended at {angle}"
        );
        let at = position(&w, door)?;
        assert!(
            (at - pivot_point()).length() > 0.999 && (at - pivot_point()).length() < 1.001,
            "the anchor came off the post: {at:?}"
        );
    }
    Ok(())
}

#[test]
fn a_hinge_between_axes_that_differ_in_the_bodies_starts_at_angle_zero() -> Result<(), PhysicsError>
{
    // The first body is turned a quarter about y so its local z axis is the world x axis; the
    // second is not turned and hinges about its own x axis: the same line in the world.
    let mut w = world(Vec3::ZERO)?;
    let quarter =
        Quat::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2).unwrap_or(Quat::IDENTITY);
    let first = w.create_body(&BodyDesc {
        rotation: quarter,
        ..BodyDesc::new(BodyKind::Static, pivot_point(), ball(0.1))
    })?;
    let second = bob(&mut w, pivot_point(), |_| {})?;
    let hinge = HingeJoint3 {
        limits: Some(AngleLimits {
            lower: -0.1,
            upper: 0.1,
        }),
        ..HingeJoint3::new(Vec3::ZERO, Vec3::ZERO, Vec3::Z, Vec3::X)
    };
    join(&mut w, first, second, Joint3::Hinge(hinge))?;
    for _ in 0..60 {
        w.step(DT)?;
    }
    let turned = w.body(second)?.rotation();
    let (y, z) = (turned.rotate(Vec3::Y), turned.rotate(Vec3::Z));
    assert!(
        (y - Vec3::Y).length() < 0.02 && (z - Vec3::Z).length() < 0.02,
        "turned to {y:?} {z:?}"
    );
    Ok(())
}

/// Runs a small cube on an `arm` long hinge, started `turn` rad round with limits of +-0.5 rad, for
/// ten seconds; returns the fastest spin, the worst anchor error and the last angle.
fn far_outside(
    gravity: Vec3,
    arm: f32,
    half: f32,
    turn: f32,
) -> Result<(f32, f32, f32), PhysicsError> {
    let mut w = world(gravity)?;
    let pin = pivot(&mut w)?;
    let rotation = Quat::from_axis_angle(Vec3::Z, turn).unwrap_or(Quat::IDENTITY);
    let door = w.create_body(&BodyDesc {
        rotation,
        ..BodyDesc::new(
            BodyKind::Dynamic,
            pivot_point() + rotation.rotate(Vec3::new(arm, 0.0, 0.0)),
            Shape3::Box {
                half_extents: Vec3::new(half, half, half),
            },
        )
    })?;
    let limits = Some(AngleLimits {
        lower: -0.5,
        upper: 0.5,
    });
    let hinge = HingeJoint3 {
        limits,
        ..HingeJoint3::new(Vec3::ZERO, Vec3::new(-arm, 0.0, 0.0), Vec3::Z, Vec3::Z)
    };
    join(&mut w, pin, door, Joint3::Hinge(hinge))?;
    let (mut fastest, mut worst_gap) = (0.0_f32, 0.0_f32);
    for _ in 0..600 {
        w.step(DT)?;
        fastest = fastest.max(w.body(door)?.angular_velocity().length());
        worst_gap = worst_gap.max(((position(&w, door)? - pivot_point()).length() - arm).abs());
    }
    Ok((fastest, worst_gap, turn_about_z(&w, door)?))
}

/// Review 8-7, loop 2: a small body on a long arm that starts far outside its hinge limits used to
/// be thrown out of the world (a 0.1 m cube 3 m from the hinge, started 1 rad round). These bodies
/// are tiny for their arms (mass times arm squared over inertia above 1000): they stay finite and
/// inside the range, but may end on the far limit (a documented limit of the joints).
#[test]
fn a_small_body_on_a_long_arm_comes_back_from_far_outside_its_limits() -> Result<(), PhysicsError> {
    let scenes = [
        (3.0_f32, 0.1_f32, 1.0_f32),
        (2.0, 0.05, 1.5),
        (6.0, 0.1, -2.0),
    ];
    for (gravity, (arm, half, turn)) in [Vec3::ZERO, STANDARD_GRAVITY]
        .into_iter()
        .flat_map(|g| scenes.map(|scene| (g, scene)))
    {
        let (fastest, gap, angle) = far_outside(gravity, arm, half, turn)?;
        assert!(
            fastest < 20.0,
            "arm {arm} half {half} turn {turn}: spin {fastest}"
        );
        assert!(gap < 0.05, "arm {arm}: anchor off by {gap}");
        assert!(angle.abs() < 0.56, "arm {arm}: ended at {angle}");
    }
    Ok(())
}

/// Review 8-7, loop 3: a hinge whose second local axis is the opposite of the first used to flip
/// by half a turn after about twenty steps when its bodies were turned in the world (the opposite
/// pose was an unstable equilibrium that rounding noise tipped over).
#[test]
fn a_hinge_with_opposite_local_axes_stays_aligned_in_a_turned_frame() -> Result<(), PhysicsError> {
    for gravity in [Vec3::ZERO, STANDARD_GRAVITY] {
        let mut w = world(gravity)?;
        let turned = Quat::from_axis_angle(Vec3::new(1.0, 0.3, 0.2), 0.7).unwrap_or(Quat::IDENTITY);
        let desc = |position| BodyDesc {
            rotation: turned,
            ..BodyDesc::new(
                BodyKind::Dynamic,
                position,
                Shape3::Box {
                    half_extents: Vec3::new(0.5, 0.5, 0.5),
                },
            )
        };
        let first = w.create_body(&BodyDesc {
            kind: BodyKind::Static,
            ..desc(pivot_point())
        })?;
        let second = w.create_body(&desc(pivot_point() + Vec3::new(2.0, 0.0, 0.0)))?;
        let hinge = HingeJoint3::new(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::Z,
            -Vec3::Z,
        );
        join(&mut w, first, second, Joint3::Hinge(hinge))?;
        for _ in 0..300 {
            w.step(DT)?;
        }
        let axis_a = turned.rotate(Vec3::Z);
        let axis_b = w.body(second)?.rotation().rotate(Vec3::Z);
        assert!(
            axis_a.dot(axis_b) > 0.99,
            "gravity {gravity:?}: axes {axis_a:?} {axis_b:?}"
        );
    }
    Ok(())
}

/// A body that is not tiny for its arm ends at the limit it was started past (with gravity pulling
/// the door down, the lower limit, which is where the door hangs).
#[test]
fn a_body_far_outside_its_limits_ends_at_the_nearest_limit() -> Result<(), PhysicsError> {
    let scenes = [
        (Vec3::ZERO, 2.0_f32, 0.3_f32, 1.0_f32, 0.5_f32),
        (Vec3::ZERO, 2.0, 0.3, -2.0, -0.5),
        (Vec3::ZERO, 1.0, 0.2, 1.5, 0.5),
        (STANDARD_GRAVITY, 2.0, 0.3, 1.0, -0.5),
    ];
    for (gravity, arm, half, turn, expected) in scenes {
        let (fastest, gap, angle) = far_outside(gravity, arm, half, turn)?;
        assert!(
            fastest < 20.0 && gap < 0.05,
            "arm {arm}: spin {fastest} gap {gap}"
        );
        assert!(
            (angle - expected).abs() < 0.03,
            "arm {arm} turn {turn}: ended at {angle}"
        );
    }
    Ok(())
}
