//! Layer 2 glue crate: the pieces of the frame loop that need no clock and no window.
//!
//! The application owns the loop ([platform-ports](../../../wiki/architecture/platform-ports.md)
//! section 3); this crate supplies the fixed-step accumulator. It has no dependencies; its tests use `base`.

pub mod fixed_step;

pub use fixed_step::{FixedStep, FixedStepError, Steps};
