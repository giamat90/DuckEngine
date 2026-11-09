//! What is asked of a device and what it gives: channel layout, sample rate, block size.

use crate::error::AudioError;

/// The rate a device without a preference of its own gives when the request says 0.
pub const DEFAULT_SAMPLE_RATE: u32 = 48_000;
/// The block size a device without a preference of its own gives when the request says 0.
pub const DEFAULT_BLOCK_FRAMES: u32 = 512;
/// The lowest sample rate the port accepts.
pub const MIN_SAMPLE_RATE: u32 = 8_000;
/// The highest sample rate the port accepts.
pub const MAX_SAMPLE_RATE: u32 = 384_000;
/// The largest block, in frames, the port accepts.
pub const MAX_BLOCK_FRAMES: u32 = 16_384;

/// Which channels the output has. Interleaved in this order: mono; left, right; front left, front
/// right, centre, low frequency, back left, back right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelLayout {
    /// One channel.
    Mono,
    /// Left and right.
    Stereo,
    /// Five channels and one low-frequency channel.
    Surround51,
}

impl ChannelLayout {
    /// The number of interleaved channels.
    #[must_use]
    pub const fn channels(self) -> u8 {
        match self {
            Self::Mono => 1,
            Self::Stereo => 2,
            Self::Surround51 => 6,
        }
    }
}

/// What the caller proposes to [`crate::device::IAudioDevice::open`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamRequest {
    /// Frames per second; 0 takes the device's own rate.
    pub sample_rate: u32,
    /// The channel layout wanted.
    pub layout: ChannelLayout,
    /// The block size wanted, in frames; 0 takes the device's own choice. Only a hint: a real
    /// device may give another size, and the format returned by `open` is what counts.
    pub block_hint_frames: u32,
}

impl StreamRequest {
    /// Stereo at the device's own rate and block size.
    #[must_use]
    pub const fn native_stereo() -> Self {
        Self {
            sample_rate: 0,
            layout: ChannelLayout::Stereo,
            block_hint_frames: 0,
        }
    }

    /// Checks the numbers against the limits of the port.
    ///
    /// # Errors
    /// [`AudioError::InvalidRequest`] for a rate outside `MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE` (0 is
    /// allowed) or a block hint above `MAX_BLOCK_FRAMES`.
    pub fn validate(&self) -> Result<(), AudioError> {
        let rate_ok = self.sample_rate == 0
            || (MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&self.sample_rate);
        if rate_ok && self.block_hint_frames <= MAX_BLOCK_FRAMES {
            Ok(())
        } else {
            Err(AudioError::InvalidRequest)
        }
    }
}

/// What the device really gives, returned by [`crate::device::IAudioDevice::open`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamFormat {
    /// Frames per second.
    pub sample_rate: u32,
    /// The channel layout.
    pub layout: ChannelLayout,
    /// The most frames one callback is asked for. A callback may ask for fewer, and the number
    /// may differ from call to call.
    pub max_block_frames: u32,
}

impl StreamFormat {
    /// The number of samples (frames times channels) of a buffer of `frames` frames.
    #[must_use]
    pub fn samples_for(&self, frames: u32) -> usize {
        frames as usize * usize::from(self.layout.channels())
    }

    /// The format of a device that gives exactly what is asked and fills in the defaults for 0.
    ///
    /// # Errors
    /// [`AudioError::InvalidRequest`] as [`StreamRequest::validate`].
    pub fn exact(request: StreamRequest) -> Result<Self, AudioError> {
        request.validate()?;
        let or_default = |asked: u32, default: u32| if asked == 0 { default } else { asked };
        Ok(Self {
            sample_rate: or_default(request.sample_rate, DEFAULT_SAMPLE_RATE),
            layout: request.layout,
            max_block_frames: or_default(request.block_hint_frames, DEFAULT_BLOCK_FRAMES),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(sample_rate: u32, block_hint_frames: u32) -> StreamRequest {
        StreamRequest {
            sample_rate,
            layout: ChannelLayout::Mono,
            block_hint_frames,
        }
    }

    #[test]
    fn layouts_have_their_channel_counts() {
        assert_eq!(ChannelLayout::Mono.channels(), 1);
        assert_eq!(ChannelLayout::Stereo.channels(), 2);
        assert_eq!(ChannelLayout::Surround51.channels(), 6);
    }

    #[test]
    fn exact_takes_what_is_asked() {
        let format = StreamFormat::exact(request(44_100, 256));
        assert_eq!(
            format,
            Ok(StreamFormat {
                sample_rate: 44_100,
                layout: ChannelLayout::Mono,
                max_block_frames: 256
            })
        );
    }

    #[test]
    fn exact_fills_in_the_defaults_for_zero() {
        let format = StreamFormat::exact(StreamRequest::native_stereo());
        assert_eq!(
            format,
            Ok(StreamFormat {
                sample_rate: DEFAULT_SAMPLE_RATE,
                layout: ChannelLayout::Stereo,
                max_block_frames: DEFAULT_BLOCK_FRAMES
            })
        );
    }

    #[test]
    fn the_rate_limits_are_inclusive() {
        assert!(request(MIN_SAMPLE_RATE, 0).validate().is_ok());
        assert!(request(MAX_SAMPLE_RATE, 0).validate().is_ok());
        assert_eq!(
            request(MIN_SAMPLE_RATE - 1, 0).validate(),
            Err(AudioError::InvalidRequest)
        );
        assert_eq!(
            request(MAX_SAMPLE_RATE + 1, 0).validate(),
            Err(AudioError::InvalidRequest)
        );
    }

    #[test]
    fn the_block_limit_is_inclusive() {
        assert!(request(0, MAX_BLOCK_FRAMES).validate().is_ok());
        assert_eq!(
            request(0, MAX_BLOCK_FRAMES + 1).validate(),
            Err(AudioError::InvalidRequest)
        );
    }

    #[test]
    fn exact_refuses_an_invalid_request() {
        assert_eq!(
            StreamFormat::exact(request(1, 0)),
            Err(AudioError::InvalidRequest)
        );
    }

    #[test]
    fn samples_for_counts_channels() {
        let format = StreamFormat {
            sample_rate: DEFAULT_SAMPLE_RATE,
            layout: ChannelLayout::Surround51,
            max_block_frames: 64,
        };
        assert_eq!(format.samples_for(100), 600);
        assert_eq!(format.samples_for(0), 0);
    }
}
