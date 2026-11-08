//! Errors of the audio port. Values, never panics.

use core::fmt;

/// Why a port call failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AudioError {
    /// The machine has no usable output device.
    NoDevice,
    /// The request is outside what the port accepts at all (rate, block size, argument).
    InvalidRequest,
    /// The device cannot give the requested format and the adapter does not convert.
    FormatRefused,
    /// The call is not allowed in the current state (start before open, second start, ...).
    WrongState,
    /// The device was unplugged or stopped by the system.
    DeviceLost,
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::NoDevice => "no audio device",
            Self::InvalidRequest => "invalid request",
            Self::FormatRefused => "format refused",
            Self::WrongState => "wrong state for this call",
            Self::DeviceLost => "device lost",
        };
        f.write_str(text)
    }
}

impl std::error::Error for AudioError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_has_its_own_message() {
        let all = [
            AudioError::NoDevice,
            AudioError::InvalidRequest,
            AudioError::FormatRefused,
            AudioError::WrongState,
            AudioError::DeviceLost,
        ];
        let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
        for (i, text) in texts.iter().enumerate() {
            assert!(!text.is_empty());
            assert!(texts[i + 1..].iter().all(|other| other != text));
        }
    }
}
