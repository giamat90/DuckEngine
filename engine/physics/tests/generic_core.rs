//! The core is generic: a second dimension instance, written here in the test, builds a world,
//! creates bodies and steps with the same `World` code that `d2` uses. The instance is a "planar
//! 3D" one (3D positions, rotation about Z only), the shape of the locked-axis option in ADR-0018;
//! it exists to prove the `Dim` trait is narrow enough to implement for another dimension, not as
//! a 3D feature (the real `d3` is phase 8).

use base::math::Vec3;
use physics::core::{
    BodyDesc, BodyKind, Dim, HashBits, MassProperties, PhysicsError, StateHasher, World,
    WorldConfig,
};
use std::f32::consts::PI;

struct Planar3;

#[derive(Clone, Copy)]
struct Sphere {
    radius: f32,
}

impl HashBits for Sphere {
    fn write_bits(&self, hasher: &mut StateHasher) {
        hasher.write_f32(self.radius);
    }
}

impl Dim for Planar3 {
    type Vec = Vec3;
    type Ang = f32;
    type Rot = f32;
    type InvInertia = f32;
    type Shape = Sphere;

    fn apply_inverse_inertia(_rot: f32, inv_inertia: f32, torque: f32) -> f32 {
        inv_inertia * torque
    }

    fn advance_rotation(rot: f32, angular_velocity: f32, dt: f32) -> f32 {
        rot + angular_velocity * dt
    }

    fn mass_properties(shape: &Sphere, density: f32) -> Result<MassProperties<Self>, PhysicsError> {
        if !(shape.radius.is_finite() && shape.radius > 0.0 && density > 0.0) {
            return Err(PhysicsError::InvalidShape);
        }
        let mass = density * 4.0 / 3.0 * PI * shape.radius * shape.radius * shape.radius;
        let inertia = 0.4 * mass * shape.radius * shape.radius;
        Ok(MassProperties {
            mass,
            inv_inertia: 1.0 / inertia,
        })
    }
}

type World3 = World<Planar3>;

const DT: f32 = 0.01;

fn build() -> Result<World3, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: 4,
        gravity: Vec3::new(0.0, -10.0, 0.0),
    })
}

#[test]
fn a_second_dimension_steps_with_the_same_world_code() -> Result<(), PhysicsError> {
    let mut world = build()?;
    let mut desc = BodyDesc::<Planar3>::new(
        BodyKind::Dynamic,
        Vec3::new(1.0, 50.0, -2.0),
        Sphere { radius: 0.5 },
    );
    desc.linear_velocity = Vec3::new(2.0, 0.0, 1.0);
    desc.angular_velocity = 1.5;
    let handle = world.create_body(&desc)?;
    for _ in 0..100 {
        world.step(DT)?;
    }
    let body = world.body(handle)?;
    // v_y = n g dt, y = y0 + g dt^2 n (n + 1) / 2 for n = 100.
    assert!((body.linear_velocity().y + 10.0).abs() < 1e-3);
    assert!((body.position().y - (50.0 - 10.0 * DT * DT * 100.0 * 101.0 / 2.0)).abs() < 1e-3);
    assert!(
        (body.position().x - 3.0).abs() < 1e-3,
        "x {}",
        body.position().x
    );
    assert!(
        (body.position().z + 1.0).abs() < 1e-3,
        "z {}",
        body.position().z
    );
    assert!((body.rotation() - 1.5).abs() < 1e-3);
    Ok(())
}

#[test]
fn the_second_dimension_has_handles_errors_and_a_state_hash_too() -> Result<(), PhysicsError> {
    let (mut first, mut second) = (build()?, build()?);
    let desc = BodyDesc::<Planar3>::new(BodyKind::Dynamic, Vec3::ZERO, Sphere { radius: 1.0 });
    let handle = first.create_body(&desc)?;
    second.create_body(&desc)?;
    for _ in 0..10 {
        first.step(DT)?;
        second.step(DT)?;
        assert_eq!(first.state_hash(), second.state_hash());
    }
    first.destroy_body(handle)?;
    assert_eq!(first.body(handle).err(), Some(PhysicsError::StaleHandle));
    assert_ne!(first.state_hash(), second.state_hash());
    let bad = BodyDesc::<Planar3>::new(BodyKind::Dynamic, Vec3::ZERO, Sphere { radius: -1.0 });
    assert_eq!(
        first.create_body(&bad).err(),
        Some(PhysicsError::InvalidShape)
    );
    Ok(())
}
