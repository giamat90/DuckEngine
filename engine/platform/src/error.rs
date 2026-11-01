//! The one error type of every platform port.

use std::fmt;

/// Why a platform call failed. Errors are values: adapters translate OS errors into these and keep
/// the detail in their own log, never in the port.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformError {
    /// The named thing does not exist.
    NotFound,
    /// The platform refused access.
    Denied,
    /// This platform or this configuration cannot do that.
    Unsupported,
    /// A virtual path that is not allowed.
    InvalidPath,
    /// The caller's buffer is too small; `needed` bytes are required.
    BufferTooSmall {
        /// The size the buffer must have.
        needed: u64,
    },
    /// Any other failure of the operating system.
    Io,
    /// A fixed queue or pool is full.
    QueueFull,
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("not found"),
            Self::Denied => f.write_str("access denied"),
            Self::Unsupported => f.write_str("not supported on this platform"),
            Self::InvalidPath => f.write_str("invalid virtual path"),
            Self::BufferTooSmall { needed } => write!(f, "buffer too small, {needed} bytes needed"),
            Self::Io => f.write_str("operating system error"),
            Self::QueueFull => f.write_str("queue full"),
        }
    }
}

impl std::error::Error for PlatformError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_has_its_own_readable_message() {
        let cases = [
            (PlatformError::NotFound, "not found"),
            (PlatformError::Denied, "access denied"),
            (PlatformError::Unsupported, "not supported on this platform"),
            (PlatformError::InvalidPath, "invalid virtual path"),
            (
                PlatformError::BufferTooSmall { needed: 9 },
                "buffer too small, 9 bytes needed",
            ),
            (PlatformError::Io, "operating system error"),
            (PlatformError::QueueFull, "queue full"),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
        }
    }
}
