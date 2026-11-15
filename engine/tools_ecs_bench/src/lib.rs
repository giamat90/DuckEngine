//! Dev-time benchmark of the `ecs` crate (card 7-5): query iteration over one to three
//! components, serial and on the work-stealing pool, and the deterministic workload that the
//! instruction-count tool runs. Report only: no threshold is applied here (registering a gate is
//! card 7-6). Not part of the engine.

pub mod cli;
pub mod report;
pub mod workload;

pub use cli::{Command, Options, USAGE, parse};
pub use workload::{Kind, build, frames_parallel, frames_serial, run_parallel, run_serial};
