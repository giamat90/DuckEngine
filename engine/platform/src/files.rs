//! The file system port: read-only, virtual paths, caller-owned buffers, polled background reads.
//!
//! Mounting is specific to each implementation (a directory for [`StdFileSystem`], in-memory
//! files for [`MemoryFileSystem`]), so it is not part of the trait; reading is.
//!
//! A background read takes the buffer by value (`Box<[u8]>`, allocated by the caller at load
//! time) so another thread can fill it in safe code, and gives it back through
//! [`IFileSystem::read_take`] once the read has ended. Completion is polled by the caller, never
//! delivered as a callback.
//!
//! [`StdFileSystem`]: crate::std_files::StdFileSystem
//! [`MemoryFileSystem`]: crate::memory_files::MemoryFileSystem

use crate::error::PlatformError;
use crate::vpath::VPath;
use base::pool::Handle;

/// One background read, as the implementations keep it in a fixed pool. Opaque to callers: they
/// only hold its [`ReadId`].
#[derive(Debug)]
pub struct ReadRequest {
    pub(crate) path: VPath,
    pub(crate) buf: Option<Box<[u8]>>,
    pub(crate) state: ReadState,
}

impl ReadRequest {
    /// A read that has not started.
    pub(crate) fn new(path: VPath, buf: Box<[u8]>) -> Self {
        Self {
            path,
            buf: Some(buf),
            state: ReadState::Pending,
        }
    }
}

/// Names one background read; stale once its buffer was taken.
pub type ReadId = Handle<ReadRequest>;

/// How far a background read has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadState {
    /// Not finished.
    Pending,
    /// Finished: this many bytes are in the buffer.
    Done(usize),
    /// Ended with an error; the buffer can still be taken. An unknown or already taken
    /// [`ReadId`] reads as `Failed(NotFound)`.
    Failed(PlatformError),
}

/// Read-only access to files by virtual path; see the module documentation.
pub trait IFileSystem: Send + Sync {
    /// The size of the file in bytes.
    ///
    /// # Errors
    /// [`PlatformError::NotFound`] when no mount holds the path, [`PlatformError::Denied`] and
    /// [`PlatformError::Io`] for operating-system failures.
    fn size(&self, path: &VPath) -> Result<u64, PlatformError>;

    /// Reads the whole file into the start of `buf` and returns its length.
    ///
    /// # Errors
    /// As [`size`](Self::size), and [`PlatformError::BufferTooSmall`] (naming the needed size)
    /// when `buf` cannot hold the file.
    fn read_into(&self, path: &VPath, buf: &mut [u8]) -> Result<usize, PlatformError>;

    /// Starts reading the whole file into `buf`; the buffer is owned by the file system until
    /// [`read_take`](Self::read_take). A missing file or a too-small buffer is reported by
    /// [`read_poll`](Self::read_poll), not here.
    ///
    /// # Errors
    /// [`PlatformError::QueueFull`] when the fixed number of reads in flight is reached (the
    /// path and buffer are dropped), [`PlatformError::Io`] when the I/O thread cannot start.
    fn read_start(&self, path: VPath, buf: Box<[u8]>) -> Result<ReadId, PlatformError>;

    /// The state of a background read. Gets no new memory.
    fn read_poll(&self, id: ReadId) -> ReadState;

    /// The buffer of a read that is no longer pending, and the end of the [`ReadId`]. `None`
    /// while pending or for an unknown id. Gets no new memory (it frees the path of the read).
    fn read_take(&self, id: ReadId) -> Option<Box<[u8]>>;

    /// True when reads run on a thread of their own.
    fn has_background_io(&self) -> bool;
}

/// Copies `bytes` to the start of `buf`.
pub(crate) fn fill(bytes: &[u8], buf: &mut [u8]) -> Result<usize, PlatformError> {
    let needed = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    match buf.get_mut(..bytes.len()) {
        Some(head) => {
            head.copy_from_slice(bytes);
            Ok(bytes.len())
        }
        None => Err(PlatformError::BufferTooSmall { needed }),
    }
}

/// The mount with the longest prefix of `path`, and the rest of the path below it.
pub(crate) fn resolve<'a, M>(
    mounts: &'a [(VPath, M)],
    path: &'a VPath,
) -> Option<(&'a M, &'a str)> {
    mounts
        .iter()
        .filter_map(|(prefix, mount)| Some((prefix, mount, path.strip_prefix(prefix)?)))
        .max_by_key(|(prefix, _, _)| prefix.as_str().len())
        .map(|(_, mount, rest)| (mount, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vp(text: &str) -> VPath {
        VPath::new(text).unwrap_or_else(|_| panic!("bad test path {text}"))
    }

    #[test]
    fn fill_copies_into_a_big_enough_buffer() {
        let mut buf = [0_u8; 6];
        assert_eq!(fill(b"abc", &mut buf), Ok(3));
        assert_eq!(&buf, b"abc\0\0\0");
        assert_eq!(fill(b"", &mut buf), Ok(0));
        assert_eq!(fill(b"abcdef", &mut buf), Ok(6));
    }

    #[test]
    fn fill_names_the_needed_size_when_the_buffer_is_small() {
        let mut buf = [0_u8; 2];
        assert_eq!(
            fill(b"abc", &mut buf),
            Err(PlatformError::BufferTooSmall { needed: 3 })
        );
        assert_eq!(buf, [0, 0]);
    }

    #[test]
    fn the_longest_matching_prefix_wins() {
        let mounts = [(vp("a"), 1), (vp("a/b"), 2), (vp("c"), 3)];
        assert_eq!(resolve(&mounts, &vp("a/b/x")), Some((&2, "x")));
        assert_eq!(resolve(&mounts, &vp("a/y")), Some((&1, "y")));
        assert_eq!(resolve(&mounts, &vp("c/z")), Some((&3, "z")));
        assert_eq!(resolve(&mounts, &vp("d/z")), None);
        assert_eq!(resolve(&mounts, &vp("a2/z")), None);
    }
}
