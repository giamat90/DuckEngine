//! The contract suite of the device port: every adapter's own tests call [`device_contract`], and
//! an adapter is accepted when it passes unchanged (`audio-port-v0` section 8).

use crate::device::{AudioRenderer, IAudioDevice};
use crate::error::AudioError;
use crate::format::{
    ChannelLayout, DEFAULT_BLOCK_FRAMES, DEFAULT_SAMPLE_RATE, StreamFormat, StreamRequest,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

/// What a [`ProbeRenderer`] saw, readable from the test thread while a device thread runs it.
#[derive(Debug, Default)]
pub struct Probe {
    calls: AtomicU64,
    frames_total: AtomicU64,
    max_frames: AtomicU32,
    drops: AtomicU32,
    bad_length: AtomicBool,
}

impl Probe {
    /// A renderer for `format` that reports to this probe and writes 0.5 into every sample.
    #[must_use]
    pub fn renderer(self: &Arc<Self>, format: StreamFormat) -> ProbeRenderer {
        ProbeRenderer {
            probe: Arc::clone(self),
            channels: usize::from(format.layout.channels()),
        }
    }

    /// Callbacks seen.
    #[must_use]
    pub fn calls(&self) -> u64 {
        self.calls.load(Ordering::Acquire)
    }

    /// The sum of the `frames` of all callbacks.
    #[must_use]
    pub fn frames_total(&self) -> u64 {
        self.frames_total.load(Ordering::Acquire)
    }

    /// How many renderers of this probe were dropped.
    #[must_use]
    pub fn drops(&self) -> u32 {
        self.drops.load(Ordering::Acquire)
    }

    /// The largest `frames` seen.
    #[must_use]
    pub fn max_frames(&self) -> u32 {
        self.max_frames.load(Ordering::Acquire)
    }

    /// True when a callback got a slice whose length was not `frames * channels`.
    #[must_use]
    pub fn bad_length(&self) -> bool {
        self.bad_length.load(Ordering::Acquire)
    }
}

/// The renderer of a [`Probe`].
#[derive(Debug)]
pub struct ProbeRenderer {
    probe: Arc<Probe>,
    channels: usize,
}

impl AudioRenderer for ProbeRenderer {
    fn render(&mut self, out: &mut [f32], frames: u32) {
        if out.len() != frames as usize * self.channels {
            self.probe.bad_length.store(true, Ordering::Release);
        }
        self.probe.max_frames.fetch_max(frames, Ordering::AcqRel);
        out.fill(0.5);
        self.probe
            .frames_total
            .fetch_add(u64::from(frames), Ordering::AcqRel);
        self.probe.calls.fetch_add(1, Ordering::AcqRel);
    }
}

impl Drop for ProbeRenderer {
    fn drop(&mut self) {
        self.probe.drops.fetch_add(1, Ordering::AcqRel);
    }
}

fn check_closed_device<D: IAudioDevice>(device: &mut D) {
    assert!(
        device.caps().max_channels >= 1,
        "the device plays no channel"
    );
    let before = Arc::new(Probe::default());
    let format = StreamFormat {
        sample_rate: DEFAULT_SAMPLE_RATE,
        layout: ChannelLayout::Stereo,
        max_block_frames: DEFAULT_BLOCK_FRAMES,
    };
    assert_eq!(
        device.start(Box::new(before.renderer(format))),
        Err(AudioError::WrongState),
        "start before open must be refused"
    );
    assert_eq!(
        device.stop().err(),
        Some(AudioError::WrongState),
        "stop before start must be refused"
    );
    let too_slow = StreamRequest {
        sample_rate: 1,
        ..StreamRequest::native_stereo()
    };
    assert_eq!(
        device.open(too_slow),
        Err(AudioError::InvalidRequest),
        "a sample rate outside the port's limits must be refused"
    );
}

/// Requests the device must answer with a format that matches (the layout as asked, a rate and a
/// block size above zero, no more channels than `caps` says) or refuse with `FormatRefused`
/// when the layout has more channels than the device plays. A rate or block size may differ from
/// the request: both are hints to a real device.
fn check_open_table<D: IAudioDevice>(device: &mut D) {
    let max_channels = device.caps().max_channels;
    let table = [
        (0, ChannelLayout::Mono, 0),
        (44_100, ChannelLayout::Mono, 256),
        (48_000, ChannelLayout::Stereo, 1024),
        (0, ChannelLayout::Surround51, 0),
    ];
    for (sample_rate, layout, block_hint_frames) in table {
        let request = StreamRequest {
            sample_rate,
            layout,
            block_hint_frames,
        };
        match device.open(request) {
            Ok(format) => {
                assert_eq!(format.layout, layout, "the layout differs from the request");
                assert!(format.sample_rate > 0 && format.max_block_frames > 0);
                assert!(
                    format.layout.channels() <= max_channels,
                    "the format has more channels than caps says"
                );
            }
            Err(error) => {
                assert!(
                    error == AudioError::FormatRefused && layout.channels() > max_channels,
                    "open refused a layout the device plays: {error}"
                );
            }
        }
    }
}

/// Checks the device port. `pump` makes the device call the renderer at least once and may
/// block briefly (an offline device renders, a real device sleeps); it is also called after `stop`,
/// where it must change nothing. `expect_callbacks` is false for a device that never calls the
/// renderer (Null). After `stop` returns, no callback is running or will run, and the status
/// counts equal what the renderer saw.
///
/// A test of a real device checks that the machine has one before calling this and reports "not
/// run" otherwise; the suite panics when `open` fails.
///
/// # Panics
/// When the device breaks the contract (this is test code).
pub fn device_contract<D: IAudioDevice>(
    device: &mut D,
    expect_callbacks: bool,
    mut pump: impl FnMut(&mut D),
) {
    check_closed_device(device);
    check_open_table(device);
    let Ok(format) = device.open(StreamRequest::native_stereo()) else {
        panic!("open of the native stereo format failed")
    };

    let probe = Arc::new(Probe::default());
    assert_eq!(device.start(Box::new(probe.renderer(format))), Ok(()));
    assert_eq!(probe.drops(), 0, "the device dropped a started renderer");
    let second = Arc::new(Probe::default());
    assert_eq!(
        device.start(Box::new(second.renderer(format))),
        Err(AudioError::WrongState),
        "a second start must be refused"
    );
    assert_eq!(second.drops(), 1, "a refused start must drop its renderer");
    assert_eq!(
        device.open(StreamRequest::native_stereo()),
        Err(AudioError::WrongState),
        "open while started must be refused"
    );

    pump(device);
    check_callbacks(device, &probe, format, expect_callbacks);
    check_stop(device, &probe, &mut pump);

    assert!(
        device.open(StreamRequest::native_stereo()).is_ok(),
        "open after stop failed"
    );
    let again = Arc::new(Probe::default());
    assert_eq!(device.start(Box::new(again.renderer(format))), Ok(()));
    assert!(device.stop().is_ok(), "a restarted device failed to stop");
}

fn check_stop<D: IAudioDevice>(device: &mut D, probe: &Probe, pump: &mut impl FnMut(&mut D)) {
    let Ok(renderer) = device.stop() else {
        panic!("stop of a started device failed")
    };
    assert_eq!(
        probe.drops(),
        0,
        "stop did not give back the renderer it was started with"
    );
    assert_eq!(
        device.stop().err(),
        Some(AudioError::WrongState),
        "a second stop must be refused"
    );
    let calls_at_stop = probe.calls();
    pump(device);
    assert_eq!(
        probe.calls(),
        calls_at_stop,
        "the renderer was called after stop"
    );
    let status = device.status();
    assert_eq!(status.callbacks, probe.calls(), "status callbacks differ");
    assert_eq!(
        status.frames_rendered,
        probe.frames_total(),
        "status frames differ"
    );
    drop(renderer);
    assert_eq!(
        probe.drops(),
        1,
        "the given-back renderer was not the probe"
    );
}

fn check_callbacks<D: IAudioDevice>(
    device: &D,
    probe: &Probe,
    format: StreamFormat,
    expect_callbacks: bool,
) {
    assert!(
        !probe.bad_length(),
        "a callback got a slice of the wrong length"
    );
    assert!(
        probe.max_frames() <= format.max_block_frames,
        "a callback asked for more frames than max_block_frames"
    );
    let status = device.status();
    if expect_callbacks {
        assert!(probe.calls() > 0, "the renderer was never called");
        assert!(status.callbacks > 0, "the status counts no callback");
        assert!(status.frames_rendered > 0, "the status counts no frame");
    } else {
        assert_eq!(probe.calls(), 0, "the renderer was called");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::faulty::{Fault, Faulty};
    use crate::null::NullDevice;
    use crate::offline::OfflineDevice;

    fn run(fault: Fault) {
        let mut device = Faulty::new(fault);
        device_contract(&mut device, true, |device| {
            let mut out = vec![0.0; 2 * 100];
            let _ = device.inner.render_into(&mut out);
        });
    }

    #[test]
    fn the_offline_device_without_a_fault_passes() {
        let mut device = OfflineDevice::new();
        device_contract(&mut device, true, |device| {
            let mut out = vec![0.0; 2 * 100];
            let _ = device.render_into(&mut out);
        });
    }

    #[test]
    #[should_panic(expected = "start before open must be refused")]
    fn a_device_that_starts_before_open_fails() {
        run(Fault::StartsBeforeOpen);
    }

    #[test]
    #[should_panic(expected = "a second start must be refused")]
    fn a_device_that_accepts_a_second_start_fails() {
        run(Fault::SecondStartAccepted);
    }

    #[test]
    #[should_panic(expected = "a refused start must drop its renderer")]
    fn a_device_that_keeps_the_renderer_of_a_refused_start_fails() {
        run(Fault::RefusedStartKeepsRenderer);
    }

    #[test]
    #[should_panic(expected = "open while started must be refused")]
    fn a_device_that_opens_while_started_fails() {
        run(Fault::OpenWhileStartedAccepted);
    }

    #[test]
    #[should_panic(expected = "a second stop must be refused")]
    fn a_device_that_accepts_a_second_stop_fails() {
        run(Fault::SecondStopAccepted);
    }

    #[test]
    #[should_panic(expected = "stop did not give back the renderer")]
    fn a_device_that_drops_the_renderer_and_returns_another_fails() {
        run(Fault::StopGivesBackAnotherRenderer);
    }

    #[test]
    #[should_panic(expected = "the status counts no callback")]
    fn a_device_that_counts_nothing_fails() {
        run(Fault::CountsNothing);
    }

    #[test]
    #[should_panic(expected = "the layout differs from the request")]
    fn a_device_that_ignores_the_layout_fails() {
        run(Fault::IgnoresTheLayout);
    }

    #[test]
    #[should_panic(expected = "the renderer was never called")]
    fn a_device_that_never_calls_the_renderer_fails_when_calls_are_expected() {
        device_contract(&mut NullDevice::new(), true, |_| {});
    }

    #[test]
    fn the_probe_notices_a_slice_of_the_wrong_length() {
        let format = StreamFormat {
            sample_rate: DEFAULT_SAMPLE_RATE,
            layout: ChannelLayout::Stereo,
            max_block_frames: DEFAULT_BLOCK_FRAMES,
        };
        let probe = Arc::new(Probe::default());
        let mut renderer = probe.renderer(format);
        renderer.render(&mut [0.0; 4], 2);
        assert!(!probe.bad_length());
        renderer.render(&mut [0.0; 3], 2);
        assert!(probe.bad_length());
        assert_eq!(probe.calls(), 2);
        assert_eq!(probe.max_frames(), 2);
        assert_eq!(probe.frames_total(), 4);
        drop(renderer);
        assert_eq!(probe.drops(), 1);
    }
}
