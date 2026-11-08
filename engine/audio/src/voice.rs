//! One playing voice: the linear resampler, the gain ramps and the end conditions.
//!
//! Everything that crosses a render call (the read position, the two gain ramps) lives in the
//! voice, so the output does not depend on how the frames are split between calls.

use crate::sound::Sound;
use std::sync::Arc;

/// Length of a gain or pan ramp, in frames: 5.3 ms at 48 kHz.
pub const RAMP_FRAMES: u16 = 256;

/// A linear ramp towards a target, one step per output frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ramp {
    current: f32,
    target: f32,
    step: f32,
    left: u16,
}

impl Ramp {
    pub(crate) fn at(value: f32) -> Self {
        Self {
            current: value,
            target: value,
            step: 0.0,
            left: 0,
        }
    }

    /// Starts moving towards `target`; it is reached after [`RAMP_FRAMES`] ticks.
    pub(crate) fn set(&mut self, target: f32) {
        self.target = target;
        self.step = (target - self.current) / f32::from(RAMP_FRAMES);
        self.left = RAMP_FRAMES;
    }

    /// The value for this frame; then moves one step.
    pub(crate) fn tick(&mut self) -> f32 {
        let value = self.current;
        if self.left > 0 {
            self.left -= 1;
            self.current = self.next_value();
        }
        value
    }

    fn next_value(&self) -> f32 {
        if self.left == 0 {
            self.target
        } else {
            self.current + self.step
        }
    }

    pub(crate) fn settled(&self) -> bool {
        self.left == 0
    }
}

/// Splits a read position into the whole frame and the fraction after it.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the position is never negative and a sound has fewer frames than usize holds; the fraction is in [0, 1)"
)]
fn split(position: f64) -> (usize, f32) {
    let whole = position.floor();
    (whole as usize, (position - whole) as f32)
}

/// The parameters a voice is built from; the mixer has validated them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Setup {
    pub(crate) gain: f32,
    pub(crate) pan_gains: (f32, f32),
    pub(crate) pitch: f32,
    pub(crate) looping: bool,
    pub(crate) engine_rate: u32,
}

/// A voice playing one sound.
pub(crate) struct Voice {
    sound: Arc<Sound>,
    frames: usize,
    length: f64,
    position: f64,
    step: f64,
    engine_rate: u32,
    looping: bool,
    stopping: bool,
    gain: f32,
    pan_gains: (f32, f32),
    left: Ramp,
    right: Ramp,
}

impl Voice {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a frame count below 2^53 converts exactly"
    )]
    pub(crate) fn new(sound: Arc<Sound>, setup: Setup) -> Self {
        let frames = sound.frame_count();
        let mut voice = Self {
            frames,
            length: frames as f64,
            position: 0.0,
            step: 0.0,
            engine_rate: setup.engine_rate,
            looping: setup.looping,
            stopping: false,
            gain: setup.gain,
            pan_gains: setup.pan_gains,
            left: Ramp::at(setup.gain * setup.pan_gains.0),
            right: Ramp::at(setup.gain * setup.pan_gains.1),
            sound,
        };
        voice.set_pitch(setup.pitch);
        voice
    }

    /// Hands the sound back so that it is dropped by the caller, never inside the callback.
    pub(crate) fn into_sound(self) -> Arc<Sound> {
        self.sound
    }

    pub(crate) fn set_pitch(&mut self, pitch: f32) {
        self.step =
            f64::from(pitch) * f64::from(self.sound.sample_rate()) / f64::from(self.engine_rate);
    }

    pub(crate) fn set_gain(&mut self, gain: f32) {
        self.gain = gain;
        self.retarget();
    }

    pub(crate) fn set_pan_gains(&mut self, pan_gains: (f32, f32)) {
        self.pan_gains = pan_gains;
        self.retarget();
    }

    /// Fades to silence; the voice ends when the fade is done. A second call changes nothing.
    pub(crate) fn stop(&mut self) {
        if self.stopping {
            return;
        }
        self.stopping = true;
        self.left.set(0.0);
        self.right.set(0.0);
    }

    fn retarget(&mut self) {
        if !self.stopping {
            self.left.set(self.gain * self.pan_gains.0);
            self.right.set(self.gain * self.pan_gains.1);
        }
    }

    /// One frame of the source at `index`, as (left, right); a mono source feeds both.
    fn frame(&self, index: usize) -> (f32, f32) {
        let samples = self.sound.samples();
        let at = |i: usize| samples.get(i).copied().unwrap_or(0.0);
        if self.sound.channels() == 1 {
            let mono = at(index);
            (mono, mono)
        } else {
            (at(index * 2), at(index * 2 + 1))
        }
    }

    /// The source at the read position, linearly interpolated.
    fn sample(&self) -> (f32, f32) {
        let (index, fraction) = split(self.position);
        let (l0, r0) = self.frame(index);
        let next = index + 1;
        let (l1, r1) = if next < self.frames {
            self.frame(next)
        } else if self.looping {
            self.frame(0)
        } else {
            (0.0, 0.0)
        };
        (l0 + (l1 - l0) * fraction, r0 + (r1 - r0) * fraction)
    }

    fn advance(&mut self) {
        self.position += self.step;
        if self.looping && self.position >= self.length {
            self.position %= self.length;
        }
    }

    fn finished(&self) -> bool {
        let played_out = !self.looping && self.position >= self.length;
        let faded_out = self.stopping && self.left.settled() && self.right.settled();
        played_out || faded_out
    }

    /// Adds one frame (left, right) to `frame`; true when the voice has ended.
    fn mix_frame(&mut self, frame: &mut [f32]) -> bool {
        let (l, r) = self.sample();
        let (gain_left, gain_right) = (self.left.tick(), self.right.tick());
        if let [out_left, out_right] = frame {
            *out_left += l * gain_left;
            *out_right += r * gain_right;
        }
        self.advance();
        self.finished()
    }

    /// Adds the voice to `out` (interleaved stereo); true when the voice has ended.
    pub(crate) fn mix(&mut self, out: &mut [f32]) -> bool {
        out.chunks_exact_mut(2).any(|frame| self.mix_frame(frame))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ramp_reaches_its_target_exactly_after_the_ramp_length() {
        let mut ramp = Ramp::at(1.0);
        ramp.set(0.0);
        for _ in 0..RAMP_FRAMES {
            assert!(!ramp.settled());
            let _ = ramp.tick();
        }
        assert!(ramp.settled());
        assert_eq!(ramp.tick().to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn a_new_target_restarts_the_ramp_from_where_it_is() {
        let mut ramp = Ramp::at(0.0);
        ramp.set(1.0);
        for _ in 0..128 {
            let _ = ramp.tick();
        }
        ramp.set(0.0);
        assert!((ramp.tick() - 0.5).abs() < 1.0e-6);
        for _ in 0..RAMP_FRAMES {
            let _ = ramp.tick();
        }
        assert!(ramp.settled());
    }

    #[test]
    fn a_position_splits_into_a_frame_and_a_fraction() {
        assert_eq!(split(0.0), (0, 0.0));
        assert_eq!(split(3.25), (3, 0.25));
    }
}
