//! Layer 3 adapter: the window and event port on GLFW (a stopgap, see `THIRD_PARTY.md`).
//!
//! Trust: the library is code that runs in the engine's process. On Windows a bare name is looked
//! up in the application directory and System32 only; [`LIBRARY_VARIABLE`] and a path given to
//! [`GlfwWindows::load`] name any file, so they are as trusted as the program's own command line
//! (give a full path). `THIRD_PARTY.md` pins the hashes of the library the engine is tested with.
//!
//! GLFW is loaded at run time from a shared library (`glfw3.dll`, `libglfw.so.3`), so the build
//! needs no C compiler and no GLFW headers; the program needs the library next to it, on the
//! search path, or named by the environment variable [`LIBRARY_VARIABLE`]. Nothing outside this
//! crate names GLFW: the port is [`IWindowSystem`], and its behaviour is the contract suite.
//!
//! Windows are created without a graphics context; the render adapter makes its own from the
//! [`NativeToken`] (`HWND` on Windows, the X11 window on Linux). Under Wayland GLFW has no such
//! window and `native_token` is [`PlatformError::Unsupported`].

#[cfg(not(any(windows, target_os = "linux")))]
compile_error!("the GLFW adapter supports Windows and Linux only");

mod dynlib;
mod glfw;

use base::pool::Pool;
use core::marker::PhantomData;
use glfw::{Api, Sink, Window};
use platform::error::PlatformError;
use platform::window::{
    EventQueue, IWindowSystem, NativeToken, PumpResult, WindowDesc, WindowId, WindowInfo,
};

/// The environment variable that names the GLFW library to load, before the default names.
pub const LIBRARY_VARIABLE: &str = "DUCK_GLFW_LIB";

#[cfg(windows)]
const DEFAULT_LIBRARIES: [&str; 1] = ["glfw3.dll"];
#[cfg(not(windows))]
const DEFAULT_LIBRARIES: [&str; 2] = ["libglfw.so.3", "libglfw.so"];

/// The window system on GLFW. Used from the thread that created it (GLFW requires the main
/// thread for window calls on some systems); it is not `Send`.
pub struct GlfwWindows {
    /// Hands out the ids and detects stale ones; the value is a placeholder.
    ids: Pool<WindowInfo>,
    /// The GLFW window of each live id, by the index of the id.
    slots: Vec<Option<Window>>,
    /// The live windows as the callbacks see them: GLFW address and id.
    live: Vec<(usize, WindowId)>,
    /// GLFW window calls belong to one thread; this keeps the type `!Send` on purpose.
    _one_thread: PhantomData<*const ()>,
    // Last: GLFW is shut down after the windows are gone.
    api: Api,
}

impl GlfwWindows {
    /// Loads GLFW from the library at `path` and starts it; at most `max_windows` windows.
    ///
    /// # Errors
    /// See [`Api::load`](glfw::Api::load): [`PlatformError::NotFound`] without the library,
    /// [`PlatformError::Unsupported`] for a library older than 3.3, [`PlatformError::Io`] when
    /// GLFW cannot start (for example without a display).
    pub fn load(path: &str, max_windows: u32) -> Result<Self, PlatformError> {
        Ok(Self::with_api(Api::load(path)?, max_windows))
    }

    /// Loads GLFW from the library named by [`LIBRARY_VARIABLE`], else from the default name of
    /// the operating system.
    ///
    /// # Errors
    /// As [`load`](Self::load); [`PlatformError::InvalidPath`] when the variable is not text.
    /// With no variable, the most telling error of the default names (a library that was found
    /// but failed beats one that was not found).
    pub fn load_default(max_windows: u32) -> Result<Self, PlatformError> {
        if let Some(path) = std::env::var_os(LIBRARY_VARIABLE) {
            let path = path.to_str().ok_or(PlatformError::InvalidPath)?;
            return Self::load(path, max_windows);
        }
        let mut best = PlatformError::NotFound;
        for name in DEFAULT_LIBRARIES {
            match Self::load(name, max_windows) {
                Ok(windows) => return Ok(windows),
                Err(error) => best = more_telling(best, error),
            }
        }
        Err(best)
    }

    fn with_api(api: Api, max_windows: u32) -> Self {
        Self {
            ids: Pool::with_capacity(max_windows),
            slots: (0..max_windows).map(|_| None).collect(),
            live: Vec::with_capacity(max_windows as usize),
            _one_thread: PhantomData,
            api,
        }
    }

    /// Keeps the window under its id; a window with no slot (impossible while the slots are as
    /// many as the ids) is destroyed again instead of leaking.
    fn store(&mut self, id: WindowId, window: Window) -> Result<(), PlatformError> {
        let Some(slot) = self.slots.get_mut(id.index() as usize) else {
            self.api.destroy_window(window);
            return Err(PlatformError::Io);
        };
        self.live.push((window.key(), id));
        *slot = Some(window);
        Ok(())
    }

    fn window(&self, id: WindowId) -> Result<&Window, PlatformError> {
        self.ids.get(id).ok_or(PlatformError::NotFound)?;
        let slot = self.slots.get(id.index() as usize);
        slot.and_then(Option::as_ref).ok_or(PlatformError::NotFound)
    }
}

/// A library that was found but failed says more than one that was not found.
fn more_telling(old: PlatformError, new: PlatformError) -> PlatformError {
    if new == PlatformError::NotFound {
        old
    } else {
        new
    }
}

impl IWindowSystem for GlfwWindows {
    fn has_window(&self) -> bool {
        true
    }

    fn create_window(&mut self, desc: &WindowDesc<'_>) -> Result<WindowId, PlatformError> {
        if desc.width == 0 || desc.height == 0 {
            return Err(PlatformError::InvalidPath);
        }
        let placeholder = WindowInfo {
            width: desc.width,
            height: desc.height,
            framebuffer_width: 0,
            framebuffer_height: 0,
            scale: 1.0,
            focused: false,
            close_requested: false,
        };
        // Reserve the id first: a full pool must not leave a window behind.
        let id = self
            .ids
            .insert(placeholder)
            .map_err(|_| PlatformError::QueueFull)?;
        match self.api.create_window(desc.title, desc.width, desc.height) {
            Ok(window) => self.store(id, window).map(|()| id).inspect_err(|_| {
                self.ids.remove(id);
            }),
            Err(error) => {
                self.ids.remove(id);
                Err(error)
            }
        }
    }

    fn destroy_window(&mut self, id: WindowId) -> Result<(), PlatformError> {
        self.window(id)?;
        self.ids.remove(id);
        let taken = self
            .slots
            .get_mut(id.index() as usize)
            .and_then(Option::take);
        if let Some(window) = taken {
            self.live.retain(|(key, _)| *key != window.key());
            self.api.destroy_window(window);
        }
        Ok(())
    }

    fn window_info(&self, id: WindowId) -> Result<WindowInfo, PlatformError> {
        let window = self.window(id)?;
        let (width, height) = self.api.window_size(window);
        let (framebuffer_width, framebuffer_height) = self.api.framebuffer_size(window);
        Ok(WindowInfo {
            width,
            height,
            framebuffer_width,
            framebuffer_height,
            scale: self.api.scale(window),
            focused: self.api.focused(window),
            close_requested: self.api.close_requested(window),
        })
    }

    fn native_token(&self, id: WindowId) -> Result<NativeToken, PlatformError> {
        let window = self.window(id)?;
        self.api
            .native_window(window)
            .map(NativeToken)
            .ok_or(PlatformError::Unsupported)
    }

    fn pump(&mut self, queue: &mut EventQueue) -> PumpResult {
        let mut sink = Sink {
            queue,
            windows: &self.live,
            delivered: 0,
            dropped: 0,
        };
        self.api.poll(&mut sink);
        PumpResult {
            delivered: sink.delivered,
            dropped: sink.dropped,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform::contract::window_contract;
    use platform::window::Event;

    /// GLFW keeps one global state and is not thread safe, so the tests that start it take turns.
    static GLFW_IN_USE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The system (dropped before the lock is released).
    struct Rig {
        windows: GlfwWindows,
        _turn: std::sync::MutexGuard<'static, ()>,
    }

    /// The real system, or `None` with the reason printed: without the library or a display
    /// these tests are "not run: device not set up", never green.
    ///
    /// With the environment variable `DUCK_REQUIRE_GLFW` set (a job that is supposed to have a
    /// display and the library) a missing device fails the test instead.
    fn real(max: u32) -> Option<Rig> {
        let turn = GLFW_IN_USE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match GlfwWindows::load_default(max) {
            Ok(windows) => Some(Rig {
                windows,
                _turn: turn,
            }),
            Err(error) => {
                assert!(
                    std::env::var_os("DUCK_REQUIRE_GLFW").is_none(),
                    "GLFW is required here but cannot start: {error}"
                );
                eprintln!("not run: device not set up (GLFW: {error})");
                None
            }
        }
    }

    /// Win32 calls the tests use to act on and look at the operating-system window.
    #[cfg(windows)]
    mod win32 {
        #![expect(unsafe_code, reason = "two Win32 calls on a window handle, in tests")]
        use core::ffi::c_void;

        #[link(name = "user32")]
        unsafe extern "system" {
            unsafe fn PostMessageW(hwnd: *mut c_void, message: u32, w: usize, l: isize) -> i32;
            unsafe fn IsWindow(hwnd: *mut c_void) -> i32;
        }

        const WM_CLOSE: u32 = 0x0010;

        /// Asks the window to close, as the close button does.
        pub fn click_close(hwnd: u64) {
            let handle = usize::try_from(hwnd).unwrap_or(0) as *mut c_void;
            // SAFETY: `PostMessageW` only queues a message for the handle; a stale handle is refused.
            unsafe { PostMessageW(handle, WM_CLOSE, 0, 0) };
        }

        /// The operating system still has this window.
        pub fn exists(hwnd: u64) -> bool {
            let handle = usize::try_from(hwnd).unwrap_or(0) as *mut c_void;
            // SAFETY: `IsWindow` accepts any handle value.
            unsafe { IsWindow(handle) != 0 }
        }
    }

    fn token_of(windows: &GlfwWindows, id: WindowId) -> u64 {
        windows.native_token(id).map_or(0, |t| t.0)
    }

    #[cfg(windows)]
    #[test]
    fn a_close_click_comes_out_of_pump_for_the_right_window_and_sets_the_close_flag() {
        let Some(mut rig) = real(2) else { return };
        let windows = &mut rig.windows;
        let a = windows
            .create_window(&DESC)
            .unwrap_or_else(|e| panic!("{e}"));
        let b = windows
            .create_window(&DESC)
            .unwrap_or_else(|e| panic!("{e}"));
        let (token_a, token_b) = (token_of(windows, a), token_of(windows, b));
        assert!(token_a != 0 && token_b != 0 && token_a != token_b);
        assert_eq!(windows.destroy_window(a), Ok(()));
        assert!(
            !win32::exists(token_a),
            "the operating system window is gone"
        );
        assert!(win32::exists(token_b));
        win32::click_close(token_b);
        let mut queue = EventQueue::with_capacity(8);
        let wanted = Event::CloseRequested { window: b };
        assert!((0..50).any(|_| pump_and_look(windows, &mut queue, wanted)));
        assert!(windows.window_info(b).is_ok_and(|i| i.close_requested));
    }

    #[cfg(windows)]
    #[test]
    fn the_framebuffer_matches_the_size_times_the_scale() {
        let Some(mut rig) = real(1) else { return };
        let windows = &mut rig.windows;
        let id = windows
            .create_window(&DESC)
            .unwrap_or_else(|e| panic!("{e}"));
        let info = windows.window_info(id).unwrap_or_else(|e| panic!("{e}"));
        let expected = (f64::from(info.width) * f64::from(info.scale)).round();
        assert!(
            (f64::from(info.framebuffer_width) - expected).abs() <= 1.0,
            "{info:?}"
        );
    }

    /// Pumps once and says whether `wanted` came out; waits a little when it did not.
    fn pump_and_look(windows: &mut GlfwWindows, queue: &mut EventQueue, wanted: Event) -> bool {
        windows.pump(queue);
        let found = seen(queue, wanted);
        if !found {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        found
    }

    /// Empties the queue and says whether `wanted` was in it.
    fn seen(queue: &mut EventQueue, wanted: Event) -> bool {
        std::iter::from_fn(|| queue.pop()).any(|e| e == wanted)
    }

    #[test]
    fn a_failure_of_a_found_library_beats_a_library_not_found() {
        let io = PlatformError::Io;
        let missing = PlatformError::NotFound;
        assert_eq!(more_telling(missing, io), io);
        assert_eq!(more_telling(io, missing), io);
        assert_eq!(more_telling(missing, missing), missing);
        assert_eq!(
            more_telling(io, PlatformError::Unsupported),
            PlatformError::Unsupported
        );
    }

    fn drain(queue: &mut EventQueue) {
        while queue.pop().is_some() {}
    }

    const DESC: WindowDesc<'static> = WindowDesc {
        title: "duckengine-test",
        width: 200,
        height: 100,
    };

    #[test]
    fn a_missing_library_is_reported_not_found() {
        let missing = GlfwWindows::load("duckengine-no-such-glfw", 1);
        assert_eq!(missing.err(), Some(PlatformError::NotFound));
    }

    #[test]
    fn the_glfw_windows_pass_the_window_contract() {
        let Some(mut rig) = real(4) else { return };
        let windows = &mut rig.windows;
        window_contract(windows, None);
    }

    #[test]
    fn a_window_reports_its_size_and_a_live_state() {
        let Some(mut rig) = real(1) else { return };
        let windows = &mut rig.windows;
        let id = windows
            .create_window(&DESC)
            .unwrap_or_else(|e| panic!("{e}"));
        let info = windows.window_info(id).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((info.width, info.height), (200, 100));
        assert!(info.framebuffer_width > 0 && info.framebuffer_height > 0);
        assert!(info.scale > 0.0);
        assert!(!info.close_requested);
        assert_eq!(windows.destroy_window(id), Ok(()));
    }

    #[test]
    fn the_capacity_is_enforced_and_freed_by_destroy() {
        let Some(mut rig) = real(1) else { return };
        let windows = &mut rig.windows;
        let first = windows
            .create_window(&DESC)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            windows.create_window(&DESC).err(),
            Some(PlatformError::QueueFull)
        );
        assert_eq!(windows.destroy_window(first), Ok(()));
        assert!(windows.create_window(&DESC).is_ok());
    }

    #[test]
    fn a_title_with_a_nul_byte_is_refused_and_leaves_no_window_behind() {
        let Some(mut rig) = real(1) else { return };
        let windows = &mut rig.windows;
        let bad = WindowDesc {
            title: "a\0b",
            ..DESC
        };
        assert_eq!(
            windows.create_window(&bad).err(),
            Some(PlatformError::InvalidPath)
        );
        assert!(
            windows.create_window(&DESC).is_ok(),
            "the id was given back"
        );
    }

    #[test]
    fn a_second_system_in_the_same_process_is_refused_and_does_not_disturb_the_first() {
        let Some(mut rig) = real(1) else { return };
        let windows = &mut rig.windows;
        let id = windows
            .create_window(&DESC)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            GlfwWindows::load_default(1).err(),
            Some(PlatformError::QueueFull)
        );
        assert!(
            windows.window_info(id).is_ok(),
            "the first system still works"
        );
        assert_eq!(windows.destroy_window(id), Ok(()));
    }

    #[test]
    fn the_installed_callbacks_deliver_a_real_resize() {
        let Some(mut rig) = real(1) else { return };
        let windows = &mut rig.windows;
        let id = windows
            .create_window(&DESC)
            .unwrap_or_else(|e| panic!("{e}"));
        let mut queue = EventQueue::with_capacity(64);
        let mut sink = Sink {
            queue: &mut queue,
            windows: &windows.live,
            delivered: 0,
            dropped: 0,
        };
        let window = windows.window(id).unwrap_or_else(|e| panic!("{e}"));
        windows.api.resize_with(window, (400, 300), &mut sink);
        let wanted = Event::Resized {
            window: id,
            width: 400,
            height: 300,
        };
        assert!(
            (0..50).any(|_| pump_and_look(windows, &mut queue, wanted)),
            "no Resized event for 400 by 300 arrived"
        );
    }

    #[test]
    fn pumping_never_reports_more_than_the_queue_can_hold() {
        let Some(mut rig) = real(1) else { return };
        let windows = &mut rig.windows;
        let id = windows
            .create_window(&DESC)
            .unwrap_or_else(|e| panic!("{e}"));
        let mut queue = EventQueue::with_capacity(1);
        for _ in 0..5 {
            let result = windows.pump(&mut queue);
            assert!(queue.len() <= queue.capacity());
            assert_eq!(usize::try_from(result.delivered).ok(), Some(queue.len()));
            drain(&mut queue);
        }
        assert_eq!(windows.destroy_window(id), Ok(()));
    }

    #[test]
    fn a_native_token_names_the_window_when_the_system_has_one() {
        let Some(mut rig) = real(2) else { return };
        let windows = &mut rig.windows;
        let a = windows
            .create_window(&DESC)
            .unwrap_or_else(|e| panic!("{e}"));
        let b = windows
            .create_window(&DESC)
            .unwrap_or_else(|e| panic!("{e}"));
        match (windows.native_token(a), windows.native_token(b)) {
            (Ok(x), Ok(y)) => assert!(x != y && x != NativeToken::NULL && y != NativeToken::NULL),
            (x, y) => assert_eq!(
                (x, y),
                (
                    Err(PlatformError::Unsupported),
                    Err(PlatformError::Unsupported)
                )
            ),
        }
    }
}
