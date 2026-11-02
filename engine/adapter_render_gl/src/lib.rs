//! Layer 3 adapter: OpenGL 4.5 on Windows (WGL) and Linux (GLX), loaded at run time.
//!
//! This is the only code of the engine that knows OpenGL. Card 3-9 delivers the context: the
//! system OpenGL library is loaded at run time (no GL binding crate, no build dependency), a
//! 4.5 core context is made from the window's [`NativeToken`](platform::window::NativeToken)
//! (the `HWND` on Windows, the X11 window on Linux), the engine's clip space is selected
//! (`glClipControl`, depth 0 to 1) and a colour can be cleared and presented. The device
//! (buffers, textures, pipelines, draw) follows in card 3-10.
//!
//! Without a usable OpenGL driver every call that starts things reports an error value
//! ([`PlatformError`](platform::error::PlatformError)); nothing panics.

#[cfg(not(any(windows, target_os = "linux")))]
compile_error!("the OpenGL adapter supports Windows and Linux only");

// `lookup` first: it defines the `function_table!` macro the modules below use.
#[macro_use]
mod lookup;
mod context;
mod dynlib;
mod gl;
#[cfg(target_os = "linux")]
#[path = "glx.rs"]
mod native;
#[cfg(windows)]
#[path = "wgl.rs"]
mod native;
pub mod version;

pub use context::{ContextInfo, GlContext};
