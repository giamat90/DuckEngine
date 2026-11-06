//! The scheduler contract (`jobs::contract`) run on every scheduler the matrix uses, and shown to
//! fail on every deliberately faulty one. The matrix compares physics results; this test says the
//! schedulers keep their promises (each index exactly once, the join before the return, the
//! writes of the jobs visible afterwards) so a green matrix cannot come from a broken scheduler
//! that happens to agree with itself. That a call allocates nothing after warm-up is checked by
//! `jobs/tests/no_allocation.rs`, `jobs/tests/pool_no_allocation.rs` and
//! `tests/parallel_allocation.rs` for the schedulers, not repeated here.

use super::configs::{OneRange, Refusing, schedulers};
use super::faulty::{ALL, Faulty};
use jobs::contract::{order_independence_contract, refusal_contract, scheduler_contract};
use jobs::error::JobError;
use jobs::inline::InlineScheduler;
use jobs::scheduler::IScheduler;
use physics::d2::PhysicsError;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn every_scheduler_of_the_matrix_passes_the_contract() -> Result<(), PhysicsError> {
    for (name, scheduler) in schedulers()? {
        let outcome = catch_unwind(AssertUnwindSafe(|| scheduler_contract(&*scheduler)));
        assert!(outcome.is_ok(), "{name} breaks the contract");
    }
    Ok(())
}

#[test]
fn every_scheduler_of_the_matrix_gives_the_same_result_for_a_pure_loop() -> Result<(), PhysicsError>
{
    let all = schedulers()?;
    let mut refs: Vec<&dyn IScheduler> = vec![&InlineScheduler];
    refs.extend(all.iter().map(|(_, scheduler)| &**scheduler));
    order_independence_contract(&refs);
    Ok(())
}

#[test]
fn the_refusing_scheduler_refuses_and_runs_nothing() {
    let ran = AtomicUsize::new(0);
    let result = Refusing.parallel_for(10, 1, &|_| {
        ran.fetch_add(1, Ordering::Relaxed);
    });
    assert_eq!(result, Err(JobError::Busy));
    assert_eq!(ran.load(Ordering::Relaxed), 0);
    // A scheduler that hands the whole loop over in one piece still refuses a zero grain.
    refusal_contract(&OneRange);
}

#[test]
fn every_deliberately_faulty_scheduler_fails_the_contract() {
    for fault in ALL {
        let scheduler = Faulty(fault);
        let outcome = catch_unwind(AssertUnwindSafe(|| scheduler_contract(&scheduler)));
        assert!(outcome.is_err(), "{fault:?} passed the contract");
    }
}
