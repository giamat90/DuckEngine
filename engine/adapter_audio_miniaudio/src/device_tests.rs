//! Tests of the device logic with a fake backend: no sound hardware, so they run everywhere. The
//! fake renders when the test tells it to, the way a device thread would.

use crate::MiniaudioDevice;
use crate::backend::Backend;
use crate::shared::Shared;
use audio::contract::device_contract;
use audio::device::{AudioRenderer, IAudioDevice};
use audio::error::AudioError;
use audio::format::{ChannelLayout, StreamRequest};
use std::sync::{Arc, Mutex};

/// What the test can see and change of the fake.
#[derive(Default)]
struct State {
    shared: Option<Arc<Shared>>,
    running: bool,
    lost: bool,
    open_error: Option<AudioError>,
    start_fails: bool,
    rate: u32,
    opens: u32,
    closes: u32,
    last_request: Option<(u32, u32, u32)>,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<State>>);

impl Fake {
    fn with_rate(rate: u32) -> Self {
        let fake = Self::default();
        fake.edit(|s| s.rate = rate);
        fake
    }

    fn edit<T>(&self, change: impl FnOnce(&mut State) -> T) -> Option<T> {
        self.0.lock().ok().map(|mut state| change(&mut state))
    }

    fn read<T>(&self, read: impl FnOnce(&State) -> T) -> Option<T> {
        self.0.lock().ok().map(|state| read(&state))
    }

    /// One device callback of `frames` frames, if the device is running.
    fn tick(&self, frames: u32) {
        let shared = self
            .read(|s| if s.running { s.shared.clone() } else { None })
            .flatten();
        if let Some(shared) = shared {
            let mut out = vec![0.0_f32; frames as usize * shared.channels()];
            shared.render(&mut out, frames);
        }
    }
}

impl Backend for Fake {
    fn open(
        &mut self,
        sample_rate: u32,
        channels: u32,
        block_frames: u32,
        shared: &Arc<Shared>,
    ) -> Result<u32, AudioError> {
        let failure = self
            .edit(|s| {
                s.opens += 1;
                s.last_request = Some((sample_rate, channels, block_frames));
                s.shared = s.open_error.is_none().then(|| Arc::clone(shared));
                s.open_error
            })
            .flatten();
        match failure {
            Some(error) => Err(error),
            None => Ok(self.read(|s| s.rate).unwrap_or(0)),
        }
    }

    fn start(&mut self) -> Result<(), AudioError> {
        let fails = self.read(|s| s.start_fails).unwrap_or(true);
        if fails {
            return Err(AudioError::DeviceLost);
        }
        self.edit(|s| s.running = true);
        Ok(())
    }

    fn stop(&mut self) {
        self.edit(|s| s.running = false);
    }

    fn lost(&self) -> bool {
        self.read(|s| s.lost).unwrap_or(false)
    }

    fn close(&mut self) {
        self.edit(|s| {
            s.closes += 1;
            s.shared = None;
            s.running = false;
        });
    }
}

fn device(fake: &Fake) -> MiniaudioDevice {
    MiniaudioDevice::with_backend(Box::new(fake.clone()))
}

/// Writes 0.5 and counts nothing: only the device's own counters are looked at.
struct Half;

impl AudioRenderer for Half {
    fn render(&mut self, out: &mut [f32], _frames: u32) {
        out.fill(0.5);
    }
}

fn stereo(rate: u32, block: u32) -> StreamRequest {
    StreamRequest {
        sample_rate: rate,
        layout: ChannelLayout::Stereo,
        block_hint_frames: block,
    }
}

#[test]
fn the_fake_passes_the_contract_suite() {
    let fake = Fake::with_rate(44_100);
    fake.edit(|s| s.start_fails = false);
    let mut device = device(&fake);
    device_contract(&mut device, true, |_| fake.tick(300));
}

#[test]
fn open_returns_what_the_backend_gave() {
    let fake = Fake::with_rate(44_100);
    let mut device = device(&fake);
    let format = device.open(stereo(48_000, 256));
    assert_eq!(format.map(|f| f.sample_rate), Ok(44_100));
    assert_eq!(format.map(|f| f.max_block_frames), Ok(256));
    assert_eq!(format.map(|f| f.layout), Ok(ChannelLayout::Stereo));
    assert_eq!(
        fake.read(|s| s.last_request),
        Some(Some((48_000, 2, 256))),
        "the request reaches the backend as asked"
    );
}

#[test]
fn a_zero_block_hint_means_the_default_block() {
    let fake = Fake::with_rate(48_000);
    let mut device = device(&fake);
    let format = device.open(StreamRequest::native_stereo());
    assert_eq!(
        format.map(|f| f.max_block_frames),
        Ok(audio::format::DEFAULT_BLOCK_FRAMES)
    );
}

#[test]
fn the_adapter_plays_mono_and_stereo_and_refuses_wider() {
    let fake = Fake::with_rate(48_000);
    let mut device = device(&fake);
    assert_eq!(device.caps().max_channels, 2);
    let mono = StreamRequest {
        layout: ChannelLayout::Mono,
        ..StreamRequest::native_stereo()
    };
    assert!(device.open(mono).is_ok());
    let wide = StreamRequest {
        layout: ChannelLayout::Surround51,
        ..StreamRequest::native_stereo()
    };
    assert_eq!(device.open(wide), Err(AudioError::FormatRefused));
    assert_eq!(
        fake.read(|s| s.opens),
        Some(1),
        "a refusal never reaches the backend"
    );
}

#[test]
fn an_invalid_request_is_refused_before_the_backend() {
    let fake = Fake::with_rate(48_000);
    let mut device = device(&fake);
    assert_eq!(device.open(stereo(1, 0)), Err(AudioError::InvalidRequest));
    assert_eq!(fake.read(|s| s.opens), Some(0));
}

#[test]
fn a_backend_failure_is_passed_on_and_leaves_the_device_closed() {
    let fake = Fake::with_rate(48_000);
    fake.edit(|s| s.open_error = Some(AudioError::NoDevice));
    let mut device = device(&fake);
    assert_eq!(
        device.open(StreamRequest::native_stereo()),
        Err(AudioError::NoDevice)
    );
    assert_eq!(
        device.start(Box::new(Half)).err(),
        Some(AudioError::WrongState),
        "a device whose open failed is not open"
    );
}

#[test]
fn a_backend_with_no_rate_is_no_device() {
    let fake = Fake::with_rate(0);
    let mut device = device(&fake);
    assert_eq!(
        device.open(StreamRequest::native_stereo()),
        Err(AudioError::NoDevice)
    );
    assert_eq!(
        fake.read(|s| s.closes),
        Some(2),
        "the stream is closed again"
    );
}

#[test]
fn a_second_open_closes_the_first_stream() {
    let fake = Fake::with_rate(48_000);
    let mut device = device(&fake);
    assert!(device.open(StreamRequest::native_stereo()).is_ok());
    assert!(device.open(StreamRequest::native_stereo()).is_ok());
    assert_eq!(fake.read(|s| s.opens), Some(2));
    assert_eq!(fake.read(|s| s.closes), Some(2));
}

#[test]
fn a_start_that_fails_gives_the_error_and_drops_the_renderer() {
    let fake = Fake::with_rate(48_000);
    fake.edit(|s| s.start_fails = true);
    let mut device = device(&fake);
    assert!(device.open(StreamRequest::native_stereo()).is_ok());
    assert_eq!(device.start(Box::new(Half)), Err(AudioError::DeviceLost));
    assert_eq!(
        device.stop().err(),
        Some(AudioError::WrongState),
        "the device did not start"
    );
    fake.edit(|s| s.start_fails = false);
    assert_eq!(device.start(Box::new(Half)), Ok(()));
}

#[test]
fn the_status_counts_callbacks_and_frames_and_survives_stop() {
    let fake = Fake::with_rate(48_000);
    fake.edit(|s| s.start_fails = false);
    let mut device = device(&fake);
    assert!(device.open(stereo(0, 128)).is_ok());
    assert_eq!(device.start(Box::new(Half)), Ok(()));
    fake.tick(100);
    fake.tick(300);
    let status = device.status();
    assert_eq!(status.callbacks, 1 + 3);
    assert_eq!(status.frames_rendered, 400);
    assert_eq!(status.underruns, 0);
    let held = fake.read(|s| s.shared.clone()).flatten();
    if let Some(shared) = held {
        shared.with_slot_taken(|| shared.render(&mut [0.0; 8], 4));
    }
    assert_eq!(
        device.status().underruns,
        1,
        "a missed callback is an underrun"
    );
    assert!(device.stop().is_ok());
    assert_eq!(device.status().callbacks, 4);
    assert_eq!(device.status().frames_rendered, 400);
}

#[test]
fn a_lost_device_is_reported_and_stop_still_gives_the_renderer_back() {
    let fake = Fake::with_rate(48_000);
    fake.edit(|s| s.start_fails = false);
    let mut device = device(&fake);
    assert!(!device.status().lost, "nothing is lost before open");
    assert!(device.open(StreamRequest::native_stereo()).is_ok());
    assert_eq!(device.start(Box::new(Half)), Ok(()));
    fake.edit(|s| s.lost = true);
    assert!(device.status().lost);
    assert!(device.stop().is_ok());
    assert!(device.open(StreamRequest::native_stereo()).is_ok());
}

#[test]
fn the_shim_reports_what_the_build_did() {
    let built_in = env!("DUCK_MINIAUDIO_COMPILED") == "1";
    assert_eq!(crate::is_compiled_in(), built_in);
}

#[test]
fn a_device_without_the_header_reports_no_device() {
    if crate::is_compiled_in() {
        return;
    }
    let mut device = MiniaudioDevice::new();
    assert_eq!(
        device.open(StreamRequest::native_stereo()),
        Err(AudioError::NoDevice)
    );
}
