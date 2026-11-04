//! The core is generic: a second dimension instance, written here in the test, builds a world,
//! creates bodies and steps with the same `World` code that `d2` uses. The instance is a "planar
//! 3D" one (3D positions, rotation about Z only), the shape of the locked-axis option in ADR-0018;
//! it exists to prove the `Dim` trait is narrow enough to implement for another dimension, not as
//! a 3D feature (the real `d3` is phase 8).

use base::math::Vec3;
use physics::core::{
    BodyDesc, BodyKind, Bounds, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, Dim,
    HashBits, MassProperties, PhysicsError, Placed, StateHasher, World, WorldConfig,
};
use std::f32::consts::PI;

struct Planar3;

#[derive(Clone, Copy)]
struct Sphere {
    radius: f32,
    /// Added to the radius of the box only (not to the mass), so a test can make the box too large
    /// to represent for a body whose mass is fine.
    reach: f32,
}

impl HashBits for Sphere {
    fn write_bits(&self, hasher: &mut StateHasher) {
        hasher.write_f32(self.radius);
        hasher.write_f32(self.reach);
    }
}

/// A 3D axis-aligned box, written here to show that the tree and the pair list need nothing from
/// the dimension but the `Bounds` trait.
#[derive(Clone, Copy, Default, PartialEq)]
struct Box3 {
    min: [f32; 3],
    max: [f32; 3],
}

impl HashBits for Box3 {
    fn write_bits(&self, hasher: &mut StateHasher) {
        for value in self.min.into_iter().chain(self.max) {
            hasher.write_f32(value);
        }
    }
}

/// The depth two spheres overlap by; a second dimension needs only this much of a manifold.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Depth(f32);

impl HashBits for Depth {
    fn write_bits(&self, hasher: &mut StateHasher) {
        hasher.write_f32(self.0);
    }
}

impl Bounds for Box3 {
    fn overlaps(&self, other: &Self) -> bool {
        (0..3).all(|i| self.min[i] <= other.max[i] && other.min[i] <= self.max[i])
    }

    fn union(&self, other: &Self) -> Self {
        Self {
            min: std::array::from_fn(|i| self.min[i].min(other.min[i])),
            max: std::array::from_fn(|i| self.max[i].max(other.max[i])),
        }
    }

    fn contains(&self, inner: &Self) -> bool {
        (0..3).all(|i| self.min[i] <= inner.min[i] && inner.max[i] <= self.max[i])
    }

    fn measure(&self) -> f32 {
        let d: [f32; 3] = std::array::from_fn(|i| self.max[i] - self.min[i]);
        2.0 * (d[0] * d[1] + d[1] * d[2] + d[0] * d[2])
    }

    fn fattened(&self, margin: f32) -> Self {
        Self {
            min: self.min.map(|v| v - margin),
            max: self.max.map(|v| v + margin),
        }
    }

    fn is_valid(&self) -> bool {
        (0..3).all(|i| {
            self.min[i].is_finite() && self.max[i].is_finite() && self.min[i] <= self.max[i]
        })
    }
}

impl Dim for Planar3 {
    type Vec = Vec3;
    type Ang = f32;
    type Rot = f32;
    type InvInertia = f32;
    type Shape = Sphere;
    type Bounds = Box3;
    type Manifold = Depth;

    fn apply_inverse_inertia(_rot: f32, inv_inertia: f32, torque: f32) -> f32 {
        inv_inertia * torque
    }

    fn advance_rotation(rot: f32, angular_velocity: f32, dt: f32) -> f32 {
        rot + angular_velocity * dt
    }

    fn shape_bounds(shape: &Sphere, position: Vec3, _rot: f32) -> Box3 {
        let r = shape.radius + shape.reach;
        Box3 {
            min: [position.x - r, position.y - r, position.z - r],
            max: [position.x + r, position.y + r, position.z + r],
        }
    }

    fn collide(a: &Placed<Self>, b: &Placed<Self>, margin: f32) -> Option<Depth> {
        let distance = (b.position - a.position).length();
        let depth = a.shape.radius + b.shape.radius - distance;
        (depth >= -margin).then_some(Depth(depth))
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

fn sphere(radius: f32) -> Sphere {
    Sphere { radius, reach: 0.0 }
}

const DT: f32 = 0.01;

fn build() -> Result<World3, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: 4,
        gravity: Vec3::new(0.0, -10.0, 0.0),
        max_pairs: 256,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 256,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
    })
}

#[test]
fn a_second_dimension_steps_with_the_same_world_code() -> Result<(), PhysicsError> {
    let mut world = build()?;
    let mut desc =
        BodyDesc::<Planar3>::new(BodyKind::Dynamic, Vec3::new(1.0, 50.0, -2.0), sphere(0.5));
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
    let desc = BodyDesc::<Planar3>::new(BodyKind::Dynamic, Vec3::ZERO, sphere(1.0));
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
    let bad = BodyDesc::<Planar3>::new(BodyKind::Dynamic, Vec3::ZERO, sphere(-1.0));
    assert_eq!(
        first.create_body(&bad).err(),
        Some(PhysicsError::InvalidShape)
    );
    Ok(())
}

#[test]
fn the_second_dimension_gets_its_pairs_from_the_same_tree_and_pair_list() -> Result<(), PhysicsError>
{
    let mut world = World::<Planar3>::new(WorldConfig {
        max_bodies: 8,
        gravity: Vec3::ZERO,
        max_pairs: 16,
        broadphase_margin: 0.0,
        max_manifolds: 256,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
    })?;
    let ball = |x: f32, y: f32, z: f32| {
        BodyDesc::<Planar3>::new(BodyKind::Dynamic, Vec3::new(x, y, z), sphere(1.0))
    };
    let a = world.create_body(&ball(0.0, 0.0, 0.0))?;
    let b = world.create_body(&ball(1.5, 0.0, 0.0))?;
    world.create_body(&ball(0.0, 0.0, 5.0))?; // far away along z, the axis 2D does not have
    let floor = BodyDesc::<Planar3>::new(BodyKind::Static, Vec3::new(0.0, -2.0, 0.0), sphere(1.0));
    let c = world.create_body(&floor)?;
    world.step(DT)?;
    let found: Vec<_> = world
        .pairs()
        .as_slice()
        .iter()
        .map(|p| (p.first, p.second))
        .collect();
    assert_eq!(found, vec![(a, b), (a, c), (b, c)]);
    assert!(world.pairs().is_complete());
    Ok(())
}

#[test]
fn a_box_the_dimension_cannot_represent_is_refused_at_creation_and_in_the_step()
-> Result<(), PhysicsError> {
    let mut world = build()?;
    let wide = |x: f32, reach: f32| {
        let mut desc = BodyDesc::<Planar3>::new(
            BodyKind::Dynamic,
            Vec3::new(x, 0.0, 0.0),
            Sphere { radius: 1.0, reach },
        );
        desc.linear_velocity = Vec3::new(2.0e38, 0.0, 0.0);
        desc
    };
    // The box reaches past the largest float: refused, and no slot is used up.
    assert_eq!(
        world.create_body(&wide(3.0e38, f32::INFINITY)).err(),
        Some(PhysicsError::NotFinite)
    );
    assert!(world.is_empty());
    // The box fits now but would not after one step of 1 s (x = 3.2e38, plus a reach of 3e37).
    let handle = world.create_body(&wide(1.2e38, 3.0e37))?;
    let (state, tree) = (world.state_hash(), world.broadphase_hash());
    assert_eq!(world.step(1.0), Err(PhysicsError::NotFinite));
    assert_eq!((world.state_hash(), world.broadphase_hash()), (state, tree));
    assert_eq!(world.step_count(), 0);
    assert_eq!((handle.index(), handle.generation()), (0, 0));
    Ok(())
}
