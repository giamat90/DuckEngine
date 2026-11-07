//! The distance joint against closed forms and bounds fixed before they were measured (card 4-16).

use super::common::{
    DT, ball, downward_crossing, pin, rod, rod_error, step_n, world, world_with_gravity,
};
use base::math::Vec2;
use physics::d2::{
    DistanceJoint2, Joint2, JointDesc, LengthLimits, PhysicsError, STANDARD_GRAVITY, Spring,
};
use std::f32::consts::PI;

fn spring_joint(
    first: physics::d2::BodyHandle,
    second: physics::d2::BodyHandle,
    spring: Spring,
    limits: Option<LengthLimits>,
) -> JointDesc {
    JointDesc::new(
        first,
        second,
        Joint2::Distance(DistanceJoint2 {
            spring: Some(spring),
            limits,
            ..DistanceJoint2::new(Vec2::ZERO, Vec2::ZERO, 2.0)
        }),
    )
}

#[test]
fn a_rigid_rod_keeps_its_length_under_a_swinging_load() -> Result<(), PhysicsError> {
    let mut world = world()?;
    let anchor = pin(&mut world, 0.0, 0.0)?;
    let bob = ball(&mut world, 0.25, Vec2::new(1.4, -1.4))?;
    let length = (1.4_f32 * 1.4 * 2.0).sqrt();
    let joint = rod(&mut world, anchor, bob, (Vec2::ZERO, Vec2::ZERO), length)?;
    world.set_linear_velocity(bob, Vec2::new(2.0, 2.0))?;
    let mut worst = 0.0_f32;
    for _ in 0..300 {
        world.step(DT)?;
        worst = worst.max(rod_error(&world, joint)?);
    }
    assert!(worst < 0.01, "worst length error {worst} m");
    Ok(())
}

#[test]
fn a_rod_under_a_heavy_load_stays_within_a_centimetre() -> Result<(), PhysicsError> {
    let mut world = world()?;
    let anchor = pin(&mut world, 0.0, 5.0)?;
    let mut desc = physics::d2::BodyDesc::new(
        physics::d2::BodyKind::Dynamic,
        Vec2::new(0.0, 3.0),
        physics::d2::Shape2::Circle { radius: 0.5 },
    );
    desc.density = 50.0;
    let load = world.create_body(&desc)?;
    let joint = rod(&mut world, anchor, load, (Vec2::ZERO, Vec2::ZERO), 2.0)?;
    let mut worst = 0.0_f32;
    for _ in 0..240 {
        world.step(DT)?;
        worst = worst.max(rod_error(&world, joint)?);
    }
    assert!(worst < 0.01, "worst length error {worst} m");
    Ok(())
}

#[test]
fn a_rod_swings_like_a_pendulum() -> Result<(), PhysicsError> {
    let mut world = world()?;
    let anchor = pin(&mut world, 0.0, 0.0)?;
    let (cos, sin) = physics::d2::Rot2::from_angle(0.1).direction();
    let bob = ball(&mut world, 0.25, Vec2::new(2.0 * sin, -2.0 * cos))?;
    // A rod does not turn the bob, so the bob is a point mass: T = 2 pi sqrt(L / g) ... but its
    // spin is free and unloaded, so only the point-mass formula applies.
    rod(&mut world, anchor, bob, (Vec2::ZERO, Vec2::ZERO), 2.0)?;
    let mut xs = vec![world.body(bob)?.position().x];
    for _ in 0..720 {
        world.step(DT)?;
        xs.push(world.body(bob)?.position().x);
    }
    let expected = 2.0 * PI * (2.0 / -STANDARD_GRAVITY.y).sqrt();
    let (first, next) = downward_crossing(&xs, 0).ok_or(PhysicsError::NotFinite)?;
    let (second, _) = downward_crossing(&xs, next).ok_or(PhysicsError::NotFinite)?;
    let period = second - first;
    assert!(
        (period - expected).abs() < 0.02 * expected,
        "period {period} against {expected}"
    );
    Ok(())
}

/// A ball pulled 0.5 m past the rest length of a spring joint in zero gravity; the samples are the
/// stretch (length minus rest length) at every step.
fn stretches(hertz: f32, damping_ratio: f32, steps: u32) -> Result<Vec<f32>, PhysicsError> {
    let mut world = world_with_gravity(Vec2::ZERO)?;
    let anchor = pin(&mut world, 0.0, 0.0)?;
    let bob = ball(&mut world, 0.25, Vec2::new(2.5, 0.0))?;
    let spring = Spring {
        hertz,
        damping_ratio,
    };
    world.create_joint(&spring_joint(anchor, bob, spring, None))?;
    let mut samples = vec![world.body(bob)?.position().x - 2.0];
    for _ in 0..steps {
        world.step(DT)?;
        samples.push(world.body(bob)?.position().x - 2.0);
    }
    Ok(samples)
}

#[test]
fn a_spring_oscillates_at_its_frequency() -> Result<(), PhysicsError> {
    for hertz in [1.0_f32, 2.0, 3.0] {
        let samples = stretches(hertz, 0.0, 240)?;
        let (first, next) = downward_crossing(&samples, 0).ok_or(PhysicsError::NotFinite)?;
        let (second, _) = downward_crossing(&samples, next).ok_or(PhysicsError::NotFinite)?;
        let period = second - first;
        let expected = 1.0 / hertz;
        // The step is 1/60 s: a crossing is found to a fraction of a step.
        assert!(
            (period - expected).abs() < 0.03 * expected,
            "{hertz} Hz: period {period} against {expected}"
        );
    }
    Ok(())
}

#[test]
fn an_undamped_spring_keeps_its_amplitude_and_a_damped_one_loses_it() -> Result<(), PhysicsError> {
    let free = stretches(2.0, 0.0, 240)?;
    let peak = free
        .iter()
        .skip(60)
        .fold(0.0_f32, |worst, value| worst.max(value.abs()));
    // The soft step is implicit, so it takes energy out of an undamped spring: about half of the
    // amplitude in the first second at 2 Hz and four sub-steps (measured 0.26 of 0.5). The
    // bound says it never gains energy and does not die out.
    assert!(peak > 0.2 && peak < 0.505, "undamped peak {peak}");
    let damped = stretches(2.0, 0.7, 240)?;
    let tail = damped
        .iter()
        .skip(180)
        .fold(0.0_f32, |worst, value| worst.max(value.abs()));
    assert!(tail < 0.02, "damped tail {tail}");
    Ok(())
}

#[test]
fn a_spring_with_hertz_zero_and_a_maximum_is_a_rope() -> Result<(), PhysicsError> {
    let mut world = world()?;
    let anchor = pin(&mut world, 0.0, 0.0)?;
    let bob = ball(&mut world, 0.25, Vec2::new(0.0, -1.0))?;
    let rope = Spring {
        hertz: 0.0,
        damping_ratio: 0.0,
    };
    let limits = Some(LengthLimits { min: 0.0, max: 2.0 });
    let joint = world.create_joint(&spring_joint(anchor, bob, rope, limits))?;
    // Slack: in free fall until the rope is taut (1 m of fall takes about 0.45 s).
    step_n(&mut world, 20)?;
    let fallen = world.body(bob)?.position().y + 1.0;
    let free_fall = 0.5 * STANDARD_GRAVITY.y.abs() * (20.0 * DT) * (20.0 * DT);
    assert!(
        (-fallen - free_fall).abs() < 0.03,
        "fell {} against {free_fall}",
        -fallen
    );
    let mut longest = 0.0_f32;
    for _ in 0..240 {
        world.step(DT)?;
        let at = world.body(bob)?.position();
        longest = longest.max(at.length());
    }
    assert!(longest < 2.03, "stretched to {longest}");
    assert!(rod_error(&world, joint)? < 0.03);
    Ok(())
}

#[test]
fn a_minimum_length_stops_a_ball_that_flies_at_the_anchor() -> Result<(), PhysicsError> {
    let mut world = world_with_gravity(Vec2::ZERO)?;
    let anchor = pin(&mut world, 0.0, 0.0)?;
    let bob = ball(&mut world, 0.25, Vec2::new(2.0, 0.0))?;
    let strut = Spring {
        hertz: 0.0,
        damping_ratio: 0.0,
    };
    let limits = Some(LengthLimits { min: 1.0, max: 3.0 });
    world.create_joint(&spring_joint(anchor, bob, strut, limits))?;
    world.set_linear_velocity(bob, Vec2::new(-3.0, 0.0))?;
    let mut nearest = f32::MAX;
    for _ in 0..120 {
        world.step(DT)?;
        nearest = nearest.min(world.body(bob)?.position().length());
    }
    assert!(nearest > 0.96, "got to {nearest}");
    Ok(())
}

#[test]
fn a_rod_between_two_free_bodies_keeps_them_apart_and_conserves_momentum()
-> Result<(), PhysicsError> {
    let mut world = world_with_gravity(Vec2::ZERO)?;
    let a = ball(&mut world, 0.25, Vec2::new(0.0, 0.0))?;
    let b = ball(&mut world, 0.5, Vec2::new(2.0, 0.0))?;
    let joint = rod(&mut world, a, b, (Vec2::ZERO, Vec2::ZERO), 2.0)?;
    world.set_linear_velocity(a, Vec2::new(0.0, 3.0))?;
    step_n(&mut world, 180)?;
    let total = {
        let (first, second) = (world.body(a)?, world.body(b)?);
        first.linear_velocity() * first.mass() + second.linear_velocity() * second.mass()
    };
    let expected = Vec2::new(0.0, 3.0) * world.body(a)?.mass();
    assert!((total - expected).length() < 1e-3, "{total:?}");
    assert!(rod_error(&world, joint)? < 0.01);
    Ok(())
}

#[test]
fn a_spring_with_limits_is_held_inside_them_by_a_fast_ball() -> Result<(), PhysicsError> {
    let mut world = world_with_gravity(Vec2::ZERO)?;
    let anchor = pin(&mut world, 0.0, 0.0)?;
    let bob = ball(&mut world, 0.25, Vec2::new(2.0, 0.0))?;
    let spring = Spring {
        hertz: 2.0,
        damping_ratio: 0.5,
    };
    let limits = Some(LengthLimits { min: 1.5, max: 2.5 });
    world.create_joint(&spring_joint(anchor, bob, spring, limits))?;
    // Fast enough that the 2 Hz spring alone cannot stop it before the limit (kinetic energy 200 m
    // against about 20 m stored by half a metre of stretch).
    world.set_linear_velocity(bob, Vec2::new(20.0, 0.0))?;
    let mut longest = 0.0_f32;
    for _ in 0..240 {
        world.step(DT)?;
        longest = longest.max(world.body(bob)?.position().length());
    }
    assert!(longest < 2.55, "stretched to {longest}");
    assert!(longest > 2.4, "reached the limit: {longest}");
    Ok(())
}

#[test]
fn equal_limits_hold_a_spring_like_a_rod() -> Result<(), PhysicsError> {
    let mut world = world_with_gravity(Vec2::ZERO)?;
    let anchor = pin(&mut world, 0.0, 0.0)?;
    let bob = ball(&mut world, 0.25, Vec2::new(2.5, 0.0))?;
    let spring = Spring {
        hertz: 2.0,
        damping_ratio: 0.5,
    };
    let limits = Some(LengthLimits { min: 2.0, max: 2.0 });
    world.create_joint(&spring_joint(anchor, bob, spring, limits))?;
    world.set_linear_velocity(bob, Vec2::new(0.0, 3.0))?;
    step_n(&mut world, 60)?;
    let mut worst = 0.0_f32;
    for _ in 0..120 {
        world.step(DT)?;
        worst = worst.max((world.body(bob)?.position().length() - 2.0).abs());
    }
    assert!(worst < 0.05, "length off by {worst}");
    Ok(())
}

#[test]
fn a_rod_whose_anchors_coincide_builds_no_impulse_against_nothing() -> Result<(), PhysicsError> {
    // Two anchors on one point and a rod of length 1: there is no direction to pull along. The
    // ball must not be thrown when the anchors part (no hidden impulse piles up meanwhile).
    let mut world = world_with_gravity(Vec2::ZERO)?;
    let anchor = pin(&mut world, 0.0, 0.0)?;
    let bob = ball(&mut world, 0.25, Vec2::ZERO)?;
    rod(&mut world, anchor, bob, (Vec2::ZERO, Vec2::ZERO), 1.0)?;
    step_n(&mut world, 30)?;
    assert!(world.body(bob)?.linear_velocity().length() < 1e-6);
    Ok(())
}
