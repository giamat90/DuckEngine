//! The GLFW C API, bound by hand: the functions the adapter needs, looked up in the shared library
//! at run time, and the callbacks that turn GLFW events into port events.
//!
//! Everything GLFW-shaped stops here. [`Api`] is safe to use: a [`Window`] cannot be used after
//! it is destroyed (destroying consumes it), and every call goes through `&Api`, which owns the
//! library.
#![expect(
    unsafe_code,
    reason = "GLFW is a C interface; every unsafe block has a SAFETY comment"
)]

use crate::dynlib::Library;
use core::cell::Cell;
use core::ffi::{c_char, c_int, c_void};
use core::mem::transmute;
use core::ptr::{NonNull, null_mut};
use core::sync::atomic::{AtomicBool, Ordering};
use platform::error::PlatformError;
use platform::window::{Action, Event, EventQueue, WindowId};

const GLFW_FOCUSED: c_int = 0x0002_0001;
const GLFW_CLIENT_API: c_int = 0x0002_2001;
const GLFW_NO_API: c_int = 0;

type CloseFn = unsafe extern "C" fn(*mut c_void);
type FocusFn = unsafe extern "C" fn(*mut c_void, c_int);
type SizeFn = unsafe extern "C" fn(*mut c_void, c_int, c_int);
type KeyFn = unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_int);
type ButtonFn = unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int);
type MoveFn = unsafe extern "C" fn(*mut c_void, f64, f64);
/// GLFW callback setters: each takes the callback of its own type and returns the previous one.
type SetClose = unsafe extern "C" fn(*mut c_void, Option<CloseFn>) -> Option<CloseFn>;
type SetFocus = unsafe extern "C" fn(*mut c_void, Option<FocusFn>) -> Option<FocusFn>;
type SetSize = unsafe extern "C" fn(*mut c_void, Option<SizeFn>) -> Option<SizeFn>;
type SetKey = unsafe extern "C" fn(*mut c_void, Option<KeyFn>) -> Option<KeyFn>;
type SetButton = unsafe extern "C" fn(*mut c_void, Option<ButtonFn>) -> Option<ButtonFn>;
type SetMove = unsafe extern "C" fn(*mut c_void, Option<MoveFn>) -> Option<MoveFn>;
type GetPair = unsafe extern "C" fn(*mut c_void, *mut c_int, *mut c_int);

#[cfg(windows)]
type NativeRaw = *mut c_void;
#[cfg(not(windows))]
type NativeRaw = core::ffi::c_ulong;
type NativeFn = unsafe extern "C" fn(*mut c_void) -> NativeRaw;

/// One GLFW window. Not `Copy`: [`Api::destroy_window`] takes it by value.
#[derive(Debug)]
pub struct Window(NonNull<c_void>);

impl Window {
    /// The address of the window, to match it with the window of a callback.
    #[must_use]
    pub fn key(&self) -> usize {
        self.0.as_ptr().addr()
    }
}

/// The loaded library and its functions; `glfwInit` has succeeded for a value of this type.
pub struct Api {
    init: unsafe extern "C" fn() -> c_int,
    terminate: unsafe extern "C" fn(),
    window_hint: unsafe extern "C" fn(c_int, c_int),
    create_window:
        unsafe extern "C" fn(c_int, c_int, *const c_char, *mut c_void, *mut c_void) -> *mut c_void,
    destroy_window: unsafe extern "C" fn(*mut c_void),
    poll_events: unsafe extern "C" fn(),
    get_window_size: GetPair,
    get_framebuffer_size: GetPair,
    get_content_scale: unsafe extern "C" fn(*mut c_void, *mut f32, *mut f32),
    should_close: unsafe extern "C" fn(*mut c_void) -> c_int,
    get_attrib: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    set_close: SetClose,
    set_focus: SetFocus,
    set_size: SetSize,
    set_key: SetKey,
    set_button: SetButton,
    set_cursor: SetMove,
    set_scroll: SetMove,
    native: Option<NativeFn>,
    // Released after `glfwTerminate` (see `Drop`), before the library is closed.
    _only: OnlyOne,
    // Last: the library is closed after every pointer above is gone.
    _library: Library,
}

/// GLFW keeps one global state, and a second library handle in the same process shares it, so
/// dropping one `Api` would tear down the windows of the other. This flag allows one at a time.
static IN_USE: AtomicBool = AtomicBool::new(false);

/// The right to be the one `Api` of the process.
#[derive(Debug)]
struct OnlyOne;

impl OnlyOne {
    fn take() -> Option<Self> {
        IN_USE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self)
    }
}

impl Drop for OnlyOne {
    fn drop(&mut self) {
        IN_USE.store(false, Ordering::Release);
    }
}

/// Looks up `$name` in `$lib` as a function of the type the context asks for; `Option<fn>`.
macro_rules! function {
    ($lib:expr, $name:literal) => {
        $lib.symbol($name).map(|address| {
            // SAFETY: the symbol has the C signature the field or variable declares, in GLFW 3.3
            // and later, which `version_ok` checks before any pointer is used.
            unsafe { transmute::<*mut c_void, _>(address.as_ptr()) }
        })
    };
}

impl Api {
    /// Loads GLFW from `path`, checks it is version 3.3 or later and initialises it.
    ///
    /// # Errors
    /// [`PlatformError::NotFound`] when the library cannot be loaded, [`PlatformError::Unsupported`]
    /// when it is older than 3.3 or lacks a function, [`PlatformError::QueueFull`] when another
    /// `Api` of this process is alive (GLFW has one global state), [`PlatformError::Io`] when
    /// `glfwInit` fails (for example without a display).
    pub fn load(path: &str) -> Result<Self, PlatformError> {
        let library = Library::open(path).ok_or(PlatformError::NotFound)?;
        if !version_ok(&library) {
            return Err(PlatformError::Unsupported);
        }
        let only = OnlyOne::take().ok_or(PlatformError::QueueFull)?;
        let api = Self::bind(library, only).ok_or(PlatformError::Unsupported)?;
        // SAFETY: `glfwInit` takes no arguments; this runs on the thread that will use the API.
        // If it fails, dropping `api` calls `glfwTerminate`, which is allowed before a successful
        // init and then does nothing.
        if unsafe { (api.init)() } == 0 {
            return Err(PlatformError::Io);
        }
        Ok(api)
    }

    fn bind(library: Library, only: OnlyOne) -> Option<Self> {
        Some(Self {
            init: function!(library, c"glfwInit")?,
            terminate: function!(library, c"glfwTerminate")?,
            window_hint: function!(library, c"glfwWindowHint")?,
            create_window: function!(library, c"glfwCreateWindow")?,
            destroy_window: function!(library, c"glfwDestroyWindow")?,
            poll_events: function!(library, c"glfwPollEvents")?,
            get_window_size: function!(library, c"glfwGetWindowSize")?,
            get_framebuffer_size: function!(library, c"glfwGetFramebufferSize")?,
            get_content_scale: function!(library, c"glfwGetWindowContentScale")?,
            should_close: function!(library, c"glfwWindowShouldClose")?,
            get_attrib: function!(library, c"glfwGetWindowAttrib")?,
            set_close: function!(library, c"glfwSetWindowCloseCallback")?,
            set_focus: function!(library, c"glfwSetWindowFocusCallback")?,
            set_size: function!(library, c"glfwSetWindowSizeCallback")?,
            set_key: function!(library, c"glfwSetKeyCallback")?,
            set_button: function!(library, c"glfwSetMouseButtonCallback")?,
            set_cursor: function!(library, c"glfwSetCursorPosCallback")?,
            set_scroll: function!(library, c"glfwSetScrollCallback")?,
            native: native_lookup(&library),
            _only: only,
            _library: library,
        })
    }

    /// Creates a window without a graphics context (the render adapter makes its own from the
    /// native token) and installs the event callbacks.
    ///
    /// # Errors
    /// [`PlatformError::InvalidPath`] for a title with a NUL byte or a size that does not fit,
    /// [`PlatformError::Io`] when GLFW cannot create the window.
    pub fn create_window(
        &self,
        title: &str,
        width: u32,
        height: u32,
    ) -> Result<Window, PlatformError> {
        let title = std::ffi::CString::new(title).map_err(|_| PlatformError::InvalidPath)?;
        let width = c_int::try_from(width).map_err(|_| PlatformError::InvalidPath)?;
        let height = c_int::try_from(height).map_err(|_| PlatformError::InvalidPath)?;
        // SAFETY: GLFW is initialised (a value of `Api` exists), the title is NUL-terminated and
        // lives for the call, and null monitor and share window mean "windowed, no sharing".
        let raw = unsafe {
            (self.window_hint)(GLFW_CLIENT_API, GLFW_NO_API);
            (self.create_window)(width, height, title.as_ptr(), null_mut(), null_mut())
        };
        let window = NonNull::new(raw).map(Window).ok_or(PlatformError::Io)?;
        self.install_callbacks(&window);
        Ok(window)
    }

    fn install_callbacks(&self, window: &Window) {
        let w = window.0.as_ptr();
        // SAFETY: the window is live; each setter's type names the callback type it takes, so the
        // compiler checks every pairing; the previous callbacks (none) are ignored.
        unsafe {
            (self.set_close)(w, Some(on_close));
            (self.set_focus)(w, Some(on_focus));
            (self.set_size)(w, Some(on_size));
            (self.set_key)(w, Some(on_key));
            (self.set_button)(w, Some(on_button));
            (self.set_cursor)(w, Some(on_cursor));
            (self.set_scroll)(w, Some(on_scroll));
        }
    }

    /// Destroys the window; no callback runs for it afterwards.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "taking the window by value is what ends its use"
    )]
    pub fn destroy_window(&self, window: Window) {
        // SAFETY: the window is live and consumed here, so it cannot be used again.
        unsafe { (self.destroy_window)(window.0.as_ptr()) };
    }

    /// Size of the window in screen units.
    #[must_use]
    pub fn window_size(&self, window: &Window) -> (u32, u32) {
        pair(self.get_window_size, window)
    }

    /// Size of the framebuffer in pixels.
    #[must_use]
    pub fn framebuffer_size(&self, window: &Window) -> (u32, u32) {
        pair(self.get_framebuffer_size, window)
    }

    /// Pixels per screen unit (the horizontal content scale).
    #[must_use]
    pub fn scale(&self, window: &Window) -> f32 {
        let (mut x, mut y) = (1.0_f32, 1.0_f32);
        // SAFETY: the window is live and both out pointers are valid for the call.
        unsafe { (self.get_content_scale)(window.0.as_ptr(), &raw mut x, &raw mut y) };
        x
    }

    /// The user asked to close the window.
    #[must_use]
    pub fn close_requested(&self, window: &Window) -> bool {
        // SAFETY: the window is live.
        unsafe { (self.should_close)(window.0.as_ptr()) != 0 }
    }

    /// The window has input focus.
    #[must_use]
    pub fn focused(&self, window: &Window) -> bool {
        // SAFETY: the window is live.
        unsafe { (self.get_attrib)(window.0.as_ptr(), GLFW_FOCUSED) != 0 }
    }

    /// The operating-system window (`HWND` on Windows, the X11 window on Linux) as a number, or
    /// `None` when GLFW has none (for example under Wayland).
    #[must_use]
    pub fn native_window(&self, window: &Window) -> Option<u64> {
        let lookup = self.native?;
        // SAFETY: the window is live.
        let raw = unsafe { lookup(window.0.as_ptr()) };
        native_number(raw)
    }

    /// Polls GLFW; the callbacks append the events to `sink`.
    ///
    /// Only events raised inside this call are queued. A callback that fires during
    /// `create_window` or `destroy_window` finds no sink and is ignored; the state it reported is
    /// still available from the window queries. On Windows GLFW can stay inside this call while
    /// the user drags or resizes a window.
    pub fn poll(&self, sink: &mut Sink<'_>) {
        // SAFETY: GLFW is initialised and this runs on the thread that created the windows.
        with_active(sink, || unsafe { (self.poll_events)() });
    }
}

#[cfg(test)]
impl Api {
    /// Resizes the window through GLFW with `sink` as the callback target, so a test can see
    /// the real callbacks run (GLFW may call them inside this call or at the next poll).
    #[expect(
        clippy::used_underscore_binding,
        reason = "the field is named for being kept alive, and this test helper looks up one more symbol"
    )]
    pub fn resize_with(&self, window: &Window, size: (c_int, c_int), sink: &mut Sink<'_>) {
        type Resize = unsafe extern "C" fn(*mut c_void, c_int, c_int);
        let set: Option<Resize> = function!(self._library, c"glfwSetWindowSize");
        let Some(set) = set else {
            panic!("glfwSetWindowSize is part of GLFW 3");
        };
        // SAFETY: the window is live and this runs on the thread that created it.
        with_active(sink, || unsafe { set(window.0.as_ptr(), size.0, size.1) });
    }
}

impl Drop for Api {
    fn drop(&mut self) {
        // SAFETY: GLFW destroys any window that remains, and the library is still loaded (it is
        // the last field).
        unsafe { (self.terminate)() };
    }
}

#[cfg(windows)]
fn native_lookup(library: &Library) -> Option<NativeFn> {
    function!(library, c"glfwGetWin32Window")
}

#[cfg(not(windows))]
fn native_lookup(library: &Library) -> Option<NativeFn> {
    function!(library, c"glfwGetX11Window")
}

#[cfg(windows)]
fn native_number(raw: NativeRaw) -> Option<u64> {
    u64::try_from(raw.addr()).ok().filter(|n| *n != 0)
}

#[cfg(not(windows))]
fn native_number(raw: NativeRaw) -> Option<u64> {
    // `c_ulong` is 64 bits on the 64-bit Linux targets, the only ones the port is shaped for.
    let id: u64 = raw;
    Some(id).filter(|n| *n != 0)
}

/// Runs `run` with `sink` as the target of the callbacks, then restores the previous target.
fn with_active<R>(sink: &mut Sink<'_>, run: impl FnOnce() -> R) -> R {
    let this: *mut Sink<'_> = sink;
    let previous = ACTIVE.with(|slot| slot.replace(this.cast::<c_void>()));
    let result = run();
    ACTIVE.with(|slot| slot.set(previous));
    result
}

fn pair(get: GetPair, window: &Window) -> (u32, u32) {
    let (mut a, mut b): (c_int, c_int) = (0, 0);
    // SAFETY: the window is live and both out pointers are valid for the call.
    unsafe { get(window.0.as_ptr(), &raw mut a, &raw mut b) };
    (to_u32(a), to_u32(b))
}

fn version_ok(library: &Library) -> bool {
    let get: Option<unsafe extern "C" fn(*mut c_int, *mut c_int, *mut c_int)> =
        function!(library, c"glfwGetVersion");
    let Some(get) = get else {
        return false;
    };
    let (mut major, mut minor, mut revision): (c_int, c_int, c_int) = (0, 0, 0);
    // SAFETY: the three out pointers are valid for the call; `glfwGetVersion` may be called
    // before `glfwInit`.
    unsafe { get(&raw mut major, &raw mut minor, &raw mut revision) };
    version_supported(major, minor)
}

/// GLFW 3.3 or later: the content scale and the callbacks used here exist from 3.3 on, and a
/// 4.x library is not known to keep the same C API.
fn version_supported(major: c_int, minor: c_int) -> bool {
    major == 3 && minor >= 3
}

/// Where the callbacks of one `poll` put their events.
pub struct Sink<'a> {
    /// The caller's queue.
    pub queue: &'a mut EventQueue,
    /// The live windows: the address of the GLFW window and its port id.
    pub windows: &'a [(usize, WindowId)],
    /// Events appended.
    pub delivered: u32,
    /// Events that did not fit.
    pub dropped: u32,
}

impl Sink<'_> {
    fn push(&mut self, window: *mut c_void, make: impl FnOnce(WindowId) -> Event) {
        let key = window.addr();
        let Some((_, id)) = self.windows.iter().find(|(k, _)| *k == key) else {
            return;
        };
        match self.queue.push(make(*id)) {
            Ok(()) => self.delivered += 1,
            Err(_) => self.dropped += 1,
        }
    }
}

thread_local! {
    /// The sink of the `poll` running on this thread, or null. GLFW calls the callbacks on the
    /// thread that polls, and only during the poll, so this is the one place they find it.
    static ACTIVE: Cell<*mut c_void> = const { Cell::new(null_mut()) };
}

fn with_sink(window: *mut c_void, make: impl FnOnce(WindowId) -> Event) {
    let sink = ACTIVE.with(Cell::get).cast::<Sink<'_>>();
    // SAFETY: `ACTIVE` is non-null only inside `Api::poll`, where it points at the `Sink` that
    // `poll` borrows mutably for the whole call; callbacks run one at a time on this thread.
    if let Some(sink) = unsafe { sink.as_mut() } {
        sink.push(window, make);
    }
}

unsafe extern "C" fn on_close(window: *mut c_void) {
    with_sink(window, |id| Event::CloseRequested { window: id });
}

unsafe extern "C" fn on_focus(window: *mut c_void, focused: c_int) {
    with_sink(window, |id| Event::Focus {
        window: id,
        gained: focused != 0,
    });
}

unsafe extern "C" fn on_size(window: *mut c_void, width: c_int, height: c_int) {
    with_sink(window, |id| Event::Resized {
        window: id,
        width: to_u32(width),
        height: to_u32(height),
    });
}

unsafe extern "C" fn on_key(
    window: *mut c_void,
    _key: c_int,
    scancode: c_int,
    action: c_int,
    mods: c_int,
) {
    if let Some(action) = action_from(action) {
        with_sink(window, |id| Event::Key {
            window: id,
            scancode: to_u32(scancode),
            action,
            mods: mods_from(mods),
        });
    }
}

unsafe extern "C" fn on_button(window: *mut c_void, button: c_int, action: c_int, _mods: c_int) {
    if let Some(action) = action_from(action) {
        with_sink(window, |id| Event::MouseButton {
            window: id,
            button: u8::try_from(button).unwrap_or(u8::MAX),
            action,
        });
    }
}

unsafe extern "C" fn on_cursor(window: *mut c_void, x: f64, y: f64) {
    with_sink(window, |id| Event::MouseMove {
        window: id,
        x: narrow(x),
        y: narrow(y),
    });
}

unsafe extern "C" fn on_scroll(window: *mut c_void, dx: f64, dy: f64) {
    with_sink(window, |id| Event::Scroll {
        window: id,
        dx: narrow(dx),
        dy: narrow(dy),
    });
}

/// GLFW action codes: 0 release, 1 press, 2 repeat.
fn action_from(code: c_int) -> Option<Action> {
    match code {
        0 => Some(Action::Release),
        1 => Some(Action::Press),
        2 => Some(Action::Repeat),
        _ => None,
    }
}

/// GLFW modifier bits (shift 1, control 2, alt 4, super 8) are the port's bits; others are cut.
fn mods_from(bits: c_int) -> u8 {
    u8::try_from(bits & 0xF).unwrap_or(0)
}

fn to_u32(value: c_int) -> u32 {
    u32::try_from(value).unwrap_or(0)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "screen coordinates and scroll amounts fit f32 with room to spare"
)]
fn narrow(value: f64) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform::window::mods;

    #[test]
    fn actions_map_and_unknown_codes_are_refused() {
        assert_eq!(action_from(0), Some(Action::Release));
        assert_eq!(action_from(1), Some(Action::Press));
        assert_eq!(action_from(2), Some(Action::Repeat));
        assert_eq!(action_from(3), None);
        assert_eq!(action_from(-1), None);
    }

    #[test]
    fn modifier_bits_pass_through_and_the_rest_are_cut() {
        let all = i32::from(mods::SHIFT | mods::CONTROL | mods::ALT | mods::SUPER);
        assert_eq!(mods_from(all), 0xF);
        assert_eq!(mods_from(i32::from(mods::ALT)), mods::ALT);
        assert_eq!(
            mods_from(0x10 | 0x20),
            0,
            "caps lock and num lock are not ours"
        );
        assert_eq!(mods_from(-1), 0xF);
    }

    #[test]
    fn negative_sizes_become_zero_and_numbers_narrow() {
        assert_eq!(to_u32(-5), 0);
        assert_eq!(to_u32(7), 7);
        assert!((narrow(1.5) - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn a_missing_library_is_not_found() {
        assert_eq!(
            Api::load("duckengine-no-such-glfw").err(),
            Some(PlatformError::NotFound)
        );
    }

    /// A sink for the callbacks of a window that GLFW would call `0x40`, with room for `room`
    /// events; the callbacks are called by hand, so no display is needed.
    fn with_callbacks(
        room: u32,
        run: impl FnOnce(*mut c_void),
    ) -> (Vec<Event>, Sink<'static>, WindowId) {
        let mut ids = base::pool::Pool::<platform::window::WindowInfo>::with_capacity(1);
        let info = platform::window::WindowInfo {
            width: 1,
            height: 1,
            framebuffer_width: 1,
            framebuffer_height: 1,
            scale: 1.0,
            focused: false,
            close_requested: false,
        };
        let id = ids.insert(info).unwrap_or_else(|_| panic!("one id fits"));
        let queue = Box::leak(Box::new(EventQueue::with_capacity(room)));
        let windows: &'static [(usize, WindowId)] = Box::leak(Box::new([(0x40, id)]));
        let mut sink = Sink {
            queue,
            windows,
            delivered: 0,
            dropped: 0,
        };
        let this: *mut Sink<'_> = &raw mut sink;
        let previous = ACTIVE.with(|slot| slot.replace(this.cast::<c_void>()));
        run(0x40 as *mut c_void);
        ACTIVE.with(|slot| slot.set(previous));
        let events = core::iter::from_fn(|| sink.queue.pop()).collect();
        (events, sink, id)
    }

    #[test]
    fn each_callback_becomes_the_matching_event() {
        let (events, sink, w) = with_callbacks(16, |win| {
            // SAFETY: the callbacks only read their arguments and `ACTIVE`, set by the helper.
            unsafe {
                on_close(win);
                on_focus(win, 1);
                on_size(win, 800, 600);
                on_key(win, 65, 30, 1, 3);
                on_button(win, 1, 0, 0);
                on_cursor(win, 10.5, 20.25);
                on_scroll(win, 0.0, -1.0);
            }
        });
        assert_eq!((sink.delivered, sink.dropped), (7, 0));
        assert_eq!(
            events,
            vec![
                Event::CloseRequested { window: w },
                Event::Focus {
                    window: w,
                    gained: true
                },
                Event::Resized {
                    window: w,
                    width: 800,
                    height: 600
                },
                Event::Key {
                    window: w,
                    scancode: 30,
                    action: Action::Press,
                    mods: 3
                },
                Event::MouseButton {
                    window: w,
                    button: 1,
                    action: Action::Release
                },
                Event::MouseMove {
                    window: w,
                    x: 10.5,
                    y: 20.25
                },
                Event::Scroll {
                    window: w,
                    dx: 0.0,
                    dy: -1.0
                },
            ]
        );
    }

    #[test]
    fn events_that_do_not_fit_are_counted_and_unknown_windows_and_actions_are_ignored() {
        let (events, sink, _) = with_callbacks(2, |win| {
            // SAFETY: as above.
            unsafe {
                on_close(win);
                on_close(win);
                on_close(win);
                on_close(0x99 as *mut c_void);
                on_key(win, 0, 0, 7, 0);
                on_button(win, 0, 9, 0);
            }
        });
        assert_eq!(events.len(), 2);
        assert_eq!((sink.delivered, sink.dropped), (2, 1));
    }

    #[test]
    fn callbacks_outside_a_poll_do_nothing() {
        // SAFETY: with no active sink the callback reads nothing from its argument.
        unsafe { on_close(0x40 as *mut c_void) };
    }

    #[test]
    fn only_glfw_3_3_and_later_in_the_3_series_is_accepted() {
        assert!(version_supported(3, 3));
        assert!(version_supported(3, 4));
        assert!(!version_supported(3, 2));
        assert!(!version_supported(2, 9));
        assert!(!version_supported(4, 0));
    }

    #[test]
    fn the_constants_match_the_glfw_header() {
        assert_eq!(GLFW_FOCUSED, 0x20001);
        assert_eq!(GLFW_CLIENT_API, 0x22001);
        assert_eq!(GLFW_NO_API, 0);
    }
}
