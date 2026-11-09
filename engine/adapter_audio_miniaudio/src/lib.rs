//! Layer 3 adapter: the audio device port on miniaudio (stopgap, `THIRD_PARTY.md`).
//!
//! [`MiniaudioDevice`] implements `audio::device::IAudioDevice` on the default playback device.
//! miniaudio is compiled from the pinned header together with a thin C shim at build time (see
//! `build.rs`); without the header the adapter still builds and every `open` reports
//! `AudioError::NoDevice`, so a program falls back to the Null device. All the `unsafe` code is in
//! the `ffi` module.
//!
//! The device plays mono and stereo (a wider layout is refused with `FormatRefused`); the sample
//! rate and block size are hints that miniaudio may change, and `open` returns what it really
//! gave. The renderer is called from miniaudio's thread in calls of at most `max_block_frames`
//! frames, whatever block size the backend uses.

mod backend;
mod ffi;
mod shared;
#[cfg(test)]
mod shared_tests;

use crate::backend::Backend;
use crate::shared::{Counters, Shared};
use audio::device::{AudioRenderer, DeviceCaps, DeviceStatus, IAudioDevice};
use audio::error::AudioError;
use audio::format::{DEFAULT_BLOCK_FRAMES, StreamFormat, StreamRequest};
use std::sync::Arc;

#[cfg(test)]
mod device_tests;

/// The most channels the adapter plays: stereo.
const MAX_CHANNELS: u8 = 2;

/// True when miniaudio was compiled in. When false every [`MiniaudioDevice::open`] reports
/// [`AudioError::NoDevice`].
#[must_use]
pub fn is_compiled_in() -> bool {
    ffi::compiled_in()
}

/// One open stream.
struct Opened {
    shared: Arc<Shared>,
}

/// The default playback device, through miniaudio.
pub struct MiniaudioDevice {
    backend: Box<dyn Backend>,
    counters: Arc<Counters>,
    opened: Option<Opened>,
    started: bool,
}

impl MiniaudioDevice {
    /// A closed device. Nothing is opened until [`IAudioDevice::open`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_backend(Box::new(ffi::Shim::default()))
    }

    fn with_backend(backend: Box<dyn Backend>) -> Self {
        Self {
            backend,
            counters: Arc::new(Counters::default()),
            opened: None,
            started: false,
        }
    }
}

impl Default for MiniaudioDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl IAudioDevice for MiniaudioDevice {
    fn caps(&self) -> DeviceCaps {
        DeviceCaps {
            max_channels: MAX_CHANNELS,
            native_sample_rate: 0,
        }
    }

    fn open(&mut self, request: StreamRequest) -> Result<StreamFormat, AudioError> {
        if self.started {
            return Err(AudioError::WrongState);
        }
        request.validate()?;
        let layout = request.layout;
        if layout.channels() > MAX_CHANNELS {
            return Err(AudioError::FormatRefused);
        }
        self.backend.close();
        self.opened = None;
        let block = match request.block_hint_frames {
            0 => DEFAULT_BLOCK_FRAMES,
            hint => hint,
        };
        let channels = u32::from(layout.channels());
        let shared = Arc::new(Shared::new(
            usize::from(layout.channels()),
            block,
            Arc::clone(&self.counters),
        ));
        let rate = self
            .backend
            .open(request.sample_rate, channels, block, &shared)?;
        if rate == 0 {
            self.backend.close();
            return Err(AudioError::NoDevice);
        }
        self.opened = Some(Opened { shared });
        Ok(StreamFormat {
            sample_rate: rate,
            layout,
            max_block_frames: block,
        })
    }

    fn start(&mut self, renderer: Box<dyn AudioRenderer>) -> Result<(), AudioError> {
        let Some(opened) = self.opened.as_ref().filter(|_| !self.started) else {
            return Err(AudioError::WrongState);
        };
        opened.shared.put(renderer);
        if self.backend.start().is_err() {
            drop(opened.shared.take());
            return Err(AudioError::DeviceLost);
        }
        self.started = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<Box<dyn AudioRenderer>, AudioError> {
        let Some(opened) = self.opened.as_ref().filter(|_| self.started) else {
            return Err(AudioError::WrongState);
        };
        self.backend.stop();
        self.started = false;
        opened.shared.take().ok_or(AudioError::WrongState)
    }

    fn status(&self) -> DeviceStatus {
        DeviceStatus {
            callbacks: self.counters.callbacks(),
            frames_rendered: self.counters.frames(),
            underruns: self.counters.misses(),
            lost: self.opened.is_some() && self.backend.lost(),
        }
    }
}
