//! The Null file system: files held in memory, background reads completed by the caller's thread.
//!
//! A background read completes at the next [`read_poll`](IFileSystem::read_poll). After
//! [`set_deferred(true)`](MemoryFileSystem::set_deferred) it stays `Pending` until
//! [`complete_all`](MemoryFileSystem::complete_all), so a test can force both orders.

use crate::error::PlatformError;
use crate::files::{IFileSystem, ReadId, ReadRequest, ReadState, fill};
use crate::vpath::VPath;
use base::pool::Pool;
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// How many background reads can be in flight at once.
const READ_SLOTS: u32 = 32;

struct State {
    files: BTreeMap<String, Vec<u8>>,
    reads: Pool<ReadRequest>,
    ids: Vec<ReadId>,
    deferred: bool,
}

/// In-memory files, for tests and the Headless platform.
pub struct MemoryFileSystem {
    state: Mutex<State>,
}

impl MemoryFileSystem {
    /// An empty file system; reads complete at the next poll.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(State {
                files: BTreeMap::new(),
                reads: Pool::with_capacity(READ_SLOTS),
                ids: Vec::with_capacity(READ_SLOTS as usize),
                deferred: false,
            }),
        }
    }

    /// Adds (or replaces) the file at `path`.
    ///
    /// # Errors
    /// [`PlatformError::InvalidPath`] when `path` is not a valid virtual path.
    pub fn add_file(&self, path: &str, bytes: &[u8]) -> Result<(), PlatformError> {
        let path = VPath::new(path)?;
        self.lock()
            .files
            .insert(path.as_str().to_owned(), bytes.to_vec());
        Ok(())
    }

    /// While true, background reads stay `Pending` until [`complete_all`](Self::complete_all).
    pub fn set_deferred(&self, deferred: bool) {
        self.lock().deferred = deferred;
    }

    /// Completes every pending background read now.
    pub fn complete_all(&self) {
        let mut state = self.lock();
        let State {
            files, reads, ids, ..
        } = &mut *state;
        for id in ids.iter() {
            complete_id(files, reads, *id);
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for MemoryFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

fn complete_id(files: &BTreeMap<String, Vec<u8>>, reads: &mut Pool<ReadRequest>, id: ReadId) {
    if let Some(read) = reads.get_mut(id) {
        complete(files, read);
    }
}

fn complete(files: &BTreeMap<String, Vec<u8>>, read: &mut ReadRequest) {
    if read.state != ReadState::Pending {
        return;
    }
    let Some(buf) = read.buf.as_mut() else {
        return;
    };
    read.state = match files.get(read.path.as_str()) {
        None => ReadState::Failed(PlatformError::NotFound),
        Some(bytes) => match fill(bytes, buf) {
            Ok(len) => ReadState::Done(len),
            Err(error) => ReadState::Failed(error),
        },
    };
}

impl IFileSystem for MemoryFileSystem {
    fn size(&self, path: &VPath) -> Result<u64, PlatformError> {
        let state = self.lock();
        let bytes = state
            .files
            .get(path.as_str())
            .ok_or(PlatformError::NotFound)?;
        Ok(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
    }

    fn read_into(&self, path: &VPath, buf: &mut [u8]) -> Result<usize, PlatformError> {
        let state = self.lock();
        let bytes = state
            .files
            .get(path.as_str())
            .ok_or(PlatformError::NotFound)?;
        fill(bytes, buf)
    }

    fn read_start(&self, path: VPath, buf: Box<[u8]>) -> Result<ReadId, PlatformError> {
        let mut state = self.lock();
        let id = state
            .reads
            .insert(ReadRequest::new(path, buf))
            .map_err(|_| PlatformError::QueueFull)?;
        state.ids.push(id);
        Ok(id)
    }

    fn read_poll(&self, id: ReadId) -> ReadState {
        let mut state = self.lock();
        let State {
            files,
            reads,
            deferred,
            ..
        } = &mut *state;
        let Some(read) = reads.get_mut(id) else {
            return ReadState::Failed(PlatformError::NotFound);
        };
        if !*deferred {
            complete(files, read);
        }
        read.state
    }

    fn read_take(&self, id: ReadId) -> Option<Box<[u8]>> {
        let mut state = self.lock();
        if state.reads.get(id)?.state == ReadState::Pending {
            return None;
        }
        state.ids.retain(|known| *known != id);
        state.reads.remove(id)?.buf
    }

    fn has_background_io(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{self, FIXTURE_BYTES, FIXTURE_PATH};

    fn with_fixture() -> MemoryFileSystem {
        let fs = MemoryFileSystem::new();
        assert_eq!(fs.add_file(FIXTURE_PATH, FIXTURE_BYTES), Ok(()));
        assert_eq!(fs.add_file(contract::EMPTY_PATH, b""), Ok(()));
        fs
    }

    fn vp(text: &str) -> VPath {
        VPath::new(text).unwrap_or_else(|_| panic!("bad test path {text}"))
    }

    fn buffer() -> Box<[u8]> {
        vec![0_u8; 32].into_boxed_slice()
    }

    #[test]
    fn the_memory_file_system_passes_the_contract_without_help() {
        contract::files_contract(&with_fixture(), &|| {});
    }

    #[test]
    fn the_memory_file_system_passes_the_contract_when_completion_is_forced() {
        let fs = with_fixture();
        fs.set_deferred(true);
        contract::files_contract(&fs, &|| fs.complete_all());
    }

    #[test]
    fn a_deferred_read_stays_pending_until_complete_all() {
        let fs = with_fixture();
        fs.set_deferred(true);
        let id = fs.read_start(vp(FIXTURE_PATH), buffer());
        let id = id.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(fs.read_poll(id), ReadState::Pending);
        assert!(fs.read_take(id).is_none(), "no buffer while pending");
        fs.complete_all();
        assert_eq!(fs.read_poll(id), ReadState::Done(FIXTURE_BYTES.len()));
    }

    #[test]
    fn an_undeferred_read_completes_at_the_next_poll() {
        let fs = with_fixture();
        let id = fs.read_start(vp(FIXTURE_PATH), buffer());
        let id = id.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(fs.read_poll(id), ReadState::Done(FIXTURE_BYTES.len()));
    }

    #[test]
    fn only_a_fixed_number_of_reads_can_be_in_flight() {
        let fs = with_fixture();
        for _ in 0..READ_SLOTS {
            assert!(fs.read_start(vp(FIXTURE_PATH), buffer()).is_ok());
        }
        let refused = fs.read_start(vp(FIXTURE_PATH), buffer());
        assert_eq!(refused.err(), Some(PlatformError::QueueFull));
    }

    #[test]
    fn a_taken_read_frees_its_slot() {
        let fs = with_fixture();
        let ids: Vec<_> = (0..READ_SLOTS)
            .filter_map(|_| fs.read_start(vp(FIXTURE_PATH), buffer()).ok())
            .collect();
        assert_eq!(ids.len(), READ_SLOTS as usize);
        let first = ids[0];
        fs.read_poll(first);
        assert!(fs.read_take(first).is_some());
        assert!(fs.read_start(vp(FIXTURE_PATH), buffer()).is_ok());
    }

    #[test]
    fn a_bad_path_cannot_be_added() {
        let fs = MemoryFileSystem::new();
        assert_eq!(fs.add_file("../x", b"x"), Err(PlatformError::InvalidPath));
    }

    #[test]
    fn a_replaced_file_is_read_with_its_new_bytes() {
        let fs = with_fixture();
        assert_eq!(fs.add_file(FIXTURE_PATH, b"new"), Ok(()));
        assert_eq!(fs.size(&vp(FIXTURE_PATH)), Ok(3));
    }

    #[test]
    fn it_has_no_thread_of_its_own() {
        assert!(!MemoryFileSystem::default().has_background_io());
    }

    #[test]
    fn taking_one_read_does_not_hide_the_other_pending_reads_from_complete_all() {
        let fs = with_fixture();
        let first = fs.read_start(vp(FIXTURE_PATH), buffer());
        let first = first.unwrap_or_else(|e| panic!("{e}"));
        let second = fs.read_start(vp(FIXTURE_PATH), buffer());
        let second = second.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(fs.read_poll(first), ReadState::Done(FIXTURE_BYTES.len()));
        assert!(fs.read_take(first).is_some());
        fs.set_deferred(true);
        fs.complete_all();
        assert_eq!(fs.read_poll(second), ReadState::Done(FIXTURE_BYTES.len()));
    }
}
