//! Throw-away harness of the ADR-0004 spike (card 4-5): four work distribution candidates built
//! from `std` only, each an [`IScheduler`] of the `jobs` crate, plus synthetic workloads.
//! Report only; not part of the engine (design: `wiki/architecture/physics-and-jobs-v0.md`
//! section 4, evidence: `wiki/spikes/scheduler-spike.md`).
//!
//! The one `unsafe` of the crate is the lifetime erasure in [`erased`], the same code any pool
//! needs to call a borrowed body from its threads; all four candidates share it.
#![expect(
    unsafe_code,
    reason = "lifetime erasure of the borrowed job, one module, protocol in erased.rs"
)]

pub mod deques;
pub mod erased;
pub mod job;
pub mod shared_queue;
pub mod static_partition;
pub mod stealing;
pub mod sync;
pub mod workers;
pub mod workloads;

use deques::{ChaseLevDeque, MutexDeque};
use jobs::inline::InlineScheduler;
use jobs::scheduler::IScheduler;
use platform::error::PlatformError;
use platform::threads::IThreads;
use shared_queue::SharedQueueScheduler;
use static_partition::StaticPartitionScheduler;
use stealing::StealingScheduler;

/// The candidates, plus the single-thread reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The `jobs` inline scheduler: the serial reference.
    Inline,
    /// (a) shared queue, `Mutex` and `Condvar`.
    SharedQueue,
    /// (b) per-worker mutex-protected deques with stealing.
    MutexDeques,
    /// (c) per-worker Chase-Lev deques with stealing.
    LockFree,
    /// (d) static partition.
    StaticPartition,
}

impl Kind {
    /// Every kind, the reference first.
    pub const ALL: [Self; 5] = [
        Self::Inline,
        Self::SharedQueue,
        Self::MutexDeques,
        Self::LockFree,
        Self::StaticPartition,
    ];

    /// Short name used in the report and on the command line.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::SharedQueue => "a-shared-queue",
            Self::MutexDeques => "b-mutex-deques",
            Self::LockFree => "c-lock-free",
            Self::StaticPartition => "d-static",
        }
    }

    /// The kind called `name`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
}

/// Builds a scheduler of `kind` with `workers` threads at the same time (the caller included).
/// `idle_spin` is how many iterations an idle worker spins before it parks (candidates (b) to
/// (d); (a) always parks).
///
/// # Errors
/// The error of the thread port when the threads cannot be created.
pub fn build(
    kind: Kind,
    workers: u32,
    idle_spin: u32,
    threads: &dyn IThreads,
) -> Result<Box<dyn IScheduler>, PlatformError> {
    Ok(match kind {
        Kind::Inline => Box::new(InlineScheduler),
        Kind::SharedQueue => Box::new(SharedQueueScheduler::new(workers, threads)?),
        Kind::MutexDeques => Box::new(StealingScheduler::<MutexDeque>::new(
            workers, idle_spin, threads,
        )?),
        Kind::LockFree => Box::new(StealingScheduler::<ChaseLevDeque>::new(
            workers, idle_spin, threads,
        )?),
        Kind::StaticPartition => {
            Box::new(StaticPartitionScheduler::new(workers, idle_spin, threads)?)
        }
    })
}
