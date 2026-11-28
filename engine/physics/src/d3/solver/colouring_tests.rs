//! Tests of the colouring of the 3D solver and of the order inside a colour (card 8-19).

use super::{Reorder, Solver3};
use crate::core::{Colour, ContactSolver, MAX_COLOURS, Pair, PhysicsError, SolverExecution};
use crate::d3::{BodyDesc, BodyKind, D3, Shape3, SleepConfig, SolverConfig, World, WorldConfig};
use base::math::Vec3;
use base::rng::Pcg32;
use jobs::error::JobError;
use jobs::inline::InlineScheduler;
use jobs::reversed::ReversedScheduler;
use jobs::scheduler::{IScheduler, RangeBody, Task};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn big_world(sleep: SleepConfig) -> Result<World, PhysicsError> {
    world_with_manifolds(sleep, 4096)
}

fn world_with_manifolds(sleep: SleepConfig, max_manifolds: u32) -> Result<World, PhysicsError> {
    world_with(sleep, max_manifolds, 4096)
}

fn world_with(
    sleep: SleepConfig,
    max_manifolds: u32,
    max_pairs: u32,
) -> Result<World, PhysicsError> {
    World::new(WorldConfig {
        max_bodies: 256,
        gravity: Vec3::new(0.0, -9.8, 0.0),
        max_pairs,
        broadphase_margin: crate::d3::DEFAULT_BROADPHASE_MARGIN,
        max_manifolds,
        max_joints: 0,
        speculative_margin: crate::d3::DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep,
    })
}

fn static_box(w: &mut World, at: Vec3, half: Vec3) -> Result<(), PhysicsError> {
    let shape = Shape3::Box { half_extents: half };
    w.create_body(&BodyDesc::new(BodyKind::Static, at, shape))?;
    Ok(())
}

/// A ball for an even `index`, a cube for an odd one, of size `size`.
fn shape_of(index: u32, size: f32) -> Shape3 {
    if index.is_multiple_of(2) {
        Shape3::Sphere { radius: size }
    } else {
        Shape3::Box {
            half_extents: Vec3::new(size, size, size),
        }
    }
}

/// Random boxes and balls dropped between a floor and two walls.
fn random_scene(seed: u64, count: u32, sleep: SleepConfig) -> Result<World, PhysicsError> {
    let mut w = big_world(sleep)?;
    static_box(&mut w, Vec3::new(0.0, -1.0, 0.0), Vec3::new(8.0, 1.0, 8.0))?;
    static_box(&mut w, Vec3::new(-4.5, 4.0, 0.0), Vec3::new(0.5, 6.0, 8.0))?;
    static_box(&mut w, Vec3::new(4.5, 4.0, 0.0), Vec3::new(0.5, 6.0, 8.0))?;
    let mut rng = Pcg32::new(seed, 11);
    for index in 0..count {
        let at = Vec3::new(
            rng.next_f32() * 6.0 - 3.0,
            0.5 + rng.next_f32() * 6.0,
            rng.next_f32() * 6.0 - 3.0,
        );
        let shape = shape_of(index, 0.25 + rng.next_f32() * 0.25);
        w.create_body(&BodyDesc::new(BodyKind::Dynamic, at, shape))?;
    }
    Ok(w)
}

/// One row of a pyramid of unit boxes: `rows - row` boxes at height `row`.
fn add_row(w: &mut World, row: u8, rows: u8, shape: &Shape3) -> Result<(), PhysicsError> {
    let count = rows - row;
    for i in 0..count {
        let x = f32::from(i) - f32::from(count - 1) / 2.0;
        let at = Vec3::new(x, 0.5 + f32::from(row), 0.0);
        w.create_body(&BodyDesc::new(BodyKind::Dynamic, at, *shape))?;
    }
    Ok(())
}

/// A pyramid of unit boxes, `rows` high, one box deep.
fn pyramid(rows: u8) -> Result<World, PhysicsError> {
    pyramid_of(rows, 4096)
}

fn pyramid_of(rows: u8, max_manifolds: u32) -> Result<World, PhysicsError> {
    pyramid_capped(rows, max_manifolds, 4096)
}

fn pyramid_capped(rows: u8, max_manifolds: u32, max_pairs: u32) -> Result<World, PhysicsError> {
    let mut w = world_with(SleepConfig::DEFAULT, max_manifolds, max_pairs)?;
    static_box(
        &mut w,
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(20.0, 1.0, 20.0),
    )?;
    let shape = Shape3::Box {
        half_extents: Vec3::new(0.5, 0.5, 0.5),
    };
    for row in 0..rows {
        add_row(&mut w, row, rows, &shape)?;
    }
    Ok(w)
}

/// The bodies of constraint `index` that move, as solver slots.
fn moving_ends(solver: &Solver3, index: u32) -> Vec<usize> {
    let Some(constraint) = solver.constraints.get(index as usize) else {
        return Vec::new();
    };
    [constraint.shape.a, constraint.shape.b]
        .into_iter()
        .filter(|&slot| solver.bodies.get(slot).is_some_and(|b| !b.is_static()))
        .collect()
}

/// Counts constraint `index` once more in `listed`.
fn tally(listed: &mut [u32], index: u32) {
    if let Some(count) = listed.get_mut(index as usize) {
        *count += 1;
    }
}

/// Checks colour `colour` of the last solve: the colour of each member, and no shared body.
/// Counts each member in `listed`.
fn check_colour(solver: &Solver3, colour: usize, listed: &mut [u32]) {
    let colouring = &solver.colouring;
    let expected = Colour::Index(u8::try_from(colour).unwrap_or(u8::MAX));
    let mut slots = Vec::new();
    for &index in colouring.members(colour) {
        tally(listed, index);
        assert_eq!(colouring.colour_of(index as usize), expected);
        slots.extend(moving_ends(solver, index));
    }
    let repeated = (0..slots.len()).any(|i| slots[..i].contains(&slots[i]));
    assert!(!repeated, "colour {colour} shares a body");
}

/// Checks the colouring the solver used in its last solve against the constraints it solved.
fn check_colouring(solver: &Solver3) {
    let colouring = &solver.colouring;
    let mut listed = vec![0_u32; solver.constraints.len()];
    for colour in 0..colouring.colour_count() {
        check_colour(solver, colour, &mut listed);
    }
    for &index in colouring.overflow() {
        if let Some(count) = listed.get_mut(index as usize) {
            *count += 1;
        }
    }
    for (index, constraint) in solver.constraints.iter().enumerate() {
        let moves = [constraint.shape.a, constraint.shape.b]
            .into_iter()
            .any(|slot| solver.active.contains(&slot));
        assert_eq!(
            listed.get(index).copied(),
            Some(u32::from(moves)),
            "constraint {index}"
        );
    }
}

/// Steps a random scene and checks its colouring after every step.
fn check_random_scene(seed: u64) -> Result<(), PhysicsError> {
    let mut w = random_scene(seed, 24, SleepConfig::DEFAULT)?;
    let mut busiest = 0;
    for _ in 0..120 {
        w.step(1.0 / 60.0)?;
        let stats = w.solver_stats();
        busiest = busiest.max(stats.solved);
        assert!(stats.colours <= MAX_COLOURS);
        check_colouring(w.solver_mut());
    }
    assert!(busiest > 5, "seed {seed}: the scene has too few contacts");
    Ok(())
}

#[test]
fn the_colouring_of_random_scenes_is_valid_and_complete_at_every_step() -> Result<(), PhysicsError>
{
    for seed in 0..4 {
        check_random_scene(seed)?;
    }
    Ok(())
}

#[test]
fn the_colouring_of_a_pyramid_is_valid_and_uses_few_colours() -> Result<(), PhysicsError> {
    let mut w = pyramid(6)?;
    for _ in 0..20 {
        w.step(1.0 / 60.0)?;
        check_colouring(w.solver_mut());
    }
    let stats = w.solver_stats();
    assert_eq!(stats.overflow, 0);
    assert!(stats.colours >= 2 && stats.colours <= 12, "{stats:?}");
    Ok(())
}

/// The state hash after each of `steps` steps with the constraints reordered inside the
/// colours, and the solve order of each step.
fn run_reordered(
    mut w: World,
    reorder: Option<Reorder>,
    steps: u32,
) -> Result<(Vec<u64>, Vec<Vec<u32>>), PhysicsError> {
    w.solver_mut().set_reorder(reorder);
    let (mut hashes, mut orders) = (Vec::new(), Vec::new());
    for _ in 0..steps {
        w.step(1.0 / 60.0)?;
        hashes.push(w.state_hash());
        orders.push(w.solver_mut().colouring.order().to_vec());
    }
    Ok((hashes, orders))
}

/// Runs `scene` in the canonical order and reordered three ways; the hashes must be equal.
fn check_reordering(
    n: usize,
    scene: fn() -> Result<World, PhysicsError>,
) -> Result<(), PhysicsError> {
    let (reference, plain) = run_reordered(scene()?, None, 80)?;
    for reorder in [Reorder::Reverse, Reorder::Shuffle(1), Reorder::Shuffle(99)] {
        let (other, orders) = run_reordered(scene()?, Some(reorder), 80)?;
        assert_eq!(reference, other, "scene {n}, {reorder:?}");
        let differs = plain.iter().zip(&orders).any(|(a, b)| a != b);
        assert!(differs, "scene {n}, {reorder:?}: nothing was reordered");
    }
    Ok(())
}

#[test]
fn the_order_inside_a_colour_does_not_change_a_single_bit() -> Result<(), PhysicsError> {
    let scenes: [fn() -> Result<World, PhysicsError>; 2] =
        [|| pyramid(5), || random_scene(1, 24, SleepConfig::DEFAULT)];
    for (n, scene) in scenes.iter().enumerate() {
        check_reordering(n, *scene)?;
    }
    Ok(())
}

// ---- the staged path (card 8-19) ---------------------------------------------------------------

/// The state hash after each of `steps` steps of `scene` with `execution`.
fn staged_run(
    scene: fn() -> Result<World, PhysicsError>,
    execution: SolverExecution,
    steps: u32,
) -> Result<Vec<u64>, PhysicsError> {
    let mut w = scene()?;
    w.set_execution(execution);
    (0..steps)
        .map(|_| w.step(1.0 / 60.0).map(|()| w.state_hash()))
        .collect()
}

fn scheduled(scheduler: Arc<dyn IScheduler>, chunk: u32) -> SolverExecution {
    SolverExecution {
        scheduler: Some(scheduler),
        simd: false,
        chunk,
    }
}

/// Counts the calls to `parallel_for` and runs the loops on the calling thread.
#[derive(Default)]
struct Counting {
    calls: AtomicUsize,
}

impl IScheduler for Counting {
    fn worker_count(&self) -> u32 {
        1
    }

    fn parallel_for(&self, count: u32, grain: u32, body: &RangeBody<'_>) -> Result<(), JobError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        InlineScheduler.parallel_for(count, grain, body)
    }

    fn run_scope(&self, tasks: &[&Task<'_>]) -> Result<(), JobError> {
        InlineScheduler.run_scope(tasks)
    }
}

/// Refuses every call, as a busy scheduler does.
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
fn the_staged_path_gives_the_bits_of_the_serial_reference() -> Result<(), PhysicsError> {
    let scenes: [fn() -> Result<World, PhysicsError>; 2] =
        [|| pyramid(4), || random_scene(3, 20, SleepConfig::DEFAULT)];
    for scene in scenes {
        let expected = staged_run(scene, SolverExecution::SERIAL, 40)?;
        for chunk in [1, 3, 1000] {
            let inline = scheduled(Arc::new(InlineScheduler), chunk);
            assert_eq!(staged_run(scene, inline, 40)?, expected, "inline {chunk}");
            let reversed = scheduled(Arc::new(ReversedScheduler), chunk);
            assert_eq!(
                staged_run(scene, reversed, 40)?,
                expected,
                "reversed {chunk}"
            );
            let refusing = scheduled(Arc::new(Refusing), chunk);
            assert_eq!(
                staged_run(scene, refusing, 40)?,
                expected,
                "refusing {chunk}"
            );
        }
    }
    Ok(())
}

#[test]
fn colours_with_several_chunks_reach_the_scheduler_and_a_single_chunk_does_not()
-> Result<(), PhysicsError> {
    let counting = Arc::new(Counting::default());
    staged_run(
        || pyramid(4),
        scheduled(Arc::clone(&counting) as Arc<dyn IScheduler>, 2),
        5,
    )?;
    assert!(counting.calls.load(Ordering::Relaxed) > 0, "chunks of two");
    let single = Arc::new(Counting::default());
    staged_run(
        || pyramid(4),
        scheduled(Arc::clone(&single) as Arc<dyn IScheduler>, 1000),
        5,
    )?;
    assert_eq!(
        single.calls.load(Ordering::Relaxed),
        0,
        "one chunk per colour"
    );
    Ok(())
}

#[test]
fn the_buffers_of_the_staged_path_follow_the_execution() -> Result<(), PhysicsError> {
    let mut w = pyramid(3)?;
    assert!(
        w.solver_mut().outputs.is_empty(),
        "nothing staged by default"
    );
    w.set_execution(scheduled(Arc::new(InlineScheduler), 4));
    let sized = w.solver_mut().outputs.len();
    assert!(sized > 0 && w.solver_mut().outputs_chunk == 4);
    w.set_execution(scheduled(Arc::new(InlineScheduler), 4));
    assert_eq!(
        w.solver_mut().outputs.len(),
        sized,
        "same chunk, same buffers"
    );
    w.set_execution(scheduled(Arc::new(InlineScheduler), 8));
    assert!(
        w.solver_mut().outputs.len() < sized,
        "bigger chunks, fewer buffers"
    );
    w.set_execution(SolverExecution::SERIAL);
    assert!(w.solver_mut().outputs.is_empty());
    assert_eq!(w.solver_mut().outputs_chunk, 0);
    Ok(())
}

// ---- the contact tests through a scheduler (card 8-104) -----------------------------------------

/// The manifolds the contact tests make for `scene` after `steps` steps, as a hash and a count.
fn contact_run(
    scene: fn() -> Result<World, PhysicsError>,
    execution: SolverExecution,
    steps: u32,
) -> Result<(u64, usize, usize), PhysicsError> {
    let mut w = scene()?;
    w.set_execution(execution);
    for _ in 0..steps {
        w.step(1.0 / 60.0)?;
    }
    let list = w.manifolds();
    Ok((list.hash(), list.len(), list.missing()))
}

/// The three schedulers of the staged tests: in order, reversed, and one that refuses every call.
fn schedulers() -> [Arc<dyn IScheduler>; 3] {
    [
        Arc::new(InlineScheduler),
        Arc::new(ReversedScheduler),
        Arc::new(Refusing),
    ]
}

#[test]
fn the_contact_tests_give_the_serial_manifolds_through_any_scheduler() -> Result<(), PhysicsError> {
    let scenes: [fn() -> Result<World, PhysicsError>; 3] = [
        || pyramid(5),
        || random_scene(5, 40, SleepConfig::DEFAULT),
        // A list too small for the manifolds: the stored ones are a prefix and the rest is counted.
        || pyramid_of(5, 10),
    ];
    let cases = scenes
        .into_iter()
        .flat_map(|scene| [1, 3, 1000].map(|chunk| (scene, chunk)));
    for (scene, chunk) in cases {
        let expected = contact_run(scene, SolverExecution::SERIAL, 30)?;
        assert!(expected.1 > 0, "the scene makes contacts");
        for scheduler in schedulers() {
            let got = contact_run(scene, scheduled(scheduler, chunk), 30)?;
            assert_eq!(got, expected, "chunk {chunk}");
        }
    }
    Ok(())
}

#[test]
fn the_contact_tests_reach_the_scheduler_when_there_are_several_chunks_of_pairs()
-> Result<(), PhysicsError> {
    let counting = Arc::new(Counting::default());
    let mut w = pyramid(6)?;
    w.set_execution(scheduled(Arc::clone(&counting) as Arc<dyn IScheduler>, 1));
    w.find_pairs();
    let before = counting.calls.load(Ordering::Relaxed);
    w.collide();
    assert!(
        counting.calls.load(Ordering::Relaxed) > before,
        "collide did not use the scheduler"
    );
    // One chunk of pairs is not worth a hand-over: the scheduler is not called.
    let single = Arc::new(Counting::default());
    let mut one = pyramid(6)?;
    one.set_execution(scheduled(
        Arc::clone(&single) as Arc<dyn IScheduler>,
        100_000,
    ));
    one.find_pairs();
    one.collide();
    assert_eq!(
        single.calls.load(Ordering::Relaxed),
        0,
        "one chunk of pairs"
    );
    // Without a scheduler the contact tests stay on the calling thread.
    let mut serial = pyramid(6)?;
    serial.find_pairs();
    serial.collide();
    assert_eq!(serial.manifolds().hash(), w.manifolds().hash());
    Ok(())
}

// ---- the pair search through a scheduler (card 8-111) -------------------------------------------

/// The pair list of `scene` after `steps` steps: the pairs in order and the count that did not fit.
fn pairs_run(
    scene: fn() -> Result<World, PhysicsError>,
    execution: SolverExecution,
    steps: u32,
) -> Result<(Vec<Pair<D3>>, usize), PhysicsError> {
    let mut w = scene()?;
    w.set_execution(execution);
    for _ in 0..steps {
        w.step(1.0 / 60.0)?;
    }
    let list = w.pairs();
    Ok((list.as_slice().to_vec(), list.missing()))
}

#[test]
fn the_pair_search_gives_the_serial_list_through_any_scheduler() -> Result<(), PhysicsError> {
    let scenes: [fn() -> Result<World, PhysicsError>; 3] = [
        || pyramid(5),
        || random_scene(7, 40, SleepConfig::DEFAULT),
        // A list too small for the pairs: the stored ones are the first found, the rest is counted.
        || pyramid_capped(5, 4096, 20),
    ];
    let cases = scenes
        .into_iter()
        .flat_map(|scene| [1, 3, 1000].map(|chunk| (scene, chunk)));
    for (scene, chunk) in cases {
        let expected = pairs_run(scene, SolverExecution::SERIAL, 30)?;
        assert!(!expected.0.is_empty(), "the scene has pairs");
        for scheduler in schedulers() {
            let got = pairs_run(scene, scheduled(scheduler, chunk), 30)?;
            assert_eq!(got, expected, "chunk {chunk}");
        }
    }
    Ok(())
}

#[test]
fn the_pair_search_reaches_the_scheduler_when_there_are_several_chunks_of_bodies()
-> Result<(), PhysicsError> {
    let counting = Arc::new(Counting::default());
    let mut w = pyramid(6)?;
    w.set_execution(scheduled(Arc::clone(&counting) as Arc<dyn IScheduler>, 1));
    let before = counting.calls.load(Ordering::Relaxed);
    w.find_pairs();
    assert!(
        counting.calls.load(Ordering::Relaxed) > before,
        "find_pairs did not use the scheduler"
    );
    // One chunk of bodies is not worth a hand-over.
    let single = Arc::new(Counting::default());
    let mut one = pyramid(6)?;
    one.set_execution(scheduled(
        Arc::clone(&single) as Arc<dyn IScheduler>,
        100_000,
    ));
    one.find_pairs();
    assert_eq!(single.calls.load(Ordering::Relaxed), 0, "one chunk");
    let mut serial = pyramid(6)?;
    serial.find_pairs();
    assert_eq!(serial.pairs().as_slice(), w.pairs().as_slice());
    Ok(())
}

/// Thirty boxes on top of one another: every body overlaps all the others, so a chunk finds more
/// pairs than its buffer holds and the search must fall back to the serial one.
fn dense_pairs(execution: SolverExecution) -> Result<(Vec<Pair<D3>>, usize), PhysicsError> {
    let mut w = big_world(SleepConfig::DEFAULT)?;
    let shape = Shape3::Box {
        half_extents: Vec3::new(0.5, 0.5, 0.5),
    };
    for index in 0..30_u8 {
        let at = Vec3::new(f32::from(index) * 0.01, 1.0, 0.0);
        w.create_body(&BodyDesc::new(BodyKind::Dynamic, at, shape))?;
    }
    w.set_execution(execution);
    w.find_pairs();
    Ok((w.pairs().as_slice().to_vec(), w.pairs().missing()))
}

#[test]
fn a_pile_that_overflows_the_chunk_buffers_gives_the_serial_list() -> Result<(), PhysicsError> {
    let expected = dense_pairs(SolverExecution::SERIAL)?;
    assert_eq!(
        expected.0.len(),
        30 * 29 / 2,
        "every body overlaps every other"
    );
    for scheduler in schedulers() {
        // One body a chunk: 29 pairs a body is more than a buffer of 16 holds.
        assert_eq!(dense_pairs(scheduled(scheduler, 1))?, expected);
    }
    Ok(())
}

#[test]
fn the_pair_search_follows_the_execution_and_the_bodies_that_go_away() -> Result<(), PhysicsError> {
    let mut serial = random_scene(9, 30, SleepConfig::DEFAULT)?;
    let mut w = random_scene(9, 30, SleepConfig::DEFAULT)?;
    // A scheduler, another chunk, a body destroyed (an empty slot), then back to serial: the list
    // is the serial one after each change.
    serial.find_pairs();
    let doomed = serial
        .pairs()
        .as_slice()
        .first()
        .map(|pair| pair.second)
        .ok_or(PhysicsError::StaleHandle)?;
    for (execution, destroy) in [
        (scheduled(Arc::new(InlineScheduler), 3), false),
        (scheduled(Arc::new(ReversedScheduler), 5), true),
        (SolverExecution::SERIAL, false),
        (scheduled(Arc::new(InlineScheduler), 1), false),
    ] {
        if destroy {
            serial.destroy_body(doomed)?;
            w.destroy_body(doomed)?;
        }
        w.set_execution(execution);
        serial.find_pairs();
        w.find_pairs();
        assert_eq!(w.pairs().as_slice(), serial.pairs().as_slice());
        assert_eq!(w.pairs().missing(), serial.pairs().missing());
    }
    Ok(())
}

// ---- the making of the constraints through a scheduler (card 8-112) -----------------------------

/// The most constraints one colour of the last solve holds.
fn widest_colour(w: &mut World) -> usize {
    let colouring = &w.solver_mut().colouring;
    (0..colouring.colour_count())
        .map(|colour| colouring.members(colour).len())
        .max()
        .unwrap_or(0)
}

#[test]
fn the_constraints_are_made_through_the_scheduler_when_there_are_several_chunks_of_manifolds()
-> Result<(), PhysicsError> {
    let mut w = pyramid(6)?;
    for _ in 0..5 {
        w.step(1.0 / 60.0)?;
    }
    // A chunk as wide as the widest colour: no colour is cut in two, so the sweeps never call the
    // scheduler and every call is the making of the constraints. Only the solver gets the
    // scheduler (not the pair search or the contact tests).
    let chunk = u32::try_from(widest_colour(&mut w)).unwrap_or(u32::MAX);
    let manifolds = w.manifolds().len();
    assert!(
        manifolds > chunk as usize && chunk > 0,
        "{manifolds} manifolds, chunk {chunk}"
    );
    let counting = Arc::new(Counting::default());
    w.solver_mut().set_execution(scheduled(
        Arc::clone(&counting) as Arc<dyn IScheduler>,
        chunk,
    ));
    w.step(1.0 / 60.0)?;
    assert!(
        counting.calls.load(Ordering::Relaxed) > 0,
        "the constraints were not made through the scheduler"
    );
    // One chunk of manifolds is not worth a hand-over.
    let single = Arc::new(Counting::default());
    let mut one = pyramid(6)?;
    one.solver_mut().set_execution(scheduled(
        Arc::clone(&single) as Arc<dyn IScheduler>,
        100_000,
    ));
    one.step(1.0 / 60.0)?;
    assert_eq!(single.calls.load(Ordering::Relaxed), 0, "one chunk");
    Ok(())
}
