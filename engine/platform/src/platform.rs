//! The platform bundle: the ports the composition root hands down, and what they can do.
//!
//! Built once by the composition root from the chosen implementations and passed explicitly; there
//! is no global platform. Services take the port they need, not the whole bundle. The window
//! system is used from the thread that created it, so a bundle that holds one is not `Send`. The
//! file system port joins the bundle in a later card.

use crate::clock::{IClock, ManualClock, StdClock};
use crate::files::IFileSystem;
use crate::headless::{HeadlessWindows, ScriptHandle};
use crate::threads::{IThreads, NullThreads, StdThreads};
use crate::window::IWindowSystem;
use std::sync::Arc;

/// What the platform can do, so engine code asks "can I?" instead of testing the OS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    /// Threads can be created.
    pub has_threads: bool,
    /// Windows can be created.
    pub has_window: bool,
    /// File reads run on a thread of their own.
    pub has_background_io: bool,
}

/// The bundle of ports.
pub struct Platform {
    /// The monotonic clock.
    pub clock: Box<dyn IClock>,
    /// Thread creation.
    pub threads: Box<dyn IThreads>,
    /// The window system; none on a platform without one (for example the plain `std` platform).
    pub windows: Option<Box<dyn IWindowSystem>>,
    /// The file system, when the application mounted one.
    pub files: Option<Box<dyn IFileSystem>>,
}

impl Platform {
    /// A bundle from chosen implementations.
    #[must_use]
    pub fn new(clock: Box<dyn IClock>, threads: Box<dyn IThreads>) -> Self {
        Self {
            clock,
            threads,
            windows: None,
            files: None,
        }
    }

    /// The same bundle with a window system.
    #[must_use]
    pub fn with_windows(mut self, windows: Box<dyn IWindowSystem>) -> Self {
        self.windows = Some(windows);
        self
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

    /// Like [`headless`](Self::headless), with virtual windows (at most `max_windows`); the
    /// returned script handle feeds their events.
    #[must_use]
    pub fn headless_windows(max_windows: u32) -> (Self, Arc<ManualClock>, ScriptHandle) {
        let (platform, clock) = Self::headless();
        let (windows, script) = HeadlessWindows::new(max_windows);
        (platform.with_windows(Box::new(windows)), clock, script)
    }

    /// The bundle with `files` as its file system.
    #[must_use]
    pub fn with_files(mut self, files: Box<dyn IFileSystem>) -> Self {
        self.files = Some(files);
        self
    }

    /// What this platform can do, read from the ports so it cannot disagree with them.
    #[must_use]
    pub fn caps(&self) -> Caps {
        Caps {
            has_threads: self.threads.has_threads(),
            has_window: self.windows.as_ref().is_some_and(|w| w.has_window()),
            has_background_io: self.files.as_ref().is_some_and(|f| f.has_background_io()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::Ticks;
    use crate::error::PlatformError;
    use crate::memory_files::MemoryFileSystem;
    use crate::std_files::StdFileSystem;

    #[test]
    fn the_std_platform_has_threads_and_a_running_clock() {
        let platform = Platform::std();
        assert!(platform.caps().has_threads);
        let spawned = platform.threads.spawn("t", Box::new(|| {}));
        assert_eq!(spawned.map(crate::threads::ThreadHandle::join), Ok(Ok(())));
    }

    #[test]
    fn a_platform_without_a_window_system_has_no_window() {
        assert!(!Platform::std().caps().has_window);
        assert!(Platform::std().windows.is_none());
    }

    #[test]
    fn the_headless_windows_platform_creates_windows_and_replays_a_script() {
        use crate::window::{Event, EventQueue, WindowDesc};
        let (mut platform, _clock, script) = Platform::headless_windows(2);
        assert!(platform.caps().has_window);
        let Some(windows) = platform.windows.as_mut() else {
            panic!("the headless windows platform has a window system");
        };
        let desc = WindowDesc {
            title: "t",
            width: 4,
            height: 3,
        };
        assert!(windows.create_window(&desc).is_ok());
        script.push(Event::Quit);
        let mut queue = EventQueue::with_capacity(2);
        assert_eq!(windows.pump(&mut queue).delivered, 1);
        assert_eq!(queue.pop(), Some(Event::Quit));
    }

    #[test]
    fn a_file_system_joins_the_bundle_and_shows_in_the_caps() {
        let (platform, _) = Platform::headless();
        assert!(platform.files.is_none());
        assert!(!platform.caps().has_background_io);
        let memory = platform.with_files(Box::new(MemoryFileSystem::new()));
        assert!(memory.files.is_some());
        assert!(
            !memory.caps().has_background_io,
            "memory reads run on the caller"
        );
        let std = Platform::std().with_files(Box::new(StdFileSystem::new()));
        assert!(std.caps().has_background_io);
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
