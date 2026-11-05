//! What the solver does with bad numbers, full buffers and bodies created between steps.

use super::common::{
    DT, crate_of, earth, ground, position, speed, step_n, tight_world, velocity, world,
};
use base::math::Vec2;
use physics::d2::{PhysicsError, World};

const HALF: Vec2 = Vec2::new(0.5, 0.5);

#[test]
fn a_step_that_would_overflow_in_the_solver_is_refused_and_changes_nothing()
-> Result<(), PhysicsError> {
    let mut world = world(4, earth())?;
    ground(&mut world)?;
    let rest = crate_of(&mut world, HALF, 0.0, 0.5)?;
    step_n(&mut world, 60)?;
    let before = world.state_hash();
    let steps = world.step_count();
    // Finite, but the impulse that would stop it overflows the spin (3e38 times a lever arm).
    world.set_linear_velocity(rest, Vec2::new(0.0, -3.0e38))?;
    let with_velocity = world.state_hash();
    assert_ne!(with_velocity, before);
    assert_eq!(world.step(DT), Err(PhysicsError::NotFinite));
    assert_eq!(world.state_hash(), with_velocity);
    assert_eq!(world.step_count(), steps);
    // The world is usable afterwards: the solver kept its warm start.
    world.set_linear_velocity(rest, Vec2::ZERO)?;
    step_n(&mut world, 60)?;
    assert!((position(&world, rest)?.y - 0.5).abs() < 0.01);
    Ok(())
}

#[test]
fn a_body_created_overlapping_the_ground_is_pushed_out_in_the_first_step()
-> Result<(), PhysicsError> {
    let mut world = world(4, earth())?;
    ground(&mut world)?;
    let sunk = crate_of(&mut world, HALF, 0.0, 0.45)?;
    // No find_pairs, no collide: the step has to see the new body. Without a contact it would fall.
    world.step(DT)?;
    assert!(position(&world, sunk)?.y > 0.45 + 0.001);
    step_n(&mut world, 120)?;
    let at = position(&world, sunk)?;
    assert!(
        (at.y - 0.5).abs() < 0.01 && speed(&world, sunk)? < 0.01,
        "{at:?}"
    );
    Ok(())
}

#[test]
fn the_push_out_speed_is_capped() -> Result<(), PhysicsError> {
    // Half a metre deep: the error asks for far more than the cap of 3 m/s.
    let mut world = world(4, earth())?;
    ground(&mut world)?;
    let deep = crate_of(&mut world, HALF, 0.0, 0.0)?;
    let mut fastest = 0.0_f32;
    for _ in 0..30 {
        let before = position(&world, deep)?.y;
        world.step(DT)?;
        fastest = fastest.max((position(&world, deep)?.y - before) / DT);
    }
    assert!(fastest <= 3.0 + 0.1, "{fastest}");
    assert!(fastest > 2.0, "{fastest}");
    Ok(())
}

#[test]
fn a_destroyed_body_stops_being_a_contact() -> Result<(), PhysicsError> {
    let mut world = world(4, earth())?;
    ground(&mut world)?;
    let rest = crate_of(&mut world, HALF, 0.0, 0.5)?;
    step_n(&mut world, 30)?;
    world.destroy_body(rest)?;
    step_n(&mut world, 3)?;
    let again = crate_of(&mut world, HALF, 0.0, 3.0)?;
    step_n(&mut world, 240)?;
    assert!((position(&world, again)?.y - 0.5).abs() < 0.01);
    Ok(())
}

#[test]
fn a_body_made_in_the_slot_of_a_destroyed_one_does_not_inherit_its_impulses()
-> Result<(), PhysicsError> {
    let mut world = world(4, earth())?;
    ground(&mut world)?;
    let old = crate_of(&mut world, HALF, 0.0, 0.5)?;
    step_n(&mut world, 60)?;
    world.destroy_body(old)?;
    // Same slot, a different place, no step in between: the cached impulses belong to the old
    // handle and must not be applied to the new one.
    let new = crate_of(&mut world, HALF, 0.0, 0.5)?;
    let mut fresh = self::world(4, earth())?;
    ground(&mut fresh)?;
    let twin = crate_of(&mut fresh, HALF, 0.0, 0.5)?;
    step_n(&mut world, 1)?;
    step_n(&mut fresh, 1)?;
    assert_eq!(position(&world, new)?, position(&fresh, twin)?);
    assert_eq!(velocity(&world, new)?, velocity(&fresh, twin)?);
    Ok(())
}

/// Four boxes on the ground, none touching another, in a world whose lists are `tight_world`.
fn four_on_the_ground(max_pairs: u32, max_manifolds: u32) -> Result<World, PhysicsError> {
    let mut world = tight_world(max_pairs, max_manifolds)?;
    ground(&mut world)?;
    for i in 0..4_u8 {
        crate_of(&mut world, HALF, f32::from(i) * 1.5, 0.5)?;
    }
    step_n(&mut world, 5)?;
    Ok(world)
}

fn all_finite(world: &World) -> bool {
    world
        .iter()
        .all(|(_, body)| body.position().x.is_finite() && body.position().y.is_finite())
}

#[test]
fn a_full_manifold_list_is_reported_and_the_step_still_succeeds() -> Result<(), PhysicsError> {
    let world = four_on_the_ground(64, 1)?;
    assert!(world.pairs().is_complete());
    assert!(!world.manifolds().is_complete());
    assert_eq!(world.manifolds().missing(), 3);
    assert!(all_finite(&world));
    Ok(())
}

#[test]
fn a_full_pair_list_is_reported_and_the_step_still_succeeds() -> Result<(), PhysicsError> {
    let world = four_on_the_ground(1, 64)?;
    assert!(!world.pairs().is_complete());
    assert!(world.pairs().missing() >= 3);
    assert!(all_finite(&world));
    Ok(())
}

#[test]
fn a_contact_free_world_is_the_old_semi_implicit_step() -> Result<(), PhysicsError> {
    // Free fall by one Euler step per step, not by sub-steps: v = n g dt and
    // x = x0 + g dt^2 n (n + 1) / 2 (see `core::body`).
    let mut world = world(2, earth())?;
    let body = crate_of(&mut world, HALF, 0.0, 100.0)?;
    step_n(&mut world, 60)?;
    let g = -9.806_65_f32;
    let expected = 100.0 + g * DT * DT * 60.0 * 61.0 / 2.0;
    assert!((position(&world, body)?.y - expected).abs() < 1.0e-3);
    Ok(())
}
