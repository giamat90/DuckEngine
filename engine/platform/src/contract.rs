//! Generic contract suites: each function takes the implementation under test, so every adapter's
//! own tests run the same checks (design: `wiki/architecture/platform-ports.md` section 4).
//! A new implementation is accepted when its suite passes unchanged.

use crate::clock::{IClock, Ticks};
use crate::error::PlatformError;
use crate::files::{IFileSystem, ReadId, ReadState};
use crate::threads::IThreads;
use crate::vpath::VPath;
use crate::window::{Event, EventQueue, IWindowSystem, NativeToken, WindowDesc, WindowId};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Clock contract: `now()` never goes backwards, `sleep(ZERO)` is harmless, and `sleep` moves time
/// forward by at least the requested span (exactly that span when `exact`, as the manual clock).
///
/// # Panics
/// When the clock breaks the contract (this is test code).
pub fn clock_contract(clock: &dyn IClock, exact: bool) {
    let mut last = clock.now();
    for _ in 0..100 {
        let now = clock.now();
        assert!(now >= last, "the clock went backwards");
        last = now;
    }
    let start = clock.now();
    clock.sleep(Ticks::ZERO);
    assert!(clock.now() >= start, "sleep(ZERO) moved time backwards");
    let wait = Ticks::from_millis(2);
    let before = clock.now();
    clock.sleep(wait);
    let waited = clock.now().saturating_sub(before);
    assert!(
        waited >= wait,
        "sleep returned early: {waited:?} < {wait:?}"
    );
    assert!(
        !exact || waited == wait,
        "sleep must advance exactly: {waited:?} != {wait:?}"
    );
}

/// Thread contract: `parallelism()` is at least 1; with `supports_threads` a spawned job runs
/// exactly once and `join` returns after it; without, `spawn` is `Unsupported` and no job runs.
///
/// # Panics
/// When the implementation breaks the contract (this is test code).
pub fn threads_contract(threads: &dyn IThreads, supports_threads: bool) {
    assert!(threads.parallelism() >= 1, "parallelism must be at least 1");
    let runs = Arc::new(AtomicU32::new(0));
    let job_runs = Arc::clone(&runs);
    let caller = std::thread::current().id();
    let other_thread = Arc::new(AtomicU32::new(0));
    let job_flag = Arc::clone(&other_thread);
    let job = Box::new(move || {
        job_runs.fetch_add(1, Ordering::SeqCst);
        job_flag.store(
            u32::from(std::thread::current().id() != caller),
            Ordering::SeqCst,
        );
    });
    let spawned = threads.spawn("contract-job", job);
    if supports_threads {
        let joined = spawned.map(crate::threads::ThreadHandle::join);
        assert_eq!(joined, Ok(Ok(())), "spawn and join must succeed");
        assert_eq!(
            runs.load(Ordering::SeqCst),
            1,
            "the job must run exactly once"
        );
        assert_eq!(
            other_thread.load(Ordering::SeqCst),
            1,
            "the job must run on its own thread"
        );
    } else {
        assert_eq!(spawned.err(), Some(PlatformError::Unsupported));
        assert_eq!(
            runs.load(Ordering::SeqCst),
            0,
            "no job may run without threads"
        );
    }
}

const DESC: WindowDesc<'static> = WindowDesc {
    title: "contract-window",
    width: 320,
    height: 240,
};

/// Window contract, for a system that can create windows. The test window is 320 by 240, a size a
/// real desktop honours (a smaller one is raised to the title bar's minimum). Create gives a window
/// with the requested size and its own native token (never the null token); a zero width or height is
/// `InvalidPath`; destroy works once and a stale id is `NotFound` for info, token and destroy
/// (also after its slot is reused); repeated `pump` calls into a capacity-1 queue keep
/// `len == len before + delivered` and never exceed the capacity. These checks need no scripted
/// events, so they also run against a real adapter. With `feed` (a way to inject events, as the
/// Headless system has) scripted events also come out in order and an overflow is reported as
/// drops; without `feed` the order and drop-count checks are skipped, so they are weaker.
///
/// # Panics
/// When the implementation breaks the contract (this is test code).
pub fn window_contract(windows: &mut dyn IWindowSystem, feed: Option<&mut dyn FnMut(Event)>) {
    assert!(
        windows.has_window(),
        "the window contract needs a system that can create windows"
    );
    let first = window_lifecycle(windows);
    assert_eq!(
        windows.window_info(first).err(),
        Some(PlatformError::NotFound),
        "stale id accepted"
    );
    let reused = windows.create_window(&DESC).ok();
    assert!(reused.is_some(), "a freed slot must be reusable");
    assert_eq!(
        windows.native_token(first).err(),
        Some(PlatformError::NotFound),
        "stale token accepted"
    );
    assert_eq!(
        windows.destroy_window(first).err(),
        Some(PlatformError::NotFound),
        "stale destroy accepted"
    );
    pump_is_bounded(windows);
    if let (Some(feed), Some(window)) = (feed, reused) {
        scripted_events(windows, feed, window);
    }
}

fn window_lifecycle(windows: &mut dyn IWindowSystem) -> WindowId {
    let a = windows.create_window(&DESC).ok();
    let b = windows.create_window(&DESC).ok();
    let (Some(a), Some(b)) = (a, b) else {
        panic!("two windows must fit");
    };
    let info = windows.window_info(a).ok();
    assert_eq!(info.map(|i| (i.width, i.height)), Some((320, 240)));
    assert_ne!(
        windows.native_token(a),
        windows.native_token(b),
        "tokens must differ"
    );
    assert_ne!(windows.native_token(a), Ok(NativeToken::NULL), "null token");
    for (width, height) in [(0, 240), (320, 0)] {
        let empty = WindowDesc {
            width,
            height,
            ..DESC
        };
        assert_eq!(
            windows.create_window(&empty).err(),
            Some(PlatformError::InvalidPath),
            "zero size accepted"
        );
    }
    assert_eq!(windows.destroy_window(b), Ok(()));
    assert_eq!(
        windows.destroy_window(b),
        Err(PlatformError::NotFound),
        "double destroy"
    );
    assert_eq!(windows.destroy_window(a), Ok(()));
    a
}

fn pump_is_bounded(windows: &mut dyn IWindowSystem) {
    let mut queue = EventQueue::with_capacity(1);
    for round in 0..6 {
        let before = queue.len();
        let result = windows.pump(&mut queue);
        assert!(queue.len() <= queue.capacity(), "pump overfilled the queue");
        assert_eq!(
            queue.len(),
            before + result.delivered as usize,
            "delivered must match the queue"
        );
        if round % 2 == 1 {
            queue.pop();
        }
    }
}

fn scripted_events(windows: &mut dyn IWindowSystem, feed: &mut dyn FnMut(Event), window: WindowId) {
    let events: Vec<Event> = (1..=5)
        .map(|n| Event::Resized {
            window,
            width: n,
            height: n,
        })
        .collect();
    for event in &events {
        feed(*event);
    }
    let mut small = EventQueue::with_capacity(2);
    let result = windows.pump(&mut small);
    assert_eq!(
        (result.delivered, result.dropped),
        (2, 3),
        "overflow must be reported"
    );
    assert_eq!(small.pop(), Some(events[0]), "events must keep their order");
    assert_eq!(small.pop(), Some(events[1]), "events must keep their order");
    for event in &events[..3] {
        feed(*event);
    }
    let mut big = EventQueue::with_capacity(8);
    let result = windows.pump(&mut big);
    assert_eq!((result.delivered, result.dropped), (3, 0));
    let got: Vec<Event> = std::iter::from_fn(|| big.pop()).collect();
    assert_eq!(got, events[..3], "events must keep their order");
}

/// The path every file system under test must hold, with [`FIXTURE_BYTES`] as its content.
pub const FIXTURE_PATH: &str = "test/hello.txt";

/// The content of [`FIXTURE_PATH`].
pub const FIXTURE_BYTES: &[u8] = b"hello, duck";

/// A second path every file system under test must hold, with no content.
pub const EMPTY_PATH: &str = "test/empty.txt";

fn fixture() -> VPath {
    VPath::new(FIXTURE_PATH).unwrap_or_else(|_| panic!("the fixture path is valid"))
}

fn empty() -> VPath {
    VPath::new(EMPTY_PATH).unwrap_or_else(|_| panic!("the empty path is valid"))
}

fn missing() -> VPath {
    VPath::new("test/missing.txt").unwrap_or_else(|_| panic!("the missing path is valid"))
}

/// Polls `id` until it is no longer pending, calling `pump` between polls.
fn wait_for(fs: &dyn IFileSystem, id: ReadId, pump: &dyn Fn()) -> ReadState {
    for _ in 0..200_000 {
        let state = fs.read_poll(id);
        if state != ReadState::Pending {
            return state;
        }
        pump();
        std::thread::sleep(std::time::Duration::from_micros(50));
    }
    panic!("a background read never finished");
}

/// File system contract. The file system holds [`FIXTURE_PATH`] with [`FIXTURE_BYTES`] and
/// [`EMPTY_PATH`] with no bytes. `pump`
/// is called while a background read is pending (the in-memory file system needs
/// `complete_all`; the `std` one needs nothing). Proves: size and synchronous reads (exact,
/// larger and too-small buffers, missing file), background reads (same bytes as the synchronous
/// read, missing file ends `Failed(NotFound)`, too-small buffer ends `Failed(BufferTooSmall)`),
/// an empty file, two reads in flight at once, the buffer length and the bytes past the file
/// left as they were, and the end of a [`ReadId`] once its buffer is taken.
///
/// # Panics
/// When the file system breaks the contract (this is test code).
pub fn files_contract(fs: &dyn IFileSystem, pump: &dyn Fn()) {
    synchronous_reads(fs);
    background_read_matches(fs, pump);
    background_read_errors(fs, pump);
    read_ids_end_when_taken(fs, pump);
    empty_and_overlapping_reads(fs, pump);
}

fn synchronous_reads(fs: &dyn IFileSystem) {
    let len = FIXTURE_BYTES.len();
    assert_eq!(fs.size(&fixture()), Ok(len as u64));
    assert_eq!(fs.size(&missing()), Err(PlatformError::NotFound));
    let mut exact = vec![0_u8; len];
    assert_eq!(fs.read_into(&fixture(), &mut exact), Ok(len));
    assert_eq!(exact, FIXTURE_BYTES);
    let mut big = vec![0xAA_u8; len + 4];
    assert_eq!(fs.read_into(&fixture(), &mut big), Ok(len));
    assert_eq!(&big[..len], FIXTURE_BYTES);
    assert_eq!(
        &big[len..],
        &[0xAA; 4],
        "bytes past the file must stay untouched"
    );
    let mut small = vec![0_u8; len - 1];
    let needed = len as u64;
    assert_eq!(
        fs.read_into(&fixture(), &mut small),
        Err(PlatformError::BufferTooSmall { needed })
    );
    assert_eq!(
        fs.read_into(&missing(), &mut big),
        Err(PlatformError::NotFound)
    );
}

fn background_read_matches(fs: &dyn IFileSystem, pump: &dyn Fn()) {
    let len = FIXTURE_BYTES.len();
    let started = fs.read_start(fixture(), vec![0_u8; len + 3].into_boxed_slice());
    let id = started.unwrap_or_else(|e| panic!("read_start failed: {e}"));
    assert_eq!(wait_for(fs, id, pump), ReadState::Done(len));
    let buf = fs
        .read_take(id)
        .unwrap_or_else(|| panic!("finished read gave no buffer"));
    assert_eq!(buf.len(), len + 3, "the buffer comes back whole");
    assert_eq!(&buf[..len], FIXTURE_BYTES);
    assert_eq!(
        &buf[len..],
        &[0; 3],
        "bytes past the file must stay untouched"
    );
}

fn background_read_errors(fs: &dyn IFileSystem, pump: &dyn Fn()) {
    let started = fs.read_start(missing(), vec![0_u8; 8].into_boxed_slice());
    let id = started.unwrap_or_else(|e| panic!("read_start failed: {e}"));
    assert_eq!(
        wait_for(fs, id, pump),
        ReadState::Failed(PlatformError::NotFound)
    );
    assert!(
        fs.read_take(id).is_some(),
        "a failed read still returns the buffer"
    );
    let needed = FIXTURE_BYTES.len() as u64;
    let small = vec![0_u8; 2].into_boxed_slice();
    let id = fs
        .read_start(fixture(), small)
        .unwrap_or_else(|e| panic!("read_start failed: {e}"));
    let expected = ReadState::Failed(PlatformError::BufferTooSmall { needed });
    assert_eq!(wait_for(fs, id, pump), expected);
    assert!(fs.read_take(id).is_some());
}

fn read_ids_end_when_taken(fs: &dyn IFileSystem, pump: &dyn Fn()) {
    let buf = vec![0_u8; 16].into_boxed_slice();
    let id = fs
        .read_start(fixture(), buf)
        .unwrap_or_else(|e| panic!("read_start failed: {e}"));
    wait_for(fs, id, pump);
    assert!(fs.read_take(id).is_some());
    assert!(fs.read_take(id).is_none(), "a buffer can be taken once");
    assert_eq!(fs.read_poll(id), ReadState::Failed(PlatformError::NotFound));
}

fn empty_and_overlapping_reads(fs: &dyn IFileSystem, pump: &dyn Fn()) {
    assert_eq!(fs.size(&empty()), Ok(0));
    assert_eq!(fs.read_into(&empty(), &mut []), Ok(0));
    let len = FIXTURE_BYTES.len();
    let start = |path: VPath, size: usize| {
        let started = fs.read_start(path, vec![0x55_u8; size].into_boxed_slice());
        started.unwrap_or_else(|e| panic!("read_start failed: {e}"))
    };
    let (first, second) = (start(fixture(), len), start(empty(), 4));
    assert_ne!(first, second);
    assert_eq!(wait_for(fs, second, pump), ReadState::Done(0));
    assert_eq!(wait_for(fs, first, pump), ReadState::Done(len));
    let second_buf = fs.read_take(second).unwrap_or_else(|| panic!("no buffer"));
    let first_buf = fs.read_take(first).unwrap_or_else(|| panic!("no buffer"));
    assert_eq!(
        &second_buf[..],
        &[0x55; 4],
        "an empty file leaves the buffer as it was"
    );
    assert_eq!(&first_buf[..], FIXTURE_BYTES);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use crate::threads::{Job, NullThreads, StdThreads, ThreadHandle};
    use std::sync::atomic::AtomicU64;

    /// Time runs backwards: every read is earlier than the one before.
    struct BackwardsClock(AtomicU64);

    impl IClock for BackwardsClock {
        fn now(&self) -> Ticks {
            let reads = self.0.fetch_add(1, Ordering::SeqCst);
            Ticks::from_nanos(1_000_u64.saturating_sub(reads))
        }

        fn sleep(&self, _duration: Ticks) {}
    }

    /// A manual clock whose sleep ignores the request.
    struct LazySleeper(ManualClock);

    impl IClock for LazySleeper {
        fn now(&self) -> Ticks {
            self.0.now()
        }

        fn sleep(&self, _duration: Ticks) {}
    }

    /// Threads that run the job inline and report success without a thread.
    struct InlineThreads;

    impl IThreads for InlineThreads {
        fn spawn(&self, _name: &str, _job: Job) -> Result<ThreadHandle, PlatformError> {
            Err(PlatformError::Io)
        }

        fn parallelism(&self) -> u32 {
            0
        }

        fn has_threads(&self) -> bool {
            false
        }
    }

    use crate::headless::{HeadlessWindows, ScriptHandle};
    use crate::window::{NativeToken, PumpResult, WindowInfo};

    fn headless_contract(system: &mut dyn IWindowSystem, script: &ScriptHandle) {
        let mut feed = |event: Event| script.push(event);
        window_contract(system, Some(&mut feed));
    }

    #[test]
    fn the_headless_windows_pass_the_window_contract() {
        let (mut windows, script) = HeadlessWindows::new(4);
        headless_contract(&mut windows, &script);
    }

    /// Forgets that a destroyed window is gone.
    struct Lenient(HeadlessWindows);

    impl IWindowSystem for Lenient {
        fn has_window(&self) -> bool {
            true
        }

        fn create_window(&mut self, desc: &WindowDesc<'_>) -> Result<WindowId, PlatformError> {
            self.0.create_window(desc)
        }

        fn destroy_window(&mut self, id: WindowId) -> Result<(), PlatformError> {
            let _ = self.0.destroy_window(id);
            Ok(())
        }

        fn window_info(&self, id: WindowId) -> Result<WindowInfo, PlatformError> {
            self.0.window_info(id)
        }

        fn native_token(&self, id: WindowId) -> Result<NativeToken, PlatformError> {
            self.0.native_token(id)
        }

        fn pump(&mut self, queue: &mut EventQueue) -> PumpResult {
            self.0.pump(queue)
        }
    }

    /// Drops events without saying so.
    struct Silent(HeadlessWindows);

    impl IWindowSystem for Silent {
        fn has_window(&self) -> bool {
            true
        }

        fn create_window(&mut self, desc: &WindowDesc<'_>) -> Result<WindowId, PlatformError> {
            self.0.create_window(desc)
        }

        fn destroy_window(&mut self, id: WindowId) -> Result<(), PlatformError> {
            self.0.destroy_window(id)
        }

        fn window_info(&self, id: WindowId) -> Result<WindowInfo, PlatformError> {
            self.0.window_info(id)
        }

        fn native_token(&self, id: WindowId) -> Result<NativeToken, PlatformError> {
            self.0.native_token(id)
        }

        fn pump(&mut self, queue: &mut EventQueue) -> PumpResult {
            let result = self.0.pump(queue);
            PumpResult {
                dropped: 0,
                ..result
            }
        }
    }

    /// Accepts a window of zero size by making it one unit large.
    struct Sloppy(HeadlessWindows);

    impl IWindowSystem for Sloppy {
        fn has_window(&self) -> bool {
            true
        }

        fn create_window(&mut self, desc: &WindowDesc<'_>) -> Result<WindowId, PlatformError> {
            let fixed = WindowDesc {
                width: desc.width.max(1),
                height: desc.height.max(1),
                ..*desc
            };
            self.0.create_window(&fixed)
        }

        fn destroy_window(&mut self, id: WindowId) -> Result<(), PlatformError> {
            self.0.destroy_window(id)
        }

        fn window_info(&self, id: WindowId) -> Result<WindowInfo, PlatformError> {
            self.0.window_info(id)
        }

        fn native_token(&self, id: WindowId) -> Result<NativeToken, PlatformError> {
            self.0.native_token(id)
        }

        fn pump(&mut self, queue: &mut EventQueue) -> PumpResult {
            self.0.pump(queue)
        }
    }

    /// Claims it delivered more events than it put in the queue.
    struct Liar(HeadlessWindows);

    impl IWindowSystem for Liar {
        fn has_window(&self) -> bool {
            true
        }

        fn create_window(&mut self, desc: &WindowDesc<'_>) -> Result<WindowId, PlatformError> {
            self.0.create_window(desc)
        }

        fn destroy_window(&mut self, id: WindowId) -> Result<(), PlatformError> {
            self.0.destroy_window(id)
        }

        fn window_info(&self, id: WindowId) -> Result<WindowInfo, PlatformError> {
            self.0.window_info(id)
        }

        fn native_token(&self, id: WindowId) -> Result<NativeToken, PlatformError> {
            self.0.native_token(id)
        }

        fn pump(&mut self, queue: &mut EventQueue) -> PumpResult {
            let result = self.0.pump(queue);
            PumpResult {
                delivered: result.delivered + 1,
                ..result
            }
        }
    }

    /// A system with no windows at all.
    struct NoWindows;

    impl IWindowSystem for NoWindows {
        fn has_window(&self) -> bool {
            false
        }

        fn create_window(&mut self, _desc: &WindowDesc<'_>) -> Result<WindowId, PlatformError> {
            Err(PlatformError::Unsupported)
        }

        fn destroy_window(&mut self, _id: WindowId) -> Result<(), PlatformError> {
            Err(PlatformError::NotFound)
        }

        fn window_info(&self, _id: WindowId) -> Result<WindowInfo, PlatformError> {
            Err(PlatformError::NotFound)
        }

        fn native_token(&self, _id: WindowId) -> Result<NativeToken, PlatformError> {
            Err(PlatformError::NotFound)
        }

        fn pump(&mut self, _queue: &mut EventQueue) -> PumpResult {
            PumpResult::default()
        }
    }

    #[test]
    #[should_panic(expected = "double destroy")]
    fn the_window_contract_rejects_a_system_that_accepts_a_double_destroy() {
        let (inner, script) = HeadlessWindows::new(4);
        headless_contract(&mut Lenient(inner), &script);
    }

    #[test]
    #[should_panic(expected = "overflow must be reported")]
    fn the_window_contract_rejects_silently_dropped_events() {
        let (inner, script) = HeadlessWindows::new(4);
        headless_contract(&mut Silent(inner), &script);
    }

    #[test]
    #[should_panic(expected = "zero size accepted")]
    fn the_window_contract_rejects_a_zero_sized_window() {
        let (inner, _script) = HeadlessWindows::new(4);
        window_contract(&mut Sloppy(inner), None);
    }

    #[test]
    #[should_panic(expected = "delivered must match")]
    fn the_window_contract_rejects_a_pump_that_miscounts() {
        let (inner, _script) = HeadlessWindows::new(4);
        window_contract(&mut Liar(inner), None);
    }

    #[test]
    #[should_panic(expected = "two windows must fit")]
    fn the_window_contract_rejects_a_system_that_cannot_hold_two_windows() {
        let (mut windows, _script) = HeadlessWindows::new(1);
        window_contract(&mut windows, None);
    }

    #[test]
    #[should_panic(expected = "can create windows")]
    fn the_window_contract_needs_a_system_with_windows() {
        window_contract(&mut NoWindows, None);
    }

    #[test]
    #[should_panic(expected = "went backwards")]
    fn the_clock_contract_rejects_a_clock_that_goes_backwards() {
        clock_contract(&BackwardsClock(AtomicU64::new(0)), false);
    }

    #[test]
    #[should_panic(expected = "returned early")]
    fn the_clock_contract_rejects_a_sleep_that_does_not_wait() {
        clock_contract(&LazySleeper(ManualClock::new()), false);
    }

    /// A manual clock whose sleep overshoots by one tick.
    struct Overshooter(ManualClock);

    impl IClock for Overshooter {
        fn now(&self) -> Ticks {
            self.0.now()
        }

        fn sleep(&self, duration: Ticks) {
            self.0.sleep(duration.saturating_add(Ticks::from_nanos(1)));
        }
    }

    fn do_nothing() {}

    /// Runs the job inline on the caller's thread.
    struct InlineRunner;

    impl IThreads for InlineRunner {
        fn spawn(&self, _name: &str, job: Job) -> Result<ThreadHandle, PlatformError> {
            job();
            let idle: Job = Box::new(do_nothing);
            StdThreads::new().spawn("inline", idle)
        }

        fn parallelism(&self) -> u32 {
            1
        }

        fn has_threads(&self) -> bool {
            true
        }
    }

    #[test]
    #[should_panic(expected = "exactly")]
    fn the_exact_clock_contract_rejects_an_overshooting_sleep() {
        clock_contract(&Overshooter(ManualClock::new()), true);
    }

    #[test]
    #[should_panic(expected = "own thread")]
    fn the_thread_contract_rejects_a_job_run_inline() {
        threads_contract(&InlineRunner, true);
    }

    #[test]
    #[should_panic(expected = "parallelism")]
    fn the_thread_contract_rejects_zero_parallelism() {
        threads_contract(&InlineThreads, false);
    }

    #[test]
    #[should_panic(expected = "spawn and join")]
    fn the_thread_contract_rejects_a_failing_spawn_when_threads_are_expected() {
        threads_contract(&NullThreads, true);
    }

    #[test]
    #[should_panic(expected = "left == right")]
    fn the_thread_contract_rejects_threads_where_none_are_expected() {
        threads_contract(&StdThreads::new(), false);
    }
}
