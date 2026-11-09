//! The device port: negotiate a format, then drive a renderer from the device's own thread.

use crate::error::AudioError;
use crate::format::{StreamFormat, StreamRequest};

/// What a device can do, known before it is opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceCaps {
    /// The most channels the device can play.
    pub max_channels: u8,
    /// The device's own sample rate; 0 when it is not known before opening.
    pub native_sample_rate: u32,
}

/// Counters a device keeps; the game thread polls them. No callback into game code exists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceStatus {
    /// Times the renderer was called.
    pub callbacks: u64,
    /// Frames the renderer was asked for, in total.
    pub frames_rendered: u64,
    /// Underruns the adapter could see.
    pub underruns: u64,
    /// True once the device was lost; the device must be opened again.
    pub lost: bool,
}

/// The callback a device drives. It runs on a thread the adapter owns, so it is `Send`.
///
/// Inside `render`: no allocation, no lock, no I/O, no blocking call, no unbounded loop, and
/// nothing that frees memory is dropped (the real-time rule of `audio-port-v0` section 3).
pub trait AudioRenderer: Send {
    /// Fills `out` with `frames` interleaved frames. `out.len()` is exactly
    /// `frames * channels`; `frames` is at most the format's `max_block_frames` and may differ from
    /// call to call. The whole slice must be written (silence when nothing plays).
    fn render(&mut self, out: &mut [f32], frames: u32);
}

/// An output device. All calls come from the game thread, never from the callback.
///
/// States: closed, open (format negotiated), started (the renderer is being called). `open` is
/// allowed when not started, `start` when open, `stop` when started and returns to open.
pub trait IAudioDevice: Send {
    /// What the device can do.
    fn caps(&self) -> DeviceCaps;

    /// Negotiates the stream: the caller proposes, the device returns what it really gives.
    ///
    /// # Errors
    /// [`AudioError::InvalidRequest`] for numbers outside the port's limits,
    /// [`AudioError::NoDevice`], [`AudioError::FormatRefused`], and [`AudioError::WrongState`]
    /// while started.
    fn open(&mut self, request: StreamRequest) -> Result<StreamFormat, AudioError>;

    /// Starts calling `renderer`, which the caller built for the format the last `open` returned
    /// (the device cannot check this; a renderer built for another layout writes the wrong
    /// number of channels).
    ///
    /// # Errors
    /// [`AudioError::WrongState`] when not open or already started (the renderer is dropped),
    /// [`AudioError::DeviceLost`].
    fn start(&mut self, renderer: Box<dyn AudioRenderer>) -> Result<(), AudioError>;

    /// Stops calling the renderer and gives it back, so that it is dropped on the calling thread
    /// and never inside the callback. When `stop` returns, no callback is running or will run.
    /// The device is open again afterwards. After a lost device (`status().lost`) `stop` still
    /// succeeds and gives the renderer back, and `open` is allowed again.
    ///
    /// # Errors
    /// [`AudioError::WrongState`] when not started.
    fn stop(&mut self) -> Result<Box<dyn AudioRenderer>, AudioError>;

    /// The counters so far; they survive `stop`.
    fn status(&self) -> DeviceStatus;
}
