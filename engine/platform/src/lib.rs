//! Layer 1 platform crate: the ports to the operating system and their implementations that need
//! nothing but `std` (design: `wiki/architecture/platform-ports.md`).
//!
//! Clock, threads and the read-only file system so far; the window port follows.

pub mod clock;
pub mod error;
pub mod files;
pub mod headless;
pub mod memory_files;
pub mod platform;
pub mod std_files;
pub mod threads;
pub mod vpath;
pub mod window;

#[cfg(any(test, feature = "contract-tests"))]
pub mod contract;
