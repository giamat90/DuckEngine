//! The mixer benchmark scene of card 5-8: a mixer with a fixed number of looping voices, stereo
//! output, blocks of 256 frames. The voices never end, so every block costs the same. Dev-time
//! only; the engine does not depend on it.
//!
//! The scene is deterministic: pan and pitch of voice `i` follow from `i`, and the sound is a
//! fixed waveform.

use audio::device::AudioRenderer;
use audio::format::{ChannelLayout, StreamFormat};
use audio::mixer::{Mixer, PlayParams};
use audio::sound::Sound;
use std::error::Error;
use std::sync::Arc;

pub use tools_render_bench::{Summary, summarize};

/// Frames per block, as the wiki says (section 14 of the audio port page).
pub const BLOCK_FRAMES: u32 = 256;
/// Output sample rate.
pub const SAMPLE_RATE: u32 = 48_000;
/// Frames of the shared sound: ten seconds, much longer than a run.
const SOUND_FRAMES: usize = 480_000;
/// Distinct pans and pitches the voices cycle through.
const VARIANTS: u32 = 8;
const NANOS_PER_SECOND: f64 = 1e9;

/// A mixer with `voices` looping voices and an output buffer for one block.
pub struct Scene {
    mixer: Mixer,
    out: Vec<f32>,
}

impl Scene {
    /// Builds the scene and starts every voice.
    ///
    /// # Errors
    /// When `voices` is 0 or too large for the mixer.
    pub fn new(voices: u32) -> Result<Self, Box<dyn Error>> {
        let format = StreamFormat {
            sample_rate: SAMPLE_RATE,
            layout: ChannelLayout::Stereo,
            max_block_frames: BLOCK_FRAMES,
        };
        let mut mixer = Mixer::new(format, voices)?;
        let sound = Arc::new(Sound::new(SAMPLE_RATE, 1, waveform())?);
        for index in 0..voices {
            let step = f32::from(u8::try_from(index % VARIANTS)?);
            let params = PlayParams {
                gain: 0.01,
                pan: step / 4.0 - 1.0,
                pitch: 0.75 + step * 0.125,
                looping: true,
                ..PlayParams::default()
            };
            mixer.play(Arc::clone(&sound), params)?;
        }
        Ok(Self {
            mixer,
            out: vec![0.0; BLOCK_FRAMES as usize * 2],
        })
    }

    /// Voices still playing.
    #[must_use]
    pub fn active_voices(&self) -> usize {
        self.mixer.active_voices()
    }

    /// Mixes one block into the output buffer.
    pub fn render_block(&mut self) {
        self.mixer.render(&mut self.out, BLOCK_FRAMES);
    }

    /// The block rendered last.
    #[must_use]
    pub fn output(&self) -> &[f32] {
        &self.out
    }
}

/// A fixed waveform with some harmonics, mono.
fn waveform() -> Vec<f32> {
    (0..SOUND_FRAMES)
        .map(|n| {
            let phase = f32::from(u16::try_from(n % 480).unwrap_or(0)) / 480.0;
            (phase * std::f32::consts::TAU).sin() * 0.6
                + (phase * 3.0 * std::f32::consts::TAU).sin() * 0.3
        })
        .collect()
}

/// The share of one core, in percent, that a block of `frames` frames at `rate` takes when
/// rendering it costs `block_ns` nanoseconds: the time spent divided by the time the block lasts.
#[must_use]
pub fn block_percent(block_ns: f64, frames: u32, rate: u32) -> f64 {
    let lasts_ns = f64::from(frames) / f64::from(rate) * NANOS_PER_SECOND;
    block_ns / lasts_ns * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_waveform_is_a_480_frame_period_of_two_sines() {
        let wave = waveform();
        assert_eq!(wave.len(), SOUND_FRAMES);
        let at = |n: usize| wave.get(n).copied().unwrap_or(f32::NAN);
        assert!(at(0).abs() < 1e-6);
        assert!((at(120) - 0.3).abs() < 1e-4, "quarter period: 0.6 - 0.3");
        assert!(at(240).abs() < 1e-3, "half period");
        assert!((at(360) + 0.3).abs() < 1e-4, "three quarters: -0.6 + 0.3");
        assert!((at(7) - at(7 + 480)).abs() < 1e-6, "periodic");
    }
}
