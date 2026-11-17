//! Card 8-4: the 3D narrowphase and a world step with bodies in contact make no allocator call on
//! the calling thread (counting allocator with per-thread counters, as in `no_allocation.rs`).

use base::alloc::{CountingAllocator, count_allocations};
use base::math::{Quat, Vec3};
use physics::core::{Dim, PhysicsError};
use physics::d3::{
    BodyDesc, BodyKind, D3, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, DebugColour,
    IPhysicsDebugDraw, Placed3, Shape3, SleepConfig, SolverConfig, World, WorldConfig, draw_debug,
};
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

fn shapes() -> [Shape3; 3] {
    [
        Shape3::Sphere { radius: 0.5 },
        Shape3::Capsule {
            half_height: 0.5,
            radius: 0.25,
        },
        Shape3::Box {
            half_extents: Vec3::new(0.5, 0.4, 0.3),
        },
    ]
}

#[test]
fn the_counter_sees_an_allocation() {
    let (boxed, counts) = count_allocations(|| Box::new(7_u32));
    assert_eq!(*boxed, 7);
    assert!(
        counts.calls() >= 1,
        "if the counter reads zero it is not installed"
    );
}

fn shape_pairs() -> impl Iterator<Item = (Shape3, Shape3)> {
    shapes()
        .into_iter()
        .flat_map(|first| shapes().into_iter().map(move |second| (first, second)))
}

/// How many of the pairs, in a few positions and turned, touch within the margin.
fn touching_pairs() -> usize {
    let turn = Quat::from_axis_angle(Vec3::new(1.0, 2.0, 3.0), 0.7).unwrap_or(Quat::IDENTITY);
    let mut touching = 0;
    for (first, second) in shape_pairs() {
        for offset in [0.2, 0.6, 0.9, 1.2] {
            let a = Placed3::new(first, Vec3::ZERO, Quat::IDENTITY);
            let b = Placed3::new(second, Vec3::new(offset, 0.3, 0.1), turn);
            touching += usize::from(D3::collide(&a, &b, 0.1).is_some());
        }
    }
    touching
}

#[test]
fn collide_makes_no_allocator_call_for_any_pair_of_shapes() {
    let (touching, counts) = count_allocations(touching_pairs);
    assert!(
        touching > 20,
        "the loop must reach the contact code: {touching}"
    );
    assert_eq!(counts.calls(), 0);
}

#[test]
fn a_step_with_bodies_in_contact_makes_no_allocator_call() -> Result<(), PhysicsError> {
    let mut world = World::new(WorldConfig {
        max_bodies: 16,
        gravity: Vec3::new(0.0, -9.81, 0.0),
        max_pairs: 64,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 64,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DEFAULT,
    })?;
    for (i, shape) in shapes().into_iter().enumerate() {
        let x = f32::from(u8::try_from(i).unwrap_or(0)) * 0.6;
        world.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::new(x, 0.0, 0.0),
            shape,
        ))?;
    }
    world.step(1.0 / 60.0)?; // warm-up
    let (result, counts) = count_allocations(|| {
        for _ in 0..100 {
            world.step(1.0 / 60.0)?;
        }
        Ok::<(), PhysicsError>(())
    });
    result?;
    assert!(!world.manifolds().as_slice().is_empty(), "the bodies touch");
    assert_eq!(counts.calls(), 0);
    // Describing the world to a sink that only counts makes no allocator call either.
    let mut sink = Counting::default();
    let ((), counts) = count_allocations(|| draw_debug(&world, &mut sink));
    assert!(sink.lines > 0 && sink.points > 0);
    assert_eq!(counts.calls(), 0);
    Ok(())
}

#[derive(Default)]
struct Counting {
    lines: usize,
    points: usize,
}

impl IPhysicsDebugDraw for Counting {
    fn line(&mut self, _from: Vec3, _to: Vec3, _colour: DebugColour) {
        self.lines += 1;
    }
    fn point(&mut self, _at: Vec3, _colour: DebugColour) {
        self.points += 1;
    }
}
