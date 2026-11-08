//! The Null device: accepts the whole port and never calls the renderer. The headless build runs
//! with it, so the engine needs no sound service.

use crate::device::{AudioRenderer, DeviceCaps, DeviceStatus, IAudioDevice};
use crate::error::AudioError;
use crate::format::{DEFAULT_SAMPLE_RATE, StreamFormat, StreamRequest};
use crate::session::Session;

/// A device that discards everything.
#[derive(Default)]
pub struct NullDevice {
    session: Session,
}

impl NullDevice {
    /// A closed Null device.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// The channels of the widest layout, which the Null device claims to play.
const NULL_MAX_CHANNELS: u8 = 6;

impl IAudioDevice for NullDevice {
    fn caps(&self) -> DeviceCaps {
        DeviceCaps {
            max_channels: NULL_MAX_CHANNELS,
            native_sample_rate: DEFAULT_SAMPLE_RATE,
        }
    }

    fn open(&mut self, request: StreamRequest) -> Result<StreamFormat, AudioError> {
        self.session.open(request)
    }

    fn start(&mut self, renderer: Box<dyn AudioRenderer>) -> Result<(), AudioError> {
        self.session.start(renderer)
    }

    fn stop(&mut self) -> Result<Box<dyn AudioRenderer>, AudioError> {
        self.session.stop()
    }

    fn status(&self) -> DeviceStatus {
        self.session.status()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{Probe, device_contract};
    use std::sync::Arc;

    #[test]
    fn the_null_device_passes_the_contract() {
        let mut device = NullDevice::new();
        device_contract(&mut device, false, |_| {});
    }

    #[test]
    fn the_renderer_is_never_called() {
        let mut device = NullDevice::new();
        let Ok(format) = device.open(StreamRequest::native_stereo()) else {
            panic!("open failed")
        };
        let probe = Arc::new(Probe::default());
        let started = device.start(Box::new(probe.renderer(format)));
        assert_eq!(started, Ok(()));
        assert_eq!(probe.calls(), 0);
        assert_eq!(device.status(), DeviceStatus::default());
    }

    #[test]
    fn caps_say_what_the_device_claims() {
        let caps = NullDevice::new().caps();
        assert_eq!(caps.max_channels, 6);
        assert_eq!(caps.native_sample_rate, DEFAULT_SAMPLE_RATE);
    }
}
