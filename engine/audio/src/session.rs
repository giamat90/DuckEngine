//! The open/start/stop state machine shared by the Null and Offline devices.

use crate::device::{AudioRenderer, DeviceStatus};
use crate::error::AudioError;
use crate::format::{StreamFormat, StreamRequest};

/// A device that gives exactly the requested format and calls its renderer on demand.
#[derive(Default)]
pub struct Session {
    format: Option<StreamFormat>,
    renderer: Option<Box<dyn AudioRenderer>>,
    status: DeviceStatus,
}

impl Session {
    pub fn open(&mut self, request: StreamRequest) -> Result<StreamFormat, AudioError> {
        if self.renderer.is_some() {
            return Err(AudioError::WrongState);
        }
        let format = StreamFormat::exact(request)?;
        self.format = Some(format);
        Ok(format)
    }

    pub fn start(&mut self, renderer: Box<dyn AudioRenderer>) -> Result<(), AudioError> {
        if self.format.is_none() || self.renderer.is_some() {
            return Err(AudioError::WrongState);
        }
        self.renderer = Some(renderer);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<Box<dyn AudioRenderer>, AudioError> {
        self.renderer.take().ok_or(AudioError::WrongState)
    }

    pub fn status(&self) -> DeviceStatus {
        self.status
    }

    /// The negotiated format while the renderer is running.
    pub fn running_format(&self) -> Result<StreamFormat, AudioError> {
        match (self.format, &self.renderer) {
            (Some(format), Some(_)) => Ok(format),
            _ => Err(AudioError::WrongState),
        }
    }

    /// Calls the renderer once and counts the call.
    pub fn pull(&mut self, out: &mut [f32], frames: u32) -> Result<(), AudioError> {
        let renderer = self.renderer.as_mut().ok_or(AudioError::WrongState)?;
        renderer.render(out, frames);
        self.status.callbacks = self.status.callbacks.saturating_add(1);
        self.status.frames_rendered = self
            .status
            .frames_rendered
            .saturating_add(u64::from(frames));
        Ok(())
    }
}
