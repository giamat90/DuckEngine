//! A single pinned file that a build compiles (a C header), fetched like the runtime library but
//! with no archive and no copy next to the executables: [`obtain_source`] downloads it once into the
//! cache, checks its SHA-256 and returns the path in the cache.
//!
//! The manifest has four keys: `file` (the name kept in the cache), `url` (https), `sha256` (64
//! lower-case hex digits) and the optional `override` (the environment variable that names a file of
//! the user's own, which [`verify_source`] checks against the same hash).

use crate::manifest::{ManifestError, field, is_hash, is_inside, pairs_of, required};
use crate::obtain::{ObtainError, Tools, cached, keep, tool_error};
use crate::sha256::file_sha256_hex;
use std::fs;
use std::path::{Path, PathBuf};

const KEYS: [&str; 4] = ["file", "url", "sha256", "override"];

/// A pinned single file and where the official copy of it comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    /// The file name kept in the cache.
    pub file: String,
    /// The https address of the file.
    pub url: String,
    /// SHA-256 of the file, 64 lower-case hex digits.
    pub sha256: String,
    /// The environment variable that, when set, names a file of the user's own.
    pub override_variable: Option<String>,
}

/// Reads a source manifest.
///
/// # Errors
/// [`ManifestError`] when a key is missing, unknown or repeated, a line is malformed, the address
/// is not https, the hash is not 64 lower-case hex digits or the file name is not a plain name.
pub fn parse_source(text: &str) -> Result<SourceFile, ManifestError> {
    let pairs = pairs_of(text, &KEYS)?;
    let source = SourceFile {
        file: required(&pairs, "file")?,
        url: required(&pairs, "url")?,
        sha256: required(&pairs, "sha256")?,
        override_variable: field(&pairs, "override"),
    };
    if !source.url.starts_with("https://") {
        return Err(ManifestError::NotHttps);
    }
    if !is_hash(&source.sha256) {
        return Err(ManifestError::BadHash("sha256".to_owned()));
    }
    if !is_inside(&source.file) || source.file.contains('/') {
        return Err(ManifestError::BadPath("file".to_owned()));
    }
    Ok(source)
}

/// Checks that `path` holds the bytes with the SHA-256 `expected`.
///
/// # Errors
/// [`ObtainError::Tool`] when the file cannot be read, [`ObtainError::SourceHash`] when it is not
/// the pinned one.
pub fn verify_source(path: &Path, expected: &str) -> Result<(), ObtainError> {
    let found = file_sha256_hex(path).map_err(|e| tool_error("the file cannot be read", &e))?;
    if found == expected {
        Ok(())
    } else {
        Err(ObtainError::SourceHash {
            expected: expected.to_owned(),
            found,
        })
    }
}

fn fetch(source: &SourceFile, work: &Path, tools: &dyn Tools) -> Result<PathBuf, ObtainError> {
    let file = work.join("download");
    tools
        .download(&source.url, &file)
        .map_err(|e| tool_error("the download failed", &e))?;
    verify_source(&file, &source.sha256)?;
    Ok(file)
}

/// The checked file in the cache folder `cache`, fetching it first when it is not there (and
/// `offline` is false). The kept file is `cache/<sha256>/<file>`; a cached file is checked again on
/// every call.
///
/// # Errors
/// [`ObtainError`]: offline without a cached copy, a failed download or a hash that is not the
/// pinned one. In every case nothing is left in the cache.
pub fn obtain_source(
    source: &SourceFile,
    cache: &Path,
    tools: &dyn Tools,
    offline: bool,
) -> Result<PathBuf, ObtainError> {
    let target = cache.join(&source.sha256).join(&source.file);
    if let Some(path) = cached(&target, &source.sha256) {
        return Ok(path);
    }
    if offline {
        return Err(ObtainError::Offline);
    }
    let work = cache.join(format!(".work-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| tool_error("no work folder", &e))?;
    let result = fetch(source, &work, tools).and_then(|file| keep(&file, &target));
    let _ = fs::remove_dir_all(&work);
    result.map(|()| target)
}
