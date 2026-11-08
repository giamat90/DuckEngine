//! A decoded sound: immutable interleaved `f32` samples with their rate and channel count.

use core::fmt;

/// The most channels a sound may have.
pub const MAX_SOUND_CHANNELS: u8 = 8;

/// Why a sound was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundError {
    /// The sample rate is 0.
    ZeroSampleRate,
    /// The channel count is 0 or above [`MAX_SOUND_CHANNELS`].
    BadChannelCount,
    /// The number of samples is not a whole number of frames.
    PartialFrame,
}

impl fmt::Display for SoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::ZeroSampleRate => "sample rate is zero",
            Self::BadChannelCount => "channel count is zero or too large",
            Self::PartialFrame => "samples do not make whole frames",
        };
        f.write_str(text)
    }
}

impl std::error::Error for SoundError {}

/// Immutable sound data, shared as `Arc<Sound>` between threads.
#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    sample_rate: u32,
    channels: u8,
    samples: Box<[f32]>,
}

impl Sound {
    /// Builds a sound from interleaved samples.
    ///
    /// # Errors
    /// [`SoundError`] for a zero rate, a channel count of 0 or above [`MAX_SOUND_CHANNELS`], or a
    /// sample count that is not a multiple of the channel count.
    pub fn new(sample_rate: u32, channels: u8, samples: Vec<f32>) -> Result<Self, SoundError> {
        if sample_rate == 0 {
            return Err(SoundError::ZeroSampleRate);
        }
        if channels == 0 || channels > MAX_SOUND_CHANNELS {
            return Err(SoundError::BadChannelCount);
        }
        if !samples.len().is_multiple_of(usize::from(channels)) {
            return Err(SoundError::PartialFrame);
        }
        Ok(Self {
            sample_rate,
            channels,
            samples: samples.into_boxed_slice(),
        })
    }

    /// Frames per second.
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Interleaved channels per frame.
    #[must_use]
    pub fn channels(&self) -> u8 {
        self.channels
    }

    /// The interleaved samples.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// The number of frames.
    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_sound_reports_its_parts() {
        let sound = Sound::new(44_100, 2, vec![0.0, 0.1, 0.2, 0.3, 0.4, 0.5]);
        let Ok(sound) = sound else {
            panic!("a valid sound was refused")
        };
        assert_eq!(sound.sample_rate(), 44_100);
        assert_eq!(sound.channels(), 2);
        assert_eq!(sound.frame_count(), 3);
        assert_eq!(sound.samples().len(), 6);
    }

    #[test]
    fn an_empty_sound_is_allowed() {
        let Ok(sound) = Sound::new(8_000, 1, Vec::new()) else {
            panic!("an empty sound was refused")
        };
        assert_eq!(sound.frame_count(), 0);
    }

    #[test]
    fn a_zero_rate_is_refused() {
        assert_eq!(Sound::new(0, 1, vec![0.0]), Err(SoundError::ZeroSampleRate));
    }

    #[test]
    fn the_channel_count_must_be_between_one_and_the_maximum() {
        assert_eq!(
            Sound::new(8_000, 0, Vec::new()),
            Err(SoundError::BadChannelCount)
        );
        assert_eq!(
            Sound::new(8_000, MAX_SOUND_CHANNELS + 1, Vec::new()),
            Err(SoundError::BadChannelCount)
        );
        assert!(Sound::new(8_000, MAX_SOUND_CHANNELS, vec![0.0; 8]).is_ok());
    }

    #[test]
    fn samples_must_make_whole_frames() {
        assert_eq!(
            Sound::new(8_000, 2, vec![0.0; 5]),
            Err(SoundError::PartialFrame)
        );
    }

    #[test]
    fn every_error_has_its_own_message() {
        let all = [
            SoundError::ZeroSampleRate,
            SoundError::BadChannelCount,
            SoundError::PartialFrame,
        ];
        let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
        for (i, text) in texts.iter().enumerate() {
            assert!(texts[i + 1..].iter().all(|other| other != text));
        }
    }
}
