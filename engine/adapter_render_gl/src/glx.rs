//! An OpenGL 4.5 core context on an X11 window, through GLX (owner decision O6; Wayland is
//! unsupported, as for the window adapter).
//!
//! The window is the X11 window the window adapter hands out as its native token. The adapter
//! opens its own connection to the X server (windows are server side, so another connection
//! can draw on them), picks the framebuffer configuration of the window's own visual, makes a
//! 4.5 core context with `glXCreateContextAttribsARB` and makes it current. `libX11.so.6` and
//! `libGL.so.1` are loaded at run time and are never unloaded (their state can outlive a
//! context).
//!
//! # Error handling
//! Xlib reports a protocol error (a bad window, a refused context) by calling a process-wide
//! error handler, and the default handler ends the process. Handlers are process-global, not
//! per connection: while a context is created, swapped or deleted this module installs its own
//! handler, which records the error as a value, and puts the previous handler back afterwards.
//! A mutex keeps two threads of this adapter from installing handlers at the same time; Xlib
//! errors of other connections that arrive in that time (for example the window adapter's) are
//! recorded here too and do not end the process. The scope waits for the server (`XSync`), so
//! `present` costs one round trip to the X server.
//!
//! The adapter's connection is a second one next to the window adapter's. Xlib is not thread
//! safe unless `XInitThreads` was called before any other Xlib call; GLFW does that, an
//! application that opens windows another way must do it itself if it uses Xlib from several
//! threads.
#![expect(
    unsafe_code,
    reason = "Xlib and GLX are C interfaces; every unsafe block has a SAFETY comment"
)]

#[cfg(not(target_pointer_width = "64"))]
compile_error!("the GLX code declares Xlib structures for LP64 systems only");

use crate::dynlib::{Library, cast};
use crate::lookup::index_of_visual;
use core::ffi::{CStr, c_char, c_int, c_long, c_ulong, c_void};
use core::ptr::{NonNull, null, null_mut};
use core::sync::atomic::{AtomicU32, Ordering};
use platform::error::PlatformError;
use platform::window::NativeToken;
use std::sync::{Mutex, MutexGuard, PoisonError};

const X_RENDERABLE: c_int = 0x8012;
const DRAWABLE_TYPE: c_int = 0x8010;
const WINDOW_BIT: c_int = 0x0001;
const RENDER_TYPE: c_int = 0x8011;
const RGBA_BIT: c_int = 0x0001;
const RED_SIZE: c_int = 8;
const GREEN_SIZE: c_int = 9;
const BLUE_SIZE: c_int = 10;
const DEPTH_SIZE: c_int = 12;
const STENCIL_SIZE: c_int = 13;
const DOUBLEBUFFER: c_int = 5;
const VISUAL_ID: c_int = 0x800B;

const CONTEXT_MAJOR_VERSION: c_int = 0x2091;
const CONTEXT_MINOR_VERSION: c_int = 0x2092;
const CONTEXT_PROFILE_MASK: c_int = 0x9126;
const CONTEXT_CORE_PROFILE_BIT: c_int = 0x0000_0001;

/// The largest X resource id: ids use 29 bits.
const MAX_XID: c_ulong = 0x1FFF_FFFF;

/// Set in [`LAST_ERROR`] when an error was recorded, so error code 0 still counts.
const RECORDED: u32 = 0x100;

/// `XWindowAttributes` of Xlib on LP64 systems (136 bytes).
#[repr(C)]
struct WindowAttributes {
    x: c_int,
    y: c_int,
    width: c_int,
    height: c_int,
    border_width: c_int,
    depth: c_int,
    visual: *mut c_void,
    root: c_ulong,
    class: c_int,
    bit_gravity: c_int,
    win_gravity: c_int,
    backing_store: c_int,
    backing_planes: c_ulong,
    backing_pixel: c_ulong,
    save_under: c_int,
    colormap: c_ulong,
    map_installed: c_int,
    map_state: c_int,
    all_event_masks: c_long,
    your_event_mask: c_long,
    do_not_propagate_mask: c_long,
    override_redirect: c_int,
    screen: *mut c_void,
}

/// `XErrorEvent` of Xlib on LP64 systems (40 bytes).
#[repr(C)]
struct ErrorEvent {
    kind: c_int,
    display: *mut c_void,
    resource_id: c_ulong,
    serial: c_ulong,
    error_code: u8,
    request_code: u8,
    minor_code: u8,
}

/// An Xlib error handler.
type ErrorHandler = unsafe extern "C" fn(*mut c_void, *mut ErrorEvent) -> c_int;

type CreateAttribsFn =
    unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, c_int, *const c_int) -> *mut c_void;

function_table! {
    /// The Xlib functions of `libX11.so.6`.
    struct Xlib {
        open_display: unsafe extern "C" fn(*const c_char) -> *mut c_void = c"XOpenDisplay",
        close_display: unsafe extern "C" fn(*mut c_void) -> c_int = c"XCloseDisplay",
        default_screen: unsafe extern "C" fn(*mut c_void) -> c_int = c"XDefaultScreen",
        get_window_attributes: unsafe extern "C" fn(*mut c_void, c_ulong, *mut WindowAttributes) -> c_int = c"XGetWindowAttributes",
        visual_id: unsafe extern "C" fn(*mut c_void) -> c_ulong = c"XVisualIDFromVisual",
        free: unsafe extern "C" fn(*mut c_void) -> c_int = c"XFree",
        set_error_handler: unsafe extern "C" fn(Option<ErrorHandler>) -> Option<ErrorHandler> = c"XSetErrorHandler",
        sync: unsafe extern "C" fn(*mut c_void, c_int) -> c_int = c"XSync",
    }
}

function_table! {
    /// The GLX functions of `libGL.so.1`.
    struct Glx {
        choose_fb_config: unsafe extern "C" fn(*mut c_void, c_int, *const c_int, *mut c_int) -> *mut *mut c_void = c"glXChooseFBConfig",
        get_fb_config_attrib: unsafe extern "C" fn(*mut c_void, *mut c_void, c_int, *mut c_int) -> c_int = c"glXGetFBConfigAttrib",
        make_current: unsafe extern "C" fn(*mut c_void, c_ulong, *mut c_void) -> c_int = c"glXMakeCurrent",
        get_current_context: unsafe extern "C" fn() -> *mut c_void = c"glXGetCurrentContext",
        destroy_context: unsafe extern "C" fn(*mut c_void, *mut c_void) = c"glXDestroyContext",
        swap_buffers: unsafe extern "C" fn(*mut c_void, c_ulong) = c"glXSwapBuffers",
        get_proc_address: unsafe extern "C" fn(*const u8) -> *mut c_void = c"glXGetProcAddressARB",
    }
}

/// The framebuffer attributes asked for: RGB 8 bits per channel (no alpha is demanded: the
/// default visual of a window usually has none), depth 24, stencil 8, double buffered, drawable
/// to a window; zero-terminated.
const fn framebuffer_attributes() -> [c_int; 19] {
    [
        X_RENDERABLE,
        1,
        DRAWABLE_TYPE,
        WINDOW_BIT,
        RENDER_TYPE,
        RGBA_BIT,
        RED_SIZE,
        8,
        GREEN_SIZE,
        8,
        BLUE_SIZE,
        8,
        DEPTH_SIZE,
        24,
        STENCIL_SIZE,
        8,
        DOUBLEBUFFER,
        1,
        0,
    ]
}

/// The attribute list for an OpenGL 4.5 core context, zero-terminated.
const fn context_attributes() -> [c_int; 7] {
    [
        CONTEXT_MAJOR_VERSION,
        4,
        CONTEXT_MINOR_VERSION,
        5,
        CONTEXT_PROFILE_MASK,
        CONTEXT_CORE_PROFILE_BIT,
        0,
    ]
}

/// The X window id of a token: `None` for the null token and for numbers that cannot be an X
/// resource id (they have more than 29 bits).
fn window_id(token: NativeToken) -> Option<c_ulong> {
    c_ulong::try_from(token.0)
        .ok()
        .filter(|&id| id != 0 && id <= MAX_XID)
}

/// The last error the handler recorded: 0 for none, else [`RECORDED`] and the error code.
static LAST_ERROR: AtomicU32 = AtomicU32::new(0);

/// Held while this module's error handler is installed, so two threads do not swap handlers
/// over each other.
static SCOPE: Mutex<()> = Mutex::new(());

/// The Xlib error handler of this module: records the error and returns, so Xlib does not end
/// the process.
unsafe extern "C" fn record_error(_display: *mut c_void, event: *mut ErrorEvent) -> c_int {
    // SAFETY: Xlib passes a pointer to a valid event for the call; a null pointer is handled.
    let code = unsafe { event.as_ref() }.map_or(0, |event| u32::from(event.error_code));
    LAST_ERROR.store(RECORDED | code, Ordering::SeqCst);
    0
}

/// Whether [`record_error`] has recorded an error since the last [`forget_error`].
fn error_recorded() -> bool {
    LAST_ERROR.load(Ordering::SeqCst) != 0
}

fn forget_error() {
    LAST_ERROR.store(0, Ordering::SeqCst);
}

/// While it lives, Xlib errors are recorded instead of ending the process; the previous handler
/// is restored when it is dropped.
struct ErrorScope<'a> {
    xlib: &'a Xlib,
    display: NonNull<c_void>,
    previous: Option<ErrorHandler>,
    _serial: MutexGuard<'static, ()>,
}

impl<'a> ErrorScope<'a> {
    fn open(xlib: &'a Xlib, display: NonNull<c_void>) -> Self {
        let serial = SCOPE.lock().unwrap_or_else(PoisonError::into_inner);
        forget_error();
        // SAFETY: `record_error` has the signature Xlib expects of an error handler and lives for
        // the whole process; `XSetErrorHandler` needs no display.
        let previous = unsafe { (xlib.set_error_handler)(Some(record_error)) };
        Self {
            xlib,
            display,
            previous,
            _serial: serial,
        }
    }

    /// Waits until the server has handled every request so far, then says whether one of them
    /// failed.
    fn failed(&self) -> bool {
        // SAFETY: `display` is an open connection; `XSync` with false keeps the event queue.
        unsafe { (self.xlib.sync)(self.display.as_ptr(), 0) };
        error_recorded()
    }
}

impl Drop for ErrorScope<'_> {
    fn drop(&mut self) {
        // SAFETY: `previous` is what `XSetErrorHandler` returned when the scope opened (null
        // means Xlib's default handler, which a `None` restores).
        unsafe { (self.xlib.set_error_handler)(self.previous) };
    }
}

/// The native side of a context: the display connection and the GLX context. The libraries are
/// leaked on purpose (see the module documentation), so the function tables never dangle.
pub struct Native {
    display: NonNull<c_void>,
    window: c_ulong,
    context: NonNull<c_void>,
    glx: Glx,
    xlib: Xlib,
}

impl Native {
    /// Makes an OpenGL 4.5 core context for the X11 window of `token` and makes it current on
    /// the calling thread.
    ///
    /// # Errors
    /// [`PlatformError::NotFound`] for the null token, an impossible window id or without
    /// `libGL`/`libX11`; [`PlatformError::Unsupported`] without an X display, when no
    /// framebuffer configuration fits the window's visual, or when the driver cannot make a
    /// 4.5 core context or lacks a function; [`PlatformError::Io`] when the server refuses a
    /// step (for example a window that does not exist).
    pub fn create(token: NativeToken) -> Result<Self, PlatformError> {
        let window = window_id(token).ok_or(PlatformError::NotFound)?;
        let x11 = Library::open("libX11.so.6")
            .ok_or(PlatformError::NotFound)?
            .leak();
        let gl = Library::open("libGL.so.1")
            .or_else(|| Library::open("libGL.so"))
            .ok_or(PlatformError::NotFound)?
            .leak();
        // SAFETY: `libX11` exports these functions with the signatures declared in `Xlib`.
        let xlib =
            unsafe { Xlib::load(|name| x11.symbol(name).map_or(null_mut(), NonNull::as_ptr)) }
                .map_err(|_| PlatformError::Unsupported)?;
        // SAFETY: `libGL` exports these functions with the signatures declared in `Glx`.
        let glx = unsafe { Glx::load(|name| gl.symbol(name).map_or(null_mut(), NonNull::as_ptr)) }
            .map_err(|_| PlatformError::Unsupported)?;
        // SAFETY: a null name asks for the display named by the environment (`DISPLAY`).
        let display = NonNull::new(unsafe { (xlib.open_display)(null()) })
            .ok_or(PlatformError::Unsupported)?;
        match make_checked(&xlib, &glx, display, window) {
            Ok(context) => Ok(Self {
                display,
                window,
                context,
                glx,
                xlib,
            }),
            Err(error) => {
                // SAFETY: the display was opened above and is closed once, here.
                unsafe { (xlib.close_display)(display.as_ptr()) };
                Err(error)
            }
        }
    }

    /// The address of the OpenGL function `name`, or null.
    pub fn proc_address(&self, name: &CStr) -> *mut c_void {
        // SAFETY: `name` is NUL-terminated; `glXGetProcAddressARB` needs no current context.
        unsafe { (self.glx.get_proc_address)(name.as_ptr().cast()) }
    }

    /// Shows the back buffer; false when the server reported an error for the swap.
    pub fn swap_buffers(&self) -> bool {
        let scope = ErrorScope::open(&self.xlib, self.display);
        // SAFETY: the display and window are live and the context is current on this thread.
        unsafe { (self.glx.swap_buffers)(self.display.as_ptr(), self.window) };
        !scope.failed()
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        let scope = ErrorScope::open(&self.xlib, self.display);
        destroy_context(&self.glx, self.display, self.context);
        let _flushed = scope.failed();
        drop(scope);
        // SAFETY: the display is closed once; nothing uses it afterwards.
        unsafe { (self.xlib.close_display)(self.display.as_ptr()) };
    }
}

/// Releases `context` from this thread if it is the current one here, then destroys it.
fn destroy_context(glx: &Glx, display: NonNull<c_void>, context: NonNull<c_void>) {
    // SAFETY: `display` is open and `context` was made on it and is destroyed once; another
    // context that is current on this thread is left alone.
    unsafe {
        if (glx.get_current_context)() == context.as_ptr() {
            (glx.make_current)(display.as_ptr(), 0, null_mut());
        }
        (glx.destroy_context)(display.as_ptr(), context.as_ptr());
    }
}

/// Makes the context inside an error scope: an X error during any step is a failure, and the
/// half-made context is destroyed.
fn make_checked(
    xlib: &Xlib,
    glx: &Glx,
    display: NonNull<c_void>,
    window: c_ulong,
) -> Result<NonNull<c_void>, PlatformError> {
    let scope = ErrorScope::open(xlib, display);
    match make_context(xlib, glx, display, window) {
        Ok(context) if scope.failed() => {
            destroy_context(glx, display, context);
            Err(PlatformError::Io)
        }
        other => other,
    }
}

/// The visual id of `window`, or `None` when the server does not know the window.
fn window_visual(xlib: &Xlib, display: NonNull<c_void>, window: c_ulong) -> Option<c_ulong> {
    // SAFETY: all-zero bytes are a valid `WindowAttributes` (integers and null pointers).
    let mut attributes: WindowAttributes = unsafe { core::mem::zeroed() };
    // SAFETY: `display` is open, `attributes` is a writable `XWindowAttributes`; a window that
    // does not exist gives a status of 0 (the error handler records the protocol error).
    let status =
        unsafe { (xlib.get_window_attributes)(display.as_ptr(), window, &raw mut attributes) };
    if status == 0 || attributes.visual.is_null() {
        return None;
    }
    // SAFETY: `visual` is the non-null `Visual` pointer Xlib just returned for this window.
    Some(unsafe { (xlib.visual_id)(attributes.visual) })
}

/// The framebuffer configuration to use: among those that match the wanted attributes, the
/// first one with the window's own visual. A configuration of another visual cannot draw on the
/// window (`BadMatch`), so no match is [`PlatformError::Unsupported`], not a fallback.
fn choose_config(
    xlib: &Xlib,
    glx: &Glx,
    display: NonNull<c_void>,
    window: c_ulong,
) -> Result<*mut c_void, PlatformError> {
    let visual = window_visual(xlib, display, window).ok_or(PlatformError::Io)?;
    let attributes = framebuffer_attributes();
    let mut count: c_int = 0;
    // SAFETY: `display` is open; `attributes` is zero-terminated and `count` is writable.
    let list = unsafe {
        let screen = (xlib.default_screen)(display.as_ptr());
        (glx.choose_fb_config)(
            display.as_ptr(),
            screen,
            attributes.as_ptr(),
            &raw mut count,
        )
    };
    let list = NonNull::new(list).ok_or(PlatformError::Unsupported)?;
    let length = usize::try_from(count).unwrap_or(0);
    // SAFETY: GLX returned `count` configuration handles at `list`; the slice is only read
    // before `XFree` below.
    let configs = unsafe { core::slice::from_raw_parts(list.as_ptr(), length) };
    let chosen = index_of_visual(
        configs
            .iter()
            .map(|&config| config_visual(glx, display, config)),
        visual,
    )
    .and_then(|index| configs.get(index))
    .copied();
    // SAFETY: `list` came from `glXChooseFBConfig`, which asks for it to be freed with `XFree`.
    unsafe { (xlib.free)(list.as_ptr().cast::<c_void>()) };
    chosen.ok_or(PlatformError::Unsupported)
}

/// The visual id of a framebuffer configuration, 0 when GLX does not say.
fn config_visual(glx: &Glx, display: NonNull<c_void>, config: *mut c_void) -> u64 {
    let mut id: c_int = 0;
    // SAFETY: `display` is open, `config` is a handle GLX returned, `id` is writable.
    let status =
        unsafe { (glx.get_fb_config_attrib)(display.as_ptr(), config, VISUAL_ID, &raw mut id) };
    if status == 0 {
        u64::try_from(id).unwrap_or(0)
    } else {
        0
    }
}

/// Makes the 4.5 core context for `window` and makes it current.
fn make_context(
    xlib: &Xlib,
    glx: &Glx,
    display: NonNull<c_void>,
    window: c_ulong,
) -> Result<NonNull<c_void>, PlatformError> {
    let config = choose_config(xlib, glx, display, window)?;
    // SAFETY: the name is NUL-terminated; `glXGetProcAddressARB` needs no current context.
    let address = unsafe { (glx.get_proc_address)(c"glXCreateContextAttribsARB".as_ptr().cast()) };
    if address.is_null() {
        return Err(PlatformError::Unsupported);
    }
    // SAFETY: the driver returned this address for `glXCreateContextAttribsARB`, whose
    // signature is `CreateAttribsFn`.
    let create = unsafe { cast::<CreateAttribsFn>(address) };
    let attributes = context_attributes();
    // SAFETY: `create` has the signature of `glXCreateContextAttribsARB`; `display` is open,
    // `config` is a handle GLX returned, no context is shared (null), direct rendering is
    // asked for (1), and `attributes` is zero-terminated and lives for the call.
    let context = unsafe { create(display.as_ptr(), config, null_mut(), 1, attributes.as_ptr()) };
    let context = NonNull::new(context).ok_or(PlatformError::Unsupported)?;
    // SAFETY: display, window and context are live and belong together.
    if unsafe { (glx.make_current)(display.as_ptr(), window, context.as_ptr()) } == 0 {
        // SAFETY: the context was created above and is destroyed once.
        unsafe { (glx.destroy_context)(display.as_ptr(), context.as_ptr()) };
        return Err(PlatformError::Io);
    }
    Ok(context)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn the_xlib_structures_have_the_sizes_xlib_expects_on_lp64() {
        assert_eq!(size_of::<WindowAttributes>(), 136);
        assert_eq!(size_of::<ErrorEvent>(), 40);
    }

    #[test]
    fn the_attribute_lists_end_in_zero_and_ask_for_4_5_core() {
        let list = context_attributes();
        assert_eq!(list[..2], [CONTEXT_MAJOR_VERSION, 4]);
        assert_eq!(list[2..4], [CONTEXT_MINOR_VERSION, 5]);
        assert_eq!(list[4..6], [CONTEXT_PROFILE_MASK, CONTEXT_CORE_PROFILE_BIT]);
        assert_eq!(list[6], 0);
        let frame = framebuffer_attributes();
        assert_eq!(frame.last(), Some(&0));
        assert_eq!(frame.len() % 2, 1, "pairs, then the terminator");
    }

    #[test]
    fn the_framebuffer_attributes_demand_no_alpha() {
        let frame = framebuffer_attributes();
        assert!(frame[..frame.len() - 1].chunks(2).all(|pair| pair[0] != 11));
    }

    #[test]
    fn only_numbers_that_can_be_x_ids_name_a_window() {
        assert_eq!(window_id(NativeToken::NULL), None);
        assert_eq!(window_id(NativeToken(1)), Some(1));
        assert_eq!(window_id(NativeToken(0x1FFF_FFFF)), Some(0x1FFF_FFFF));
        assert_eq!(window_id(NativeToken(0x2000_0000)), None);
        assert_eq!(window_id(NativeToken(u64::MAX)), None);
    }

    #[test]
    fn the_null_token_names_no_window() {
        assert!(matches!(
            Native::create(NativeToken::NULL),
            Err(PlatformError::NotFound)
        ));
    }

    #[test]
    fn a_reported_error_is_recorded_not_fatal_and_forgotten_on_request() {
        let _serial = SCOPE.lock().unwrap_or_else(PoisonError::into_inner);
        forget_error();
        assert!(!error_recorded());
        let mut event = ErrorEvent {
            kind: 0,
            display: null_mut(),
            resource_id: 0,
            serial: 0,
            error_code: 0,
            request_code: 0,
            minor_code: 0,
        };
        // SAFETY: `event` is a valid, writable error event for the call.
        assert_eq!(unsafe { record_error(null_mut(), &raw mut event) }, 0);
        assert!(error_recorded(), "error code 0 is an error too");
        forget_error();
        // SAFETY: a null event is allowed by the handler.
        assert_eq!(unsafe { record_error(null_mut(), null_mut()) }, 0);
        assert!(error_recorded());
        forget_error();
        assert!(!error_recorded());
    }
}
