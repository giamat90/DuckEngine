//! An OpenGL 4.5 core context on a Win32 window, through WGL.
//!
//! The window is the `HWND` the window adapter hands out as its native token. WGL needs a
//! pixel format on the window and a context to reach `wglCreateContextAttribsARB`, so a
//! temporary legacy context is made current first and deleted once the 4.5 core context exists.
//! `opengl32.dll` is loaded at run time; `user32` and `gdi32` are always present on Windows.
#![expect(
    unsafe_code,
    reason = "WGL and GDI are C interfaces; every unsafe block has a SAFETY comment"
)]

use crate::dynlib::{Library, cast};
use crate::lookup::is_real_address;
use core::ffi::{CStr, c_char, c_int, c_void};
use core::ptr::{NonNull, null_mut};
use platform::error::PlatformError;
use platform::window::NativeToken;

const PFD_DOUBLEBUFFER: u32 = 0x0000_0001;
const PFD_DRAW_TO_WINDOW: u32 = 0x0000_0004;
const PFD_SUPPORT_OPENGL: u32 = 0x0000_0020;
const PFD_TYPE_RGBA: u8 = 0;
const COLOR_BITS: u8 = 32;
const DEPTH_BITS: u8 = 24;
const STENCIL_BITS: u8 = 8;

const CONTEXT_MAJOR_VERSION: c_int = 0x2091;
const CONTEXT_MINOR_VERSION: c_int = 0x2092;
const CONTEXT_PROFILE_MASK: c_int = 0x9126;
const CONTEXT_CORE_PROFILE_BIT: c_int = 0x0000_0001;

/// `PIXELFORMATDESCRIPTOR`.
#[repr(C)]
struct PixelFormatDescriptor {
    size: u16,
    version: u16,
    flags: u32,
    pixel_type: u8,
    color_bits: u8,
    ignored_colour_fields: [u8; 8],
    accum_bits: u8,
    ignored_accum_fields: [u8; 4],
    depth_bits: u8,
    stencil_bits: u8,
    aux_buffers: u8,
    layer_type: u8,
    reserved: u8,
    layer_mask: u32,
    visible_mask: u32,
    damage_mask: u32,
}

#[link(name = "user32")]
unsafe extern "system" {
    unsafe fn GetDC(window: *mut c_void) -> *mut c_void;
    unsafe fn ReleaseDC(window: *mut c_void, dc: *mut c_void) -> c_int;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    unsafe fn ChoosePixelFormat(dc: *mut c_void, descriptor: *const PixelFormatDescriptor)
    -> c_int;
    unsafe fn SetPixelFormat(
        dc: *mut c_void,
        format: c_int,
        descriptor: *const PixelFormatDescriptor,
    ) -> c_int;
    unsafe fn SwapBuffers(dc: *mut c_void) -> c_int;
}

type CreateAttribsFn =
    unsafe extern "system" fn(*mut c_void, *mut c_void, *const c_int) -> *mut c_void;

function_table! {
    /// The WGL functions of `opengl32.dll`.
    struct Wgl {
        create_context: unsafe extern "system" fn(*mut c_void) -> *mut c_void = c"wglCreateContext",
        delete_context: unsafe extern "system" fn(*mut c_void) -> c_int = c"wglDeleteContext",
        make_current: unsafe extern "system" fn(*mut c_void, *mut c_void) -> c_int = c"wglMakeCurrent",
        get_current_context: unsafe extern "system" fn() -> *mut c_void = c"wglGetCurrentContext",
        get_proc_address: unsafe extern "system" fn(*const c_char) -> *mut c_void = c"wglGetProcAddress",
    }
}

/// The attribute list for an OpenGL 4.5 core context, zero-terminated.
const fn attributes() -> [c_int; 7] {
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

/// The window of a token, or `None` for the null token.
fn window_of(token: NativeToken) -> Option<NonNull<c_void>> {
    let address = usize::try_from(token.0).ok()?;
    NonNull::new(core::ptr::with_exposed_provenance_mut::<c_void>(address))
}

/// A window's device context, released when dropped.
struct DeviceContext {
    window: NonNull<c_void>,
    dc: NonNull<c_void>,
}

impl DeviceContext {
    fn of(window: NonNull<c_void>) -> Option<Self> {
        // SAFETY: `window` is a window handle given by the window adapter; `GetDC` fails with
        // null for an invalid one, which is handled.
        let dc = NonNull::new(unsafe { GetDC(window.as_ptr()) })?;
        Some(Self { window, dc })
    }

    /// Gives the window a double-buffered RGBA pixel format with depth and stencil.
    fn set_pixel_format(&self) -> bool {
        let descriptor = PixelFormatDescriptor {
            size: 40,
            version: 1,
            flags: PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER,
            pixel_type: PFD_TYPE_RGBA,
            color_bits: COLOR_BITS,
            ignored_colour_fields: [0; 8],
            accum_bits: 0,
            ignored_accum_fields: [0; 4],
            depth_bits: DEPTH_BITS,
            stencil_bits: STENCIL_BITS,
            aux_buffers: 0,
            layer_type: 0,
            reserved: 0,
            layer_mask: 0,
            visible_mask: 0,
            damage_mask: 0,
        };
        // SAFETY: `dc` is a live device context from `GetDC`; `descriptor` is a fully
        // initialised `PIXELFORMATDESCRIPTOR` that lives for both calls.
        unsafe {
            let format = ChoosePixelFormat(self.dc.as_ptr(), &raw const descriptor);
            format != 0 && SetPixelFormat(self.dc.as_ptr(), format, &raw const descriptor) != 0
        }
    }
}

impl Drop for DeviceContext {
    fn drop(&mut self) {
        // SAFETY: `dc` came from `GetDC` on `window` and is released once, here.
        unsafe { ReleaseDC(self.window.as_ptr(), self.dc.as_ptr()) };
    }
}

/// The native side of a context: the device context, the WGL context and the library.
pub struct Native {
    // The context is deleted in `drop` while the device context and the library still exist;
    // the fields drop after `drop` runs, in declaration order, library last.
    context: NonNull<c_void>,
    wgl: Wgl,
    dc: DeviceContext,
    library: Library,
}

impl Native {
    /// Makes an OpenGL 4.5 core context for the window of `token` and makes it current on the
    /// calling thread, replacing whatever context was current there.
    ///
    /// Windows lets a window's pixel format be set once: a second `create` on a window that
    /// already got its format (even after the first context was dropped) fails with
    /// [`PlatformError::Io`]. Keep one context for the window's life. Creating a context for
    /// another window replaces the current one on this thread; dropping a context releases it
    /// only while it is the current one.
    ///
    /// # Errors
    /// [`PlatformError::NotFound`] for the null token or without `opengl32.dll`;
    /// [`PlatformError::Unsupported`] when the driver cannot make a 4.5 core context (or lacks a
    /// WGL function); [`PlatformError::Io`] when Windows refuses the pixel format or the switch.
    pub fn create(token: NativeToken) -> Result<Self, PlatformError> {
        let window = window_of(token).ok_or(PlatformError::NotFound)?;
        let library = Library::open("opengl32.dll").ok_or(PlatformError::NotFound)?;
        // SAFETY: `opengl32.dll` exports these functions with the signatures declared in `Wgl`.
        let wgl =
            unsafe { Wgl::load(|name| library.symbol(name).map_or(null_mut(), NonNull::as_ptr)) }
                .map_err(|_| PlatformError::Unsupported)?;
        let dc = DeviceContext::of(window).ok_or(PlatformError::Io)?;
        if !dc.set_pixel_format() {
            return Err(PlatformError::Io);
        }
        let context = core_context(&wgl, &dc)?;
        // SAFETY: both handles are live and belong together; the thread has no other context.
        if unsafe { (wgl.make_current)(dc.dc.as_ptr(), context.as_ptr()) } == 0 {
            // SAFETY: the context was created above and is deleted once.
            unsafe { (wgl.delete_context)(context.as_ptr()) };
            return Err(PlatformError::Io);
        }
        Ok(Self {
            context,
            wgl,
            dc,
            library,
        })
    }

    /// The address of the OpenGL function `name`, or null.
    pub fn proc_address(&self, name: &CStr) -> *mut c_void {
        // SAFETY: a context is current on this thread (made in `create`); `name` is
        // NUL-terminated.
        let address = unsafe { (self.wgl.get_proc_address)(name.as_ptr()) };
        if is_real_address(address.addr()) {
            return address;
        }
        // OpenGL 1.1 functions are exported by `opengl32.dll` itself, not by the driver.
        self.library
            .symbol(name)
            .map_or(null_mut(), NonNull::as_ptr)
    }

    /// Shows the back buffer; false when Windows refuses.
    pub fn swap_buffers(&self) -> bool {
        // SAFETY: `dc` is a live device context with a double-buffered pixel format.
        unsafe { SwapBuffers(self.dc.dc.as_ptr()) != 0 }
    }
}

/// Makes the 4.5 core context: through a temporary legacy context, which is gone afterwards.
fn core_context(wgl: &Wgl, dc: &DeviceContext) -> Result<NonNull<c_void>, PlatformError> {
    // SAFETY: `dc` is a live device context with a pixel format.
    let legacy = NonNull::new(unsafe { (wgl.create_context)(dc.dc.as_ptr()) })
        .ok_or(PlatformError::Unsupported)?;
    let created = create_with_attributes(wgl, dc, legacy);
    // SAFETY: the thread's current context is released, then the legacy context (made above)
    // is deleted once.
    unsafe {
        (wgl.make_current)(null_mut(), null_mut());
        (wgl.delete_context)(legacy.as_ptr());
    }
    created
}

/// With `legacy` current, asks for the 4.5 core context.
fn create_with_attributes(
    wgl: &Wgl,
    dc: &DeviceContext,
    legacy: NonNull<c_void>,
) -> Result<NonNull<c_void>, PlatformError> {
    // SAFETY: both handles are live and belong together.
    if unsafe { (wgl.make_current)(dc.dc.as_ptr(), legacy.as_ptr()) } == 0 {
        return Err(PlatformError::Io);
    }
    // SAFETY: the legacy context is current; the name is NUL-terminated.
    let address = unsafe { (wgl.get_proc_address)(c"wglCreateContextAttribsARB".as_ptr()) };
    if !is_real_address(address.addr()) {
        return Err(PlatformError::Unsupported);
    }
    // SAFETY: the driver returned this address for `wglCreateContextAttribsARB`, whose signature
    // is `CreateAttribsFn`.
    let create = unsafe { cast::<CreateAttribsFn>(address) };
    let attributes = attributes();
    // SAFETY: `create` has the signature of `wglCreateContextAttribsARB`; `dc` is live, no
    // context is shared (null), and `attributes` is a zero-terminated list that lives for the
    // call.
    let context = unsafe { create(dc.dc.as_ptr(), null_mut(), attributes.as_ptr()) };
    NonNull::new(context).ok_or(PlatformError::Unsupported)
}

impl Drop for Native {
    fn drop(&mut self) {
        // SAFETY: the context is released from this thread only if it is the current one here
        // (another context made current since is left alone), then deleted once; the device
        // context and the library are still alive (they drop afterwards).
        unsafe {
            if (self.wgl.get_current_context)() == self.context.as_ptr() {
                (self.wgl.make_current)(null_mut(), null_mut());
            }
            (self.wgl.delete_context)(self.context.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pixel_format_descriptor_has_the_size_windows_expects() {
        assert_eq!(size_of::<PixelFormatDescriptor>(), 40);
    }

    #[test]
    fn the_null_token_names_no_window() {
        assert!(window_of(NativeToken::NULL).is_none());
        assert!(window_of(NativeToken(0x1234)).is_some());
    }

    #[test]
    fn the_attribute_list_asks_for_4_5_core_and_ends_with_zero() {
        let list = attributes();
        assert_eq!(list[..2], [CONTEXT_MAJOR_VERSION, 4]);
        assert_eq!(list[2..4], [CONTEXT_MINOR_VERSION, 5]);
        assert_eq!(list[4..6], [CONTEXT_PROFILE_MASK, CONTEXT_CORE_PROFILE_BIT]);
        assert_eq!(list[6], 0);
    }

    #[test]
    fn a_made_up_window_gives_an_error_value_not_a_panic() {
        let result = Native::create(NativeToken(0xDEAD_BEE0));
        assert!(matches!(
            result,
            Err(PlatformError::Io | PlatformError::Unsupported)
        ));
    }
}
