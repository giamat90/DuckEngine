//! The only code that touches the C shim (`csrc/duck_miniaudio.c`): six opaque functions, the
//! callback they drive and the owner of the handle they return.
#![expect(
    unsafe_code,
    reason = "the shim is a C interface; every unsafe block has a SAFETY comment"
)]

use crate::backend::Backend;
use crate::shared::Shared;
use audio::error::AudioError;
use core::ffi::{c_int, c_void};
use core::ptr::NonNull;
use std::sync::Arc;

const OK: c_int = 0;
const REFUSED: c_int = 2;
const FAILED: c_int = 3;

type RenderFn = unsafe extern "C" fn(*mut c_void, *mut f32, u32);

unsafe extern "C" {
    fn duck_ma_available() -> c_int;
    fn duck_ma_open(
        sample_rate: u32,
        channels: u32,
        block_frames: u32,
        render: RenderFn,
        user: *mut c_void,
        actual_rate: *mut u32,
        error: *mut c_int,
    ) -> *mut c_void;
    fn duck_ma_start(handle: *mut c_void) -> c_int;
    fn duck_ma_stop(handle: *mut c_void);
    fn duck_ma_lost(handle: *mut c_void) -> c_int;
    fn duck_ma_close(handle: *mut c_void);
}

/// True when miniaudio was compiled in (the pinned header could be had at build time).
pub(crate) fn compiled_in() -> bool {
    // SAFETY: the function takes nothing and only returns a constant.
    unsafe { duck_ma_available() != 0 }
}

/// What miniaudio calls on its own thread for every block: hands the buffer to [`Shared`].
unsafe extern "C" fn render_callback(user: *mut c_void, out: *mut f32, frames: u32) {
    if user.is_null() || out.is_null() || frames == 0 {
        return;
    }
    // SAFETY: `user` is the `Shared` that `Shim::open` passed; the `Shim` keeps an `Arc` of it and
    // closes the device before releasing that `Arc`, so it is alive for every callback.
    let shared = unsafe { &*user.cast::<Shared>() };
    let samples = frames as usize * shared.channels();
    // SAFETY: miniaudio gives a buffer of `frames` frames with the channel count asked for in
    // `open`, interleaved `f32`, valid and not aliased until this call returns.
    let out = unsafe { core::slice::from_raw_parts_mut(out, samples) };
    shared.render(out, frames);
}

/// The real backend: one miniaudio device behind the shim.
#[derive(Default)]
pub(crate) struct Shim {
    handle: Option<NonNull<c_void>>,
    /// Keeps the callback's data alive for as long as the device exists.
    shared: Option<Arc<Shared>>,
}

// SAFETY: the handle is only used through `&mut self` or `&self` calls of one owner at a time; the
// shim's own state is touched by miniaudio's thread only through atomics and the callback, which
// uses nothing but the `Arc<Shared>` (`Send + Sync`).
unsafe impl Send for Shim {}

impl Shim {
    fn raw(&self) -> Option<*mut c_void> {
        self.handle.map(NonNull::as_ptr)
    }
}

impl Backend for Shim {
    fn open(
        &mut self,
        sample_rate: u32,
        channels: u32,
        block_frames: u32,
        shared: &Arc<Shared>,
    ) -> Result<u32, AudioError> {
        self.close();
        let user = Arc::as_ptr(shared).cast_mut().cast::<c_void>();
        let mut actual_rate = 0_u32;
        let mut error: c_int = FAILED;
        // SAFETY: the two out pointers are to locals that live for the call; `user` stays valid
        // because `self.shared` keeps the `Arc` until `close`; `render_callback` has the type the
        // shim expects.
        let raw = unsafe {
            duck_ma_open(
                sample_rate,
                channels,
                block_frames,
                render_callback,
                user,
                &raw mut actual_rate,
                &raw mut error,
            )
        };
        let Some(handle) = NonNull::new(raw) else {
            return Err(match error {
                REFUSED => AudioError::FormatRefused,
                _ => AudioError::NoDevice,
            });
        };
        self.handle = Some(handle);
        self.shared = Some(Arc::clone(shared));
        Ok(actual_rate)
    }

    fn start(&mut self) -> Result<(), AudioError> {
        let handle = self.raw().ok_or(AudioError::WrongState)?;
        // SAFETY: `handle` is the live device of this shim.
        if unsafe { duck_ma_start(handle) } == OK {
            Ok(())
        } else {
            Err(AudioError::DeviceLost)
        }
    }

    fn stop(&mut self) {
        if let Some(handle) = self.raw() {
            // SAFETY: `handle` is the live device; the shim stops it and returns only when no
            // callback is running.
            unsafe { duck_ma_stop(handle) };
        }
    }

    fn lost(&self) -> bool {
        // SAFETY: `handle` is the live device of this shim.
        self.raw()
            .is_some_and(|handle| unsafe { duck_ma_lost(handle) } != 0)
    }

    fn close(&mut self) {
        if let Some(handle) = self.handle.take() {
            // SAFETY: `handle` came from `duck_ma_open`, is closed once (it was just taken out)
            // and the callback never runs again after this returns.
            unsafe { duck_ma_close(handle.as_ptr()) };
        }
        self.shared = None;
    }
}

impl Drop for Shim {
    fn drop(&mut self) {
        self.close();
    }
}
