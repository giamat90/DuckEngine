//! The OpenGL version a context reports, and the version this adapter needs.

use platform::error::PlatformError;

/// An OpenGL version, major then minor. Ordered: 4.6 is newer than 4.5, 5.0 newer than 4.6.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GlVersion {
    /// The major number.
    pub major: u32,
    /// The minor number.
    pub minor: u32,
}

/// The version this adapter needs: OpenGL 4.5 (direct state access, `glClipControl`).
pub const REQUIRED: GlVersion = GlVersion { major: 4, minor: 5 };

impl GlVersion {
    /// Reads the start of a `GL_VERSION` string: `"4.5.0 NVIDIA 566.03"`, `"4.6 (Core Profile)
    /// Mesa 24.0"`. `None` for text that does not start with `major.minor` and for OpenGL ES
    /// strings (`"OpenGL ES 3.2 ..."`), which are another API.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (major, rest) = number(text)?;
        let (minor, _) = number(rest.strip_prefix('.')?)?;
        Some(Self { major, minor })
    }

    /// True when this version is `required` or newer.
    #[must_use]
    pub fn at_least(self, required: Self) -> bool {
        self >= required
    }
}

/// The version check of a new context: the `GL_VERSION` text must name desktop OpenGL 4.5 or
/// newer.
///
/// # Errors
/// [`PlatformError::Unsupported`] when there is no text, it names no version (OpenGL ES
/// included) or the version is older than [`REQUIRED`].
pub fn accept(text: Option<&str>) -> Result<GlVersion, PlatformError> {
    text.and_then(GlVersion::parse)
        .filter(|version| version.at_least(REQUIRED))
        .ok_or(PlatformError::Unsupported)
}

/// A run of decimal digits at the start of `text`, and what follows it.
fn number(text: &str) -> Option<(u32, &str)> {
    let end = text
        .bytes()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, rest) = text.split_at(end);
    digits.parse().ok().map(|value| (value, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn v(major: u32, minor: u32) -> GlVersion {
        GlVersion { major, minor }
    }

    #[test]
    fn parses_the_start_of_real_version_strings() {
        assert_eq!(GlVersion::parse("4.5.0 NVIDIA 566.03"), Some(v(4, 5)));
        assert_eq!(
            GlVersion::parse("4.6 (Core Profile) Mesa 24.0.5"),
            Some(v(4, 6))
        );
        assert_eq!(GlVersion::parse("3.3.0"), Some(v(3, 3)));
        assert_eq!(GlVersion::parse("10.12"), Some(v(10, 12)));
    }

    #[test]
    fn refuses_text_without_a_version_and_es_strings() {
        for text in [
            "",
            "4",
            "4.",
            ".5",
            "x.y",
            "OpenGL ES 3.2 Mesa",
            "v4.5",
            " 4.5",
        ] {
            assert_eq!(GlVersion::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn refuses_numbers_too_large_for_u32() {
        assert_eq!(GlVersion::parse("99999999999.1"), None);
        assert_eq!(GlVersion::parse("4.99999999999"), None);
    }

    #[test]
    fn compares_major_before_minor() {
        assert!(v(4, 5).at_least(REQUIRED));
        assert!(v(4, 6).at_least(REQUIRED));
        assert!(v(5, 0).at_least(REQUIRED));
        assert!(!v(4, 4).at_least(REQUIRED));
        assert!(!v(3, 9).at_least(REQUIRED));
    }

    #[test]
    fn a_context_is_accepted_from_4_5_up() {
        assert_eq!(accept(Some("4.5.0 NVIDIA 566.03")), Ok(v(4, 5)));
        assert_eq!(accept(Some("4.6 (Core Profile) Mesa")), Ok(v(4, 6)));
    }

    #[test]
    fn a_context_is_refused_below_4_5_without_text_and_for_es() {
        let refused = Err(PlatformError::Unsupported);
        assert_eq!(accept(Some("4.4.0")), refused);
        assert_eq!(accept(Some("3.3 Mesa")), refused);
        assert_eq!(accept(Some("OpenGL ES 3.2")), refused);
        assert_eq!(accept(Some("")), refused);
        assert_eq!(accept(None), refused);
    }

    #[test]
    fn the_required_version_is_4_5() {
        assert_eq!((REQUIRED.major, REQUIRED.minor), (4, 5));
    }
}
