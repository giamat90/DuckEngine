//! The manifest of one runtime library: plain `key = value` lines (`#` starts a comment), read from
//! a file that lives next to the `build.rs` that uses it.

use std::fmt;

/// A runtime library and where the official copy of it comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Library {
    /// The file name the program loads, placed next to the executables (for example `glfw3.dll`).
    pub file: String,
    /// The https address of the official archive.
    pub url: String,
    /// SHA-256 of the archive, 64 lower-case hex digits.
    pub archive_sha256: String,
    /// The path of the file inside the archive, with `/`.
    pub member: String,
    /// SHA-256 of the file, 64 lower-case hex digits.
    pub file_sha256: String,
    /// The target operating system the library is for (`windows`).
    pub os: String,
    /// The environment variable that, when set, names a library of the user's own: nothing is
    /// fetched then.
    pub override_variable: Option<String>,
}

/// Why a manifest was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// A required key is missing.
    Missing(String),
    /// The address is not https.
    NotHttps,
    /// A hash is not 64 lower-case hex digits (the key).
    BadHash(String),
    /// A path leaves its folder or is absolute (the key).
    BadPath(String),
    /// A line is not `key = value`.
    BadLine(String),
    /// A key appears twice.
    Duplicate(String),
    /// A key this version does not know.
    UnknownKey(String),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(key) => write!(f, "the manifest has no `{key}`"),
            Self::NotHttps => write!(f, "the manifest `url` must start with https://"),
            Self::BadHash(key) => write!(f, "`{key}` must be 64 lower-case hex digits"),
            Self::BadPath(key) => write!(f, "`{key}` must be a relative path inside its folder"),
            Self::BadLine(line) => write!(f, "`{line}` is not `key = value`"),
            Self::Duplicate(key) => write!(f, "`{key}` appears twice"),
            Self::UnknownKey(key) => write!(f, "unknown key `{key}`"),
        }
    }
}

impl std::error::Error for ManifestError {}

const KEYS: [&str; 7] = [
    "file",
    "url",
    "archive_sha256",
    "member",
    "file_sha256",
    "os",
    "override",
];

pub(crate) fn is_hash(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// True for a path that stays inside its folder: not empty, not absolute, no `..`, no `\`, not
/// starting with `-` (a command line would read it as an option).
pub(crate) fn is_inside(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('-')
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != ".." && part != ".")
}

pub(crate) fn field(pairs: &[(String, String)], key: &str) -> Option<String> {
    pairs
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.clone())
}

pub(crate) fn required(pairs: &[(String, String)], key: &str) -> Result<String, ManifestError> {
    field(pairs, key).ok_or_else(|| ManifestError::Missing(key.to_owned()))
}

/// The `key = value` lines of `text`, refusing keys that are not in `keys` and repeated keys.
pub(crate) fn pairs_of(text: &str, keys: &[&str]) -> Result<Vec<(String, String)>, ManifestError> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| ManifestError::BadLine(line.to_owned()))?;
        let (key, value) = (key.trim().to_owned(), value.trim().to_owned());
        if !keys.contains(&key.as_str()) {
            return Err(ManifestError::UnknownKey(key));
        }
        if pairs.iter().any(|(name, _)| *name == key) {
            return Err(ManifestError::Duplicate(key));
        }
        pairs.push((key, value));
    }
    Ok(pairs)
}

/// Reads a manifest.
///
/// # Errors
/// [`ManifestError`] when a key is missing, unknown or repeated, a line is malformed, the address
/// is not https, a hash is not 64 lower-case hex digits or a path is not inside its folder.
pub fn parse(text: &str) -> Result<Library, ManifestError> {
    let pairs = pairs_of(text, &KEYS)?;
    let library = Library {
        file: required(&pairs, "file")?,
        url: required(&pairs, "url")?,
        archive_sha256: required(&pairs, "archive_sha256")?,
        member: required(&pairs, "member")?,
        file_sha256: required(&pairs, "file_sha256")?,
        os: required(&pairs, "os")?,
        override_variable: field(&pairs, "override"),
    };
    if !library.url.starts_with("https://") {
        return Err(ManifestError::NotHttps);
    }
    for (key, hash) in [
        ("archive_sha256", &library.archive_sha256),
        ("file_sha256", &library.file_sha256),
    ] {
        if !is_hash(hash) {
            return Err(ManifestError::BadHash(key.to_owned()));
        }
    }
    if !is_inside(&library.file) || library.file.contains('/') {
        return Err(ManifestError::BadPath("file".to_owned()));
    }
    if !is_inside(&library.member) {
        return Err(ManifestError::BadPath("member".to_owned()));
    }
    Ok(library)
}
