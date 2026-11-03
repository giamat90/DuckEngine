//! Errors of the render port. Values, never panics.

use core::fmt;

/// Why a port call failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderError {
    /// The device has no memory or slot left for the resource.
    OutOfMemory,
    /// A handle is stale, never issued, or of a destroyed resource.
    InvalidHandle,
    /// A description or argument is outside what the device accepts.
    InvalidDescription,
    /// The shader blob was refused.
    ShaderRejected,
    /// The surface no longer exists (window closed, device reset).
    SurfaceLost,
    /// The feature is not available on this device.
    Unsupported,
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::OutOfMemory => "out of memory",
            Self::InvalidHandle => "invalid handle",
            Self::InvalidDescription => "invalid description",
            Self::ShaderRejected => "shader rejected",
            Self::SurfaceLost => "surface lost",
            Self::Unsupported => "unsupported",
        };
        f.write_str(text)
    }
}

impl std::error::Error for RenderError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_has_its_own_message() {
        let all = [
            RenderError::OutOfMemory,
            RenderError::InvalidHandle,
            RenderError::InvalidDescription,
            RenderError::ShaderRejected,
            RenderError::SurfaceLost,
            RenderError::Unsupported,
        ];
        let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
        assert!(texts.iter().all(|t| !t.is_empty()));
        let pairs = texts
            .iter()
            .enumerate()
            .flat_map(|(i, a)| texts[i + 1..].iter().map(move |b| (a, b)));
        for (a, b) in pairs {
            assert_ne!(a, b);
        }
        assert_eq!(RenderError::SurfaceLost.to_string(), "surface lost");
    }

    #[test]
    fn is_a_std_error() {
        fn takes(_: &dyn std::error::Error) {}
        takes(&RenderError::Unsupported);
    }
}
