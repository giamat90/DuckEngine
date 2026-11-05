//! Body against body: a head-on collision against the closed form of the impulse, evaluated
//! independently in `f64`, momentum conservation, and a fast body against a thin wall.

use super::common::{DT, default_config, step_n, velocity, world, world_of};
use base::math::Vec2;
use physics::d2::{BodyDesc, BodyHandle, BodyKind, PhysicsError, Shape2, World};

/// Two circles on the x axis, 1 cm apart, flying at each other with no gravity.
fn head_on(
    radii: (f32, f32),
    speeds: (f32, f32),
    restitution: f32,
) -> Result<(World, BodyHandle, BodyHandle), PhysicsError> {
    let mut world = world(4, Vec2::ZERO)?;
    let gap = 0.01;
    let mut make = |radius: f32, x: f32, vx: f32| {
        let mut desc = BodyDesc::new(
            BodyKind::Dynamic,
            Vec2::new(x, 0.0),
            Shape2::Circle { radius },
        );
        desc.linear_velocity = Vec2::new(vx, 0.0);
        desc.restitution = restitution;
        world.create_body(&desc)
    };
    let left = make(radii.0, -(radii.0 + gap / 2.0), speeds.0)?;
    let right = make(radii.1, radii.1 + gap / 2.0, speeds.1)?;
    Ok((world, left, right))
}

/// The velocities after an impact in one dimension, from the impulse
/// `j = -(1 + e) v_rel / (1/m1 + 1/m2)`, in `f64`: independent of the solver.
fn closed_form(m: (f64, f64), v: (f64, f64), e: f64) -> (f64, f64) {
    let relative = v.1 - v.0;
    let impulse = -(1.0 + e) * relative / (1.0 / m.0 + 1.0 / m.1);
    (v.0 - impulse / m.0, v.1 + impulse / m.1)
}

fn mass_of(world: &World, body: BodyHandle) -> Result<f64, PhysicsError> {
    Ok(f64::from(world.body(body)?.mass()))
}

#[test]
fn a_head_on_collision_agrees_with_the_closed_form_for_any_restitution() -> Result<(), PhysicsError>
{
    for e in [0.0_f32, 0.25, 0.6, 1.0] {
        let (mut world, a, b) = head_on((0.5, 0.25), (2.0, -1.0), e)?;
        let masses = (mass_of(&world, a)?, mass_of(&world, b)?);
        step_n(&mut world, 30)?;
        let (va, vb) = (
            f64::from(velocity(&world, a)?.x),
            f64::from(velocity(&world, b)?.x),
        );
        let (ea, eb) = closed_form(masses, (2.0, -1.0), f64::from(e));
        let tolerance = if e == 0.0 { 0.03 } else { 1.0e-3 };
        assert!(
            (va - ea).abs() < tolerance && (vb - eb).abs() < tolerance,
            "e {e}: got ({va}, {vb}), closed form ({ea}, {eb})"
        );
    }
    Ok(())
}

#[test]
fn a_collision_conserves_momentum_and_makes_no_spin_for_a_centred_hit() -> Result<(), PhysicsError>
{
    let (mut world, a, b) = head_on((0.5, 0.25), (2.0, -1.0), 0.5)?;
    let (ma, mb) = (mass_of(&world, a)?, mass_of(&world, b)?);
    let before = ma * 2.0 - mb;
    step_n(&mut world, 30)?;
    let after = ma * f64::from(velocity(&world, a)?.x) + mb * f64::from(velocity(&world, b)?.x);
    assert!(
        (after - before).abs() < 1.0e-4 * before.abs(),
        "{before} -> {after}"
    );
    assert_eq!(velocity(&world, a)?.y.to_bits(), 0.0_f32.to_bits());
    assert!(world.body(a)?.angular_velocity().abs() < 1.0e-6);
    Ok(())
}

#[test]
fn equal_masses_exchange_their_velocities_in_an_elastic_collision() -> Result<(), PhysicsError> {
    let (mut world, a, b) = head_on((0.5, 0.5), (3.0, 0.0), 1.0)?;
    step_n(&mut world, 30)?;
    assert!(velocity(&world, a)?.x.abs() < 1.0e-3);
    assert!((velocity(&world, b)?.x - 3.0).abs() < 1.0e-3);
    Ok(())
}

/// A thin static wall at `x = 2` (10 cm thick) and a ball flying at it. Both margins are 0.5 m, so
/// the pair is a candidate and has a speculative contact before the ball can jump the wall.
fn wall_world() -> Result<World, PhysicsError> {
    let mut config = default_config(4, Vec2::ZERO);
    config.broadphase_margin = 0.5;
    config.speculative_margin = 0.5;
    world_of(config)
}

#[test]
fn a_fast_ball_does_not_tunnel_through_a_thin_wall_within_the_speculative_margin()
-> Result<(), PhysicsError> {
    // 20 m/s is 0.33 m per step, less than the 0.5 m margin, and ten wall thicknesses per 5 steps.
    let mut world = wall_world()?;
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec2::new(2.0, 0.0),
        Shape2::Box {
            half_extents: Vec2::new(0.05, 5.0),
        },
    ))?;
    let mut desc = BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::ZERO,
        Shape2::Circle { radius: 0.25 },
    );
    desc.linear_velocity = Vec2::new(20.0, 0.0);
    let ball = world.create_body(&desc)?;
    for _ in 0..60 {
        world.step(DT)?;
        let x = world.body(ball)?.position().x;
        assert!(
            x < 2.0 - 0.05 + 0.02,
            "the ball is inside or past the wall: x = {x}"
        );
    }
    Ok(())
}

#[test]
fn a_ball_flying_away_from_a_wall_is_not_held_back() -> Result<(), PhysicsError> {
    let mut world = wall_world()?;
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec2::new(2.0, 0.0),
        Shape2::Box {
            half_extents: Vec2::new(0.05, 5.0),
        },
    ))?;
    // The ball starts 5 mm from the wall (a contact exists) and moves away.
    let mut desc = BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(1.695, 0.0),
        Shape2::Circle { radius: 0.25 },
    );
    desc.linear_velocity = Vec2::new(-5.0, 0.0);
    let ball = world.create_body(&desc)?;
    step_n(&mut world, 10)?;
    assert!((velocity(&world, ball)?.x + 5.0).abs() < 1.0e-4);
    Ok(())
}
