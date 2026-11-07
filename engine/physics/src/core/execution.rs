//! How a contact solver executes its work: serially or through a scheduler, with the scalar
//! reference or the lane path (card 4-15). None of the choices changes a result bit: the
//! scalar serial solve is the reference, and the others are tested against it.

use jobs::scheduler::IScheduler;
use std::sync::Arc;

/// The constraints of one colour per chunk when nothing else is asked: 32. A chunk is the unit a
/// scheduler hands to a worker, so it has to be large enough to pay for the hand-over (a constraint
/// solve is some hundred floating-point operations) and small enough that a colour of a few
/// hundred constraints still makes several. The value is a starting point, not a measurement
/// (benchmarks are card 4-22).
pub const DEFAULT_CHUNK: u32 = 32;

/// Execution options of the contact solver. They change the speed, never a bit of the result.
#[derive(Clone)]
pub struct SolverExecution {
    /// The scheduler that runs the chunks of a colour; `None` runs the whole solve on the calling
    /// thread in the order of the reference. The scheduler may refuse a call (it is busy): the
    /// solver then runs that colour's chunks itself.
    pub scheduler: Option<Arc<dyn IScheduler>>,
    /// Solve four constraints of a colour at a time with `base::simd` lanes instead of one by one.
    pub simd: bool,
    /// Constraints per chunk (at least 1; 0 is read as 1). The cut of a colour into chunks depends
    /// on the colour's size and this number only, never on the number of workers, so the work
    /// each job does is the same at any thread count.
    pub chunk: u32,
}

impl SolverExecution {
    /// The scalar reference on the calling thread: what the solver does when nothing is asked.
    pub const SERIAL: Self = Self {
        scheduler: None,
        simd: false,
        chunk: DEFAULT_CHUNK,
    };

    /// True when the solver has to take the staged path (chunks, locked slots, a scatter step)
    /// instead of the plain loop of the reference.
    #[must_use]
    pub fn is_staged(&self) -> bool {
        self.scheduler.is_some() || self.simd
    }

    /// The chunk size, never zero.
    #[must_use]
    pub fn chunk_size(&self) -> usize {
        self.chunk.max(1) as usize
    }
}

impl Default for SolverExecution {
    fn default() -> Self {
        Self::SERIAL
    }
}

impl std::fmt::Debug for SolverExecution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolverExecution")
            .field(
                "workers",
                &self.scheduler.as_ref().map(|s| s.worker_count()),
            )
            .field("simd", &self.simd)
            .field("chunk", &self.chunk)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jobs::inline::InlineScheduler;

    #[test]
    fn the_default_is_the_scalar_reference_and_is_not_staged() {
        let execution = SolverExecution::default();
        assert!(execution.scheduler.is_none() && !execution.simd);
        assert_eq!(execution.chunk, DEFAULT_CHUNK);
        assert!(!execution.is_staged());
    }

    #[test]
    fn a_scheduler_or_the_lanes_make_the_solve_staged() {
        let lanes = SolverExecution {
            simd: true,
            ..SolverExecution::SERIAL
        };
        assert!(lanes.is_staged());
        let scheduled = SolverExecution {
            scheduler: Some(Arc::new(InlineScheduler)),
            ..SolverExecution::SERIAL
        };
        assert!(scheduled.is_staged());
    }

    #[test]
    fn a_zero_chunk_is_read_as_one() {
        let mut execution = SolverExecution::SERIAL;
        execution.chunk = 0;
        assert_eq!(execution.chunk_size(), 1);
        execution.chunk = 5;
        assert_eq!(execution.chunk_size(), 5);
    }

    #[test]
    fn the_debug_text_names_the_workers_without_the_scheduler() {
        let execution = SolverExecution {
            scheduler: Some(Arc::new(InlineScheduler)),
            ..SolverExecution::SERIAL
        };
        let text = format!("{execution:?}");
        assert!(
            text.contains("workers: Some(1)") && text.contains("simd: false"),
            "{text}"
        );
    }
}
