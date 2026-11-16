//! The replay hash: the same scripted session gives the same state, on one thread and on several.

use crate::common::{Failure, Recorder, run, sandbox, step};
use app_sandbox::{Requests, Sandbox};
use base::math::Vec2;
use jobs::pool::WorkStealingPool;
use jobs::scheduler::IScheduler;
use physics::d2::SolverExecution;
use platform::threads::StdThreads;
use std::sync::Arc;

/// A session with clicks and one reset, `frames` frames long.
fn session(sandbox: &mut Sandbox, frames: u32) -> Result<(), Failure> {
    let mut draw = Recorder::default();
    for frame in 0..frames {
        let requests = Requests {
            reset: frame == 150,
            spawn: (frame % 20 == 5).then(|| {
                Vec2::new(
                    f32::from(u16::try_from(frame).unwrap_or(0)) / 10.0 - 8.0,
                    8.0,
                )
            }),
        };
        step(sandbox, requests, &mut draw)?;
    }
    Ok(())
}

fn hash_of(seed: u64, execution: Option<SolverExecution>) -> Result<u64, Failure> {
    let mut sandbox = sandbox(seed)?;
    if let Some(execution) = execution {
        sandbox.set_execution(execution);
    }
    session(&mut sandbox, 300)?;
    Ok(sandbox.state_hash())
}

fn pool(workers: u32) -> Result<SolverExecution, Failure> {
    let pool = WorkStealingPool::new(workers, &StdThreads::new())?;
    Ok(SolverExecution {
        scheduler: Some(Arc::new(pool) as Arc<dyn IScheduler>),
        ..SolverExecution::SERIAL
    })
}

#[test]
fn the_same_session_gives_the_same_hash() -> Result<(), Failure> {
    assert_eq!(hash_of(24, None)?, hash_of(24, None)?);
    Ok(())
}

#[test]
fn another_seed_gives_another_hash() -> Result<(), Failure> {
    assert_ne!(hash_of(24, None)?, hash_of(25, None)?);
    Ok(())
}

#[test]
fn the_hash_is_the_same_at_one_and_at_several_threads() -> Result<(), Failure> {
    let serial = hash_of(24, None)?;
    assert_eq!(hash_of(24, Some(pool(1)?))?, serial);
    assert_eq!(hash_of(24, Some(pool(4)?))?, serial);
    Ok(())
}

#[test]
fn the_hash_follows_the_state() -> Result<(), Failure> {
    let mut sandbox = sandbox(24)?;
    let mut draw = Recorder::default();
    let before = sandbox.state_hash();
    run(&mut sandbox, 10, &mut draw)?;
    assert_ne!(sandbox.state_hash(), before);
    Ok(())
}
