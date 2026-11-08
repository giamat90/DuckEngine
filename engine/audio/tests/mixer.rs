//! Offline fidelity tests of the mixer (card 5-3): the mixer is driven by plain calls and its
//! output is compared with closed forms.

use audio::device::{AudioRenderer, IAudioDevice};
use audio::format::{ChannelLayout, StreamFormat, StreamRequest};
use audio::mixer::{
    MAX_VOICES, Mixer, MixerError, PlayParams, RAMP_FRAMES, VoiceHandle, pan_gains,
};
use audio::offline::OfflineDevice;
use audio::sound::Sound;
use std::error::Error;
use std::f32::consts::{FRAC_1_SQRT_2, TAU};
use std::sync::Arc;

type TestResult = Result<(), Box<dyn Error>>;

const RATE: u32 = 48_000;
const TOLERANCE: f32 = 1.0e-6;

fn format() -> StreamFormat {
    StreamFormat {
        sample_rate: RATE,
        layout: ChannelLayout::Stereo,
        max_block_frames: 4096,
    }
}

fn mixer(voices: u32) -> Result<Mixer, MixerError> {
    Mixer::new(format(), voices)
}

fn mono(rate: u32, samples: Vec<f32>) -> Result<Arc<Sound>, Box<dyn Error>> {
    Ok(Arc::new(Sound::new(rate, 1, samples)?))
}

fn constant(value: f32, frames: usize) -> Result<Arc<Sound>, Box<dyn Error>> {
    mono(RATE, vec![value; frames])
}

#[expect(clippy::cast_precision_loss, reason = "test sample indices are small")]
fn ramp_sound(frames: usize) -> Result<Arc<Sound>, Box<dyn Error>> {
    mono(RATE, (0..frames).map(|i| i as f32).collect())
}

/// A sine of `hz` sampled at `rate`.
#[expect(clippy::cast_precision_loss, reason = "test sample indices are small")]
fn sine(hz: f32, rate: u32, frames: usize) -> Result<Arc<Sound>, Box<dyn Error>> {
    let samples = (0..frames)
        .map(|i| (TAU * hz * i as f32 / rate as f32).sin())
        .collect();
    mono(rate, samples)
}

fn render(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
    let mut out = vec![f32::NAN; frames * 2];
    mixer.render(&mut out, u32::try_from(frames).unwrap_or(0));
    out
}

fn left(out: &[f32]) -> Vec<f32> {
    out.iter().step_by(2).copied().collect()
}

fn right(out: &[f32]) -> Vec<f32> {
    out.iter().skip(1).step_by(2).copied().collect()
}

fn hard_left() -> PlayParams {
    PlayParams {
        pan: -1.0,
        ..PlayParams::default()
    }
}

fn assert_close(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        assert!(
            (a - e).abs() <= TOLERANCE,
            "frame {i}: {a} differs from {e}"
        );
    }
}

#[test]
fn only_a_stereo_format_is_accepted() {
    let mono_format = StreamFormat {
        layout: ChannelLayout::Mono,
        ..format()
    };
    assert_eq!(
        Mixer::new(mono_format, 4).err(),
        Some(MixerError::UnsupportedFormat)
    );
    assert!(mixer(4).is_ok());
}

#[test]
fn the_voice_count_must_be_between_one_and_the_maximum() {
    assert_eq!(mixer(0).err(), Some(MixerError::InvalidParameter));
    assert_eq!(
        mixer(MAX_VOICES + 1).err(),
        Some(MixerError::InvalidParameter)
    );
    assert!(mixer(MAX_VOICES).is_ok());
}

#[test]
fn a_mixer_without_voices_writes_silence_over_the_whole_slice() -> TestResult {
    let mut mixer = mixer(4)?;
    let out = render(&mut mixer, 100);
    assert!(out.iter().all(|s| s.to_bits() == 0.0_f32.to_bits()));
    Ok(())
}

#[test]
fn a_render_call_leaves_the_samples_after_the_requested_frames_alone() -> TestResult {
    let mut mixer = mixer(4)?;
    let mut out = vec![7.0; 10];
    mixer.render(&mut out, 3);
    assert_eq!(&out[..6], &[0.0; 6]);
    assert_eq!(&out[6..], &[7.0; 4]);
    Ok(())
}

#[test]
fn the_pan_law_is_constant_power_with_minus_three_db_at_the_centre() {
    let (l, r) = pan_gains(0.0);
    assert!((l - FRAC_1_SQRT_2).abs() < TOLERANCE && (r - FRAC_1_SQRT_2).abs() < TOLERANCE);
    let (l, r) = pan_gains(-1.0);
    assert!((l - 1.0).abs() < TOLERANCE && r.abs() < TOLERANCE);
    let (l, r) = pan_gains(1.0);
    assert!(l.abs() < TOLERANCE && (r - 1.0).abs() < TOLERANCE);
    for step in -10..=10_i8 {
        let (l, r) = pan_gains(f32::from(step) / 10.0);
        assert!((l * l + r * r - 1.0).abs() < TOLERANCE, "pan step {step}");
    }
}

#[test]
fn a_centred_voice_plays_at_the_centre_gain_on_both_channels() -> TestResult {
    let mut mixer = mixer(4)?;
    mixer.play(constant(1.0, 64)?, PlayParams::default())?;
    let out = render(&mut mixer, 32);
    assert_close(&left(&out), &[FRAC_1_SQRT_2; 32]);
    assert_close(&right(&out), &[FRAC_1_SQRT_2; 32]);
    Ok(())
}

#[test]
fn a_hard_left_voice_writes_the_left_channel_only() -> TestResult {
    let mut mixer = mixer(4)?;
    mixer.play(constant(0.5, 64)?, hard_left())?;
    let out = render(&mut mixer, 16);
    assert_close(&left(&out), &[0.5; 16]);
    assert_close(&right(&out), &[0.0; 16]);
    Ok(())
}

#[test]
fn the_gain_scales_the_voice() -> TestResult {
    let mut mixer = mixer(4)?;
    let params = PlayParams {
        gain: 0.25,
        ..hard_left()
    };
    mixer.play(constant(1.0, 64)?, params)?;
    assert_close(&left(&render(&mut mixer, 8)), &[0.25; 8]);
    Ok(())
}

#[test]
fn a_sine_voice_at_the_engine_rate_equals_the_analytic_sine() -> TestResult {
    let mut mixer = mixer(4)?;
    let sound = sine(440.0, RATE, 2000)?;
    let expected = sound.samples().to_vec();
    mixer.play(sound, hard_left())?;
    assert_close(&left(&render(&mut mixer, 2000)), &expected);
    Ok(())
}

#[test]
fn two_voices_add_up() -> TestResult {
    let mut mixer = mixer(4)?;
    mixer.play(constant(0.25, 64)?, hard_left())?;
    mixer.play(constant(0.5, 64)?, hard_left())?;
    assert_close(&left(&render(&mut mixer, 8)), &[0.75; 8]);
    Ok(())
}

#[test]
fn a_stereo_source_keeps_its_two_channels_apart() -> TestResult {
    let mut mixer = mixer(4)?;
    let sound = Arc::new(Sound::new(RATE, 2, vec![1.0, 0.5, 1.0, 0.5])?);
    mixer.play(sound, PlayParams::default())?;
    let out = render(&mut mixer, 2);
    assert_close(&left(&out), &[FRAC_1_SQRT_2; 2]);
    assert_close(&right(&out), &[0.5 * FRAC_1_SQRT_2; 2]);
    Ok(())
}

#[test]
fn a_stereo_source_plays_every_frame_and_interpolates_both_channels() -> TestResult {
    let mut mixer = mixer(4)?;
    let sound = Arc::new(Sound::new(RATE, 2, vec![1.0, 10.0, 2.0, 20.0, 3.0, 30.0])?);
    let params = PlayParams {
        pitch: 0.5,
        ..PlayParams::default()
    };
    mixer.play(sound, params)?;
    let out = render(&mut mixer, 5);
    let centre = |values: &[f32]| values.iter().map(|v| v * FRAC_1_SQRT_2).collect::<Vec<_>>();
    assert_close(&left(&out), &centre(&[1.0, 1.5, 2.0, 2.5, 3.0]));
    assert_close(&right(&out), &centre(&[10.0, 15.0, 20.0, 25.0, 30.0]));
    Ok(())
}

#[test]
fn a_looping_voice_interpolates_across_the_loop_point() -> TestResult {
    let mut mixer = mixer(4)?;
    let params = PlayParams {
        pitch: 0.5,
        looping: true,
        ..hard_left()
    };
    mixer.play(mono(RATE, vec![1.0, 2.0, 3.0])?, params)?;
    let out = left(&render(&mut mixer, 8));
    assert_close(&out, &[1.0, 1.5, 2.0, 2.5, 3.0, 2.0, 1.0, 1.5]);
    Ok(())
}

#[test]
fn a_gain_change_reaches_both_channels_of_a_centred_voice() -> TestResult {
    let mut mixer = mixer(4)?;
    let handle = mixer.play(constant(1.0, 2000)?, PlayParams::default())?;
    mixer.set_gain(handle, 2.0)?;
    let out = render(&mut mixer, 300);
    let target = 2.0 * FRAC_1_SQRT_2;
    assert!((left(&out)[299] - target).abs() < TOLERANCE);
    assert!((right(&out)[299] - target).abs() < TOLERANCE);
    Ok(())
}

#[test]
fn a_zero_sample_rate_is_refused() {
    let format = StreamFormat {
        sample_rate: 0,
        ..format()
    };
    assert_eq!(
        Mixer::new(format, 4).err(),
        Some(MixerError::InvalidParameter)
    );
}

#[test]
fn a_slice_shorter_than_the_frames_is_zeroed_and_the_frames_that_fit_are_rendered() -> TestResult {
    let mut mixer = mixer(4)?;
    mixer.play(constant(1.0, 64)?, hard_left())?;
    let mut out = vec![9.0; 6];
    mixer.render(&mut out, 100);
    assert_eq!(out, vec![1.0, 0.0, 1.0, 0.0, 1.0, 0.0]);
    let mut odd = vec![9.0; 5];
    mixer.render(&mut odd, 100);
    assert_eq!(odd[..4], [1.0, 0.0, 1.0, 0.0]);
    assert!(odd[4].abs() < TOLERANCE, "the odd sample is zeroed");
    Ok(())
}

#[test]
fn the_last_frame_of_a_one_shot_fades_towards_silence_when_slowed_down() -> TestResult {
    let mut mixer = mixer(4)?;
    let params = PlayParams {
        pitch: 0.5,
        ..hard_left()
    };
    mixer.play(mono(RATE, vec![1.0, 1.0])?, params)?;
    assert_close(&left(&render(&mut mixer, 5)), &[1.0, 1.0, 1.0, 0.5, 0.0]);
    Ok(())
}

#[test]
fn a_one_frame_loop_repeats_forever() -> TestResult {
    let mut mixer = mixer(4)?;
    let params = PlayParams {
        looping: true,
        ..hard_left()
    };
    mixer.play(mono(RATE, vec![0.75])?, params)?;
    assert_close(&left(&render(&mut mixer, 10)), &[0.75; 10]);
    assert_eq!(mixer.active_voices(), 1);
    Ok(())
}

#[test]
fn a_step_longer_than_the_loop_wraps_more_than_once() -> TestResult {
    let mut mixer = mixer(4)?;
    let params = PlayParams {
        pitch: 16.0,
        looping: true,
        ..hard_left()
    };
    mixer.play(mono(RATE, vec![1.0, 2.0, 3.0])?, params)?;
    // 16 frames per output frame on a loop of 3: positions 0, 1, 2, 0, 1, 2 (16 mod 3 = 1).
    assert_close(
        &left(&render(&mut mixer, 6)),
        &[1.0, 2.0, 3.0, 1.0, 2.0, 3.0],
    );
    Ok(())
}

#[test]
fn a_pitch_change_keeps_the_fractional_position() -> TestResult {
    let mut mixer = mixer(4)?;
    let params = PlayParams {
        pitch: 0.5,
        ..hard_left()
    };
    let handle = mixer.play(ramp_sound(64)?, params)?;
    assert_close(&left(&render(&mut mixer, 3)), &[0.0, 0.5, 1.0]);
    mixer.set_pitch(handle, 1.0)?;
    assert_close(&left(&render(&mut mixer, 2)), &[1.5, 2.5]);
    Ok(())
}

#[test]
fn a_second_stop_does_not_lengthen_the_fade() -> TestResult {
    let mut mixer = mixer(4)?;
    let params = PlayParams {
        looping: true,
        ..hard_left()
    };
    let handle = mixer.play(constant(1.0, 16)?, params)?;
    mixer.stop(handle)?;
    let _ = render(&mut mixer, 100);
    mixer.stop(handle)?;
    let _ = render(&mut mixer, 155);
    assert_eq!(mixer.active_voices(), 1, "one frame of the fade is left");
    let _ = render(&mut mixer, 1);
    assert_eq!(mixer.active_voices(), 0, "the fade ended on schedule");
    Ok(())
}

#[test]
fn a_stopping_voice_keeps_fading_when_its_gain_is_set() -> TestResult {
    let mut mixer = mixer(4)?;
    let params = PlayParams {
        looping: true,
        ..hard_left()
    };
    let handle = mixer.play(constant(1.0, 16)?, params)?;
    mixer.stop(handle)?;
    mixer.set_gain(handle, 1.0)?;
    let out = left(&render(&mut mixer, 300));
    assert!(out[299].abs() < TOLERANCE);
    assert_eq!(mixer.active_voices(), 0);
    Ok(())
}

#[test]
fn the_stop_fade_is_linear_and_lasts_exactly_the_ramp() -> TestResult {
    let mut mixer = mixer(4)?;
    let params = PlayParams {
        looping: true,
        ..hard_left()
    };
    let handle = mixer.play(constant(1.0, 16)?, params)?;
    mixer.stop(handle)?;
    let ramp = usize::from(RAMP_FRAMES);
    let out = left(&render(&mut mixer, ramp - 1));
    assert_eq!(mixer.active_voices(), 1);
    #[expect(clippy::cast_precision_loss, reason = "frame indices are small")]
    let expected: Vec<f32> = (0..ramp - 1).map(|i| 1.0 - i as f32 / 256.0).collect();
    assert_close(&out, &expected);
    let _ = render(&mut mixer, 1);
    assert_eq!(mixer.active_voices(), 0);
    Ok(())
}

#[test]
fn bad_values_not_a_number_are_refused_by_every_setter() -> TestResult {
    let mut mixer = mixer(4)?;
    let handle = mixer.play(constant(1.0, 8)?, PlayParams::default())?;
    assert_eq!(
        mixer.set_gain(handle, f32::NAN),
        Err(MixerError::InvalidParameter)
    );
    assert_eq!(
        mixer.set_pan(handle, f32::NAN),
        Err(MixerError::InvalidParameter)
    );
    assert_eq!(
        mixer.set_pitch(handle, f32::NAN),
        Err(MixerError::InvalidParameter)
    );
    Ok(())
}

#[test]
fn a_one_shot_voice_ends_and_hands_back_its_sound() -> TestResult {
    let mut mixer = mixer(4)?;
    let sound = constant(1.0, 4)?;
    let handle = mixer.play(Arc::clone(&sound), hard_left())?;
    assert_eq!(mixer.active_voices(), 1);
    let out = render(&mut mixer, 8);
    assert_close(&left(&out), &[1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(mixer.active_voices(), 0);
    let Some((done, back)) = mixer.pop_finished() else {
        return Err("no finished voice".into());
    };
    assert_eq!(done, handle);
    assert!(Arc::ptr_eq(&back, &sound));
    assert!(mixer.pop_finished().is_none());
    Ok(())
}

#[test]
fn a_looping_voice_repeats_at_the_loop_point() -> TestResult {
    let mut mixer = mixer(4)?;
    let sound = mono(RATE, vec![1.0, 2.0, 3.0])?;
    let params = PlayParams {
        looping: true,
        ..hard_left()
    };
    mixer.play(sound, params)?;
    let out = render(&mut mixer, 8);
    assert_close(&left(&out), &[1.0, 2.0, 3.0, 1.0, 2.0, 3.0, 1.0, 2.0]);
    assert_eq!(mixer.active_voices(), 1);
    Ok(())
}

#[test]
fn pitch_two_plays_the_sound_twice_as_fast() -> TestResult {
    let mut mixer = mixer(4)?;
    let params = PlayParams {
        pitch: 2.0,
        ..hard_left()
    };
    mixer.play(ramp_sound(64)?, params)?;
    assert_close(&left(&render(&mut mixer, 4)), &[0.0, 2.0, 4.0, 6.0]);
    Ok(())
}

#[test]
fn pitch_one_half_interpolates_linearly_between_samples() -> TestResult {
    let mut mixer = mixer(4)?;
    let params = PlayParams {
        pitch: 0.5,
        ..hard_left()
    };
    mixer.play(ramp_sound(64)?, params)?;
    assert_close(&left(&render(&mut mixer, 4)), &[0.0, 0.5, 1.0, 1.5]);
    Ok(())
}

#[test]
fn a_sound_of_a_lower_rate_is_resampled_to_the_engine_rate() -> TestResult {
    let mut mixer = mixer(4)?;
    let sound = mono(RATE / 2, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0])?;
    mixer.play(sound, hard_left())?;
    assert_close(&left(&render(&mut mixer, 4)), &[0.0, 0.5, 1.0, 1.5]);
    Ok(())
}

#[test]
fn a_resampled_sine_keeps_its_frequency_within_the_interpolation_error() -> TestResult {
    let mut mixer = mixer(4)?;
    let sound = sine(440.0, 24_000, 2400)?;
    mixer.play(sound, hard_left())?;
    let out = left(&render(&mut mixer, 4000));
    // Linear interpolation error of a sine is at most (w*h)^2/8, with w*h = 2*pi*440/24000 = 0.115
    // radians per source sample: 1.65e-3.
    for (i, sample) in out.iter().enumerate().take(4000) {
        #[expect(clippy::cast_precision_loss, reason = "test sample indices are small")]
        let exact = (TAU * 440.0 * i as f32 / RATE as f32).sin();
        assert!((sample - exact).abs() < 1.7e-3, "frame {i}");
    }
    Ok(())
}

#[test]
fn a_gain_change_moves_linearly_over_the_ramp_and_never_jumps() -> TestResult {
    let mut mixer = mixer(4)?;
    let handle = mixer.play(constant(1.0, 2000)?, hard_left())?;
    let _ = render(&mut mixer, 10);
    mixer.set_gain(handle, 0.0)?;
    let out = left(&render(&mut mixer, 400));
    let ramp = usize::from(RAMP_FRAMES);
    assert!((out[0] - 1.0).abs() < TOLERANCE, "starts at the old gain");
    assert!(
        (out[ramp / 2] - 0.5).abs() < 1.0e-4,
        "halfway at half the ramp"
    );
    assert!(out[ramp].abs() < TOLERANCE, "ends at the target");
    assert!(out[ramp..].iter().all(|s| s.abs() < TOLERANCE));
    for pair in out.windows(2) {
        assert!((pair[0] - pair[1]).abs() < 2.0 / f32::from(RAMP_FRAMES));
    }
    Ok(())
}

#[test]
fn a_pan_change_ramps_both_channels() -> TestResult {
    let mut mixer = mixer(4)?;
    let handle = mixer.play(constant(1.0, 2000)?, hard_left())?;
    let _ = render(&mut mixer, 1);
    mixer.set_pan(handle, 1.0)?;
    let out = render(&mut mixer, 400);
    let ramp = usize::from(RAMP_FRAMES);
    assert!(left(&out)[ramp + 5].abs() < TOLERANCE);
    assert!((right(&out)[ramp + 5] - 1.0).abs() < TOLERANCE);
    assert!(right(&out)[0].abs() < TOLERANCE);
    Ok(())
}

#[test]
fn a_pitch_change_takes_effect_at_once() -> TestResult {
    let mut mixer = mixer(4)?;
    let handle = mixer.play(ramp_sound(64)?, hard_left())?;
    assert_close(&left(&render(&mut mixer, 2)), &[0.0, 1.0]);
    mixer.set_pitch(handle, 2.0)?;
    assert_close(&left(&render(&mut mixer, 2)), &[2.0, 4.0]);
    Ok(())
}

#[test]
fn stop_fades_the_voice_out_and_then_finishes_it() -> TestResult {
    let mut mixer = mixer(4)?;
    let params = PlayParams {
        looping: true,
        ..hard_left()
    };
    let handle = mixer.play(constant(1.0, 16)?, params)?;
    let _ = render(&mut mixer, 5);
    mixer.stop(handle)?;
    let out = left(&render(&mut mixer, 400));
    assert!(out[0] > 0.99);
    assert!(out[300].abs() < TOLERANCE, "silent after the fade");
    assert_eq!(mixer.active_voices(), 0);
    assert_eq!(mixer.pop_finished().map(|(h, _)| h), Some(handle));
    Ok(())
}

#[test]
fn the_output_does_not_depend_on_how_the_frames_are_split() -> TestResult {
    const TOTAL: usize = 3000;
    let build = || -> Result<Mixer, Box<dyn Error>> {
        let mut mixer = mixer(8)?;
        let looping = PlayParams {
            gain: 0.6,
            pan: 0.3,
            pitch: 1.37,
            looping: true,
        };
        let handle = mixer.play(sine(330.0, 44_100, 1000)?, looping)?;
        mixer.set_gain(handle, 0.2)?;
        mixer.set_pan(handle, -0.7)?;
        mixer.play(ramp_sound(900)?, PlayParams::default())?;
        let stereo = Arc::new(Sound::new(
            32_000,
            2,
            vec![0.5, -0.25, 0.1, 0.9, -0.4, 0.3],
        )?);
        mixer.play(
            stereo,
            PlayParams {
                pitch: 0.7,
                looping: true,
                ..PlayParams::default()
            },
        )?;
        Ok(mixer)
    };
    let mut whole = build()?;
    let reference = render(&mut whole, TOTAL);
    for chunk in [1_usize, 7, 255, 256, 257, 1000] {
        let mut mixer = build()?;
        let mut pieces = Vec::new();
        let mut done = 0;
        while done < TOTAL {
            let frames = chunk.min(TOTAL - done);
            pieces.extend(render(&mut mixer, frames));
            done += frames;
        }
        let same = pieces
            .iter()
            .zip(&reference)
            .all(|(a, b)| a.to_bits() == b.to_bits());
        assert!(same, "chunks of {chunk} frames differ from one call");
    }
    Ok(())
}

#[test]
fn the_same_commands_give_the_same_bits_twice() -> TestResult {
    let run = || -> Result<Vec<f32>, Box<dyn Error>> {
        let mut mixer = mixer(8)?;
        mixer.play(
            sine(500.0, RATE, 700)?,
            PlayParams {
                pitch: 1.1,
                ..PlayParams::default()
            },
        )?;
        Ok(render(&mut mixer, 1000))
    };
    let first: Vec<u32> = run()?.iter().map(|s| s.to_bits()).collect();
    let second: Vec<u32> = run()?.iter().map(|s| s.to_bits()).collect();
    assert_eq!(first, second);
    Ok(())
}

#[test]
fn a_full_mixer_refuses_play_until_the_finished_sound_is_taken() -> TestResult {
    let mut mixer = mixer(2)?;
    mixer.play(constant(1.0, 4)?, hard_left())?;
    mixer.play(constant(1.0, 400)?, hard_left())?;
    assert_eq!(
        mixer.play(constant(1.0, 4)?, hard_left()).err(),
        Some(MixerError::VoicesExhausted)
    );
    let _ = render(&mut mixer, 10);
    assert_eq!(
        mixer.play(constant(1.0, 4)?, hard_left()).err(),
        Some(MixerError::VoicesExhausted),
        "the finished voice still holds its sound until it is taken"
    );
    assert!(mixer.pop_finished().is_some());
    assert!(mixer.play(constant(1.0, 4)?, hard_left()).is_ok());
    Ok(())
}

#[test]
fn a_handle_goes_stale_when_its_voice_ends() -> TestResult {
    let mut mixer = mixer(1)?;
    let old: VoiceHandle = mixer.play(constant(1.0, 2)?, hard_left())?;
    let _ = render(&mut mixer, 4);
    assert_eq!(mixer.set_gain(old, 0.5), Err(MixerError::StaleHandle));
    assert_eq!(mixer.set_pan(old, 0.0), Err(MixerError::StaleHandle));
    assert_eq!(mixer.set_pitch(old, 1.0), Err(MixerError::StaleHandle));
    assert_eq!(mixer.stop(old), Err(MixerError::StaleHandle));
    assert!(mixer.pop_finished().is_some());
    let new = mixer.play(constant(1.0, 2)?, hard_left())?;
    assert_ne!(old, new, "the slot is reused with a new generation");
    assert_eq!(mixer.set_gain(old, 0.5), Err(MixerError::StaleHandle));
    assert!(mixer.set_gain(new, 0.5).is_ok());
    Ok(())
}

#[test]
fn bad_play_parameters_are_refused_and_start_no_voice() -> TestResult {
    let mut mixer = mixer(4)?;
    let sound = constant(1.0, 8)?;
    let with = |gain, pan, pitch| PlayParams {
        gain,
        pan,
        pitch,
        looping: false,
    };
    let bad = [
        with(f32::NAN, 0.0, 1.0),
        with(-0.1, 0.0, 1.0),
        with(f32::INFINITY, 0.0, 1.0),
        with(1.0, 1.5, 1.0),
        with(1.0, f32::NAN, 1.0),
        with(1.0, 0.0, 0.0),
        with(1.0, 0.0, -1.0),
        with(1.0, 0.0, 100.0),
        with(1.0, 0.0, f32::NAN),
    ];
    for params in bad {
        assert_eq!(
            mixer.play(Arc::clone(&sound), params).err(),
            Some(MixerError::InvalidParameter)
        );
    }
    assert_eq!(mixer.active_voices(), 0);
    Ok(())
}

#[test]
fn bad_parameter_changes_are_refused() -> TestResult {
    let mut mixer = mixer(4)?;
    let handle = mixer.play(constant(1.0, 8)?, PlayParams::default())?;
    assert_eq!(
        mixer.set_gain(handle, -1.0),
        Err(MixerError::InvalidParameter)
    );
    assert_eq!(
        mixer.set_pan(handle, 2.0),
        Err(MixerError::InvalidParameter)
    );
    assert_eq!(
        mixer.set_pitch(handle, 0.0),
        Err(MixerError::InvalidParameter)
    );
    Ok(())
}

#[test]
fn sounds_the_mixer_cannot_play_are_refused() -> TestResult {
    let mut mixer = mixer(4)?;
    let surround = Arc::new(Sound::new(RATE, 3, vec![0.0; 9])?);
    assert_eq!(
        mixer.play(surround, PlayParams::default()).err(),
        Some(MixerError::UnsupportedSound)
    );
    let empty = mono(RATE, Vec::new())?;
    assert_eq!(
        mixer.play(empty, PlayParams::default()).err(),
        Some(MixerError::UnsupportedSound)
    );
    Ok(())
}

#[test]
fn a_mixer_runs_behind_the_offline_device() -> TestResult {
    let mut device = OfflineDevice::new();
    let format = device.open(StreamRequest::native_stereo())?;
    let mut mixer = Mixer::new(format, 4)?;
    mixer.play(constant(1.0, 100)?, hard_left())?;
    device.start(Box::new(mixer))?;
    let mut out = vec![0.0; 2 * 50];
    device.render_into(&mut out)?;
    assert_close(&left(&out), &[1.0; 50]);
    let _ = device.stop()?;
    Ok(())
}

#[test]
fn every_mixer_error_has_its_own_message() {
    let all = [
        MixerError::UnsupportedFormat,
        MixerError::VoicesExhausted,
        MixerError::InvalidParameter,
        MixerError::UnsupportedSound,
        MixerError::StaleHandle,
    ];
    let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
    for (i, a) in texts.iter().enumerate() {
        assert!(!a.is_empty());
        for b in &texts[i + 1..] {
            assert_ne!(a, b);
        }
    }
}
