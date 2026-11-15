//! Parallel iteration gives the serial result on every scheduler, in any order and grouping, and
//! reports a refused call as a value.

use ecs::{EcsError, Entity, Read, World, Write};
use jobs::error::JobError;
use jobs::inline::InlineScheduler;
use jobs::pool::WorkStealingPool;
use jobs::reversed::ReversedScheduler;
use jobs::scheduler::{IScheduler, RangeBody, Task};
use jobs::shuffled::ShuffledScheduler;
use platform::threads::StdThreads;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Position(f32);
#[derive(Debug, Clone, Copy, PartialEq)]
struct Velocity(f32);
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mass(f32);
struct Tag;

fn insert<T: ecs::Component>(world: &mut World, entity: Entity, value: T) {
    world
        .insert(entity, value)
        .unwrap_or_else(|e| panic!("insert: {e}"));
}

/// Entities with Position and Velocity, every third also with Mass, every fifth with Position
/// only (a second table), so there are several tables of several sizes.
fn build(n: u32) -> World {
    let mut world = World::new();
    for i in 0..n {
        let entity = world.spawn().unwrap_or_else(|e| panic!("spawn: {e}"));
        let at = f32::from(u16::try_from(i).unwrap_or(0));
        insert(&mut world, entity, Position(at));
        if i % 5 != 0 {
            insert(&mut world, entity, Velocity(1.0 + at / 8.0));
        }
        if i % 3 == 0 {
            insert(&mut world, entity, Mass(2.0));
        }
    }
    world
}

fn positions(world: &World) -> Vec<u32> {
    let all: Vec<Entity> = world.entities().collect();
    all.iter()
        .map(|e| world.get::<Position>(*e).map_or(0, |p| p.0.to_bits()))
        .collect()
}

fn integrate(world: &mut World) {
    let done = world.for_each::<(Write<Position>, Read<Velocity>), _>(|(p, v)| p.0 += v.0);
    assert_eq!(done, Ok(()));
}

fn integrate_par(world: &mut World, scheduler: &dyn IScheduler, grain: u32) {
    let done =
        world.par_for_each::<(Write<Position>, Read<Velocity>), _>(scheduler, grain, |(p, v)| {
            p.0 += v.0;
        });
    assert_eq!(done, Ok(()));
}

const ROWS: u32 = 700;

fn serial_result() -> Vec<u32> {
    let mut world = build(ROWS);
    integrate(&mut world);
    positions(&world)
}

#[test]
fn every_test_scheduler_gives_the_serial_result() {
    let want = serial_result();
    let schedulers: [Box<dyn IScheduler>; 3] = [
        Box::new(InlineScheduler),
        Box::new(ReversedScheduler),
        Box::new(ShuffledScheduler::new(7, 64)),
    ];
    for scheduler in &schedulers {
        for grain in [1, 7, 64, 10_000] {
            let mut world = build(ROWS);
            integrate_par(&mut world, scheduler.as_ref(), grain);
            assert_eq!(positions(&world), want, "grain {grain}");
        }
    }
}

#[test]
fn a_work_stealing_pool_gives_the_serial_result_at_any_thread_count() {
    let want = serial_result();
    let threads = StdThreads::new();
    for workers in [1, 2, 4] {
        let Ok(pool) = WorkStealingPool::new(workers, &threads) else {
            continue; // no threads on this platform: the inline schedulers above cover it
        };
        for grain in [1, 16, 128] {
            let mut world = build(ROWS);
            integrate_par(&mut world, &pool, grain);
            assert_eq!(positions(&world), want, "{workers} workers, grain {grain}");
        }
    }
}

#[test]
fn a_read_only_parallel_query_sees_every_matching_entity_once() {
    let serial = {
        let mut world = build(ROWS);
        let mut n = 0_u32;
        let done = world.for_each::<(Read<Position>, Read<Mass>), _>(|_| n += 1);
        assert_eq!(done, Ok(()));
        n
    };
    let count = std::sync::atomic::AtomicU32::new(0);
    let mut world = build(ROWS);
    let done = world.par_for_each::<(Read<Position>, Read<Mass>), _>(&ReversedScheduler, 3, |_| {
        count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    assert_eq!(done, Ok(()));
    assert_eq!(count.into_inner(), serial);
    assert!(serial > 0);
}

#[test]
fn a_zero_grain_is_refused_and_nothing_runs() {
    let mut world = build(10);
    let before = positions(&world);
    let done = world.par_for_each::<Write<Position>, _>(&InlineScheduler, 0, |p| p.0 += 1.0);
    assert_eq!(done, Err(EcsError::Jobs(JobError::InvalidGrain)));
    assert_eq!(positions(&world), before);
}

#[test]
fn a_query_that_names_a_component_twice_is_refused_in_parallel_too() {
    let mut world = build(10);
    let done =
        world.par_for_each::<(Read<Position>, Write<Position>), _>(&InlineScheduler, 1, |_| {});
    assert_eq!(done, Err(EcsError::ConflictingAccess));
}

/// A scheduler that refuses every call.
struct Busy;

impl IScheduler for Busy {
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
fn a_refused_scheduler_call_comes_back_as_an_error() {
    let mut world = build(ROWS);
    let done = world.par_for_each::<Write<Position>, _>(&Busy, 1, |p| p.0 += 1.0);
    assert_eq!(done, Err(EcsError::Jobs(JobError::Busy)));
}

#[test]
fn a_single_chunk_table_runs_on_the_calling_thread_without_the_scheduler() {
    let mut world = build(10);
    let before = positions(&world);
    let caller = std::thread::current().id();
    let foreign = std::sync::atomic::AtomicU32::new(0);
    let done = world.par_for_each::<Write<Position>, _>(&Busy, 1_000, |p| {
        p.0 += 1.0;
        if std::thread::current().id() != caller {
            foreign.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    });
    assert_eq!(done, Ok(()));
    assert_eq!(foreign.into_inner(), 0);
    assert_ne!(positions(&world), before);
}

#[test]
fn a_registered_component_in_no_table_matches_nothing() {
    let mut world = build(10);
    assert!(world.register::<Tag>().is_ok());
    let mut seen = 0;
    let done = world.for_each::<(Read<Position>, Read<Tag>), _>(|_| seen += 1);
    assert_eq!(done, Ok(()));
    assert_eq!(seen, 0);
}

#[test]
fn a_table_larger_than_the_chunk_cap_is_still_fully_visited() {
    let mut world = build(5_000);
    let mut serial = build(5_000);
    integrate(&mut serial);
    integrate_par(&mut world, &ShuffledScheduler::new(3, 64), 1);
    assert_eq!(positions(&world), positions(&serial));
}

/// An inline scheduler that counts the calls it gets.
struct Counting(std::sync::atomic::AtomicU32);

impl IScheduler for Counting {
    fn worker_count(&self) -> u32 {
        1
    }
    fn parallel_for(&self, count: u32, grain: u32, body: &RangeBody<'_>) -> Result<(), JobError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        InlineScheduler.parallel_for(count, grain, body)
    }
    fn run_scope(&self, tasks: &[&Task<'_>]) -> Result<(), JobError> {
        InlineScheduler.run_scope(tasks)
    }
}

fn calls(scheduler: &Counting) -> u32 {
    scheduler.0.load(std::sync::atomic::Ordering::Relaxed)
}

#[test]
fn every_query_shape_really_goes_to_the_scheduler_on_a_large_table() {
    // 600 entities with all three components share one big table: more than one chunk at grain 8.
    let mut world = World::new();
    for i in 0..600_u16 {
        let e = world.spawn().unwrap_or_else(|e| panic!("spawn: {e}"));
        insert(&mut world, e, Position(f32::from(i)));
        insert(&mut world, e, Velocity(1.0));
        insert(&mut world, e, Mass(2.0));
    }
    let one = Counting(std::sync::atomic::AtomicU32::new(0));
    assert_eq!(
        world.par_for_each::<Read<Position>, _>(&one, 8, |_| {}),
        Ok(())
    );
    assert!(calls(&one) >= 1, "one component, read");
    let two = Counting(std::sync::atomic::AtomicU32::new(0));
    assert_eq!(
        world.par_for_each::<(Read<Position>, Read<Velocity>), _>(&two, 8, |_| {}),
        Ok(())
    );
    assert!(calls(&two) >= 1, "two components, read");
    let three = Counting(std::sync::atomic::AtomicU32::new(0));
    assert_eq!(
        world.par_for_each::<(Read<Position>, Read<Velocity>, Read<Mass>), _>(&three, 8, |_| {}),
        Ok(())
    );
    assert!(calls(&three) >= 1, "three components, read");
}
