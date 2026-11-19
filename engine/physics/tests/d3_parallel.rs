//! Card 8-19: the parallel colour solve of the 3D contact solver. The state hash after every step
//! is the same bit for bit whether the solver runs the serial reference or runs the chunks of each
//! colour through a scheduler, at any worker count, chunk size, order and grouping of the chunks
//! (D0 and D1). The reference is the serial solver, which sweeps the constraints colour by colour.

use base::dmath::{cos, sin};
use base::math::{Quat, Vec3};
use base::rng::Pcg32;
use jobs::error::JobError;
use jobs::inline::InlineScheduler;
use jobs::pool::WorkStealingPool;
use jobs::reversed::ReversedScheduler;
use jobs::scheduler::{IScheduler, RangeBody, Task};
use jobs::shuffled::ShuffledScheduler;
use physics::d3::{
    BallJoint3, BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, Joint3,
    JointDesc, PhysicsError, STANDARD_GRAVITY, Shape3, SleepConfig, SolverConfig, SolverExecution,
    World, WorldConfig,
};
use platform::threads::StdThreads;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const DT: f32 = 1.0 / 60.0;

/// Worker counts of the determinism matrix (owner choice O10: 4 is the scaling target).
const WORKERS: [u32; 4] = [1, 2, 4, 6];

/// A chunk size small enough that the colours of the test scenes are cut into several chunks.
const SMALL_CHUNK: u32 = 4;

fn config(sleep: SleepConfig, gravity: Vec3) -> WorldConfig {
    WorldConfig {
        max_bodies: 256,
        gravity,
        max_pairs: 8192,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 8192,
        max_joints: 64,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep,
    }
}

fn cuboid(x: f32, y: f32, z: f32) -> Shape3 {
    Shape3::Box {
        half_extents: Vec3::new(x, y, z),
    }
}

fn floor(world: &mut World) -> Result<(), PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, -1.0, 0.0),
        cuboid(50.0, 1.0, 50.0),
    ))?;
    Ok(())
}

fn crate_at(world: &mut World, at: Vec3, half: f32) -> Result<(), PhysicsError> {
    world.create_body(&BodyDesc::new(
        BodyKind::Dynamic,
        at,
        cuboid(half, half, half),
    ))?;
    Ok(())
}

/// One row of a pyramid of unit crates, two crates deep.
fn add_row(world: &mut World, row: u8, rows: u8, x: f32) -> Result<(), PhysicsError> {
    let count = rows - row;
    for i in 0..count {
        let dx = f32::from(i) - f32::from(count - 1) / 2.0;
        for depth in [-0.5, 0.5] {
            crate_at(world, Vec3::new(x + dx, 0.5 + f32::from(row), depth), 0.5)?;
        }
    }
    Ok(())
}

/// A pyramid of unit crates, `rows` high, two crates deep.
fn pyramid_of(world: &mut World, rows: u8, x: f32) -> Result<(), PhysicsError> {
    for row in 0..rows {
        add_row(world, row, rows, x)?;
    }
    Ok(())
}

/// A pyramid six rows high on a floor.
fn pyramid(sleep: SleepConfig) -> Result<World, PhysicsError> {
    let mut world = World::new(config(sleep, STANDARD_GRAVITY))?;
    floor(&mut world)?;
    pyramid_of(&mut world, 6, 0.0)?;
    Ok(world)
}

/// Boxes, spheres and capsules dropped at random places between four walls.
fn pile(sleep: SleepConfig) -> Result<World, PhysicsError> {
    let mut world = World::new(config(sleep, STANDARD_GRAVITY))?;
    floor(&mut world)?;
    for (x, z) in [(-4.0, 0.0), (4.0, 0.0), (0.0, -4.0), (0.0, 4.0)] {
        let (wx, wz) = if x == 0.0 { (4.5, 0.5) } else { (0.5, 4.5) };
        world.create_body(&BodyDesc::new(
            BodyKind::Static,
            Vec3::new(x, 3.0, z),
            cuboid(wx, 4.0, wz),
        ))?;
    }
    let mut rng = Pcg32::new(7, 11);
    for index in 0..60_u32 {
        let at = Vec3::new(
            rng.next_f32() * 5.0 - 2.5,
            0.5 + rng.next_f32() * 6.0,
            rng.next_f32() * 5.0 - 2.5,
        );
        let size = 0.2 + rng.next_f32() * 0.2;
        let shape = match index % 3 {
            0 => Shape3::Sphere { radius: size },
            1 => cuboid(size, size, size),
            _ => Shape3::Capsule {
                half_height: size,
                radius: size * 0.6,
            },
        };
        let mut desc = BodyDesc::new(BodyKind::Dynamic, at, shape);
        desc.rotation = Quat::from_axis_angle(Vec3::new(1.0, 1.0, 0.0), rng.next_f32())
            .unwrap_or(Quat::IDENTITY);
        world.create_body(&desc)?;
    }
    Ok(world)
}

/// One large ball with more small balls touching it than there are colours, so part of its
/// constraints overflow into the serial set, next to a small pyramid that does not.
fn hub(sleep: SleepConfig) -> Result<World, PhysicsError> {
    let mut world = World::new(config(sleep, Vec3::ZERO))?;
    world.create_body(&BodyDesc::new(
        BodyKind::Dynamic,
        Vec3::new(-8.0, 0.0, 0.0),
        Shape3::Sphere { radius: 1.0 },
    ))?;
    let spokes: u8 = 30;
    let golden = std::f32::consts::PI * (3.0 - 5.0_f32.sqrt());
    for k in 0..spokes {
        // Points spread over a sphere (a spiral), each pushed in at the hub.
        let height = 1.0 - 2.0 * (f32::from(k) + 0.5) / f32::from(spokes);
        let ring = (1.0 - height * height).max(0.0).sqrt();
        let turn = golden * f32::from(k);
        let direction = Vec3::new(ring * cos(turn), height, ring * sin(turn));
        let ball = world.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::new(-8.0, 0.0, 0.0) + direction * 1.1,
            Shape3::Sphere { radius: 0.1 },
        ))?;
        world.set_linear_velocity(ball, direction * -2.0)?;
    }
    floor(&mut world)?;
    pyramid_of(&mut world, 3, 8.0)?;
    Ok(world)
}

/// Two pyramids that fall asleep; later a ball dropped on one of them wakes it.
fn sleepers() -> Result<World, PhysicsError> {
    let mut world = World::new(config(SleepConfig::DEFAULT, STANDARD_GRAVITY))?;
    floor(&mut world)?;
    pyramid_of(&mut world, 3, -6.0)?;
    pyramid_of(&mut world, 2, 6.0)?;
    Ok(world)
}

/// A chain of balls hanging above a heap of crates, so contacts and joints share bodies.
fn chain_over_crates() -> Result<World, PhysicsError> {
    let mut world = World::new(config(SleepConfig::DISABLED, STANDARD_GRAVITY))?;
    floor(&mut world)?;
    pyramid_of(&mut world, 3, 0.0)?;
    let pin = world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec3::new(0.0, 6.0, 0.0),
        Shape3::Sphere { radius: 0.1 },
    ))?;
    let mut last = pin;
    for link in 1..=4_u8 {
        let next = world.create_body(&BodyDesc::new(
            BodyKind::Dynamic,
            Vec3::new(0.0, 6.0 - f32::from(link) * 0.8, 0.0),
            Shape3::Sphere { radius: 0.3 },
        ))?;
        let joint = BallJoint3::new(Vec3::new(0.0, -0.4, 0.0), Vec3::new(0.0, 0.4, 0.0));
        let joint = if link == 1 {
            BallJoint3::new(Vec3::ZERO, Vec3::new(0.0, 0.8, 0.0))
        } else {
            joint
        };
        world.create_joint(&JointDesc::new(last, next, Joint3::Ball(joint)))?;
        last = next;
    }
    Ok(world)
}

/// Steps before the ball is dropped, and after.
const SLEEP_STEPS: u32 = 220;
const WAKE_STEPS: u32 = 220;

/// The hash and the number of sleeping bodies after every step of the sleep-and-wake scene.
fn wake_run(execution: SolverExecution) -> Result<Vec<(u64, usize)>, PhysicsError> {
    let mut world = sleepers()?;
    world.set_execution(execution);
    let mut seen = Vec::new();
    for step in 0..SLEEP_STEPS + WAKE_STEPS {
        if step == SLEEP_STEPS {
            world.create_body(&BodyDesc::new(
                BodyKind::Dynamic,
                Vec3::new(-6.0, 6.0, 0.0),
                Shape3::Sphere { radius: 0.4 },
            ))?;
        }
        world.step(DT)?;
        seen.push((world.state_hash(), world.sleeping_count()));
    }
    Ok(seen)
}

/// The hash after every step of `steps` steps of `world`.
fn hashes(world: &mut World, steps: u32) -> Result<Vec<u64>, PhysicsError> {
    let mut seen = Vec::new();
    for _ in 0..steps {
        world.step(DT)?;
        seen.push(world.state_hash());
    }
    Ok(seen)
}

/// The hashes of `scene` run with `execution`.
fn run(
    scene: &dyn Fn() -> Result<World, PhysicsError>,
    execution: SolverExecution,
    steps: u32,
) -> Result<Vec<u64>, PhysicsError> {
    let mut world = scene()?;
    world.set_execution(execution);
    hashes(&mut world, steps)
}

fn scheduled(scheduler: Arc<dyn IScheduler>, chunk: u32) -> SolverExecution {
    SolverExecution {
        scheduler: Some(scheduler),
        simd: false,
        chunk,
    }
}

fn pool(workers: u32) -> Result<Arc<dyn IScheduler>, PhysicsError> {
    WorkStealingPool::new(workers, &StdThreads::new())
        .map(|pool| Arc::new(pool) as Arc<dyn IScheduler>)
        .map_err(|_| PhysicsError::NotFinite)
}

/// The scenes of the matrix with the number of steps each runs.
type Scene = (&'static str, fn() -> Result<World, PhysicsError>, u32);

const SCENES: [Scene; 5] = [
    ("pyramid", || pyramid(SleepConfig::DISABLED), 80),
    ("pile", || pile(SleepConfig::DEFAULT), 100),
    ("hub", || hub(SleepConfig::DISABLED), 50),
    ("chain over crates", chain_over_crates, 80),
    ("pile asleep", || pile(SleepConfig::DEFAULT), 330),
];

fn reference(scene: &Scene) -> Result<Vec<u64>, PhysicsError> {
    run(&scene.1, SolverExecution::SERIAL, scene.2)
}

/// The matrix would prove little if its scenes did not have the shape the names promise.
#[test]
fn the_scenes_have_the_shape_their_names_promise() -> Result<(), PhysicsError> {
    let mut hub_world = hub(SleepConfig::DISABLED)?;
    for _ in 0..5 {
        hub_world.step(DT)?;
    }
    let stats = hub_world.solver_stats();
    assert!(
        stats.overflow > 0,
        "the hub overflows the colours: {stats:?}"
    );
    assert!(stats.colours > 1, "and still uses several: {stats:?}");

    let mut stacked = pyramid(SleepConfig::DISABLED)?;
    for _ in 0..5 {
        stacked.step(DT)?;
    }
    let stats = stacked.solver_stats();
    assert!(
        stats.colours > 2,
        "a pyramid needs several colours: {stats:?}"
    );
    assert_eq!(stats.overflow, 0, "and none overflows: {stats:?}");

    let mut asleep = pile(SleepConfig::DEFAULT)?;
    for _ in 0..330 {
        asleep.step(DT)?;
    }
    assert!(asleep.sleeping_count() > 0, "part of the pile sleeps");
    Ok(())
}

#[test]
fn the_staged_path_without_a_scheduler_equals_the_reference() -> Result<(), PhysicsError> {
    // Chunks run on the calling thread: the gather, compute and scatter steps alone change nothing.
    for scene in &SCENES {
        let staged = scheduled(Arc::new(InlineScheduler), SMALL_CHUNK);
        assert_eq!(
            run(&scene.1, staged, scene.2)?,
            reference(scene)?,
            "{}",
            scene.0
        );
    }
    Ok(())
}

/// Runs every scene through a pool of `workers`.
fn check_pool(workers: u32) -> Result<(), PhysicsError> {
    let scheduler = pool(workers)?;
    for scene in &SCENES[..4] {
        let execution = scheduled(Arc::clone(&scheduler), SMALL_CHUNK);
        let got = run(&scene.1, execution, scene.2)?;
        assert_eq!(got, reference(scene)?, "{} workers {workers}", scene.0);
    }
    Ok(())
}

#[test]
fn a_pool_of_one_worker_equals_the_reference() -> Result<(), PhysicsError> {
    check_pool(WORKERS[0])
}

#[test]
fn a_pool_of_two_workers_equals_the_reference() -> Result<(), PhysicsError> {
    check_pool(WORKERS[1])
}

#[test]
fn a_pool_of_four_workers_equals_the_reference() -> Result<(), PhysicsError> {
    check_pool(WORKERS[2])
}

#[test]
fn a_pool_of_six_workers_equals_the_reference() -> Result<(), PhysicsError> {
    check_pool(WORKERS[3])
}

#[test]
fn islands_that_sleep_and_wake_are_the_same_in_every_configuration() -> Result<(), PhysicsError> {
    let expected = wake_run(SolverExecution::SERIAL)?;
    let counts: Vec<usize> = expected.iter().map(|&(_, asleep)| asleep).collect();
    let peak = counts.iter().copied().max().unwrap_or(0);
    assert!(peak >= 10, "the pyramids fall asleep: {peak}");
    assert!(
        counts.windows(2).any(|pair| pair[1] < pair[0]),
        "the ball wakes a pyramid"
    );
    for workers in WORKERS {
        let execution = scheduled(pool(workers)?, SMALL_CHUNK);
        assert_eq!(wake_run(execution)?, expected, "workers {workers}");
    }
    Ok(())
}

#[test]
fn the_chunk_size_does_not_change_a_bit() -> Result<(), PhysicsError> {
    let scheduler = pool(4)?;
    for scene in &SCENES[..4] {
        let expected = reference(scene)?;
        for chunk in [1, 5, 32, 1000] {
            let execution = scheduled(Arc::clone(&scheduler), chunk);
            assert_eq!(
                run(&scene.1, execution, scene.2)?,
                expected,
                "{} chunk {chunk}",
                scene.0
            );
        }
    }
    Ok(())
}

#[test]
fn the_order_and_grouping_of_the_chunks_do_not_change_a_bit() -> Result<(), PhysicsError> {
    let schedulers: [Arc<dyn IScheduler>; 4] = [
        Arc::new(ReversedScheduler),
        Arc::new(ShuffledScheduler::new(1, 4096)),
        Arc::new(ShuffledScheduler::new(99, 4096)),
        Arc::new(OneRange),
    ];
    for scene in &SCENES[..4] {
        let expected = reference(scene)?;
        for scheduler in &schedulers {
            let execution = scheduled(Arc::clone(scheduler), SMALL_CHUNK);
            assert_eq!(run(&scene.1, execution, scene.2)?, expected, "{}", scene.0);
        }
    }
    Ok(())
}

#[test]
fn a_scheduler_that_refuses_leaves_the_result_unchanged() -> Result<(), PhysicsError> {
    for scene in &SCENES[..4] {
        let execution = scheduled(Arc::new(Refusing), SMALL_CHUNK);
        assert_eq!(
            run(&scene.1, execution, scene.2)?,
            reference(scene)?,
            "{}",
            scene.0
        );
    }
    Ok(())
}

#[test]
fn repeated_parallel_runs_are_identical() -> Result<(), PhysicsError> {
    let scheduler = pool(4)?;
    let scene = &SCENES[1];
    let expected = reference(scene)?;
    for _ in 0..5 {
        let execution = scheduled(Arc::clone(&scheduler), SMALL_CHUNK);
        assert_eq!(run(&scene.1, execution, scene.2)?, expected);
    }
    Ok(())
}

#[test]
fn one_pool_can_serve_several_worlds_in_turn_and_the_execution_can_change()
-> Result<(), PhysicsError> {
    let scheduler = pool(3)?;
    let scene = &SCENES[0];
    let expected = reference(scene)?;
    let mut world = (scene.1)()?;
    world.set_execution(scheduled(Arc::clone(&scheduler), SMALL_CHUNK));
    let mut seen = hashes(&mut world, 40)?;
    world.set_execution(SolverExecution::SERIAL);
    seen.extend(hashes(&mut world, scene.2 - 40)?);
    assert_eq!(seen, expected);
    Ok(())
}

/// Counts the calls and the chunks of the loops it runs, and runs them on the calling thread.
#[derive(Default)]
struct Counting {
    calls: AtomicUsize,
    chunks: AtomicUsize,
    largest: AtomicUsize,
}

impl IScheduler for Counting {
    fn worker_count(&self) -> u32 {
        1
    }

    fn parallel_for(&self, count: u32, grain: u32, body: &RangeBody<'_>) -> Result<(), JobError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.chunks.fetch_add(count as usize, Ordering::Relaxed);
        self.largest.fetch_max(count as usize, Ordering::Relaxed);
        InlineScheduler.parallel_for(count, grain, body)
    }

    fn run_scope(&self, tasks: &[&Task<'_>]) -> Result<(), JobError> {
        InlineScheduler.run_scope(tasks)
    }
}

/// Calls the body once with the whole range: the coarsest grouping a scheduler may make.
struct OneRange;

impl IScheduler for OneRange {
    fn worker_count(&self) -> u32 {
        1
    }

    fn parallel_for(&self, count: u32, grain: u32, body: &RangeBody<'_>) -> Result<(), JobError> {
        if grain == 0 {
            return Err(JobError::InvalidGrain);
        }
        if count > 0 {
            body(0..count);
        }
        Ok(())
    }

    fn run_scope(&self, tasks: &[&Task<'_>]) -> Result<(), JobError> {
        for task in tasks {
            task();
        }
        Ok(())
    }
}

/// Refuses every call, as a busy scheduler does; no job runs.
struct Refusing;

impl IScheduler for Refusing {
    fn worker_count(&self) -> u32 {
        1
    }

    fn parallel_for(&self, _: u32, _: u32, _: &RangeBody<'_>) -> Result<(), JobError> {
        Err(JobError::Busy)
    }

    fn run_scope(&self, _: &[&Task<'_>]) -> Result<(), JobError> {
        Err(JobError::Busy)
    }
}

#[test]
fn colours_with_several_chunks_go_to_the_scheduler_and_small_ones_do_not()
-> Result<(), PhysicsError> {
    let counting = Arc::new(Counting::default());
    let execution = scheduled(Arc::clone(&counting) as Arc<dyn IScheduler>, SMALL_CHUNK);
    run(&SCENES[0].1, execution, 40)?;
    let calls = counting.calls.load(Ordering::Relaxed);
    let chunks = counting.chunks.load(Ordering::Relaxed);
    assert!(calls > 0, "the scheduler was used");
    assert!(
        chunks >= 2 * calls,
        "every call has more than one chunk: {chunks} in {calls}"
    );
    assert!(
        counting.largest.load(Ordering::Relaxed) >= 3,
        "a colour of a pyramid has chunks to spare"
    );
    // A chunk size larger than any colour never reaches the scheduler.
    let huge = Arc::new(Counting::default());
    let execution = scheduled(Arc::clone(&huge) as Arc<dyn IScheduler>, 1_000_000);
    run(&SCENES[0].1, execution, 40)?;
    assert_eq!(huge.calls.load(Ordering::Relaxed), 0);
    Ok(())
}

#[test]
fn the_floating_point_control_state_of_the_caller_is_deterministic_and_restored()
-> Result<(), PhysicsError> {
    let before = base::fpenv::current();
    let execution = scheduled(pool(2)?, SMALL_CHUNK);
    run(&SCENES[0].1, execution, 20)?;
    assert_eq!(base::fpenv::current(), before);
    assert!(base::fpenv::assert_deterministic().is_ok() || before.is_err());
    Ok(())
}

#[test]
fn the_stats_report_the_colours_of_the_last_step() -> Result<(), PhysicsError> {
    let mut world = pyramid(SleepConfig::DISABLED)?;
    assert_eq!(world.solver_stats().colours, 0, "nothing solved yet");
    world.step(DT)?;
    let stats = world.solver_stats();
    assert!(stats.solved > 0 && stats.colours > 0, "{stats:?}");
    Ok(())
}
