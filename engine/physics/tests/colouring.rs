//! The graph colouring seen through the public 2D API (card 4-14): the counters of the solve report
//! how many colours a scene needs, a body with more contacts than there are colours overflows into
//! the serial set without losing a constraint, and sleeping removes constraints from the solve.

use base::math::Vec2;
use physics::d2::{
    BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, MAX_COLOURS,
    PhysicsError, Rot2, STANDARD_GRAVITY, Shape2, SleepConfig, SolverConfig, SolverStats, World,
    WorldConfig,
};

const DT: f32 = 1.0 / 60.0;

fn world(gravity: Vec2, sleep: SleepConfig) -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: 128,
        gravity,
        max_pairs: 4096,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 4096,
        max_joints: 0,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep,
    })
}

fn pyramid_world(rows: u8, sleep: SleepConfig) -> Result<World, PhysicsError> {
    let mut world = world(STANDARD_GRAVITY, sleep)?;
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec2::new(0.0, -1.0),
        Shape2::Box {
            half_extents: Vec2::new(50.0, 1.0),
        },
    ))?;
    for row in 0..rows {
        let count = rows - row;
        for i in 0..count {
            let x = f32::from(i) - f32::from(count - 1) / 2.0;
            world.create_body(&BodyDesc::new(
                BodyKind::Dynamic,
                Vec2::new(x, 0.5 + f32::from(row)),
                Shape2::Box {
                    half_extents: Vec2::new(0.5, 0.5),
                },
            ))?;
        }
    }
    Ok(world)
}

/// One ball of radius 1 in zero gravity with `count` balls of radius 0.1 touching it all around.
fn hub(count: u8, sleep: SleepConfig) -> Result<World, PhysicsError> {
    let mut world = world(Vec2::ZERO, sleep)?;
    world.create_body(&BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::ZERO,
        Shape2::Circle { radius: 1.0 },
    ))?;
    let step = std::f32::consts::TAU / f32::from(count);
    for k in 0..count {
        let spoke = Rot2::from_angle(step * f32::from(k)).rotate(Vec2::new(1.1, 0.0));
        world.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            spoke,
            Shape2::Circle { radius: 0.1 },
        ))?;
    }
    Ok(world)
}

#[test]
fn the_counters_of_a_pyramid_account_for_every_manifold() -> Result<(), PhysicsError> {
    let mut world = pyramid_world(10, SleepConfig::DISABLED)?;
    world.step(DT)?;
    let manifolds = world.manifolds().len();
    let stats = world.solver_stats();
    println!("10-high pyramid, first step: {stats:?}, manifolds of the next step {manifolds}");
    assert!(stats.solved > 100, "{stats:?}");
    assert_eq!(stats.frozen, 0);
    assert_eq!(stats.overflow, 0);
    assert!(
        stats.colours >= 3 && stats.colours <= 8,
        "colours of the pyramid: {}",
        stats.colours
    );
    // The next step solves the manifolds the first step made.
    world.step(DT)?;
    let after = world.solver_stats();
    assert_eq!(after.solved + after.frozen, manifolds);
    Ok(())
}

#[test]
fn a_body_with_more_contacts_than_colours_overflows_into_the_serial_set() -> Result<(), PhysicsError>
{
    let touching = u8::try_from(MAX_COLOURS + 6).unwrap_or(0);
    let mut world = hub(touching, SleepConfig::DISABLED)?;
    world.step(DT)?;
    assert_eq!(world.manifolds().len(), usize::from(touching));
    world.step(DT)?;
    let stats = world.solver_stats();
    assert_eq!(stats.colours, MAX_COLOURS);
    assert_eq!(stats.overflow, 6);
    assert_eq!(stats.solved, usize::from(touching), "nothing is lost");
    assert_eq!(stats.frozen, 0);
    Ok(())
}

#[test]
fn a_body_with_exactly_as_many_contacts_as_colours_does_not_overflow() -> Result<(), PhysicsError> {
    let touching = u8::try_from(MAX_COLOURS).unwrap_or(0);
    let mut world = hub(touching, SleepConfig::DISABLED)?;
    world.step(DT)?;
    world.step(DT)?;
    let stats = world.solver_stats();
    assert_eq!(stats.colours, MAX_COLOURS);
    assert_eq!(stats.overflow, 0);
    Ok(())
}

#[test]
fn the_overflow_set_is_solved_and_deterministic() -> Result<(), PhysicsError> {
    let run = || -> Result<Vec<u64>, PhysicsError> {
        let mut world = hub(30, SleepConfig::DISABLED)?;
        let mut hashes = Vec::new();
        for _ in 0..60 {
            world.step(DT)?;
            hashes.push(world.state_hash());
            assert_eq!(world.solver_stats().overflow, 6);
        }
        Ok(hashes)
    };
    assert_eq!(run()?, run()?);
    Ok(())
}

#[test]
fn a_sleeping_scene_solves_no_constraint() -> Result<(), PhysicsError> {
    let mut world = pyramid_world(4, SleepConfig::DEFAULT)?;
    for _ in 0..120 {
        world.step(DT)?;
    }
    assert_eq!(world.sleeping_count(), 10);
    let stats = world.solver_stats();
    assert_eq!(stats.solved, 0);
    assert_eq!(stats.colours, 0);
    assert_eq!(stats.overflow, 0);
    assert_eq!(stats.frozen, world.manifolds().len());
    Ok(())
}

#[test]
fn a_world_without_contacts_reports_nothing() -> Result<(), PhysicsError> {
    let mut world = world(STANDARD_GRAVITY, SleepConfig::DEFAULT)?;
    world.create_body(&BodyDesc::new(
        BodyKind::Dynamic,
        Vec2::new(0.0, 100.0),
        Shape2::Circle { radius: 0.5 },
    ))?;
    world.step(DT)?;
    assert_eq!(world.solver_stats(), SolverStats::default());
    Ok(())
}
