//! The mixer v0: fixed-capacity voices summed into one stereo master (card 5-3).
//!
//! The mixer is built before the device starts and then owned by the audio thread, so
//! [`AudioRenderer::render`] allocates nothing, locks nothing and drops nothing that frees memory.
//! A voice that ends moves its `Arc<Sound>` into a retire list with room reserved for every
//! voice; the owner takes the sounds out with [`Mixer::pop_finished`] on a thread where freeing is
//! allowed. Commands are plain method calls here; card 5-5 carries them over a queue.
//!
//! Design: `wiki/architecture/audio-port-v0.md` sections 3 and 6.

use crate::device::AudioRenderer;
use crate::format::{ChannelLayout, StreamFormat};
use crate::sound::Sound;
use crate::voice::{Setup, Voice};
use base::pool::Handle;
use core::f32::consts::FRAC_PI_4;
use core::fmt;
use std::sync::Arc;

pub use crate::voice::RAMP_FRAMES;

/// The most voices a mixer may be built with.
pub const MAX_VOICES: u32 = 4096;
/// The largest pitch ratio; the smallest is anything above 0.
pub const MAX_PITCH: f32 = 16.0;

/// Marks the handles of voices.
pub enum VoiceTag {}

/// Names a playing voice; stale once the voice has ended.
pub type VoiceHandle = Handle<VoiceTag>;

/// Why the mixer refused a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MixerError {
    /// The mixer mixes stereo output only.
    UnsupportedFormat,
    /// Every voice is playing or waiting for [`Mixer::pop_finished`].
    VoicesExhausted,
    /// A gain, pan, pitch or voice count outside its range, or not a number.
    InvalidParameter,
    /// The sound is empty or has more than two channels.
    UnsupportedSound,
    /// The handle names a voice that has ended.
    StaleHandle,
}

impl fmt::Display for MixerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::UnsupportedFormat => "the mixer mixes stereo output only",
            Self::VoicesExhausted => "every voice is in use",
            Self::InvalidParameter => "a parameter is out of range",
            Self::UnsupportedSound => "the sound is empty or has more than two channels",
            Self::StaleHandle => "the voice has ended",
        };
        f.write_str(text)
    }
}

impl std::error::Error for MixerError {}

/// How a voice plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayParams {
    /// Linear gain, finite and not negative.
    pub gain: f32,
    /// -1 is hard left, 0 the centre, 1 hard right.
    pub pan: f32,
    /// Playback speed ratio in (0, [`MAX_PITCH`]]; 2 plays an octave up.
    pub pitch: f32,
    /// Repeat the sound until stopped.
    pub looping: bool,
}

impl Default for PlayParams {
    fn default() -> Self {
        Self {
            gain: 1.0,
            pan: 0.0,
            pitch: 1.0,
            looping: false,
        }
    }
}

/// Constant-power pan law: (left, right) gains with `left² + right² = 1`, so the centre is -3 dB
/// on each channel. `pan` is clamped to [-1, 1]. A stereo source gets the same gains on its
/// own two channels (a balance control).
#[must_use]
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
    (angle.cos().max(0.0), angle.sin().max(0.0))
}

pub(crate) fn valid_gain(gain: f32) -> bool {
    gain.is_finite() && gain >= 0.0
}

pub(crate) fn valid_pan(pan: f32) -> bool {
    (-1.0..=1.0).contains(&pan)
}

pub(crate) fn valid_pitch(pitch: f32) -> bool {
    pitch > 0.0 && pitch <= MAX_PITCH
}

pub(crate) fn valid_params(params: &PlayParams) -> bool {
    valid_gain(params.gain) && valid_pan(params.pan) && valid_pitch(params.pitch)
}

struct Slot {
    generation: u32,
    voice: Option<Voice>,
}

/// The mixer: owns the voices and renders them into an interleaved stereo buffer.
pub struct Mixer {
    format: StreamFormat,
    slots: Vec<Slot>,
    retired: Vec<(VoiceHandle, Arc<Sound>)>,
    active: usize,
}

impl Mixer {
    /// Builds a mixer with room for `max_voices` voices, all allocation done here.
    ///
    /// # Errors
    /// `UnsupportedFormat` unless the layout is stereo; `InvalidParameter` when `max_voices` is 0
    /// or above [`MAX_VOICES`], or the sample rate is 0.
    pub fn new(format: StreamFormat, max_voices: u32) -> Result<Self, MixerError> {
        if format.layout != ChannelLayout::Stereo {
            return Err(MixerError::UnsupportedFormat);
        }
        if max_voices == 0 || max_voices > MAX_VOICES || format.sample_rate == 0 {
            return Err(MixerError::InvalidParameter);
        }
        let capacity = usize::try_from(max_voices).map_err(|_| MixerError::InvalidParameter)?;
        let slots = (0..capacity)
            .map(|_| Slot {
                generation: 0,
                voice: None,
            })
            .collect();
        Ok(Self {
            format,
            slots,
            retired: Vec::with_capacity(capacity),
            active: 0,
        })
    }

    /// Starts a voice.
    ///
    /// # Errors
    /// `UnsupportedSound`, `InvalidParameter`, or `VoicesExhausted` when every voice is playing
    /// or still waiting in [`Mixer::pop_finished`].
    ///
    /// On an error the `sound` argument is dropped here; a caller on the audio thread must
    /// therefore keep another reference (the game side does, card 5-5) or check first.
    pub fn play(
        &mut self,
        sound: Arc<Sound>,
        params: PlayParams,
    ) -> Result<VoiceHandle, MixerError> {
        if sound.frame_count() == 0 || sound.channels() > 2 {
            return Err(MixerError::UnsupportedSound);
        }
        if !valid_params(&params) {
            return Err(MixerError::InvalidParameter);
        }
        if self.active + self.retired.len() >= self.slots.len() {
            return Err(MixerError::VoicesExhausted);
        }
        let Some(index) = self.slots.iter().position(|slot| slot.voice.is_none()) else {
            return Err(MixerError::VoicesExhausted);
        };
        let setup = Setup {
            gain: params.gain,
            pan_gains: pan_gains(params.pan),
            pitch: params.pitch,
            looping: params.looping,
            engine_rate: self.format.sample_rate,
        };
        let slot = self
            .slots
            .get_mut(index)
            .ok_or(MixerError::VoicesExhausted)?;
        slot.voice = Some(Voice::new(sound, setup));
        self.active += 1;
        let index = u32::try_from(index).map_err(|_| MixerError::VoicesExhausted)?;
        Ok(Handle::from_raw(index, slot.generation))
    }

    fn voice_mut(&mut self, handle: VoiceHandle) -> Result<&mut Voice, MixerError> {
        let index = usize::try_from(handle.index()).map_err(|_| MixerError::StaleHandle)?;
        self.slots
            .get_mut(index)
            .filter(|slot| slot.generation == handle.generation())
            .and_then(|slot| slot.voice.as_mut())
            .ok_or(MixerError::StaleHandle)
    }

    /// Fades the voice out over [`RAMP_FRAMES`] frames and ends it.
    ///
    /// # Errors
    /// `StaleHandle`.
    pub fn stop(&mut self, handle: VoiceHandle) -> Result<(), MixerError> {
        self.voice_mut(handle)?.stop();
        Ok(())
    }

    /// Ramps the voice gain to `gain` over [`RAMP_FRAMES`] frames.
    ///
    /// # Errors
    /// `StaleHandle`, or `InvalidParameter` for a gain that is negative or not finite.
    pub fn set_gain(&mut self, handle: VoiceHandle, gain: f32) -> Result<(), MixerError> {
        let voice = self.voice_mut(handle)?;
        if !valid_gain(gain) {
            return Err(MixerError::InvalidParameter);
        }
        voice.set_gain(gain);
        Ok(())
    }

    /// Ramps the voice pan to `pan` over [`RAMP_FRAMES`] frames.
    ///
    /// # Errors
    /// `StaleHandle`, or `InvalidParameter` for a pan outside [-1, 1].
    pub fn set_pan(&mut self, handle: VoiceHandle, pan: f32) -> Result<(), MixerError> {
        let voice = self.voice_mut(handle)?;
        if !valid_pan(pan) {
            return Err(MixerError::InvalidParameter);
        }
        voice.set_pan_gains(pan_gains(pan));
        Ok(())
    }

    /// Changes the playback speed; takes effect at the next frame, without a ramp.
    ///
    /// # Errors
    /// `StaleHandle`, or `InvalidParameter` for a pitch outside (0, [`MAX_PITCH`]].
    pub fn set_pitch(&mut self, handle: VoiceHandle, pitch: f32) -> Result<(), MixerError> {
        let voice = self.voice_mut(handle)?;
        if !valid_pitch(pitch) {
            return Err(MixerError::InvalidParameter);
        }
        voice.set_pitch(pitch);
        Ok(())
    }

    /// Takes one ended voice with the sound it played, so the caller drops the sound where
    /// freeing is allowed. Until it is taken, the voice still counts against the capacity. The
    /// last ended voice comes first (a stack, not a queue); card 5-5 turns these into events.
    pub fn pop_finished(&mut self) -> Option<(VoiceHandle, Arc<Sound>)> {
        self.retired.pop()
    }

    /// The number of voices playing.
    #[must_use]
    pub fn active_voices(&self) -> usize {
        self.active
    }

    fn mix_slot(&mut self, index: usize, out: &mut [f32]) {
        let Ok(raw_index) = u32::try_from(index) else {
            return;
        };
        let Some(slot) = self.slots.get_mut(index) else {
            return;
        };
        let Some(voice) = slot.voice.as_mut() else {
            return;
        };
        if !voice.mix(out) {
            return;
        }
        let handle = Handle::from_raw(raw_index, slot.generation);
        slot.generation = slot.generation.wrapping_add(1);
        if let Some(voice) = slot.voice.take() {
            // Room is reserved: `play` counts retired voices against the capacity.
            self.retired.push((handle, voice.into_sound()));
            self.active = self.active.saturating_sub(1);
        }
    }
}

impl AudioRenderer for Mixer {
    fn render(&mut self, out: &mut [f32], frames: u32) {
        // A slice shorter than `frames` frames is a caller bug; the part that exists is still
        // zeroed and the frames that fit are rendered, so that no stale data is played. Samples
        // beyond the requested frames are left alone.
        let wanted = usize::try_from(frames).unwrap_or(0).saturating_mul(2);
        let zeroed = wanted.min(out.len());
        let Some(out) = out.get_mut(..zeroed) else {
            return;
        };
        out.fill(0.0);
        let Some(out) = out.get_mut(..zeroed & !1) else {
            return;
        };
        for index in 0..self.slots.len() {
            self.mix_slot(index, out);
        }
    }
}
