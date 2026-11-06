//! The execution configurations of the harness. A configuration is a solver execution and, when it
//! has a scheduler, a probe that counts what the scheduler was asked to do, so a configuration that
//! quietly runs serial (a chunk size too large for the scenes, a refused call) is seen and
//! reported by name, and a comparison that never left the calling thread cannot pass for D1.

use jobs::error::JobError;
use jobs::inline::InlineScheduler;
use jobs::pool::WorkStealingPool;
use jobs::reversed::ReversedScheduler;
use jobs::scheduler::{IScheduler, RangeBody, Task};
use jobs::shuffled::ShuffledScheduler;
use physics::d2::{DEFAULT_CHUNK, PhysicsError, SolverExecution};
use platform::threads::StdThreads;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, ThreadId};

/// Worker counts of the matrix: the degenerate pool, two, an odd count, and more workers than
/// the busiest colour has chunks at the larger chunk sizes.
pub const WORKERS: [u32; 4] = [1, 2, 3, 7];

/// The seeds of the shuffled schedulers.
pub const SEEDS: [u64; 3] = [1, 99, 0x00C0_FFEE];

/// A chunk that cuts the colours of the scenes into many chunks, and one in between.
pub const TINY_CHUNK: u32 = 1;
pub const ODD_CHUNK: u32 = 7;

/// A chunk larger than any colour: no call reaches the scheduler.
pub const HUGE_CHUNK: u32 = 1_000_000;

/// Chunks per call a shuffled scheduler can order before it refuses (the scenes make more).
const REFUSING_CHUNKS: u32 = 2;

/// Runs of a configuration that repeats.
const REPEATED_RUNS: usize = 2;

/// Chunks per call the ordinary shuffled schedulers can order.
const SHUFFLE_CAPACITY: u32 = 4096;

/// What the scheduler of a configuration was asked to do.
#[derive(Default)]
pub struct Probe {
    calls: AtomicUsize,
    threads: Mutex<Vec<ThreadId>>,
}

impl Probe {
    /// Calls of `parallel_for` and `run_scope`.
    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }

    /// Threads that ran a chunk, the calling thread included.
    pub fn threads(&self) -> usize {
        self.threads.lock().map_or(0, |seen| seen.len())
    }

    fn note_thread(&self) {
        let Ok(mut seen) = self.threads.lock() else {
            return;
        };
        let me = thread::current().id();
        if !seen.contains(&me) {
            seen.push(me);
        }
    }
}

/// A scheduler that counts what goes through it and passes everything on to `inner`.
struct Observed {
    inner: Arc<dyn IScheduler>,
    probe: Arc<Probe>,
}

impl IScheduler for Observed {
    fn worker_count(&self) -> u32 {
        self.inner.worker_count()
    }

    fn parallel_for(&self, count: u32, grain: u32, body: &RangeBody<'_>) -> Result<(), JobError> {
        self.probe.calls.fetch_add(1, Ordering::Relaxed);
        self.inner.parallel_for(count, grain, &|range| {
            self.probe.note_thread();
            body(range);
        })
    }

    fn run_scope(&self, tasks: &[&Task<'_>]) -> Result<(), JobError> {
        self.probe.calls.fetch_add(1, Ordering::Relaxed);
        self.inner.run_scope(tasks)
    }
}

/// Refuses every call, as a busy scheduler does; no job runs.
pub struct Refusing;

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

/// Calls the body once with the whole range: the coarsest grouping a scheduler may make.
pub struct OneRange;

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

/// One configuration: a name, the execution, and the probe of its scheduler if it has one.
pub struct Config {
    pub name: String,
    pub execution: SolverExecution,
    pub probe: Option<Arc<Probe>>,
    /// True when the configuration is meant to hand chunks to its scheduler (not refusing, not
    /// a chunk too large to cut a colour).
    pub uses_scheduler: bool,
    /// Runs of each scene (D0: every run must equal the reference). Two for the configurations
    /// that have threads or a scheduler with state, which are the ones that could differ from run
    /// to run; one for the others, which have no source of difference between runs (the
    /// reference itself is run twice). Halves the time of the matrix in a debug build.
    pub runs: usize,
}

/// How a configuration is made: the scheduler, the lanes and the chunk size.
struct Plan {
    name: String,
    scheduler: Option<Arc<dyn IScheduler>>,
    simd: bool,
    chunk: u32,
    uses_scheduler: bool,
    repeats: bool,
}

impl Plan {
    fn into_config(self) -> Config {
        let probe = Arc::new(Probe::default());
        let has_scheduler = self.scheduler.is_some();
        let scheduler = self.scheduler.map(|inner| {
            let probe = Arc::clone(&probe);
            Arc::new(Observed { inner, probe }) as Arc<dyn IScheduler>
        });
        Config {
            name: self.name,
            execution: SolverExecution {
                scheduler,
                simd: self.simd,
                chunk: self.chunk,
            },
            probe: has_scheduler.then_some(probe),
            uses_scheduler: self.uses_scheduler,
            runs: if self.repeats { REPEATED_RUNS } else { 1 },
        }
    }
}

fn pool(workers: u32) -> Result<Arc<dyn IScheduler>, PhysicsError> {
    WorkStealingPool::new(workers, &StdThreads::new())
        .map(|pool| Arc::new(pool) as Arc<dyn IScheduler>)
        .map_err(|_| PhysicsError::NotFinite)
}

/// The scalar and the lane variant of one scheduler at one chunk size.
fn both_paths(label: &str, scheduler: &Arc<dyn IScheduler>, chunk: u32) -> Vec<Plan> {
    [false, true]
        .into_iter()
        .map(|simd| Plan {
            name: format!(
                "{label}, chunk {chunk}, {}",
                if simd { "lanes" } else { "scalar" }
            ),
            scheduler: Some(Arc::clone(scheduler)),
            simd,
            chunk,
            uses_scheduler: chunk <= ODD_CHUNK,
            repeats: label.starts_with("pool") || label.starts_with("shuffled"),
        })
        .collect()
}

fn serial_plans() -> Vec<Plan> {
    vec![Plan {
        name: "serial, lanes".to_owned(),
        scheduler: None,
        simd: true,
        chunk: DEFAULT_CHUNK,
        uses_scheduler: false,
        repeats: false,
    }]
}

/// A scheduler and its name.
pub type Named = (String, Arc<dyn IScheduler>);

/// Every scheduler the harness runs (the refusing ones apart) and a name for it. Pools are built once and shared by all
/// scenes of a test, so a pool also serves several worlds in turn.
pub fn schedulers() -> Result<Vec<Named>, PhysicsError> {
    let mut all: Vec<Named> = vec![
        ("inline".to_owned(), Arc::new(InlineScheduler)),
        ("reversed".to_owned(), Arc::new(ReversedScheduler)),
        ("one range".to_owned(), Arc::new(OneRange)),
    ];
    for seed in SEEDS {
        let shuffled = ShuffledScheduler::new(seed, SHUFFLE_CAPACITY);
        all.push((format!("shuffled {seed:#x}"), Arc::new(shuffled)));
    }
    for workers in WORKERS {
        all.push((format!("pool of {workers}"), pool(workers)?));
    }
    Ok(all)
}

/// The schedulers whose chunk size is varied, and the chunk sizes besides `ODD_CHUNK`: the ones
/// that interleave or reorder the most. (Every pair would take twice as long; the debug build is
/// slow with the lane path.)
const CHUNK_VARIED: [(&str, &[u32]); 3] = [
    ("reversed", &[TINY_CHUNK]),
    ("pool of 3", &[TINY_CHUNK, DEFAULT_CHUNK, HUGE_CHUNK]),
    ("shuffled 0x1", &[TINY_CHUNK, DEFAULT_CHUNK, HUGE_CHUNK]),
];

/// The configurations of the matrix, after the reference (the scalar serial solve).
///
/// # Errors
/// When a pool cannot start its threads.
pub fn configs() -> Result<Vec<Config>, PhysicsError> {
    let mut plans = serial_plans();
    for (label, scheduler) in schedulers()? {
        plans.extend(both_paths(&label, &scheduler, ODD_CHUNK));
        let varied = CHUNK_VARIED.iter().find(|(name, _)| *name == label);
        for &chunk in varied.map_or(&[][..], |(_, chunks)| chunks) {
            plans.extend(both_paths(&label, &scheduler, chunk));
        }
    }
    plans.extend(refusing_plans());
    Ok(plans.into_iter().map(Plan::into_config).collect())
}

fn refusing_plans() -> Vec<Plan> {
    let refusing: Arc<dyn IScheduler> = Arc::new(Refusing);
    let crowded: Arc<dyn IScheduler> = Arc::new(ShuffledScheduler::new(5, REFUSING_CHUNKS));
    let mut plans = Vec::new();
    for (label, scheduler) in [
        ("refusing", refusing),
        ("shuffled, too many chunks", crowded),
    ] {
        for mut plan in both_paths(label, &scheduler, ODD_CHUNK) {
            plan.uses_scheduler = false;
            plans.push(plan);
        }
    }
    plans
}
