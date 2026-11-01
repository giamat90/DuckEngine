//! The `std` file system: directories mounted under virtual prefixes, background reads on one
//! I/O thread owned by the file system.
//!
//! The thread starts at the first [`read_start`](IFileSystem::read_start). The caller polls for
//! completion; nothing is delivered as a callback and no engine code runs on the I/O thread.
//! Synchronous reads go through `std::fs` on the caller's thread and may allocate for the OS
//! path; they are for load time. Polling and taking a finished read make no allocator call.

use crate::error::PlatformError;
use crate::files::{IFileSystem, ReadId, ReadRequest, ReadState, resolve};
use crate::vpath::VPath;
use base::pool::Pool;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;

/// How many background reads can be in flight at once.
const READ_SLOTS: u32 = 32;

struct Queue {
    reads: Pool<ReadRequest>,
    waiting: VecDeque<ReadId>,
    shutdown: bool,
}

struct Shared {
    mounts: Mutex<Vec<(VPath, PathBuf)>>,
    queue: Mutex<Queue>,
    wake: Condvar,
}

/// Directories on disk, mounted under virtual prefixes.
pub struct StdFileSystem {
    shared: Arc<Shared>,
    io_thread: Mutex<Option<JoinHandle<()>>>,
}

impl StdFileSystem {
    /// A file system with no mounts and no I/O thread yet.
    #[must_use]
    pub fn new() -> Self {
        let queue = Queue {
            reads: Pool::with_capacity(READ_SLOTS),
            waiting: VecDeque::with_capacity(READ_SLOTS as usize),
            shutdown: false,
        };
        let shared = Shared {
            mounts: Mutex::new(Vec::new()),
            queue: Mutex::new(queue),
            wake: Condvar::new(),
        };
        Self {
            shared: Arc::new(shared),
            io_thread: Mutex::new(None),
        }
    }

    /// Mounts the directory `dir` under the virtual prefix `prefix`. The longest matching prefix
    /// wins when mounts overlap.
    ///
    /// # Errors
    /// [`PlatformError::InvalidPath`] for a bad or already mounted prefix,
    /// [`PlatformError::NotFound`] when `dir` is not a directory.
    pub fn mount_dir(&self, prefix: &str, dir: impl Into<PathBuf>) -> Result<(), PlatformError> {
        let prefix = VPath::new(prefix)?;
        // The canonical root is what later reads are checked against, so a link inside the
        // directory cannot lead out of it.
        let dir = std::fs::canonicalize(dir.into()).map_err(|e| translate(&e))?;
        if !dir.is_dir() {
            return Err(PlatformError::NotFound);
        }
        let mut mounts = lock(&self.shared.mounts);
        if mounts.iter().any(|(known, _)| *known == prefix) {
            return Err(PlatformError::InvalidPath);
        }
        mounts.push((prefix, dir));
        Ok(())
    }

    fn ensure_io_thread(&self) -> Result<(), PlatformError> {
        let mut thread = lock(&self.io_thread);
        if thread.is_some() {
            return Ok(());
        }
        let shared = Arc::clone(&self.shared);
        let spawned = std::thread::Builder::new()
            .name("duck-file-io".to_owned())
            .spawn(move || io_loop(&shared))
            .map_err(|_| PlatformError::Io)?;
        *thread = Some(spawned);
        Ok(())
    }
}

impl Default for StdFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for StdFileSystem {
    fn drop(&mut self) {
        lock(&self.shared.queue).shutdown = true;
        self.shared.wake.notify_all();
        if let Some(thread) = lock(&self.io_thread).take() {
            // A panicking I/O thread has nothing left to report at shutdown.
            let _ = thread.join();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The canonical OS path of `path`: `NotFound` when no mount holds it, the file is missing, or a
/// link leads out of the mounted directory.
fn os_path(shared: &Shared, path: &VPath) -> Result<PathBuf, PlatformError> {
    let mounts = lock(&shared.mounts);
    let (dir, rest) = resolve(&mounts, path).ok_or(PlatformError::NotFound)?;
    let real = std::fs::canonicalize(dir.join(rest)).map_err(|e| translate(&e))?;
    if real.starts_with(dir) {
        Ok(real)
    } else {
        Err(PlatformError::NotFound)
    }
}

fn translate(error: &std::io::Error) -> PlatformError {
    match error.kind() {
        ErrorKind::NotFound => PlatformError::NotFound,
        ErrorKind::PermissionDenied => PlatformError::Denied,
        _ => PlatformError::Io,
    }
}

/// Opens the file and checks, on the open handle, that it is a regular file.
fn open_file(path: &Path) -> Result<(File, u64), PlatformError> {
    // Windows refuses to open a directory with "access denied": that is "not a file" here.
    if path.is_dir() {
        return Err(PlatformError::NotFound);
    }
    let file = File::open(path).map_err(|e| translate(&e))?;
    let meta = file.metadata().map_err(|e| translate(&e))?;
    if meta.is_file() {
        Ok((file, meta.len()))
    } else {
        Err(PlatformError::NotFound)
    }
}

fn file_size(path: &Path) -> Result<u64, PlatformError> {
    open_file(path).map(|(_, len)| len)
}

/// Reads the whole file into the start of `buf`. A file that grew after it was measured is an
/// `Io` error, not a silently cut read.
fn read_file(path: &Path, buf: &mut [u8]) -> Result<usize, PlatformError> {
    let (mut file, needed) = open_file(path)?;
    let len = usize::try_from(needed).map_err(|_| PlatformError::BufferTooSmall { needed })?;
    let target = buf
        .get_mut(..len)
        .ok_or(PlatformError::BufferTooSmall { needed })?;
    file.read_exact(target).map_err(|e| translate(&e))?;
    let mut probe = [0_u8; 1];
    match file.read(&mut probe) {
        Ok(0) => Ok(len),
        Ok(_) => Err(PlatformError::Io),
        Err(e) => Err(translate(&e)),
    }
}

impl IFileSystem for StdFileSystem {
    fn size(&self, path: &VPath) -> Result<u64, PlatformError> {
        file_size(&os_path(&self.shared, path)?)
    }

    fn read_into(&self, path: &VPath, buf: &mut [u8]) -> Result<usize, PlatformError> {
        read_file(&os_path(&self.shared, path)?, buf)
    }

    fn read_start(&self, path: VPath, buf: Box<[u8]>) -> Result<ReadId, PlatformError> {
        self.ensure_io_thread()?;
        let mut queue = lock(&self.shared.queue);
        let id = queue
            .reads
            .insert(ReadRequest::new(path, buf))
            .map_err(|_| PlatformError::QueueFull)?;
        queue.waiting.push_back(id);
        drop(queue);
        self.shared.wake.notify_one();
        Ok(id)
    }

    fn read_poll(&self, id: ReadId) -> ReadState {
        lock(&self.shared.queue)
            .reads
            .get(id)
            .map_or(ReadState::Failed(PlatformError::NotFound), |r| r.state)
    }

    fn read_take(&self, id: ReadId) -> Option<Box<[u8]>> {
        let mut queue = lock(&self.shared.queue);
        if queue.reads.get(id)?.state == ReadState::Pending {
            return None;
        }
        queue.reads.remove(id)?.buf
    }

    fn has_background_io(&self) -> bool {
        true
    }
}

/// A read taken off the queue: the I/O thread owns its path and buffer until it finishes.
struct Job {
    id: ReadId,
    path: VPath,
    buf: Box<[u8]>,
}

fn io_loop(shared: &Shared) {
    while let Some(mut job) = next_job(shared) {
        let result = os_path(shared, &job.path).and_then(|path| read_file(&path, &mut job.buf));
        finish(shared, job, result);
    }
}

/// Waits for a job; `None` at shutdown.
fn next_job(shared: &Shared) -> Option<Job> {
    let mut queue = lock(&shared.queue);
    loop {
        if queue.shutdown {
            return None;
        }
        if let Some(job) = pop_job(&mut queue) {
            return Some(job);
        }
        queue = shared
            .wake
            .wait(queue)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

/// Takes the next waiting read. The path is cloned under the lock (a small allocation on the I/O
/// thread, never on the caller's). If the I/O thread ever died, `ensure_io_thread` would still
/// see it and reads would stay `Pending`; under `panic = abort` that cannot happen.
fn pop_job(queue: &mut Queue) -> Option<Job> {
    let id = queue.waiting.pop_front()?;
    let read = queue.reads.get_mut(id)?;
    let buf = read.buf.take()?;
    Some(Job {
        id,
        path: read.path.clone(),
        buf,
    })
}

fn finish(shared: &Shared, job: Job, result: Result<usize, PlatformError>) {
    let mut queue = lock(&shared.queue);
    if let Some(read) = queue.reads.get_mut(job.id) {
        read.buf = Some(job.buf);
        read.state = result.map_or_else(ReadState::Failed, ReadState::Done);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{self, FIXTURE_BYTES, FIXTURE_PATH};

    /// A fresh directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("duck-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("temp dir: {e}"));
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn vp(text: &str) -> VPath {
        VPath::new(text).unwrap_or_else(|_| panic!("bad test path {text}"))
    }

    fn fixture_fs(dir: &TempDir) -> StdFileSystem {
        std::fs::write(dir.0.join("hello.txt"), FIXTURE_BYTES).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(dir.0.join("empty.txt"), b"").unwrap_or_else(|e| panic!("{e}"));
        let fs = StdFileSystem::new();
        assert_eq!(fs.mount_dir("test", &dir.0), Ok(()));
        fs
    }

    #[test]
    fn the_std_file_system_passes_the_contract() {
        let dir = TempDir::new("contract");
        contract::files_contract(&fixture_fs(&dir), &|| {});
    }

    #[test]
    fn mounting_checks_the_prefix_and_the_directory() {
        let dir = TempDir::new("mount");
        let fs = StdFileSystem::new();
        assert_eq!(
            fs.mount_dir("../x", &dir.0),
            Err(PlatformError::InvalidPath)
        );
        assert_eq!(
            fs.mount_dir("a", dir.0.join("nope")),
            Err(PlatformError::NotFound)
        );
        assert_eq!(fs.mount_dir("a", &dir.0), Ok(()));
        assert_eq!(fs.mount_dir("a", &dir.0), Err(PlatformError::InvalidPath));
    }

    #[test]
    fn the_longest_prefix_decides_which_directory_serves_a_file() {
        let outer = TempDir::new("outer");
        let inner = TempDir::new("inner");
        std::fs::write(outer.0.join("f.txt"), b"outer").unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(inner.0.join("f.txt"), b"in").unwrap_or_else(|e| panic!("{e}"));
        let fs = StdFileSystem::new();
        assert_eq!(fs.mount_dir("a", &outer.0), Ok(()));
        assert_eq!(fs.mount_dir("a/b", &inner.0), Ok(()));
        assert_eq!(fs.size(&vp("a/f.txt")), Ok(5));
        assert_eq!(fs.size(&vp("a/b/f.txt")), Ok(2));
    }

    #[test]
    fn an_unmounted_path_and_a_directory_are_not_found() {
        let dir = TempDir::new("notfound");
        std::fs::create_dir(dir.0.join("sub")).unwrap_or_else(|e| panic!("{e}"));
        let fs = fixture_fs(&dir);
        assert_eq!(fs.size(&vp("other/x")), Err(PlatformError::NotFound));
        assert_eq!(fs.size(&vp("test/sub")), Err(PlatformError::NotFound));
    }

    #[test]
    fn an_inner_mount_does_not_fall_back_to_the_outer_one_when_it_misses() {
        let outer = TempDir::new("fallback-outer");
        let inner = TempDir::new("fallback-inner");
        std::fs::write(outer.0.join("b"), b"x").unwrap_or_else(|e| panic!("{e}"));
        let fs = StdFileSystem::new();
        assert_eq!(fs.mount_dir("a", &outer.0), Ok(()));
        assert_eq!(fs.mount_dir("a/b", &inner.0), Ok(()));
        assert_eq!(fs.size(&vp("a/b/f")), Err(PlatformError::NotFound));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_that_leads_out_of_the_mounted_directory_is_not_followed() {
        let outside = TempDir::new("link-outside");
        let inside = TempDir::new("link-inside");
        std::fs::write(outside.0.join("secret.txt"), b"s").unwrap_or_else(|e| panic!("{e}"));
        std::os::unix::fs::symlink(&outside.0, inside.0.join("out"))
            .unwrap_or_else(|e| panic!("{e}"));
        let fs = StdFileSystem::new();
        assert_eq!(fs.mount_dir("m", &inside.0), Ok(()));
        assert_eq!(
            fs.size(&vp("m/out/secret.txt")),
            Err(PlatformError::NotFound)
        );
    }

    #[test]
    fn only_a_fixed_number_of_reads_can_be_in_flight() {
        let dir = TempDir::new("full");
        let fs = fixture_fs(&dir);
        for _ in 0..READ_SLOTS {
            let started = fs.read_start(vp(FIXTURE_PATH), vec![0_u8; 16].into_boxed_slice());
            assert!(started.is_ok());
        }
        let refused = fs.read_start(vp(FIXTURE_PATH), vec![0_u8; 16].into_boxed_slice());
        assert_eq!(refused.err(), Some(PlatformError::QueueFull));
    }

    #[test]
    fn dropping_the_file_system_stops_the_io_thread_even_with_reads_waiting() {
        let dir = TempDir::new("drop");
        let fs = fixture_fs(&dir);
        for _ in 0..8 {
            let _ = fs.read_start(vp(FIXTURE_PATH), vec![0_u8; 16].into_boxed_slice());
        }
        drop(fs);
    }

    #[test]
    fn it_reports_a_thread_of_its_own() {
        assert!(StdFileSystem::default().has_background_io());
    }

    #[test]
    fn a_poll_of_an_unknown_read_is_not_found() {
        let dir = TempDir::new("unknown");
        let fs = fixture_fs(&dir);
        let id = fs.read_start(vp(FIXTURE_PATH), vec![0_u8; 16].into_boxed_slice());
        let id = id.unwrap_or_else(|e| panic!("{e}"));
        while fs.read_take(id).is_none() {
            std::thread::yield_now();
        }
        assert_eq!(fs.read_poll(id), ReadState::Failed(PlatformError::NotFound));
    }

    #[test]
    fn dropping_the_file_system_ends_the_io_thread_and_frees_the_shared_state() {
        let dir = TempDir::new("weak");
        let fs = fixture_fs(&dir);
        let id = fs.read_start(vp(FIXTURE_PATH), vec![0_u8; 16].into_boxed_slice());
        assert!(id.is_ok());
        let shared = Arc::downgrade(&fs.shared);
        drop(fs);
        assert!(
            shared.upgrade().is_none(),
            "the I/O thread still holds the state"
        );
    }

    #[test]
    fn operating_system_errors_become_platform_errors() {
        let kind = |kind| translate(&std::io::Error::from(kind));
        assert_eq!(kind(ErrorKind::NotFound), PlatformError::NotFound);
        assert_eq!(kind(ErrorKind::PermissionDenied), PlatformError::Denied);
        assert_eq!(kind(ErrorKind::TimedOut), PlatformError::Io);
    }
}
