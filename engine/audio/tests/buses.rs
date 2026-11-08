//! Buses (card 5-6): a fixed tree of buses under the master, with gain and mute per bus, and the
//! stated quality bound of the linear resampler.

use audio::bus::{BusId, MAX_BUSES};
use audio::device::AudioRenderer;
use audio::format::{ChannelLayout, StreamFormat};
use audio::mixer::{Mixer, MixerError, PlayParams, RAMP_FRAMES};
use audio::sound::Sound;
use std::error::Error;
use std::f64::consts::TAU;
use std::sync::Arc;

type TestResult = Result<(), Box<dyn Error>>;

const RATE: u32 = 48_000;
const CENTRE: f32 = std::f32::consts::FRAC_1_SQRT_2;
const TOLERANCE: f32 = 1.0e-6;

fn format() -> StreamFormat {
    StreamFormat {
        sample_rate: RATE,
        layout: ChannelLayout::Stereo,
        max_block_frames: 4096,
    }
}

fn bus(index: u8) -> BusId {
    BusId::from_index(index)
}

fn constant(value: f32, frames: usize) -> Result<Arc<Sound>, Box<dyn Error>> {
    Ok(Arc::new(Sound::new(RATE, 1, vec![value; frames])?))
}

fn on(bus: BusId) -> PlayParams {
    PlayParams {
        bus,
        ..PlayParams::default()
    }
}

fn render(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
    let mut out = vec![f32::NAN; frames * 2];
    mixer.render(&mut out, u32::try_from(frames).unwrap_or(0));
    out
}

fn left(out: &[f32]) -> Vec<f32> {
    out.iter().step_by(2).copied().collect()
}

fn last_left(out: &[f32]) -> f32 {
    out.get(out.len().saturating_sub(2))
        .copied()
        .unwrap_or(f32::NAN)
}

fn close(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() <= TOLERANCE
}

/// A mixer with the master and two children of the master (buses 1 and 2).
fn two_children() -> Result<Mixer, MixerError> {
    Mixer::with_buses(format(), 8, &[BusId::MASTER, BusId::MASTER])
}

#[test]
fn a_mixer_without_extra_buses_has_the_master_only() -> Result<(), MixerError> {
    assert_eq!(Mixer::new(format(), 4)?.bus_count(), 1);
    assert_eq!(two_children()?.bus_count(), 3);
    assert_eq!(BusId::MASTER.index(), 0);
    Ok(())
}

#[test]
fn the_bus_table_is_checked_when_the_mixer_is_built() {
    let too_many = vec![BusId::MASTER; usize::from(MAX_BUSES)];
    assert_eq!(
        Mixer::with_buses(format(), 4, &too_many).err(),
        Some(MixerError::InvalidParameter)
    );
    let largest = vec![BusId::MASTER; usize::from(MAX_BUSES) - 1];
    assert!(Mixer::with_buses(format(), 4, &largest).is_ok());
    // A bus may only have a parent that comes before it in the table.
    for parents in [vec![bus(1)], vec![BusId::MASTER, bus(2)], vec![bus(7)]] {
        assert_eq!(
            Mixer::with_buses(format(), 4, &parents).err(),
            Some(MixerError::InvalidParameter),
            "{parents:?}"
        );
    }
    assert!(Mixer::with_buses(format(), 4, &[BusId::MASTER, bus(1), bus(1), bus(2)]).is_ok());
}

#[test]
fn a_voice_on_the_master_is_unchanged_by_the_bus_machinery() -> TestResult {
    let mut mixer = two_children()?;
    mixer.play(constant(1.0, 1000)?, PlayParams::default())?;
    let out = render(&mut mixer, 500);
    assert!(left(&out).iter().all(|s| close(*s, CENTRE)));
    Ok(())
}

#[test]
fn a_master_only_mixer_gives_the_bits_of_a_direct_sum() -> TestResult {
    // What the mixer did before buses existed: each frame is sample times the pan gain, and
    // two voices are added in slot order. The bus machinery must not change a single bit.
    let (pan_left, pan_right) = audio::mixer::pan_gains(0.3);
    let mut mixer = Mixer::new(format(), 4)?;
    let first = PlayParams {
        pan: 0.3,
        gain: 0.7,
        ..PlayParams::default()
    };
    mixer.play(constant(0.4, 600)?, first)?;
    mixer.play(constant(0.2, 600)?, PlayParams::default())?;
    let out = render(&mut mixer, 500);
    let centre = audio::mixer::pan_gains(0.0);
    let expected_left = 0.4_f32 * (0.7 * pan_left) + 0.2_f32 * centre.0;
    let expected_right = 0.4_f32 * (0.7 * pan_right) + 0.2_f32 * centre.1;
    for frame in out.chunks_exact(2) {
        assert_eq!(frame[0].to_bits(), expected_left.to_bits());
        assert_eq!(frame[1].to_bits(), expected_right.to_bits());
    }
    Ok(())
}

#[test]
fn a_voice_on_a_child_bus_reaches_the_master() -> TestResult {
    let mut mixer = two_children()?;
    mixer.play(constant(1.0, 1000)?, on(bus(1)))?;
    let out = render(&mut mixer, 500);
    assert!(left(&out).iter().all(|s| close(*s, CENTRE)));
    Ok(())
}

#[test]
fn bus_gains_multiply_down_the_tree() -> TestResult {
    let mut mixer = Mixer::with_buses(format(), 4, &[BusId::MASTER, bus(1)])?;
    mixer.play(constant(1.0, 5000)?, on(bus(2)))?;
    mixer.set_bus_gain(bus(2), 0.5)?;
    mixer.set_bus_gain(bus(1), 0.5)?;
    mixer.set_bus_gain(BusId::MASTER, 0.5)?;
    let out = render(&mut mixer, 1000);
    assert!(close(last_left(&out), CENTRE * 0.125));
    Ok(())
}

#[test]
fn two_buses_add_up_in_the_master() -> TestResult {
    let mut mixer = two_children()?;
    mixer.play(constant(0.25, 5000)?, on(bus(1)))?;
    mixer.play(constant(0.5, 5000)?, on(bus(2)))?;
    mixer.set_bus_gain(bus(1), 0.5)?;
    let out = render(&mut mixer, 1000);
    assert!(close(last_left(&out), CENTRE * (0.25 * 0.5 + 0.5)));
    Ok(())
}

#[test]
fn a_bus_gain_change_is_linear_over_the_ramp_and_never_jumps() -> TestResult {
    let mut mixer = two_children()?;
    mixer.play(constant(1.0, 5000)?, on(bus(1)))?;
    render(&mut mixer, 10);
    mixer.set_bus_gain(bus(1), 0.0)?;
    let out = left(&render(&mut mixer, 400));
    let ramp = usize::from(RAMP_FRAMES);
    for (i, sample) in out.iter().enumerate() {
        let expected = if i < ramp {
            CENTRE * (1.0 - f32::from(u16::try_from(i).unwrap_or(0)) / f32::from(RAMP_FRAMES))
        } else {
            0.0
        };
        assert!(
            (sample - expected).abs() <= 1.0e-5,
            "frame {i}: {sample} vs {expected}"
        );
    }
    Ok(())
}

#[test]
fn muting_fades_the_bus_out_and_unmuting_brings_back_its_own_gain() -> TestResult {
    let mut mixer = two_children()?;
    mixer.play(constant(1.0, 20_000)?, on(bus(1)))?;
    mixer.set_bus_gain(bus(1), 0.5)?;
    render(&mut mixer, 500);
    mixer.set_bus_muted(bus(1), true)?;
    let muted = render(&mut mixer, 500);
    assert!(
        left(&muted)
            .iter()
            .skip(usize::from(RAMP_FRAMES))
            .all(|s| close(*s, 0.0))
    );
    // The gain set while muted is kept for later.
    mixer.set_bus_gain(bus(1), 0.25)?;
    let still_muted = render(&mut mixer, 500);
    assert!(
        left(&still_muted)
            .iter()
            .skip(usize::from(RAMP_FRAMES))
            .all(|s| close(*s, 0.0))
    );
    mixer.set_bus_muted(bus(1), false)?;
    let back = render(&mut mixer, 500);
    assert!(close(last_left(&back), CENTRE * 0.25));
    Ok(())
}

#[test]
fn muting_one_bus_leaves_its_sibling_alone() -> TestResult {
    let mut mixer = two_children()?;
    mixer.play(constant(0.25, 20_000)?, on(bus(1)))?;
    mixer.play(constant(0.5, 20_000)?, on(bus(2)))?;
    mixer.set_bus_muted(bus(1), true)?;
    let out = render(&mut mixer, 1000);
    assert!(close(last_left(&out), CENTRE * 0.5));
    Ok(())
}

#[test]
fn muting_a_parent_silences_its_children() -> TestResult {
    let mut mixer = Mixer::with_buses(format(), 4, &[BusId::MASTER, bus(1)])?;
    mixer.play(constant(1.0, 20_000)?, on(bus(2)))?;
    mixer.set_bus_muted(bus(1), true)?;
    let out = render(&mut mixer, 1000);
    assert!(close(last_left(&out), 0.0));
    Ok(())
}

#[test]
fn the_master_gain_and_mute_apply_to_everything() -> TestResult {
    let mut mixer = two_children()?;
    mixer.play(constant(1.0, 20_000)?, on(bus(2)))?;
    mixer.set_bus_gain(BusId::MASTER, 0.5)?;
    assert!(close(last_left(&render(&mut mixer, 1000)), CENTRE * 0.5));
    mixer.set_bus_muted(BusId::MASTER, true)?;
    assert!(close(last_left(&render(&mut mixer, 1000)), 0.0));
    Ok(())
}

#[test]
fn a_voice_keeps_its_bus_and_ends_on_it() -> TestResult {
    let mut mixer = two_children()?;
    let handle = mixer.play(constant(1.0, 100)?, on(bus(2)))?;
    render(&mut mixer, 200);
    assert_eq!(mixer.pop_finished().map(|(h, _)| h), Some(handle));
    assert_eq!(mixer.active_voices(), 0);
    Ok(())
}

#[test]
fn bad_bus_arguments_are_refused_and_start_no_voice() -> TestResult {
    let mut mixer = two_children()?;
    assert_eq!(
        mixer.play(constant(1.0, 100)?, on(bus(3))).err(),
        Some(MixerError::InvalidParameter)
    );
    assert_eq!(mixer.active_voices(), 0);
    assert_eq!(
        mixer.set_bus_gain(bus(3), 0.5).err(),
        Some(MixerError::InvalidParameter)
    );
    assert_eq!(
        mixer.set_bus_muted(bus(200), true).err(),
        Some(MixerError::InvalidParameter)
    );
    for bad in [-0.1, f32::NAN, f32::INFINITY] {
        assert_eq!(
            mixer.set_bus_gain(bus(1), bad).err(),
            Some(MixerError::InvalidParameter)
        );
    }
    assert!(mixer.set_bus_gain(bus(1), 0.0).is_ok());
    Ok(())
}

fn change_midway(mixer: &mut Mixer) -> Result<(), MixerError> {
    mixer.set_bus_muted(bus(3), true)?;
    mixer.set_bus_gain(BusId::MASTER, 0.8)
}

/// Renders a scene in calls of the given sizes; the bus changes come before the third call.
fn render_split(chunks: &[usize]) -> Result<Vec<f32>, Box<dyn Error>> {
    let mut mixer = Mixer::with_buses(format(), 8, &[BusId::MASTER, bus(1), BusId::MASTER])?;
    mixer.play(constant(0.5, 2000)?, on(bus(2)))?;
    mixer.play(constant(0.25, 700)?, on(bus(3)))?;
    mixer.play(constant(0.125, 3000)?, PlayParams::default())?;
    mixer.set_bus_gain(bus(1), 0.5)?;
    let mut all = Vec::new();
    for (index, frames) in chunks.iter().enumerate() {
        if index == 2 {
            change_midway(&mut mixer)?;
        }
        all.extend(render(&mut mixer, *frames));
    }
    Ok(all)
}

#[test]
fn the_output_does_not_depend_on_how_the_frames_are_split() -> TestResult {
    // The commands come before the third call, which starts after 8 frames in both runs.
    let a = render_split(&[1, 7, 255, 256, 257, 1024, 1000])?;
    let b = render_split(&[1, 7, 2792])?;
    assert_eq!(a.len(), b.len());
    let first = a
        .iter()
        .zip(&b)
        .position(|(x, y)| x.to_bits() != y.to_bits());
    assert_eq!(
        first,
        None,
        "{:?} {:?}",
        first.map(|i| a[i]),
        first.map(|i| b[i])
    );
    Ok(())
}

#[test]
fn a_long_call_spans_several_internal_chunks_without_a_seam() -> TestResult {
    let mut mixer = two_children()?;
    mixer.play(constant(0.5, 5000)?, on(bus(1)))?;
    let out = left(&render(&mut mixer, 1500));
    assert!(out.iter().take(1500).all(|s| close(*s, CENTRE * 0.5)));
    Ok(())
}

#[test]
fn odd_empty_and_long_slices_are_handled() -> TestResult {
    let mut mixer = two_children()?;
    mixer.play(constant(1.0, 5000)?, on(bus(1)))?;
    // An odd number of samples: the last one is zeroed, not rendered.
    let mut odd = vec![f32::NAN; 599];
    mixer.render(&mut odd, 300);
    assert!(odd[..598].iter().all(|s| close(*s, CENTRE)));
    assert!(close(odd[598], 0.0));
    // No frames: nothing is touched.
    let mut untouched = vec![7.0_f32; 8];
    mixer.render(&mut untouched, 0);
    assert!(untouched.iter().all(|s| close(*s, 7.0)));
    // A slice longer than the frames: the tail is left alone.
    let mut long = vec![f32::NAN; 40];
    mixer.render(&mut long, 10);
    assert!(long[..20].iter().all(|s| close(*s, CENTRE)));
    assert!(long[20..].iter().all(|s| s.is_nan()));
    Ok(())
}

#[test]
fn a_slice_shorter_than_the_frames_is_still_zeroed_and_rendered_as_far_as_it_goes() -> TestResult {
    let mut mixer = two_children()?;
    mixer.play(constant(1.0, 5000)?, on(bus(1)))?;
    let mut out = vec![f32::NAN; 600];
    mixer.render(&mut out, 1000);
    assert!(out.iter().all(|s| close(*s, CENTRE)));
    Ok(())
}

/// Estimates the sine of a known frequency by correlation (exact when the samples cover whole
/// periods, which the callers arrange); returns the ratio of the signal power to the power of
/// what is left (the error), in decibels.
#[expect(clippy::cast_precision_loss, reason = "test indices are small")]
fn signal_to_error_db(samples: &[f32], hz: f64) -> f64 {
    let n = samples.len() as f64;
    let (mut sin_dot, mut cos_dot) = (0.0_f64, 0.0_f64);
    for (i, s) in samples.iter().enumerate() {
        let phase = TAU * hz * i as f64 / f64::from(RATE);
        sin_dot += f64::from(*s) * phase.sin();
        cos_dot += f64::from(*s) * phase.cos();
    }
    let (a, b) = (2.0 * sin_dot / n, 2.0 * cos_dot / n);
    let mut error = 0.0_f64;
    for (i, s) in samples.iter().enumerate() {
        let phase = TAU * hz * i as f64 / f64::from(RATE);
        let fit = a * phase.sin() + b * phase.cos();
        error += (f64::from(*s) - fit).powi(2);
    }
    let signal = f64::midpoint(a * a, b * b) * n;
    10.0 * (signal / error).log10()
}

/// A tone of `hz` at `rate`, played at `pitch` through the mixer; the output is a tone of
/// `hz * pitch * rate / RATE`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the sine is in [-1, 1]; only the rounding to f32 is lost"
)]
fn resampled_quality(hz: f64, rate: u32, pitch: f32) -> Result<f64, Box<dyn Error>> {
    let samples: Vec<f32> = (0..20_000_u32)
        .map(|i| (TAU * hz * f64::from(i) / f64::from(rate)).sin() as f32)
        .collect();
    let sound = Arc::new(Sound::new(rate, 1, samples)?);
    let mut mixer = Mixer::new(format(), 2)?;
    let params = PlayParams {
        pan: -1.0,
        pitch,
        ..PlayParams::default()
    };
    mixer.play(sound, params)?;
    render(&mut mixer, 500);
    let out = left(&render(&mut mixer, 4800));
    let output_hz = hz * f64::from(pitch) * f64::from(rate) / f64::from(RATE);
    // The first 500 frames were skipped, so the phase is not zero: the fit uses sine and cosine.
    Ok(signal_to_error_db(&out, output_hz))
}

#[test]
fn the_linear_resampler_keeps_a_low_tone_clean() -> TestResult {
    // 1 kHz at 48 kHz, pitch 1.1: the interpolation error of a linear resampler is
    // (w h)^2 / 8 of the amplitude, here 0.2%; measured 55.6 dB, bound 55 dB.
    let db = resampled_quality(1000.0, RATE, 1.1)?;
    assert!(db > 55.0, "{db} dB");
    Ok(())
}

#[test]
fn the_linear_resampler_is_weak_for_a_tone_near_a_fifth_of_the_rate() -> TestResult {
    // 8 kHz at 48 kHz, pitch 1.1: about 6 samples per period; the error is a few percent
    // (measured 26.3 dB). The bounds document the weakness of linear interpolation: above 25 dB,
    // and below 30 dB, so that a better resampler makes this test fail and be replaced.
    let db = resampled_quality(8000.0, RATE, 1.1)?;
    assert!(db > 25.0, "{db} dB");
    assert!(db < 30.0, "{db} dB");
    Ok(())
}
