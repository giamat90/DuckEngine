//! Layer 1 platform crate: the ports to the operating system and their implementations that need
//! nothing but `std` (design: `wiki/architecture/platform-ports.md`).
//!
//! This card holds the clock and the thread ports; the file system and window ports follow.

pub mod clock;
pub mod error;
pub mod headless;
pub mod platform;
pub mod threads;
pub mod window;

#[cfg(any(test, feature = "contract-tests"))]
pub mod contract;
