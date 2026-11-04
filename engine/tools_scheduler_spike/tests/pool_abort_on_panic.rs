//! A job that panics ends the process (ADR-0004 point 8): `cargo test` unwinds, so the pool's
//! work loops hold an abort guard. Without it the unwinding caller would free the job under the
//! workers, and an unwinding worker would leave the caller waiting on the latch forever.
//!
//! Each case re-runs this test binary as a child process (chosen by an environment variable) and
//! checks how the child ended: abnormally (abort), not with the exit code 101 of a failed test
//! (an unwind) and not with success (the panic was swallowed).

use jobs::pool::WorkStealingPool;
use jobs::scheduler::IScheduler;
use platform::threads::StdThreads;
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

const CASE_VARIABLE: &str = "DUCK_POOL_PANIC_CASE";
/// The exit code of a Rust test process whose test failed by unwinding.
const UNWIND_EXIT_CODE: i32 = 101;
/// Longest a child may run; a healthy one ends within a fraction of a second.
const CHILD_DEADLINE: Duration = Duration::from_secs(20);

fn on_a_worker_thread() -> bool {
    std::thread::current()
        .name()
        .is_some_and(|name| name.starts_with("duck-job-"))
}

/// The child: a pool whose job panics on the caller (`caller`) or on a worker (`worker`).
fn child(case: &str) {
    let pool = WorkStealingPool::new(3, &StdThreads::new())
        .unwrap_or_else(|e| panic!("a pool of 3 workers: {e}"));
    let on_worker = case == "worker";
    let _ = pool.for_each(64, 1, &|_| {
        assert!(
            on_a_worker_thread() != on_worker,
            "the job panics on purpose"
        );
        // Give the other threads the time to join in.
        std::thread::sleep(Duration::from_millis(2));
    });
    // Reached only when the panic was swallowed: end the child normally so the parent notices.
    std::process::exit(0);
}

/// Waits for the child; a child that hangs (a worker died and the caller waits for it for ever)
/// is killed and fails the test.
fn wait_with_deadline(process: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + CHILD_DEADLINE;
    loop {
        if let Ok(Some(status)) = process.try_wait() {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = process.kill();
            let _ = process.wait();
            panic!("the child hung instead of aborting");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn parent(test: &str, case: &str) {
    let exe = std::env::current_exe().unwrap_or_else(|e| panic!("the test binary: {e}"));
    let mut process = Command::new(exe)
        .args(["--exact", test, "--test-threads=1", "--nocapture"])
        .env(CASE_VARIABLE, case)
        .spawn()
        .unwrap_or_else(|e| panic!("the child process: {e}"));
    let status = wait_with_deadline(&mut process);
    assert!(!status.success(), "the panic was swallowed");
    assert_ne!(
        status.code(),
        Some(UNWIND_EXIT_CODE),
        "the panic unwound instead of aborting"
    );
}

#[test]
fn a_panic_in_the_callers_own_job_aborts_the_process() {
    match std::env::var(CASE_VARIABLE).as_deref() {
        Ok("caller") => child("caller"),
        _ => parent(
            "a_panic_in_the_callers_own_job_aborts_the_process",
            "caller",
        ),
    }
}

#[test]
fn a_panic_in_a_workers_job_aborts_the_process() {
    match std::env::var(CASE_VARIABLE).as_deref() {
        Ok("worker") => child("worker"),
        _ => parent("a_panic_in_a_workers_job_aborts_the_process", "worker"),
    }
}
