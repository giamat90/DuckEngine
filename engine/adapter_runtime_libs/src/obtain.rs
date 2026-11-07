//! Getting the library: download, check, unpack, check again, keep in a cache; and copying it next
//! to the executables.
//!
//! The download and the unpacking are behind [`Tools`] so that the tests run without a network.
//! Nothing is kept unless both hashes match: the cache holds only checked files, a failed attempt
//! leaves nothing behind, and a cached file is checked again before every use.

use crate::manifest::Library;
use crate::sha256::file_sha256_hex;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What the helper needs from the system: fetch a file, take one member out of an archive.
pub trait Tools {
    /// Downloads `url` to the file `dest`.
    ///
    /// # Errors
    /// Any failure to download.
    fn download(&self, url: &str, dest: &Path) -> io::Result<()>;

    /// Unpacks `member` of `archive` into the folder `dir`, keeping its path under `dir`.
    ///
    /// # Errors
    /// Any failure to unpack.
    fn unpack(&self, archive: &Path, member: &str, dir: &Path) -> io::Result<()>;
}

/// Why a library could not be obtained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObtainError {
    /// Offline was asked for and the cache holds no checked copy.
    Offline,
    /// The download or the unpacking failed (what happened).
    Tool(String),
    /// The archive does not hold the member.
    MemberMissing,
    /// The downloaded archive is not the one the manifest pins.
    ArchiveHash { expected: String, found: String },
    /// The unpacked file is not the one the manifest pins.
    FileHash { expected: String, found: String },
}

impl fmt::Display for ObtainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Offline => write!(f, "offline was asked for and there is no cached copy"),
            Self::Tool(what) => write!(f, "{what}"),
            Self::MemberMissing => write!(f, "the archive does not contain the library"),
            Self::ArchiveHash { expected, found } => write!(
                f,
                "the downloaded archive has SHA-256 {found}, not the pinned {expected}; it was discarded"
            ),
            Self::FileHash { expected, found } => write!(
                f,
                "the unpacked library has SHA-256 {found}, not the pinned {expected}; it was discarded"
            ),
        }
    }
}

impl std::error::Error for ObtainError {}

fn tool_error(what: &str, error: &io::Error) -> ObtainError {
    ObtainError::Tool(format!("{what}: {error}"))
}

/// The checked file in the cache, if there is one; a damaged one is removed.
fn cached(path: &Path, expected: &str) -> Option<PathBuf> {
    if !path.is_file() {
        return None;
    }
    if file_sha256_hex(path).is_ok_and(|found| found == expected) {
        return Some(path.to_path_buf());
    }
    let _ = fs::remove_file(path);
    None
}

/// Downloads, checks and unpacks into `work`; returns the unpacked file.
fn fetch(lib: &Library, work: &Path, tools: &dyn Tools) -> Result<PathBuf, ObtainError> {
    let archive = work.join("archive");
    tools
        .download(&lib.url, &archive)
        .map_err(|e| tool_error("the download failed", &e))?;
    let found =
        file_sha256_hex(&archive).map_err(|e| tool_error("the archive cannot be read", &e))?;
    if found != lib.archive_sha256 {
        return Err(ObtainError::ArchiveHash {
            expected: lib.archive_sha256.clone(),
            found,
        });
    }
    let unpacked = work.join("unpacked");
    fs::create_dir_all(&unpacked).map_err(|e| tool_error("no work folder", &e))?;
    tools
        .unpack(&archive, &lib.member, &unpacked)
        .map_err(|e| tool_error("the unpacking failed", &e))?;
    let file = unpacked.join(&lib.member);
    if !file.is_file() {
        return Err(ObtainError::MemberMissing);
    }
    let found = file_sha256_hex(&file).map_err(|e| tool_error("the library cannot be read", &e))?;
    if found != lib.file_sha256 {
        return Err(ObtainError::FileHash {
            expected: lib.file_sha256.clone(),
            found,
        });
    }
    Ok(file)
}

/// The checked library in the cache folder `cache`, fetching it first when it is not there (and
/// `offline` is false). The kept file is `cache/<file sha256>/<file name>`.
///
/// # Errors
/// [`ObtainError`]: offline without a cached copy, a failed tool, a missing member or a hash that
/// is not the pinned one. In every case nothing is left in the cache.
pub fn obtain(
    lib: &Library,
    cache: &Path,
    tools: &dyn Tools,
    offline: bool,
) -> Result<PathBuf, ObtainError> {
    let target = cache.join(&lib.file_sha256).join(&lib.file);
    if let Some(path) = cached(&target, &lib.file_sha256) {
        return Ok(path);
    }
    if offline {
        return Err(ObtainError::Offline);
    }
    let work = cache.join(format!(".work-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| tool_error("no work folder", &e))?;
    let result = fetch(lib, &work, tools).and_then(|file| keep(&file, &target));
    let _ = fs::remove_dir_all(&work);
    result.map(|()| target)
}

fn keep(file: &Path, target: &Path) -> Result<(), ObtainError> {
    if let Some(folder) = target.parent() {
        fs::create_dir_all(folder).map_err(|e| tool_error("the cache cannot be written", &e))?;
    }
    fs::rename(file, target)
        .or_else(|_| fs::copy(file, target).map(|_| ()))
        .map_err(|e| tool_error("the cache cannot be written", &e))
}

/// Copies `source` as `file_name` into each of `dirs` that exists and does not already hold the
/// same bytes; returns the copies made.
///
/// # Errors
/// The error of the first copy that fails.
pub fn place(source: &Path, file_name: &str, dirs: &[PathBuf]) -> io::Result<Vec<PathBuf>> {
    let bytes = fs::read(source)?;
    let mut placed = Vec::new();
    for dir in dirs.iter().filter(|dir| dir.is_dir()) {
        let dest = dir.join(file_name);
        if fs::read(&dest).is_ok_and(|have| have == bytes) {
            continue;
        }
        fs::write(&dest, &bytes)?;
        placed.push(dest);
    }
    Ok(placed)
}

/// The system's own `curl` and `tar`: on Windows the ones in `System32` (Git's GNU `tar` may come
/// first on the search path and cannot read a zip), elsewhere the ones on the search path.
pub struct SystemTools;

fn system_tool(name: &str) -> PathBuf {
    match std::env::var_os("SystemRoot") {
        Some(root) if cfg!(windows) => Path::new(&root)
            .join("System32")
            .join(format!("{name}.exe")),
        _ => PathBuf::from(name),
    }
}

fn run(tool: &str, args: &[&std::ffi::OsStr]) -> io::Result<()> {
    let output = Command::new(system_tool(tool)).args(args).output()?;
    if output.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&output.stderr);
    Err(io::Error::other(format!(
        "{tool} exited with {}: {}",
        output.status,
        said.trim()
    )))
}

impl Tools for SystemTools {
    fn download(&self, url: &str, dest: &Path) -> io::Result<()> {
        run(
            "curl",
            &[
                "--fail".as_ref(),
                "--location".as_ref(),
                "--silent".as_ref(),
                "--show-error".as_ref(),
                "--proto".as_ref(),
                "=https".as_ref(),
                "--max-time".as_ref(),
                "300".as_ref(),
                "--output".as_ref(),
                dest.as_os_str(),
                url.as_ref(),
            ],
        )
    }

    fn unpack(&self, archive: &Path, member: &str, dir: &Path) -> io::Result<()> {
        run(
            "tar",
            &[
                "-xf".as_ref(),
                archive.as_os_str(),
                "-C".as_ref(),
                dir.as_os_str(),
                member.as_ref(),
            ],
        )
    }
}
