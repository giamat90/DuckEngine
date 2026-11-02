//! The OpenGL functions this adapter calls, declared by hand and looked up in the context.
//!
//! [`Gl`] is a table of function pointers plus safe wrappers. A wrapper is only sound when the
//! context that loaded the table is current on the calling thread; [`Gl`] is private to the
//! crate and only [`GlContext`](crate::GlContext), which is not `Send` and makes its context
//! current when it is created, holds one. The table grows with the device in card 3-10: add a
//! line to the declaration and a wrapper.
#![expect(
    unsafe_code,
    reason = "OpenGL is a C interface; every unsafe block has a SAFETY comment"
)]

use core::ffi::{CStr, c_void};

/// `GL_VENDOR`, `GL_RENDERER`, `GL_VERSION`: the strings `glGetString` returns.
pub const VENDOR: u32 = 0x1F00;
pub const RENDERER: u32 = 0x1F01;
pub const VERSION: u32 = 0x1F02;

const VIEWPORT: u32 = 0x0BA2;
const COLOR_BUFFER_BIT: u32 = 0x0000_4000;
const RGBA: u32 = 0x1908;
const UNSIGNED_BYTE: u32 = 0x1401;
/// `GL_LOWER_LEFT` and `GL_ZERO_TO_ONE`: the arguments of `glClipControl` that give the engine's
/// depth range 0 to 1 and a bottom-left origin.
const LOWER_LEFT: u32 = 0x8CA1;
const ZERO_TO_ONE: u32 = 0x935F;
/// `GL_CLIP_ORIGIN` and `GL_CLIP_DEPTH_MODE`: the state `glClipControl` sets.
const CLIP_ORIGIN: u32 = 0x935C;
const CLIP_DEPTH_MODE: u32 = 0x935D;

function_table! {
    /// The OpenGL 4.5 core functions of this card.
    pub(crate) struct Gl {
        get_string: unsafe extern "system" fn(u32) -> *const u8 = c"glGetString",
        get_error: unsafe extern "system" fn() -> u32 = c"glGetError",
        clear_color: unsafe extern "system" fn(f32, f32, f32, f32) = c"glClearColor",
        clear: unsafe extern "system" fn(u32) = c"glClear",
        viewport: unsafe extern "system" fn(i32, i32, i32, i32) = c"glViewport",
        read_pixels: unsafe extern "system" fn(i32, i32, i32, i32, u32, u32, *mut c_void) = c"glReadPixels",
        finish: unsafe extern "system" fn() = c"glFinish",
        clip_control: unsafe extern "system" fn(u32, u32) = c"glClipControl",
        get_integerv: unsafe extern "system" fn(u32, *mut i32) = c"glGetIntegerv",
    }
}

impl Gl {
    /// Looks the functions up with `lookup`, the proc-address function of a context that is
    /// current on this thread; `Err` names the first function the driver lacks.
    pub fn load_current(lookup: impl FnMut(&CStr) -> *mut c_void) -> Result<Self, &'static CStr> {
        // SAFETY: OpenGL's proc-address functions return the function with the C signature of
        // the name that was asked for, and the declarations above follow the OpenGL 4.5 core
        // specification.
        unsafe { Self::load(lookup) }
    }

    /// The text `glGetString(name)` returns, or `None` when GL returns null.
    pub fn string(&self, name: u32) -> Option<String> {
        // SAFETY: a context is current (see the module documentation) and `name` is one of the
        // string names; the call has no other precondition.
        let text = unsafe { (self.get_string)(name) };
        if text.is_null() {
            return None;
        }
        // SAFETY: a non-null result of `glGetString` points at a NUL-terminated string that
        // stays valid until the context is destroyed; it is copied before this returns.
        let text = unsafe { CStr::from_ptr(text.cast()) };
        Some(text.to_string_lossy().into_owned())
    }

    /// The oldest error flag (`GL_NO_ERROR` is 0), which is cleared.
    pub fn error(&self) -> u32 {
        // SAFETY: a context is current; the call takes no arguments.
        unsafe { (self.get_error)() }
    }

    /// Sets the colour `clear` writes.
    pub fn set_clear_colour(&self, rgba: [f32; 4]) {
        // SAFETY: a context is current; any four floats are accepted (clamped by GL).
        unsafe { (self.clear_color)(rgba[0], rgba[1], rgba[2], rgba[3]) };
    }

    /// Clears the colour buffer of the framebuffer that is bound for drawing.
    pub fn clear_colour_buffer(&self) {
        // SAFETY: a context is current; `COLOR_BUFFER_BIT` is a valid mask.
        unsafe { (self.clear)(COLOR_BUFFER_BIT) };
    }

    /// Sets the viewport rectangle.
    pub fn set_viewport(&self, x: i32, y: i32, width: i32, height: i32) {
        // SAFETY: a context is current; a negative size only raises a GL error flag.
        unsafe { (self.viewport)(x, y, width, height) };
    }

    /// The pixel at (`x`, `y`) of the read framebuffer as RGBA8, `y` counted from the bottom.
    pub fn read_pixel(&self, x: i32, y: i32) -> [u8; 4] {
        let mut pixel = [0u8; 4];
        // SAFETY: a context is current; one RGBA/UNSIGNED_BYTE pixel is exactly four bytes, and
        // `pixel` has four bytes of room; the pack alignment of 4 does not need more for one
        // pixel.
        unsafe {
            (self.read_pixels)(
                x,
                y,
                1,
                1,
                RGBA,
                UNSIGNED_BYTE,
                pixel.as_mut_ptr().cast::<c_void>(),
            );
        }
        pixel
    }

    /// The viewport rectangle: x, y, width, height.
    pub fn viewport(&self) -> [i32; 4] {
        let mut rectangle = [0i32; 4];
        // SAFETY: a context is current; `GL_VIEWPORT` returns exactly four integers and
        // `rectangle` has room for four.
        unsafe { (self.get_integerv)(VIEWPORT, rectangle.as_mut_ptr()) };
        rectangle
    }

    /// One integer state value, for example `CLIP_ORIGIN`.
    pub fn integer(&self, name: u32) -> i32 {
        let mut value = 0i32;
        // SAFETY: a context is current; the state names read through this wrapper return
        // exactly one integer and `value` has room for one.
        unsafe { (self.get_integerv)(name, &raw mut value) };
        value
    }

    /// Waits until every issued command has finished.
    pub fn finish_all(&self) {
        // SAFETY: a context is current; the call takes no arguments.
        unsafe { (self.finish)() };
    }

    /// The clip control state: the clip origin and the depth mode, as `GLenum` values.
    pub fn clip_control_state(&self) -> [u32; 2] {
        [CLIP_ORIGIN, CLIP_DEPTH_MODE].map(|name| u32::try_from(self.integer(name)).unwrap_or(0))
    }

    /// Selects a bottom-left origin and the depth range 0 to 1, the engine's convention.
    pub fn use_engine_clip_space(&self) {
        // SAFETY: a context is current and this is OpenGL 4.5, where `glClipControl` exists
        // (the context creation refuses lower versions before calling this); both arguments are
        // valid enums.
        unsafe { (self.clip_control)(LOWER_LEFT, ZERO_TO_ONE) };
    }
}
