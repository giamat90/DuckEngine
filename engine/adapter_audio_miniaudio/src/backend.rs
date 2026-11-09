//! What the device logic needs from a sound backend. The real one is the miniaudio shim
//! ([`crate::ffi::Shim`]); the tests use a fake that renders when they tell it to.

use crate::shared::Shared;
use audio::error::AudioError;
use std::sync::Arc;

/// One sound device, driven from the game thread. The backend calls `shared.render` from its own
/// thread while started.
pub(crate) trait Backend: Send {
    /// Opens the default playback device for `channels` channels of `f32`, at `sample_rate` (0 for
    /// the device's own) and with blocks of about `block_frames`; returns the rate it really has.
    /// Closes an earlier stream first.
    fn open(
        &mut self,
        sample_rate: u32,
        channels: u32,
        block_frames: u32,
        shared: &Arc<Shared>,
    ) -> Result<u32, AudioError>;

    /// Starts calling `shared.render`.
    fn start(&mut self) -> Result<(), AudioError>;

    /// Stops; when it returns no call of `shared.render` is running or will run.
    fn stop(&mut self);

    /// True once the device went away without being stopped.
    fn lost(&self) -> bool;

    /// Closes the stream, if any.
    fn close(&mut self);
}
