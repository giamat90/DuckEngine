//! The decoder port: bytes of a sound file in, an immutable [`Sound`] out.

use crate::sound::Sound;
use core::fmt;

/// Why a file could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecodeError {
    /// The bytes end before the data they announce.
    Truncated,
    /// The bytes are not a file this decoder reads.
    NotThisFormat,
    /// The file is of this format but uses a feature the decoder does not support.
    Unsupported,
    /// A field has a value the format does not allow.
    Invalid,
    /// The decoded sound would be larger than the decoder's limit.
    TooLarge,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Truncated => "the file is truncated",
            Self::NotThisFormat => "not a file of this format",
            Self::Unsupported => "unsupported feature of the format",
            Self::Invalid => "invalid field value",
            Self::TooLarge => "the decoded sound is too large",
        };
        f.write_str(text)
    }
}

impl std::error::Error for DecodeError {}

/// A decoder for one file format. It runs on a loader or game thread, where allocation is
/// allowed, never on the audio thread. The input is untrusted.
pub trait IAudioDecoder: Send + Sync {
    /// Decodes a whole file held in memory.
    ///
    /// # Errors
    /// [`DecodeError`] when the bytes are not a valid file of the format.
    fn decode(&self, bytes: &[u8]) -> Result<Sound, DecodeError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct OneSample;

    impl IAudioDecoder for OneSample {
        fn decode(&self, bytes: &[u8]) -> Result<Sound, DecodeError> {
            let byte = *bytes.first().ok_or(DecodeError::Truncated)?;
            Sound::new(8_000, 1, vec![f32::from(byte)]).map_err(|_| DecodeError::Invalid)
        }
    }

    #[test]
    fn a_decoder_is_usable_as_a_trait_object() {
        let decoder: Box<dyn IAudioDecoder> = Box::new(OneSample);
        assert_eq!(decoder.decode(&[]), Err(DecodeError::Truncated));
        let Ok(sound) = decoder.decode(&[3]) else {
            panic!("decoding failed")
        };
        assert_eq!(sound.samples(), &[3.0]);
    }

    #[test]
    fn every_error_has_its_own_message() {
        let all = [
            DecodeError::Truncated,
            DecodeError::NotThisFormat,
            DecodeError::Unsupported,
            DecodeError::Invalid,
            DecodeError::TooLarge,
        ];
        let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
        for (i, text) in texts.iter().enumerate() {
            assert!(texts[i + 1..].iter().all(|other| other != text));
        }
    }
}
