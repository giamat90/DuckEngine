//! The OpenGL context: made from a window's native token, current on the creating thread.

use crate::gl::{Gl, RENDERER, VENDOR, VERSION};
use crate::native::Native;
use crate::version::{GlVersion, accept};
use core::marker::PhantomData;
use platform::error::PlatformError;
use platform::window::NativeToken;

/// What the driver says about a context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextInfo {
    /// `GL_VENDOR`.
    pub vendor: String,
    /// `GL_RENDERER`: the device, for example the GPU name or `llvmpipe`.
    pub renderer: String,
    /// `GL_VERSION`, the full text.
    pub version_text: String,
    /// The version read from `version_text`; always 4.5 or newer.
    pub version: GlVersion,
}

/// An OpenGL 4.5 core context on a window, current on the thread that made it.
///
/// Not `Send`: an OpenGL context belongs to one thread at a time, and this one is never released
/// from its thread. Dropping it deletes the context; drop it before the window is destroyed.
/// Clip space is the engine's: bottom-left origin, depth 0 to 1.
pub struct GlContext {
    info: ContextInfo,
    gl: Gl,
    /// The context stays current on one thread; this keeps the type `!Send` on purpose.
    _one_thread: PhantomData<*const ()>,
    // Last: the context is deleted after everything that calls into it is gone.
    native: Native,
}

impl GlContext {
    /// Makes an OpenGL 4.5 core context for the window named by `token` (the `HWND` on Windows,
    /// the X11 window on Linux, from the window adapter) and makes it current.
    ///
    /// The new context replaces any context that was current on the calling thread; dropping a
    /// context releases it only while it is still the current one. On Windows a window takes one
    /// context for its life (its pixel format can be set once). On Linux the adapter opens its
    /// own connection to the X server and installs a temporary, process-wide Xlib error handler
    /// while it creates, swaps and deletes; an application that calls Xlib from other threads
    /// must have called `XInitThreads` first (GLFW does).
    ///
    /// # Errors
    /// [`PlatformError::NotFound`] for the null token or without the system OpenGL library;
    /// [`PlatformError::Unsupported`] when the driver cannot make a 4.5 core context, lacks a
    /// function this adapter needs or reports an older version; [`PlatformError::Io`] when the
    /// operating system refuses a step (for example a window that is gone).
    pub fn create(token: NativeToken) -> Result<Self, PlatformError> {
        let native = Native::create(token)?;
        let gl = Gl::load_current(|name| native.proc_address(name))
            .map_err(|_| PlatformError::Unsupported)?;
        let version_text = gl.string(VERSION);
        let version = accept(version_text.as_deref())?;
        gl.use_engine_clip_space();
        let info = ContextInfo {
            vendor: gl.string(VENDOR).unwrap_or_default(),
            renderer: gl.string(RENDERER).unwrap_or_default(),
            version_text: version_text.unwrap_or_default(),
            version,
        };
        Ok(Self {
            info,
            gl,
            _one_thread: PhantomData,
            native,
        })
    }

    /// What the driver reported when the context was made.
    #[must_use]
    pub fn info(&self) -> &ContextInfo {
        &self.info
    }

    /// Sets the viewport to the whole `width` by `height` pixel area.
    pub fn set_viewport(&self, width: u32, height: u32) {
        self.gl.set_viewport(0, 0, to_i32(width), to_i32(height));
    }

    /// The viewport as x, y, width, height in pixels (a context starts with the window size).
    #[must_use]
    pub fn viewport(&self) -> [u32; 4] {
        self.gl
            .viewport()
            .map(|value| u32::try_from(value).unwrap_or(0))
    }

    /// The clip control state as `[origin, depth mode]` (`GL_CLIP_ORIGIN`, `GL_CLIP_DEPTH_MODE`):
    /// `[GL_LOWER_LEFT, GL_ZERO_TO_ONE]` (`[0x8CA1, 0x935F]`) for the engine's clip space.
    #[must_use]
    pub fn clip_control_state(&self) -> [u32; 2] {
        self.gl.clip_control_state()
    }

    /// Clears the back buffer to `rgba` (each channel 0 to 1).
    pub fn clear(&self, rgba: [f32; 4]) {
        self.gl.set_clear_colour(rgba);
        self.gl.clear_colour_buffer();
    }

    /// The back buffer pixel at (`x`, `y`), `y` counted from the bottom, as RGBA8. Waits for
    /// the GPU; for tests and tools, not for frames.
    #[must_use]
    pub fn read_pixel(&self, x: u32, y: u32) -> [u8; 4] {
        self.gl.finish_all();
        self.gl.read_pixel(to_i32(x), to_i32(y))
    }

    /// Shows the back buffer. `false` when the operating system refused.
    #[must_use]
    pub fn present(&self) -> bool {
        self.native.swap_buffers()
    }

    /// The oldest OpenGL error flag, cleared; 0 is "no error".
    #[must_use]
    pub fn take_error(&self) -> u32 {
        self.gl.error()
    }
}

/// A pixel count or coordinate as the `i32` OpenGL takes; saturates.
fn to_i32(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::to_i32;

    #[test]
    fn counts_saturate_at_the_largest_i32() {
        assert_eq!(to_i32(0), 0);
        assert_eq!(to_i32(640), 640);
        assert_eq!(to_i32(i32::MAX.unsigned_abs()), i32::MAX);
        assert_eq!(to_i32(u32::MAX), i32::MAX);
    }
}
