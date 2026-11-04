//! The process port, reduced to what the engine needs from it so far: ending the process at once.

/// Ends the process abnormally without unwinding and without running destructors.
///
/// For the places that must not return into code that would be unsound after a failure (the job
/// pool uses it when a job panics under a build that unwinds). Exercised by the abort tests of
/// the job pool, which run a child process.
pub fn abort() -> ! {
    std::process::abort()
}
