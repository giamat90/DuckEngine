//! D0 replay through the public 2D API: the same scene built and stepped twice in one run gives the
//! same hash after every step. Nothing is stored (the cross-platform golden hash is card 4-20).

use base::math::Vec2;
use physics::d2::{
    BodyDesc, BodyKind, PhysicsError, Rot2, STANDARD_GRAVITY, Shape2, World, WorldConfig,
};

const DT: f32 = 1.0 / 60.0;
const STEPS: usize = 600;

/// A scene with a static floor and bodies of both shapes, spin, damping, a body created later and a
/// body destroyed, built from a seed-free rule so it is the same on every call.
fn run() -> Result<Vec<u64>, PhysicsError> {
    let mut world = World::new(WorldConfig {
        max_bodies: 32,
        gravity: STANDARD_GRAVITY,
    })?;
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec2::new(0.0, -1.0),
        Shape2::Box {
            half_extents: Vec2::new(20.0, 1.0),
        },
    ))?;
    let mut handles = Vec::new();
    for i in 0..10_u8 {
        let x = f32::from(i);
        let shape = if i % 2 == 0 {
            Shape2::Circle {
                radius: 0.25 + 0.05 * x,
            }
        } else {
            Shape2::Box {
                half_extents: Vec2::new(0.3, 0.2 + 0.02 * x),
            }
        };
        let mut desc = BodyDesc::new(BodyKind::Dynamic, Vec2::new(x, 5.0 + x), shape);
        desc.rotation = Rot2::from_angle(0.1 * x);
        desc.angular_velocity = 0.7 - 0.1 * x;
        desc.linear_velocity = Vec2::new(0.5, 0.0);
        desc.linear_damping = 0.01 * x;
        desc.density = 1.0 + x;
        handles.push(world.create_body(&desc)?);
    }
    let mut hashes = Vec::with_capacity(STEPS);
    for step in 0..STEPS {
        if step == 100 {
            world.destroy_body(handles[3])?;
        }
        if step == 200 {
            world.create_body(&BodyDesc::new(
                BodyKind::Dynamic,
                Vec2::new(2.0, 9.0),
                Shape2::Circle { radius: 0.4 },
            ))?;
        }
        if step % 50 == 0 {
            world.apply_force(handles[0], Vec2::new(30.0, 0.0))?;
            world.apply_torque(handles[1], 2.0)?;
        }
        world.step(DT)?;
        hashes.push(world.state_hash());
    }
    Ok(hashes)
}

#[test]
fn the_same_scene_replays_to_the_same_hash_at_every_step() -> Result<(), PhysicsError> {
    let first = run()?;
    let second = run()?;
    assert_eq!(first.len(), STEPS);
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn the_hash_changes_every_step_while_bodies_move() -> Result<(), PhysicsError> {
    let hashes = run()?;
    assert!(hashes.windows(2).all(|pair| pair[0] != pair[1]));
    Ok(())
}
