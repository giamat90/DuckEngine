//! Test support: an Offline device with one deliberate fault, to prove that the contract suite
//! catches each rule it checks.

use crate::device::{AudioRenderer, DeviceCaps, DeviceStatus, IAudioDevice};
use crate::error::AudioError;
use crate::format::{ChannelLayout, StreamFormat, StreamRequest};
use crate::offline::OfflineDevice;

/// The rule an adapter breaks.
#[derive(Clone, Copy, PartialEq)]
pub enum Fault {
    StartsBeforeOpen,
    SecondStartAccepted,
    RefusedStartKeepsRenderer,
    OpenWhileStartedAccepted,
    SecondStopAccepted,
    StopGivesBackAnotherRenderer,
    CountsNothing,
    IgnoresTheLayout,
}

struct Dummy;

impl AudioRenderer for Dummy {
    fn render(&mut self, out: &mut [f32], _frames: u32) {
        out.fill(0.0);
    }
}

/// An Offline device that breaks one rule.
pub struct Faulty {
    pub inner: OfflineDevice,
    fault: Fault,
    started: bool,
    stopped_once: bool,
    kept: Vec<Box<dyn AudioRenderer>>,
}

impl Faulty {
    pub fn new(fault: Fault) -> Self {
        Self {
            inner: OfflineDevice::new(),
            fault,
            started: false,
            stopped_once: false,
            kept: Vec::new(),
        }
    }

    /// What `stop` hands back: the renderer, or (the fault) another one after dropping it.
    fn given_back(&self, real: Box<dyn AudioRenderer>) -> Box<dyn AudioRenderer> {
        if self.fault == Fault::StopGivesBackAnotherRenderer {
            Box::new(Dummy)
        } else {
            real
        }
    }

    fn open_faulty(&self, request: StreamRequest) -> Option<Result<StreamFormat, AudioError>> {
        (self.started && self.fault == Fault::OpenWhileStartedAccepted)
            .then(|| StreamFormat::exact(request))
    }
}

impl IAudioDevice for Faulty {
    fn caps(&self) -> DeviceCaps {
        self.inner.caps()
    }

    fn open(&mut self, request: StreamRequest) -> Result<StreamFormat, AudioError> {
        if let Some(answer) = self.open_faulty(request) {
            return answer;
        }
        let format = self.inner.open(request)?;
        Ok(if self.fault == Fault::IgnoresTheLayout {
            StreamFormat {
                layout: ChannelLayout::Stereo,
                ..format
            }
        } else {
            format
        })
    }

    fn start(&mut self, renderer: Box<dyn AudioRenderer>) -> Result<(), AudioError> {
        match (self.started, self.fault) {
            (false, Fault::StartsBeforeOpen) | (true, Fault::SecondStartAccepted) => Ok(()),
            (true, Fault::RefusedStartKeepsRenderer) => {
                self.kept.push(renderer);
                Err(AudioError::WrongState)
            }
            _ => {
                self.inner.start(renderer)?;
                self.started = true;
                Ok(())
            }
        }
    }

    fn stop(&mut self) -> Result<Box<dyn AudioRenderer>, AudioError> {
        let was_stopped = self.stopped_once;
        match self.inner.stop() {
            Ok(real) => {
                self.started = false;
                self.stopped_once = true;
                Ok(self.given_back(real))
            }
            Err(_) if was_stopped && self.fault == Fault::SecondStopAccepted => Ok(Box::new(Dummy)),
            Err(error) => Err(error),
        }
    }

    fn status(&self) -> DeviceStatus {
        if self.fault == Fault::CountsNothing {
            DeviceStatus::default()
        } else {
            self.inner.status()
        }
    }
}
