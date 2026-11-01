//! The platform bundle: the ports the composition root hands down, and what they can do.
//!
//! Built once by the composition root from the chosen implementations and passed explicitly; there
//! is no global platform. Services take the port they need, not the whole bundle. The file system
//! and window ports join the bundle in later cards.

use crate::clock::{IClock, ManualClock, StdClock};
use crate::threads::{IThreads, NullThreads, StdThreads};
use std::sync::Arc;

/// What the platform can do, so engine code asks "can I?" instead of testing the OS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    /// Threads can be created.
    pub has_threads: bool,
}

/// The bundle of ports.
pub struct Platform {
    /// The monotonic clock.
    pub clock: Box<dyn IClock>,
    /// Thread creation.
    pub threads: Box<dyn IThreads>,
}

impl Platform {
    /// A bundle from chosen implementations.
    #[must_use]
    pub fn new(clock: Box<dyn IClock>, threads: Box<dyn IThreads>) -> Self {
        Self { clock, threads }
    }

    /// The operating-system implementations that need only `std`.
    #[must_use]
    pub fn std() -> Self {
        Self::new(Box::new(StdClock::new()), Box::new(StdThreads::new()))
    }

    /// The Headless platform: a manual clock (the returned handle moves it) and no threads.
    #[must_use]
    pub fn headless() -> (Self, Arc<ManualClock>) {
        let clock = Arc::new(ManualClock::new());
        let platform = Self::new(Box::new(Arc::clone(&clock)), Box::new(NullThreads));
        (platform, clock)
    }

    /// What this platform can do, read from the ports so it cannot disagree with them.
    #[must_use]
    pub fn caps(&self) -> Caps {
        Caps {
            has_threads: self.threads.has_threads(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Ticks;
    use crate::error::PlatformError;

    #[test]
    fn the_std_platform_has_threads_and_a_running_clock() {
        let platform = Platform::std();
        assert!(platform.caps().has_threads);
        let spawned = platform.threads.spawn("t", Box::new(|| {}));
        assert_eq!(spawned.map(crate::threads::ThreadHandle::join), Ok(Ok(())));
    }

    #[test]
    fn the_headless_platform_has_no_threads_and_a_manual_clock() {
        let (platform, clock) = Platform::headless();
        assert!(!platform.caps().has_threads);
        assert_eq!(
            platform.threads.spawn("t", Box::new(|| {})).err(),
            Some(PlatformError::Unsupported)
        );
        assert_eq!(platform.clock.now(), Ticks::ZERO);
        clock.advance(Ticks::from_millis(16));
        assert_eq!(platform.clock.now(), Ticks::from_millis(16));
    }
}
