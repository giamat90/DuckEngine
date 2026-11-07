//! The revolute joint against closed forms and bounds fixed before they were measured (card 4-16).

use super::common::{
    DT, angle, ball, downward_crossing, hinge_at, hinge_error, pin, plank, step_n, world,
    world_point, world_with_gravity,
};
use base::math::Vec2;
use physics::d2::{
    AngleLimits, Joint2, JointDesc, Motor, PhysicsError, RevoluteJoint2, Rot2, STANDARD_GRAVITY,
};
use std::f32::consts::PI;

/// The pendulum of the tests: a circle of radius `R` hung `L` below a pin by a hinge.
const L: f32 = 2.0;
const R: f32 = 0.25;
/// The amplitude, in radians: small enough for the small-angle formula.
const AMPLITUDE: f32 = 0.1;

#[test]
fn a_pendulum_swings_with_the_period_of_a_physical_pendulum() -> Result<(), PhysicsError> {
    let mut world = world()?;
    let pivot = pin(&mut world, 0.0, 0.0)?;
    let (cos, sin) = Rot2::from_angle(AMPLITUDE).direction();
    let bob = ball(&mut world, R, Vec2::new(L * sin, -L * cos))?;
    hinge_at(&mut world, pivot, bob, Vec2::ZERO)?;
    let mut xs = vec![world.body(bob)?.position().x];
    for _ in 0..720 {
        world.step(DT)?;
        xs.push(world.body(bob)?.position().x);
    }
    // Physical pendulum: T = 2 pi sqrt((L^2 + R^2 / 2) / (g L)) for a disc of radius R.
    let expected = 2.0 * PI * ((L * L + R * R / 2.0) / (-STANDARD_GRAVITY.y * L)).sqrt();
    let (first, next) = downward_crossing(&xs, 0).ok_or(PhysicsError::NotFinite)?;
    let (second, _) = downward_crossing(&xs, next).ok_or(PhysicsError::NotFinite)?;
    let period = second - first;
    assert!(
        (period - expected).abs() < 0.02 * expected,
        "period {period} against {expected}"
    );
    Ok(())
}

#[test]
fn the_pendulum_hinge_holds_its_anchors_together() -> Result<(), PhysicsError> {
    let mut world = world()?;
    let pivot = pin(&mut world, 0.0, 0.0)?;
    let bob = ball(&mut world, R, Vec2::new(L, 0.0))?;
    let joint = hinge_at(&mut world, pivot, bob, Vec2::ZERO)?;
    let mut worst = 0.0_f32;
    for _ in 0..600 {
        world.step(DT)?;
        worst = worst.max(hinge_error(&world, joint)?);
    }
    // Released horizontally, the bob swings through the bottom at about 6 m/s.
    assert!(worst < 0.01, "worst anchor separation {worst} m");
    let at = world.body(bob)?.position();
    assert!(at.length() < L + 0.01, "{at:?}");
    Ok(())
}

/// Two light links (0.2 kg each) and a weight of `density` (0.64 m^2 of it) on a pivot; the worst
/// separation of the three hinge anchors over six seconds.
fn worst_separation_under_load(density: f32) -> Result<f32, PhysicsError> {
    let mut world = world()?;
    let pivot = pin(&mut world, 0.0, 0.0)?;
    let half = Vec2::new(0.5, 0.1);
    let upper = plank(&mut world, Vec2::new(0.5, 0.0), half, 1.0)?;
    let lower = plank(&mut world, Vec2::new(1.5, 0.0), half, 1.0)?;
    let weight = plank(
        &mut world,
        Vec2::new(2.4, 0.0),
        Vec2::new(0.4, 0.4),
        density,
    )?;
    let joints = [
        hinge_at(&mut world, pivot, upper, Vec2::ZERO)?,
        hinge_at(&mut world, upper, lower, Vec2::new(1.0, 0.0))?,
        hinge_at(&mut world, lower, weight, Vec2::new(2.0, 0.0))?,
    ];
    let mut worst = 0.0_f32;
    for _ in 0..360 {
        world.step(DT)?;
        for joint in joints {
            worst = worst.max(hinge_error(&world, joint)?);
        }
    }
    Ok(worst)
}

#[test]
fn a_weight_ten_times_the_links_keeps_the_hinges_together() -> Result<(), PhysicsError> {
    // 0.64 m^2 at density 3 is 1.92 kg against 0.2 kg links: a ratio of about 10.
    let worst = worst_separation_under_load(3.0)?;
    assert!(worst < 0.03, "worst anchor separation {worst} m");
    Ok(())
}

#[test]
fn a_weight_sixty_times_the_links_stays_within_ten_centimetres() -> Result<(), PhysicsError> {
    // A stress case beyond the card (12.8 kg against 0.2 kg links, a ratio of 64). The bound is
    // looser on purpose: a soft joint stretches under such a ratio, and the test pins that it does
    // not explode. The measured value is in reviews/4-16.md.
    let worst = worst_separation_under_load(20.0)?;
    assert!(worst < 0.1, "worst anchor separation {worst} m");
    Ok(())
}

/// A bar of half length 1 hinged at its left end to a pin at `(0, 5)`, level.
fn bar_on_a_pin(
    world: &mut physics::d2::World,
    limits: Option<AngleLimits>,
    motor: Option<Motor>,
) -> Result<physics::d2::BodyHandle, PhysicsError> {
    let pivot = pin(world, 0.0, 5.0)?;
    let bar = plank(world, Vec2::new(1.0, 5.0), Vec2::new(1.0, 0.1), 1.0)?;
    world.create_joint(&JointDesc::new(
        pivot,
        bar,
        Joint2::Revolute(RevoluteJoint2 {
            limits,
            motor,
            ..RevoluteJoint2::new(Vec2::ZERO, Vec2::new(-1.0, 0.0))
        }),
    ))?;
    Ok(bar)
}

#[test]
fn the_lower_limit_stops_a_falling_bar() -> Result<(), PhysicsError> {
    let mut world = world()?;
    let limits = AngleLimits {
        lower: -0.5,
        upper: 0.5,
    };
    let bar = bar_on_a_pin(&mut world, Some(limits), None)?;
    let mut lowest = 0.0_f32;
    for _ in 0..240 {
        world.step(DT)?;
        lowest = lowest.min(angle(&world, bar)?);
    }
    assert!(lowest > -0.55, "went through the limit: {lowest}");
    let at = angle(&world, bar)?;
    assert!((at + 0.5).abs() < 0.03, "rests at {at}");
    assert!(world.body(bar)?.angular_velocity().abs() < 0.05);
    Ok(())
}

#[test]
fn the_upper_limit_stops_a_rising_bar() -> Result<(), PhysicsError> {
    let mut world = world_with_gravity(-STANDARD_GRAVITY)?;
    let limits = AngleLimits {
        lower: -0.5,
        upper: 0.5,
    };
    let bar = bar_on_a_pin(&mut world, Some(limits), None)?;
    let mut highest = 0.0_f32;
    for _ in 0..240 {
        world.step(DT)?;
        highest = highest.max(angle(&world, bar)?);
    }
    assert!(highest < 0.55, "went through the limit: {highest}");
    let at = angle(&world, bar)?;
    assert!((at - 0.5).abs() < 0.03, "rests at {at}");
    Ok(())
}

#[test]
fn a_bar_without_limits_swings_past_the_horizontal() -> Result<(), PhysicsError> {
    let mut world = world()?;
    let bar = bar_on_a_pin(&mut world, None, None)?;
    step_n(&mut world, 120)?;
    assert!(angle(&world, bar)? < -1.0);
    Ok(())
}

#[test]
fn limits_are_measured_from_the_reference_angle() -> Result<(), PhysicsError> {
    let mut world = world()?;
    let pivot = pin(&mut world, 0.0, 5.0)?;
    let bar = plank(&mut world, Vec2::new(1.0, 5.0), Vec2::new(1.0, 0.1), 1.0)?;
    // Joint angle zero means the bar is turned by 0.4 rad: the limits then hold it at -0.5 + 0.4.
    world.create_joint(&JointDesc::new(
        pivot,
        bar,
        Joint2::Revolute(RevoluteJoint2 {
            reference_angle: 0.4,
            limits: Some(AngleLimits {
                lower: -0.5,
                upper: 0.5,
            }),
            ..RevoluteJoint2::new(Vec2::ZERO, Vec2::new(-1.0, 0.0))
        }),
    ))?;
    step_n(&mut world, 240)?;
    let at = angle(&world, bar)?;
    assert!((at - (-0.5 + 0.4)).abs() < 0.03, "rests at {at}");
    Ok(())
}

#[test]
fn the_limit_holds_near_half_a_turn() -> Result<(), PhysicsError> {
    // The bar starts turned by 2.5 rad, close to the wrap at pi, and is spun toward the upper limit.
    let mut world = world_with_gravity(Vec2::ZERO)?;
    let pivot = pin(&mut world, 0.0, 5.0)?;
    let mut desc = physics::d2::BodyDesc::new(
        physics::d2::BodyKind::Dynamic,
        Vec2::new(0.801, 4.402),
        physics::d2::Shape2::Box {
            half_extents: Vec2::new(1.0, 0.1),
        },
    );
    desc.rotation = Rot2::from_angle(2.5);
    desc.angular_velocity = 3.0;
    let bar = world.create_body(&desc)?;
    let local = Vec2::new(1.0, 0.0);
    let hinge_point = world_point(&world, bar, local)?;
    assert!(
        (hinge_point - Vec2::new(0.0, 5.0)).length() < 0.05,
        "{hinge_point:?}"
    );
    world.create_joint(&JointDesc::new(
        pivot,
        bar,
        Joint2::Revolute(RevoluteJoint2 {
            limits: Some(AngleLimits {
                lower: 1.0,
                upper: 2.8,
            }),
            ..RevoluteJoint2::new(Vec2::ZERO, local)
        }),
    ))?;
    let mut highest = f32::MIN;
    for _ in 0..120 {
        world.step(DT)?;
        highest = highest.max(angle(&world, bar)?);
    }
    assert!(highest < 2.85, "went through the upper limit: {highest}");
    Ok(())
}

#[test]
fn a_motor_turns_a_bar_to_its_target_speed() -> Result<(), PhysicsError> {
    let mut world = world_with_gravity(Vec2::ZERO)?;
    let motor = Motor {
        max_torque: 1000.0,
        speed: 2.0,
    };
    let bar = bar_on_a_pin(&mut world, None, Some(motor))?;
    step_n(&mut world, 60)?;
    let spin = world.body(bar)?.angular_velocity();
    assert!((spin - 2.0).abs() < 0.05, "spin {spin}");
    Ok(())
}

#[test]
fn a_motor_is_limited_by_its_torque() -> Result<(), PhysicsError> {
    // The bar is 2 m by 0.2 m with density 1: m = 0.4 kg, I about the pin = m (4 + 0.04) / 12 +
    // m 1^2 = 0.5347 kg m^2. A torque of 0.5 N m turns it up by 0.935 rad/s^2.
    let mut world = world_with_gravity(Vec2::ZERO)?;
    let motor = Motor {
        max_torque: 0.5,
        speed: 5.0,
    };
    let bar = bar_on_a_pin(&mut world, None, Some(motor))?;
    step_n(&mut world, 60)?;
    let spin = world.body(bar)?.angular_velocity();
    let expected = 0.5 / (0.4 * (4.04 / 12.0 + 1.0));
    assert!(
        (spin - expected).abs() < 0.03 * expected,
        "spin {spin} against {expected}"
    );
    Ok(())
}

#[test]
fn a_motor_with_no_torque_lets_the_bar_fall() -> Result<(), PhysicsError> {
    let mut world = world()?;
    let motor = Motor {
        max_torque: 0.0,
        speed: 3.0,
    };
    let bar = bar_on_a_pin(&mut world, None, Some(motor))?;
    step_n(&mut world, 60)?;
    assert!(world.body(bar)?.angular_velocity() < -1.0);
    Ok(())
}

#[test]
fn a_hinge_between_two_free_bodies_conserves_momentum() -> Result<(), PhysicsError> {
    let mut world = world_with_gravity(Vec2::ZERO)?;
    let a = plank(&mut world, Vec2::new(0.0, 0.0), Vec2::new(0.5, 0.5), 1.0)?;
    let b = plank(&mut world, Vec2::new(2.0, 0.0), Vec2::new(0.5, 0.5), 3.0)?;
    let joint = hinge_at(&mut world, a, b, Vec2::new(1.0, 0.0))?;
    world.set_linear_velocity(a, Vec2::new(1.0, 2.0))?;
    step_n(&mut world, 120)?;
    let momentum = |handle| -> Result<Vec2, PhysicsError> {
        let body = world.body(handle)?;
        Ok(body.linear_velocity() * body.mass())
    };
    let total = momentum(a)? + momentum(b)?;
    // Mass 1 at (1, 2): total momentum (1, 2) before; a hinge applies equal and opposite impulses.
    assert!((total - Vec2::new(1.0, 2.0)).length() < 1e-3, "{total:?}");
    assert!(hinge_error(&world, joint)? < 0.01);
    Ok(())
}

/// A bar of half length 1 hinged at its right end to a pin at `(0, 5)`, turned by `body_angle`
/// and spun at `spin`, with the joint angle measured from `reference` and limited to `[-0.5, 0.5]`.
/// Returns the bar after two seconds in zero gravity.
fn bar_near_the_wrap(reference: f32, body_angle: f32, spin: f32) -> Result<f32, PhysicsError> {
    let mut world = world_with_gravity(Vec2::ZERO)?;
    let pivot = pin(&mut world, 0.0, 5.0)?;
    let rotation = Rot2::from_angle(body_angle);
    let local = Vec2::new(1.0, 0.0);
    let mut desc = physics::d2::BodyDesc::new(
        physics::d2::BodyKind::Dynamic,
        Vec2::new(0.0, 5.0) - rotation.rotate(local),
        physics::d2::Shape2::Box {
            half_extents: Vec2::new(1.0, 0.1),
        },
    );
    desc.rotation = rotation;
    desc.angular_velocity = spin;
    let bar = world.create_body(&desc)?;
    world.create_joint(&JointDesc::new(
        pivot,
        bar,
        Joint2::Revolute(RevoluteJoint2 {
            reference_angle: reference,
            limits: Some(AngleLimits {
                lower: -0.5,
                upper: 0.5,
            }),
            ..RevoluteJoint2::new(Vec2::ZERO, local)
        }),
    ))?;
    step_n(&mut world, 120)?;
    angle(&world, bar)
}

#[test]
fn the_joint_angle_is_unwound_across_the_wrap_in_both_directions() -> Result<(), PhysicsError> {
    // The bound is 0.08 rad: the limit is soft, so the bar overshoots it by a few hundredths. A
    // missing unwind would be off by a whole turn or leave the limit unmet.
    // Reference 3.0 and the bar at -3.0: the raw angle is -6.0, the joint angle +0.283 (inside).
    // Spun up it meets the upper limit at joint angle 0.5, a body angle of 0.5 + 3.0 - 2 pi.
    let at = bar_near_the_wrap(3.0, -3.0, 1.0)?;
    let expected = 0.5 + 3.0 - 2.0 * PI;
    assert!(
        (at - expected).abs() < 0.08,
        "rests at {at} against {expected}"
    );
    // Mirrored: reference -3.0 and the bar at +3.0, spun down to the lower limit.
    let at = bar_near_the_wrap(-3.0, 3.0, -1.0)?;
    let expected = -0.5 - 3.0 + 2.0 * PI;
    assert!(
        (at - expected).abs() < 0.08,
        "rests at {at} against {expected}"
    );
    Ok(())
}
