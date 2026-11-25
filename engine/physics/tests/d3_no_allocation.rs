//! Card 8-4: the 3D narrowphase and a world step with bodies in contact make no allocator call on
//! the calling thread (counting allocator with per-thread counters, as in `no_allocation.rs`).

use base::alloc::{CountingAllocator, count_allocations};
use base::math::{Quat, Vec3};
use physics::core::{Dim, PhysicsError};
use physics::d3::{
    AngleLimits, BallJoint3, BodyDesc, BodyKind, D3, DEFAULT_BROADPHASE_MARGIN,
    DEFAULT_SPECULATIVE_MARGIN, DebugColour, DistanceJoint3, HingeJoint3, Hull, IPhysicsDebugDraw,
    Joint3, JointDesc, LengthLimits, Placed3, Shape3, SleepConfig, SolverConfig, World,
    WorldConfig, draw_debug,
};
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

fn hull() -> Shape3 {
    let corners = [
        Vec3::new(-0.5, -0.4, -0.3),
        Vec3::new(0.5, -0.4, -0.3),
        Vec3::new(0.0, 0.5, -0.3),
        Vec3::new(0.0, 0.0, 0.5),
        Vec3::new(0.3, 0.1, 0.1),
    ];
    match Hull::new(&corners) {
        Ok(hull) => Shape3::Hull(hull),
        Err(error) => panic!("a hull: {error:?}"),
    }
}

/// A cube of half side 0.5: its faces line up with a box's, so the face clipping of card 8-18 runs.
fn cube_hull() -> Shape3 {
    let corner = |n: u8| {
        let side = |bit: u8| if n >> bit & 1 == 1 { 0.5 } else { -0.5 };
        Vec3::new(side(0), side(1), side(2))
    };
    let corners = [0, 1, 2, 3, 4, 5, 6, 7].map(corner);
    match Hull::new(&corners) {
        Ok(hull) => Shape3::Hull(hull),
        Err(error) => panic!("a cube hull: {error:?}"),
    }
}

fn shapes() -> [Shape3; 5] {
    [
        Shape3::Sphere { radius: 0.5 },
        Shape3::Capsule {
            half_height: 0.5,
            radius: 0.25,
        },
        Shape3::Box {
            half_extents: Vec3::new(0.5, 0.4, 0.3),
        },
        hull(),
        cube_hull(),
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
        for offset in [0.2, 0.6, 0.9, 1.2, 0.0] {
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
        touching > 30,
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
    // The solver pushes overlapping bodies apart (card 8-6), so the bodies rest on a floor and
    // keep touching it, and each other where they are close, through the measured steps.
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, -1.0, 0.0),
        Shape3::Box {
            half_extents: Vec3::new(20.0, 1.0, 20.0),
        },
    ))?;
    for (i, shape) in shapes().into_iter().enumerate() {
        let x = f32::from(u8::try_from(i).unwrap_or(0)) * 0.6;
        world.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::new(x, 1.0, 0.0),
            shape,
        ))?;
    }
    for _ in 0..120 {
        world.step(1.0 / 60.0)?; // warm-up: land and settle
    }
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

/// A world with a static post and a chain of three small bodies hanging from it: a ball joint, a
/// hinge with limits that the swing reaches, and a rope.
fn world_with_joints() -> Result<World, PhysicsError> {
    let mut world = World::new(WorldConfig {
        max_bodies: 8,
        gravity: Vec3::new(0.0, -9.81, 0.0),
        max_pairs: 16,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 16,
        max_joints: 8,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DISABLED,
    })?;
    let mut previous = world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, 10.0, 0.0),
        Shape3::Sphere { radius: 0.1 },
    ))?;
    let hinge = HingeJoint3 {
        limits: Some(AngleLimits {
            lower: -0.3,
            upper: 0.3,
        }),
        ..HingeJoint3::new(
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::Z,
            Vec3::Z,
        )
    };
    let rope = DistanceJoint3 {
        limits: Some(LengthLimits { min: 1.0, max: 3.0 }),
        ..DistanceJoint3::new(Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO, 2.0)
    };
    let joints = [
        Joint3::Ball(BallJoint3::new(Vec3::ZERO, Vec3::new(-1.0, 0.0, 0.0))),
        Joint3::Hinge(hinge),
        Joint3::Distance(rope),
    ];
    for (i, joint) in joints.into_iter().enumerate() {
        let x = 1.0 + 2.0 * f32::from(u8::try_from(i).unwrap_or(0));
        let body = world.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::new(x, 10.0, 0.0),
            Shape3::Sphere { radius: 0.1 },
        ))?;
        world.create_joint(&JointDesc::new(previous, body, joint))?;
        previous = body;
    }
    Ok(world)
}

#[test]
fn a_step_with_joints_makes_no_allocator_call() -> Result<(), PhysicsError> {
    let mut world = world_with_joints()?;
    for _ in 0..60 {
        world.step(1.0 / 60.0)?; // warm-up
    }
    let (result, counts) = count_allocations(|| {
        for _ in 0..200 {
            world.step(1.0 / 60.0)?;
        }
        Ok::<(), PhysicsError>(())
    });
    result?;
    assert_eq!(
        world.solver_stats().joints_solved,
        3,
        "the joints are solved"
    );
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

/// Card 8-74 (the 3D half of the test of card 8-53): asking a world for its deepest overlap is one
/// pass over the manifolds and returns by value, so it makes no allocator call, for every pair of
/// shapes that overlaps.
#[test]
fn asking_for_the_deepest_overlap_makes_no_allocator_call() -> Result<(), PhysicsError> {
    let mut asked = 0;
    for (first, second) in shape_pairs() {
        // A step of 1e-6 s only fills the manifolds: the bodies do not move visibly.
        let mut world = World::new(WorldConfig {
            max_bodies: 4,
            gravity: Vec3::ZERO,
            max_pairs: 16,
            broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
            max_manifolds: 16,
            max_joints: 0,
            speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
            solver: SolverConfig::DEFAULT,
            sleep: SleepConfig::DISABLED,
        })?;
        world.create_body(&BodyDesc::new(BodyKind::Dynamic, Vec3::ZERO, first))?;
        world.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::new(0.0, 0.3, 0.0),
            second,
        ))?;
        world.step(1.0e-6)?;
        let (found, counts) = count_allocations(|| world.deepest_overlap());
        assert_eq!(counts.calls(), 0, "{counts:?}");
        asked += usize::from(found.is_some());
    }
    // Every pair overlaps by construction (measured: all of them gave a report); a pair that did
    // not would make the test pass on nothing.
    assert_eq!(
        asked,
        shape_pairs().count(),
        "only {asked} pairs overlapped"
    );
    Ok(())
}
