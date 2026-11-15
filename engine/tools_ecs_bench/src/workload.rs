//! The workloads: the shape of the phase 7 sandbox (a transform on every entity, a velocity on
//! most, a body on most of those) so a query meets three tables of different sizes.

use ecs::{EcsError, Read, World, Write};
use jobs::scheduler::IScheduler;
use std::hint::black_box;

#[derive(Clone, Copy)]
struct Transform {
    x: f32,
    y: f32,
    rot: f32,
}

#[derive(Clone, Copy)]
struct Velocity {
    x: f32,
    y: f32,
}

#[derive(Clone, Copy)]
struct Body {
    mass: f32,
    radius: f32,
}

/// One entity in this many has a transform only, and one has a transform and a velocity.
const SPARSE_EVERY: u32 = 8;

/// Values repeat after this many entities, so they stay exact in an `f32`.
const VALUE_CYCLE: u32 = 1024;

/// Time step of the integration workload, seconds (a power of two: exact in an `f32`).
const STEP: f32 = 1.0 / 64.0;

/// The three query shapes: one, two (one written) and three components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Read the transform.
    Sum,
    /// Write the transform, read the velocity.
    Integrate,
    /// Read the transform, the velocity and the body.
    Extract,
}

impl Kind {
    /// Every workload, in report order.
    pub const ALL: [Self; 3] = [Self::Sum, Self::Integrate, Self::Extract];

    /// Name in the report.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Sum => "1 component, read",
            Self::Integrate => "2 components, one written",
            Self::Extract => "3 components, read",
        }
    }
}

fn value(index: u32) -> f32 {
    f32::from(u16::try_from(index % VALUE_CYCLE).unwrap_or(0))
}

/// A world of `entities` entities in three tables.
///
/// # Errors
/// Any refusal of the world (not expected for sizes below the entity limit).
pub fn build(entities: u32) -> Result<World, EcsError> {
    let mut world = World::new();
    for index in 0..entities {
        let entity = world.spawn()?;
        let v = value(index);
        world.insert(
            entity,
            Transform {
                x: v,
                y: v + 1.0,
                rot: 0.0,
            },
        )?;
        if index % SPARSE_EVERY != 0 {
            world.insert(entity, Velocity { x: 1.0, y: -1.0 })?;
        }
        if index % SPARSE_EVERY > 1 {
            world.insert(
                entity,
                Body {
                    mass: 1.0 + v,
                    radius: 0.5,
                },
            )?;
        }
    }
    Ok(world)
}

fn mix(a: f32, b: f32, c: f32) -> u64 {
    u64::from(a.to_bits())
        .wrapping_mul(3)
        .wrapping_add(u64::from(b.to_bits()).wrapping_mul(5))
        .wrapping_add(u64::from(c.to_bits()).wrapping_mul(7))
}

/// Sum of the bits of every transform: order independent, so the same on any schedule.
fn digest(world: &mut World) -> Result<u64, EcsError> {
    let mut total = 0_u64;
    world.for_each::<Read<Transform>, _>(|t| total = total.wrapping_add(mix(t.x, t.y, t.rot)))?;
    Ok(total)
}

/// Runs the workload `kind` over the world `frames` times on the calling thread and returns a
/// checksum of the world at the end. The read-only workloads fold their values into
/// `black_box` and no shared counter: one atomic per entity would measure the cache line, not the
/// query.
///
/// # Errors
/// Any refusal of the world.
pub fn run_serial(world: &mut World, kind: Kind, frames: u32) -> Result<u64, EcsError> {
    frames_serial(world, kind, frames)?;
    digest(world)
}

/// The frames of [`run_serial`] without the closing checksum (what the report times).
///
/// # Errors
/// Any refusal of the world.
pub fn frames_serial(world: &mut World, kind: Kind, frames: u32) -> Result<(), EcsError> {
    for _ in 0..frames {
        match kind {
            Kind::Sum => world.for_each::<Read<Transform>, _>(|t| {
                black_box(mix(t.x, t.y, t.rot));
            })?,
            Kind::Integrate => world
                .for_each::<(Write<Transform>, Read<Velocity>), _>(|(t, v)| integrate(t, *v))?,
            Kind::Extract => world.for_each::<(Read<Transform>, Read<Velocity>, Read<Body>), _>(
                |(t, v, b)| {
                    black_box(extract(t, *v, *b));
                },
            )?,
        }
    }
    Ok(())
}

/// Like [`run_serial`], each query on `scheduler` with chunks of at least `grain` rows.
///
/// # Errors
/// Any refusal of the world or the scheduler.
pub fn run_parallel(
    world: &mut World,
    kind: Kind,
    frames: u32,
    scheduler: &dyn IScheduler,
    grain: u32,
) -> Result<u64, EcsError> {
    frames_parallel(world, kind, frames, scheduler, grain)?;
    digest(world)
}

/// The frames of [`run_parallel`] without the closing checksum (what the report times).
///
/// # Errors
/// Any refusal of the world or the scheduler.
pub fn frames_parallel(
    world: &mut World,
    kind: Kind,
    frames: u32,
    scheduler: &dyn IScheduler,
    grain: u32,
) -> Result<(), EcsError> {
    for _ in 0..frames {
        match kind {
            Kind::Sum => world.par_for_each::<Read<Transform>, _>(scheduler, grain, |t| {
                black_box(mix(t.x, t.y, t.rot));
            })?,
            Kind::Integrate => world.par_for_each::<(Write<Transform>, Read<Velocity>), _>(
                scheduler,
                grain,
                |(t, v)| integrate(t, *v),
            )?,
            Kind::Extract => world
                .par_for_each::<(Read<Transform>, Read<Velocity>, Read<Body>), _>(
                    scheduler,
                    grain,
                    |(t, v, b)| {
                        black_box(extract(t, *v, *b));
                    },
                )?,
        }
    }
    Ok(())
}

fn integrate(t: &mut Transform, v: Velocity) {
    t.x += v.x * STEP;
    t.y += v.y * STEP;
    t.rot += STEP;
}

fn extract(t: &Transform, v: Velocity, b: Body) -> u64 {
    mix(t.x * b.mass, t.y + v.x, b.radius)
}
