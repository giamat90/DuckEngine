//! Layer 1 jobs crate: the `IScheduler` port, the inline scheduler and the test schedulers that
//! change the execution order on one thread (design: `wiki/architecture/physics-and-jobs-v0.md`
//! section 4). No threads yet: the work-stealing pool follows in card 4-7.

pub mod error;
pub mod inline;
pub mod reversed;
pub mod scheduler;
pub mod shuffled;

#[cfg(any(test, feature = "contract-tests"))]
pub mod contract;

#[cfg(test)]
mod faulty;
