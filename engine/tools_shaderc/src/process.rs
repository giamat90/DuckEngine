//! Running an external tool as a child process, and the scratch directory it works in.
//!
//! The program and every argument are separate values handed to the operating system; there is no
//! shell and no command string. The child's output goes to files in the work directory (not pipes),
//! so a chatty tool can never block on a full pipe, and the run is stopped after a time limit.
//! Anything read back from a tool's files is bounded first, so a tool that writes without end
//! cannot make this program allocate without end.
//!
//! **Trust.** The tools are downloaded programs. Their version is checked before use (see
//! [`crate::tools`]), which catches a wrong build but is not a hash check and does not prove who
//! built the file; they run with the caller's rights. Keep them in a folder only you can write.

use core::fmt;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// How often a running child is checked.
const POLL: Duration = Duration::from_millis(10);

/// The most a tool's captured output (standard output or error) may hold.
pub const MAX_OUTPUT_LEN: usize = 1 << 20;

/// Why a tool could not be run to completion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolError {
    /// The program could not be started (missing, not executable).
    Start(String),
    /// The program ran past the time limit and was killed.
    Timeout,
    /// A file in the work directory could not be created or read.
    Io(String),
    /// A file the tool wrote is larger than the limit for it.
    TooLarge,
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start(why) => write!(f, "cannot start the tool: {why}"),
            Self::Timeout => f.write_str("the tool did not finish in time and was stopped"),
            Self::Io(why) => write!(f, "work file problem: {why}"),
            Self::TooLarge => f.write_str("the tool wrote more than the allowed size"),
        }
    }
}

impl std::error::Error for ToolError {}

/// What a finished child printed and whether it succeeded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolRun {
    /// The exit status was success.
    pub success: bool,
    /// Standard output, then standard error, `\n` line ends.
    pub output: String,
}

/// A fresh directory under the system temporary directory, removed when dropped.
#[derive(Debug)]
pub struct WorkDir {
    path: PathBuf,
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

impl WorkDir {
    /// Creates a new empty directory named from the process id and a counter.
    ///
    /// # Errors
    /// The error of the failed directory creation (also when the name is already taken).
    pub fn create() -> io::Result<Self> {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!("duck-shaderc-{}-{n}", std::process::id());
        let path = std::env::temp_dir().join(name);
        make_private_dir(&path)?;
        Ok(Self { path })
    }

    /// The directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn io_error(e: &io::Error) -> ToolError {
    ToolError::Io(e.to_string())
}

/// Owner-only permissions for the scratch directory on Unix.
#[cfg(unix)]
const PRIVATE_DIR_MODE: u32 = 0o700;

/// Creates `path` (it must not exist), readable by its owner only on Unix. On Windows the
/// temporary directory is already private to the user.
fn make_private_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(PRIVATE_DIR_MODE).create(path)
    }
    #[cfg(not(unix))]
    {
        fs::create_dir(path)
    }
}

/// Reads a whole file, at most `limit` bytes; a longer file is [`ToolError::TooLarge`] and only
/// `limit + 1` bytes are ever held.
///
/// # Errors
/// [`ToolError::Io`] when the file cannot be opened or read, [`ToolError::TooLarge`] past `limit`.
pub fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, ToolError> {
    let file = File::open(path).map_err(|e| io_error(&e))?;
    let cap = u64::try_from(limit).map_err(|_| ToolError::TooLarge)?;
    let mut bytes = Vec::new();
    file.take(cap.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| io_error(&e))?;
    if bytes.len() > limit {
        return Err(ToolError::TooLarge);
    }
    Ok(bytes)
}

/// A tool's captured output as text with `\n` line ends, bounded by [`MAX_OUTPUT_LEN`].
fn read_output(path: &Path) -> Result<String, ToolError> {
    let bytes = read_bounded(path, MAX_OUTPUT_LEN)?;
    Ok(crate::tools::normalize_newlines(&String::from_utf8_lossy(
        &bytes,
    )))
}

fn wait(child: &mut std::process::Child, timeout: Duration) -> Result<bool, ToolError> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| io_error(&e))? {
            return Ok(status.success());
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ToolError::Timeout);
        }
        thread::sleep(POLL);
    }
}

/// Runs `program` with `args` in `work`, writing its output to `<tag>.out` and `<tag>.err` there.
///
/// # Errors
/// [`ToolError::Start`] when the program cannot be started, [`ToolError::Timeout`] after `timeout`,
/// [`ToolError::Io`] for a problem with the output files. A program that runs and fails is not an
/// error here: see [`ToolRun::success`].
pub fn run(
    program: &Path,
    args: &[OsString],
    work: &Path,
    tag: &str,
    timeout: Duration,
) -> Result<ToolRun, ToolError> {
    let out_path = work.join(format!("{tag}.out"));
    let err_path = work.join(format!("{tag}.err"));
    let out = File::create(&out_path).map_err(|e| io_error(&e))?;
    let err = File::create(&err_path).map_err(|e| io_error(&e))?;
    let mut child = Command::new(program)
        .args(args)
        .current_dir(work)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(|e| ToolError::Start(format!("{}: {e}", program.display())))?;
    let success = wait(&mut child, timeout)?;
    let output = format!("{}{}", read_output(&out_path)?, read_output(&err_path)?);
    Ok(ToolRun { success, output })
}

#[cfg(test)]
mod tests;
