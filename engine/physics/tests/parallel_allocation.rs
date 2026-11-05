//! A parallel step allocates nothing (card 4-15): not on the calling thread (counted over the steady
//! state after a warm-up) and not on any worker thread of the pool (counted over the whole life of
//! the thread). Counting allocator with per-thread counters, as the other allocation tests do.

use base::alloc::{AllocCounts, CountingAllocator, count_allocations, current_counts};
use base::math::Vec2;
use jobs::pool::WorkStealingPool;
use jobs::scheduler::IScheduler;
use physics::d2::{
    BodyDesc, BodyKind, DEFAULT_BROADPHASE_MARGIN, DEFAULT_SPECULATIVE_MARGIN, PhysicsError,
    STANDARD_GRAVITY, Shape2, SleepConfig, SolverConfig, SolverExecution, World, WorldConfig,
};
use platform::error::PlatformError;
use platform::threads::{IThreads, Job, StdThreads, ThreadHandle};
use std::alloc::System;
use std::sync::{Arc, Mutex, PoisonError};

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const DT: f32 = 1.0 / 60.0;
const WARM_UP_STEPS: u32 = 30;
const MEASURED_STEPS: u32 = 120;
const WORKERS: u32 = 4;
const SMALL_CHUNK: u32 = 6;

/// Threads of the operating system whose jobs report the allocator calls made while they ran.
struct MeasuredThreads {
    inner: StdThreads,
    counts: Arc<Mutex<Vec<AllocCounts>>>,
}

impl IThreads for MeasuredThreads {
    fn spawn(&self, name: &str, job: Job) -> Result<ThreadHandle, PlatformError> {
        let counts = Arc::clone(&self.counts);
        self.inner.spawn(
            name,
            Box::new(move || {
                let before = current_counts();
                job();
                let made = current_counts().since(&before);
                counts
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(made);
            }),
        )
    }

    fn parallelism(&self) -> u32 {
        self.inner.parallelism()
    }

    fn has_threads(&self) -> bool {
        self.inner.has_threads()
    }
}

fn world() -> Result<World, PhysicsError> {
    let mut world = World::new(WorldConfig {
        max_bodies: 128,
        gravity: STANDARD_GRAVITY,
        max_pairs: 1024,
        broadphase_margin: DEFAULT_BROADPHASE_MARGIN,
        max_manifolds: 1024,
        speculative_margin: DEFAULT_SPECULATIVE_MARGIN,
        solver: SolverConfig::DEFAULT,
        sleep: SleepConfig::DISABLED,
    })?;
    world.create_body(&BodyDesc::new(
        BodyKind::Static,
        Vec2::new(0.0, -1.0),
        Shape2::Box {
            half_extents: Vec2::new(50.0, 1.0),
        },
    ))?;
    for row in 0..8_u8 {
        for i in 0..(8 - row) {
            let x = f32::from(i) - f32::from(7 - row) / 2.0;
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

/// Calls the calling thread made while it stepped `MEASURED_STEPS` steps after a warm-up.
fn caller_calls(execution: SolverExecution) -> Result<u64, PhysicsError> {
    let mut world = world()?;
    world.set_execution(execution);
    for _ in 0..WARM_UP_STEPS {
        world.step(DT)?;
    }
    let (result, counts) = count_allocations(|| {
        for _ in 0..MEASURED_STEPS {
            world.step(DT)?;
        }
        Ok::<(), PhysicsError>(())
    });
    result?;
    assert!(
        world.solver_stats().colours >= 2,
        "the pyramid is in contact, so the solve had colours"
    );
    Ok(counts.calls())
}

#[test]
fn the_counter_sees_an_allocation() {
    let ((), counts) = count_allocations(|| drop(std::hint::black_box(vec![1_u8; 16])));
    assert!(
        counts.calls() >= 1,
        "the counting allocator is not installed"
    );
}

#[test]
fn a_step_with_lanes_and_no_scheduler_allocates_nothing() -> Result<(), PhysicsError> {
    let execution = SolverExecution {
        simd: true,
        ..SolverExecution::SERIAL
    };
    assert_eq!(caller_calls(execution)?, 0);
    Ok(())
}

#[test]
fn a_parallel_step_allocates_nothing_on_the_caller_or_on_any_worker() -> Result<(), PhysicsError> {
    let threads = MeasuredThreads {
        inner: StdThreads::new(),
        counts: Arc::new(Mutex::new(Vec::new())),
    };
    let pool = WorkStealingPool::new(WORKERS, &threads).map_err(|_| PhysicsError::NotFinite)?;
    let scheduler: Arc<dyn IScheduler> = Arc::new(pool);
    for simd in [false, true] {
        let execution = SolverExecution {
            scheduler: Some(Arc::clone(&scheduler)),
            simd,
            chunk: SMALL_CHUNK,
        };
        assert_eq!(caller_calls(execution)?, 0, "caller thread, simd {simd}");
    }
    drop(scheduler); // joins the workers, which report their counts as they end
    let reports = threads
        .counts
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(reports.len(), WORKERS as usize - 1, "one report per worker");
    for counts in reports {
        assert_eq!(
            (counts.allocations, counts.reallocations),
            (0, 0),
            "worker thread: {counts:?}"
        );
    }
    Ok(())
}
