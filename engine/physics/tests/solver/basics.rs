//! One body and the ground: rest height, friction against closed forms, restitution.

use super::common::{
    DT, angle, ball, crate_of, earth, ground, position, speed, step_n, velocity, world, world_with,
};
use base::math::Vec2;
use physics::d2::{BodyDesc, BodyKind, PhysicsError, Shape2, SolverConfig};

const HALF: Vec2 = Vec2::new(0.5, 0.5);

#[test]
fn a_box_dropped_on_the_ground_comes_to_rest_on_its_surface() -> Result<(), PhysicsError> {
    let mut world = world(4, earth())?;
    ground(&mut world)?;
    let rest = crate_of(&mut world, HALF, 0.0, 1.0)?;
    step_n(&mut world, 180)?;
    let at = position(&world, rest)?;
    // Resting height: the surface at 0 plus the half height; the contact tolerates the 1 mm slop.
    assert!((at.y - 0.5).abs() < 0.003, "{at:?}");
    assert!(at.x.abs() < 0.005, "{at:?}");
    assert!(speed(&world, rest)? < 0.01);
    assert!(angle(&world, rest)?.abs() < 0.005);
    Ok(())
}

#[test]
fn a_circle_dropped_on_the_ground_comes_to_rest_on_its_surface() -> Result<(), PhysicsError> {
    let mut world = world(4, earth())?;
    ground(&mut world)?;
    let rest = ball(&mut world, 0.25, 0.0, 1.0)?;
    step_n(&mut world, 240)?;
    let at = position(&world, rest)?;
    assert!((at.y - 0.25).abs() < 0.01, "{at:?}");
    assert!(speed(&world, rest)? < 0.01);
    Ok(())
}

#[test]
fn the_sub_step_and_relax_counts_are_settings_and_each_setting_rests() -> Result<(), PhysicsError> {
    for (sub_steps, relax_iterations) in [(1, 0), (1, 1), (4, 0), (4, 1), (8, 2)] {
        let solver = SolverConfig {
            sub_steps,
            relax_iterations,
            ..SolverConfig::DEFAULT
        };
        let mut world = world_with(4, earth(), solver)?;
        ground(&mut world)?;
        let rest = crate_of(&mut world, HALF, 0.0, 0.6)?;
        step_n(&mut world, 240)?;
        let at = position(&world, rest)?;
        assert!(
            (at.y - 0.5).abs() < 0.02 && speed(&world, rest)? < 0.02,
            "sub_steps {sub_steps}, relax {relax_iterations}: {at:?}"
        );
    }
    Ok(())
}

/// A box with the given friction on the ground of the same friction, so the mixed coefficient is
/// that value (the square root of its square).
fn slider(
    friction: f32,
    speed_x: f32,
) -> Result<(physics::d2::World, physics::d2::BodyHandle), PhysicsError> {
    let mut world = world(4, earth())?;
    let mut floor = BodyDesc::new(
        BodyKind::Static,
        Vec2::new(0.0, -1.0),
        Shape2::Box {
            half_extents: Vec2::new(50.0, 1.0),
        },
    );
    floor.friction = friction;
    world.create_body(&floor)?;
    let mut body = BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(0.0, 0.5),
        Shape2::Box { half_extents: HALF },
    );
    body.friction = friction;
    body.linear_velocity = Vec2::new(speed_x, 0.0);
    let handle = world.create_body(&body)?;
    Ok((world, handle))
}

#[test]
fn a_sliding_box_stops_after_the_distance_friction_allows() -> Result<(), PhysicsError> {
    // Coulomb: deceleration mu g, so the stopping distance is v^2 / (2 mu g).
    for (mu, v0) in [(0.5_f32, 3.0_f32), (0.3, 2.0), (0.8, 4.0)] {
        let (mut world, body) = slider(mu, v0)?;
        step_n(&mut world, 240)?;
        let expected = v0 * v0 / (2.0 * mu * 9.806_65);
        let at = position(&world, body)?;
        assert!(
            (at.x - expected).abs() < 0.05 * expected,
            "mu {mu}: stopped at {} expected {expected}",
            at.x
        );
        assert!(speed(&world, body)? < 0.01);
        assert!((at.y - 0.5).abs() < 0.01);
    }
    Ok(())
}

#[test]
fn a_frictionless_box_keeps_sliding() -> Result<(), PhysicsError> {
    let (mut world, body) = slider(0.0, 3.0)?;
    step_n(&mut world, 60)?;
    assert!((velocity(&world, body)?.x - 3.0).abs() < 1.0e-3);
    Ok(())
}

/// A slope of tangent `0.75` (cos 0.8, sin 0.6) with a unit box resting on it. Returns the world,
/// the box and the unit vector down the slope.
fn slope(
    friction: f32,
) -> Result<(physics::d2::World, physics::d2::BodyHandle, Vec2), PhysicsError> {
    use super::common::rotation_of;
    let turn = rotation_of(0.8, 0.6);
    let up = Vec2::new(-0.6, 0.8);
    let mut world = world(4, earth())?;
    let mut floor = BodyDesc::new(
        BodyKind::Static,
        Vec2::ZERO,
        Shape2::Box {
            half_extents: Vec2::new(30.0, 1.0),
        },
    );
    floor.rotation = turn;
    floor.friction = friction;
    world.create_body(&floor)?;
    let mut body = BodyDesc::new(
        BodyKind::Dynamic,
        up * 1.5,
        Shape2::Box { half_extents: HALF },
    );
    body.rotation = turn;
    body.friction = friction;
    let handle = world.create_body(&body)?;
    // The slab is turned counter-clockwise, so its x axis (0.8, 0.6) points uphill.
    Ok((world, handle, Vec2::new(-0.8, -0.6)))
}

#[test]
fn a_box_on_a_slope_steeper_than_the_friction_angle_slides_with_the_analytic_acceleration()
-> Result<(), PhysicsError> {
    let (mut world, body, down) = slope(0.5)?;
    step_n(&mut world, 90)?;
    // a = g (sin - mu cos) = 9.80665 * (0.6 - 0.5 * 0.8)
    let expected = 9.806_65 * (0.6 - 0.5 * 0.8) * (90.0 * DT);
    let along = velocity(&world, body)?.dot(down);
    assert!(
        (along - expected).abs() < 0.05 * expected,
        "speed {along}, expected {expected}"
    );
    Ok(())
}

#[test]
fn a_box_on_a_slope_shallower_than_the_friction_angle_stays() -> Result<(), PhysicsError> {
    let (mut world, body, _) = slope(0.9)?;
    let start = position(&world, body)?;
    step_n(&mut world, 240)?;
    let moved = (position(&world, body)? - start).length();
    assert!(moved < 0.02, "moved {moved}");
    assert!(speed(&world, body)? < 0.01);
    Ok(())
}

fn bouncer(
    restitution: f32,
    height: f32,
) -> Result<(physics::d2::World, physics::d2::BodyHandle), PhysicsError> {
    let mut world = world(4, earth())?;
    ground(&mut world)?;
    let mut desc = BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(0.0, height + 0.25),
        Shape2::Circle { radius: 0.25 },
    );
    desc.restitution = restitution;
    let handle = world.create_body(&desc)?;
    Ok((world, handle))
}

/// The upward speed the ball first has after it hit the ground, and the highest its lowest point
/// gets afterwards, in metres above the ground.
fn first_rebound(
    world: &mut physics::d2::World,
    ball: physics::d2::BodyHandle,
) -> Result<(f32, f32), PhysicsError> {
    let mut leaving = 0.0_f32;
    let mut peak = 0.0_f32;
    for _ in 0..400 {
        world.step(DT)?;
        let v = velocity(world, ball)?.y;
        let height = position(world, ball)?.y - 0.25;
        if leaving == 0.0 && v > 0.0 {
            leaving = v;
        }
        if leaving > 0.0 {
            peak = peak.max(height);
        }
        if leaving > 0.0 && v < 0.0 {
            break;
        }
    }
    Ok((leaving, peak))
}

/// First rebound as a height only.
fn first_rebound_height(
    world: &mut physics::d2::World,
    ball: physics::d2::BodyHandle,
) -> Result<f32, PhysicsError> {
    Ok(first_rebound(world, ball)?.1)
}

#[test]
fn a_bouncing_ball_leaves_at_restitution_times_its_arrival_speed() -> Result<(), PhysicsError> {
    // Dropped from 2 m, it arrives at sqrt(2 g h) (free fall by whole steps, which is within 1% of
    // it) and, by the definition of restitution, leaves at e times that. The height it reaches is
    // not asserted: a ball that moves 10 cm per step is already embedded in the ground by a few
    // centimetres when its contact first exists, which shifts the whole rebound upward or downward.
    let arrival = (2.0_f32 * 9.806_65 * 2.0).sqrt();
    for e in [0.3_f32, 0.5, 0.8] {
        let (mut world, ball) = bouncer(e, 2.0)?;
        let (leaving, _) = first_rebound(&mut world, ball)?;
        let expected = e * arrival;
        assert!(
            (leaving - expected).abs() < 0.03 * expected,
            "e {e}: left at {leaving}, expected {expected}"
        );
    }
    Ok(())
}

#[test]
fn a_ball_without_restitution_does_not_bounce() -> Result<(), PhysicsError> {
    let (mut world, ball) = bouncer(0.0, 2.0)?;
    let rise = first_rebound_height(&mut world, ball)?;
    assert!(rise < 0.02, "rose {rise}");
    step_n(&mut world, 120)?;
    assert!((position(&world, ball)?.y - 0.25).abs() < 0.01);
    Ok(())
}

#[test]
fn an_approach_slower_than_the_threshold_does_not_bounce() -> Result<(), PhysicsError> {
    // 0.0127 m of fall is 0.5 m/s at the ground, half the default threshold of 1 m/s.
    let (mut world, ball) = bouncer(0.8, 0.0127)?;
    let rise = first_rebound_height(&mut world, ball)?;
    assert!(rise < 0.005, "rose {rise}");
    step_n(&mut world, 120)?;
    assert!((position(&world, ball)?.y - 0.25).abs() < 0.01);
    Ok(())
}

#[test]
fn restitution_is_the_larger_of_the_two_bodies() -> Result<(), PhysicsError> {
    let mut world = world(4, earth())?;
    let mut floor = BodyDesc::new(
        BodyKind::Static,
        Vec2::new(0.0, -1.0),
        Shape2::Box {
            half_extents: Vec2::new(50.0, 1.0),
        },
    );
    floor.restitution = 0.5;
    world.create_body(&floor)?;
    let ball = ball(&mut world, 0.25, 0.0, 2.25)?;
    let rise = first_rebound_height(&mut world, ball)?;
    assert!((rise - 0.5).abs() < 0.05, "rose {rise}");
    Ok(())
}

type Spinner = (physics::d2::World, physics::d2::BodyHandle);

/// A frictionless ball one millimetre above the ground (so a contact exists and the solver owns
/// it), with the given damping, spin and speed; or the same ball alone in a world with no ground.
fn spinner(grounded: bool, damping: f32, spin: f32, speed_x: f32) -> Result<Spinner, PhysicsError> {
    let mut world = world(4, Vec2::new(0.0, 0.0))?;
    if grounded {
        ground(&mut world)?;
    }
    let mut desc = BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(0.0, 0.251),
        Shape2::Circle { radius: 0.25 },
    );
    desc.friction = 0.0;
    desc.linear_damping = damping;
    desc.angular_damping = damping;
    desc.angular_velocity = spin;
    desc.linear_velocity = Vec2::new(speed_x, 0.0);
    let handle = world.create_body(&desc)?;
    Ok((world, handle))
}

#[test]
fn a_body_in_contact_is_damped_like_one_that_is_free() -> Result<(), PhysicsError> {
    for grounded in [true, false] {
        let (mut world, ball) = spinner(grounded, 1.0, 10.0, 5.0)?;
        step_n(&mut world, 60)?;
        // Damping 1 per second for one second leaves about 1/e of the speed (the per-sub-step
        // factors differ from the per-step one by 0.1% over this time).
        let kept = 0.367_879_f32; // 1/e
        let linear = velocity(&world, ball)?.x;
        assert!(
            (linear - 5.0 * kept).abs() < 0.01 * 5.0 * kept,
            "linear {linear}"
        );
        let spin = world.body(ball)?.angular_velocity();
        assert!(
            (spin - 10.0 * kept).abs() < 0.01 * 10.0 * kept,
            "spin {spin}"
        );
    }
    Ok(())
}

#[test]
fn a_torque_turns_a_body_in_contact_as_it_does_a_free_one() -> Result<(), PhysicsError> {
    let mut spins = Vec::new();
    for grounded in [true, false] {
        let (mut world, ball) = spinner(grounded, 0.0, 0.0, 0.0)?;
        world.apply_torque(ball, 3.0)?;
        world.step(DT)?;
        spins.push(world.body(ball)?.angular_velocity());
    }
    assert!(spins[0] > 0.1, "spin {}", spins[0]);
    assert!((spins[0] - spins[1]).abs() < 1e-4 * spins[1], "{spins:?}");
    Ok(())
}

#[test]
fn an_approach_below_the_threshold_does_not_bounce_even_when_perfectly_elastic()
-> Result<(), PhysicsError> {
    let (mut world, ball) = bouncer(1.0, 0.0127)?;
    let (leaving, _) = first_rebound(&mut world, ball)?;
    assert!(leaving < 0.05, "left at {leaving}");
    Ok(())
}

#[test]
fn a_bouncy_ball_at_rest_stays_at_rest() -> Result<(), PhysicsError> {
    let (mut world, ball) = bouncer(0.8, 0.0)?;
    step_n(&mut world, 180)?;
    assert!(speed(&world, ball)? < 0.02);
    assert!((position(&world, ball)?.y - 0.25).abs() < 0.01);
    Ok(())
}
