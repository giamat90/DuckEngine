//! The Offline device: gives exactly the requested format and renders on demand, on the calling
//! thread, with no clock. Tests, golden audio and benchmarks use it.

use crate::device::{AudioRenderer, DeviceCaps, DeviceStatus, IAudioDevice};
use crate::error::AudioError;
use crate::format::{DEFAULT_SAMPLE_RATE, StreamFormat, StreamRequest};
use crate::session::Session;

/// The channels of the widest layout, which the Offline device claims to play.
const OFFLINE_MAX_CHANNELS: u8 = 6;

/// A device that renders when asked. Before every callback it fills the slice with NaN, so a
/// renderer that does not write the whole slice shows up in the output.
#[derive(Default)]
pub struct OfflineDevice {
    session: Session,
}

impl OfflineDevice {
    /// A closed Offline device.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Renders `out.len() / channels` frames in callbacks of the largest size the format allows.
    ///
    /// # Errors
    /// [`AudioError::WrongState`] when not started, [`AudioError::InvalidRequest`] when `out` is
    /// not a whole number of frames.
    pub fn render_into(&mut self, out: &mut [f32]) -> Result<(), AudioError> {
        let block = self.session.running_format()?.max_block_frames;
        self.render_in_chunks(out, block)
    }

    /// Like [`Self::render_into`] with callbacks of `chunk_frames` frames (the last one smaller
    /// when the length is not a multiple).
    ///
    /// # Errors
    /// [`AudioError::WrongState`] when not started, [`AudioError::InvalidRequest`] when
    /// `chunk_frames` is 0 or above `max_block_frames`, or `out` is not a whole number of frames.
    pub fn render_in_chunks(
        &mut self,
        out: &mut [f32],
        chunk_frames: u32,
    ) -> Result<(), AudioError> {
        let format = self.session.running_format()?;
        let channels = usize::from(format.layout.channels());
        if chunk_frames == 0
            || chunk_frames > format.max_block_frames
            || !out.len().is_multiple_of(channels)
        {
            return Err(AudioError::InvalidRequest);
        }
        for piece in out.chunks_mut(format.samples_for(chunk_frames)) {
            piece.fill(f32::NAN);
            let frames =
                u32::try_from(piece.len() / channels).map_err(|_| AudioError::InvalidRequest)?;
            self.session.pull(piece, frames)?;
        }
        Ok(())
    }
}

impl IAudioDevice for OfflineDevice {
    fn caps(&self) -> DeviceCaps {
        DeviceCaps {
            max_channels: OFFLINE_MAX_CHANNELS,
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
    use crate::format::{ChannelLayout, DEFAULT_BLOCK_FRAMES};
    use std::sync::{Arc, Mutex};

    /// Writes its call counter into every sample and records the frame counts it was given.
    struct Recorder {
        frames_seen: Arc<Mutex<Vec<u32>>>,
        value: f32,
    }

    impl AudioRenderer for Recorder {
        fn render(&mut self, out: &mut [f32], frames: u32) {
            out.fill(self.value);
            self.frames_seen
                .lock()
                .map(|mut seen| seen.push(frames))
                .ok();
        }
    }

    /// Does not write anything.
    struct Lazy;

    impl AudioRenderer for Lazy {
        fn render(&mut self, _out: &mut [f32], _frames: u32) {}
    }

    fn started(renderer: Box<dyn AudioRenderer>, request: StreamRequest) -> OfflineDevice {
        let mut device = OfflineDevice::new();
        assert!(device.open(request).is_ok());
        assert_eq!(device.start(renderer), Ok(()));
        device
    }

    fn recorder(value: f32) -> (Box<dyn AudioRenderer>, Arc<Mutex<Vec<u32>>>) {
        let frames_seen = Arc::new(Mutex::new(Vec::new()));
        let renderer = Recorder {
            frames_seen: Arc::clone(&frames_seen),
            value,
        };
        (Box::new(renderer), frames_seen)
    }

    fn seen(frames_seen: &Arc<Mutex<Vec<u32>>>) -> Vec<u32> {
        frames_seen.lock().map(|v| v.clone()).unwrap_or_default()
    }

    #[test]
    fn the_offline_device_passes_the_contract() {
        let mut device = OfflineDevice::new();
        device_contract(&mut device, true, |device| {
            // The contract also pumps a stopped device, where rendering is refused: nothing runs.
            let mut out = vec![0.0; 2 * 1000];
            let _ = device.render_into(&mut out);
        });
    }

    #[test]
    fn the_output_is_what_the_renderer_wrote() {
        let (renderer, _) = recorder(0.25);
        let mut device = started(renderer, StreamRequest::native_stereo());
        let mut out = vec![0.0; 2 * 100];
        assert_eq!(device.render_into(&mut out), Ok(()));
        assert!(out.iter().all(|&s| s.to_bits() == 0.25_f32.to_bits()));
    }

    #[test]
    fn callbacks_are_as_large_as_the_format_allows() {
        let (renderer, frames_seen) = recorder(0.0);
        let mut device = started(renderer, StreamRequest::native_stereo());
        let mut out = vec![0.0; 2 * 1300];
        assert_eq!(device.render_into(&mut out), Ok(()));
        assert_eq!(
            seen(&frames_seen),
            vec![DEFAULT_BLOCK_FRAMES, DEFAULT_BLOCK_FRAMES, 276]
        );
    }

    #[test]
    fn chunks_of_any_size_up_to_the_maximum_are_possible() {
        let (renderer, frames_seen) = recorder(0.0);
        let mut device = started(renderer, StreamRequest::native_stereo());
        let mut out = vec![0.0; 2 * 20];
        assert_eq!(device.render_in_chunks(&mut out, 7), Ok(()));
        assert_eq!(seen(&frames_seen), vec![7, 7, 6]);
    }

    #[test]
    fn a_chunk_of_zero_or_above_the_maximum_is_refused() {
        let (renderer, _) = recorder(0.0);
        let mut device = started(renderer, StreamRequest::native_stereo());
        let mut out = vec![0.0; 8];
        assert_eq!(
            device.render_in_chunks(&mut out, 0),
            Err(AudioError::InvalidRequest)
        );
        assert_eq!(
            device.render_in_chunks(&mut out, DEFAULT_BLOCK_FRAMES + 1),
            Err(AudioError::InvalidRequest)
        );
    }

    #[test]
    fn a_partial_frame_is_refused() {
        let (renderer, _) = recorder(0.0);
        let mut device = started(renderer, StreamRequest::native_stereo());
        let mut out = vec![0.0; 3];
        assert_eq!(
            device.render_into(&mut out),
            Err(AudioError::InvalidRequest)
        );
    }

    #[test]
    fn rendering_needs_a_started_device() {
        let mut device = OfflineDevice::new();
        let mut out = vec![0.0; 4];
        assert_eq!(device.render_into(&mut out), Err(AudioError::WrongState));
        assert!(device.open(StreamRequest::native_stereo()).is_ok());
        assert_eq!(device.render_into(&mut out), Err(AudioError::WrongState));
    }

    #[test]
    fn a_renderer_that_writes_nothing_leaves_nan_behind() {
        let mut device = started(Box::new(Lazy), StreamRequest::native_stereo());
        let mut out = vec![0.0; 8];
        assert_eq!(device.render_into(&mut out), Ok(()));
        assert!(out.iter().all(|s| s.is_nan()));
    }

    #[test]
    fn the_status_counts_callbacks_and_frames_and_survives_stop() {
        let (renderer, _) = recorder(0.0);
        let mut device = started(renderer, StreamRequest::native_stereo());
        let mut out = vec![0.0; 2 * 600];
        assert_eq!(device.render_into(&mut out), Ok(()));
        assert!(device.stop().is_ok());
        let status = device.status();
        assert_eq!(status.callbacks, 2);
        assert_eq!(status.frames_rendered, 600);
        assert!(!status.lost);
    }

    fn check_layout(layout: ChannelLayout, channels: usize) {
        let probe = Arc::new(Probe::default());
        let request = StreamRequest {
            sample_rate: 0,
            layout,
            block_hint_frames: 64,
        };
        let mut device = OfflineDevice::new();
        let Ok(format) = device.open(request) else {
            panic!("open failed")
        };
        assert_eq!(device.start(Box::new(probe.renderer(format))), Ok(()));
        let mut out = vec![0.0; channels * 200];
        assert_eq!(device.render_into(&mut out), Ok(()));
        assert!(!probe.bad_length());
        assert_eq!(probe.max_frames(), 64);
    }

    #[test]
    fn mono_and_surround_buffers_have_their_channel_counts() {
        check_layout(ChannelLayout::Mono, 1);
        check_layout(ChannelLayout::Surround51, 6);
    }
}
