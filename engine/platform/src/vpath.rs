//! Virtual paths: the only path type that crosses the file system port.
//!
//! A [`VPath`] is UTF-8 with `/` separators, relative, case-sensitive, without `.` or `..`
//! segments, without drive letters or backslashes. A raw OS path never reaches engine code.

use crate::error::PlatformError;

/// A validated virtual path such as `assets/textures/duck.png`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VPath(String);

impl VPath {
    /// Validates `text`.
    ///
    /// # Errors
    /// [`PlatformError::InvalidPath`] when it is empty, starts or ends with `/`, has an empty,
    /// `.` or `..` segment, or contains `\`, `:` or a control character.
    pub fn new(text: &str) -> Result<Self, PlatformError> {
        if text.split('/').all(valid_segment) {
            Ok(Self(text.to_owned()))
        } else {
            Err(PlatformError::InvalidPath)
        }
    }

    /// The path as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// What is left of this path after `prefix` and the `/` that follows it, or `None` when the
    /// path is not below `prefix` (whole segments only: `assets` is not a prefix of `assets2/x`).
    #[must_use]
    pub fn strip_prefix(&self, prefix: &Self) -> Option<&str> {
        self.0.strip_prefix(prefix.as_str())?.strip_prefix('/')
    }
}

/// Names Windows treats as devices, with or without an extension.
const DEVICE_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A segment that means the same on Windows and Linux: no `.` or `..`, no trailing dot or space,
/// no device name, none of the characters Windows refuses in a name.
fn valid_segment(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or_default();
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment.ends_with(['.', ' '])
        && !DEVICE_NAMES.iter().any(|d| d.eq_ignore_ascii_case(stem))
        && !segment
            .chars()
            .any(|c| c.is_control() || r#"\:*?<>|""#.contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_relative_paths_are_accepted() {
        for text in [
            "a",
            "assets/duck.png",
            "a/b/c/d.txt",
            "Ünï/çode.txt",
            "a b/c",
        ] {
            assert_eq!(
                VPath::new(text).map(|p| p.as_str().to_owned()),
                Ok(text.to_owned())
            );
        }
    }

    #[test]
    fn bad_paths_are_refused() {
        let bad = [
            "",
            "/",
            "/abs",
            "a/",
            "a//b",
            ".",
            "..",
            "a/./b",
            "a/../b",
            "../a",
            "a\\b",
            "C:/x",
            "a:b",
            "a\0b",
            "a\nb",
            "a/b.",
            "a/b ",
            "CON",
            "con.txt",
            "a/NUL.txt",
            "lpt1",
            "a*b",
            "a?b",
            "a<b",
            "a>b",
            "a|b",
            "a\"b",
        ];
        for text in bad {
            assert_eq!(
                VPath::new(text),
                Err(PlatformError::InvalidPath),
                "{text:?}"
            );
        }
    }

    #[test]
    fn paths_are_case_sensitive() {
        assert_ne!(VPath::new("A").ok(), VPath::new("a").ok());
    }

    #[test]
    fn strip_prefix_cuts_whole_segments_only() {
        let prefix = VPath::new("assets").unwrap_or_else(|_| unreachable!());
        let inside = VPath::new("assets/x/y.txt").unwrap_or_else(|_| unreachable!());
        let sibling = VPath::new("assets2/x").unwrap_or_else(|_| unreachable!());
        let same = VPath::new("assets").unwrap_or_else(|_| unreachable!());
        assert_eq!(inside.strip_prefix(&prefix), Some("x/y.txt"));
        assert_eq!(sibling.strip_prefix(&prefix), None);
        assert_eq!(same.strip_prefix(&prefix), None);
    }
}
