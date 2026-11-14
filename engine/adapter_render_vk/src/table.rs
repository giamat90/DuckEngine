//! A table of the device's own records, keyed by the handles the bookkeeping side issued.
//!
//! The device lets a bookkeeping [`NullDevice`](render::null::NullDevice) hand out the handles
//! and check every call; this table keeps what only the Vulkan side knows (resource ids, sizes) at
//! the slot a handle names, and answers `None` for a handle whose slot was reused (a stale one).
//! The GL adapter has the same table: adapters do not share code.

use base::pool::Handle;
use render::error::RenderError;

/// More slots than any pool of the device can hold; a larger index is not one of its handles.
const MAX_SLOTS: usize = 1 << 20;

/// Records by handle index, each with the generation it belongs to.
pub struct Table<R> {
    slots: Vec<Option<(u32, R)>>,
}

impl<R> Table<R> {
    /// An empty table.
    pub fn new() -> Self {
        Self { slots: Vec::new() }
    }

    /// Stores `record` for `handle`, replacing what an earlier generation left there.
    ///
    /// # Errors
    /// `OutOfMemory` when the table cannot grow to the handle's index; the record comes back
    /// with the error, so whatever it names can still be deleted.
    pub fn insert<M>(&mut self, handle: Handle<M>, record: R) -> Result<(), (RenderError, R)> {
        let Some(index) = self.grow_to(handle.index()) else {
            return Err((RenderError::OutOfMemory, record));
        };
        self.slots[index] = Some((handle.generation(), record));
        Ok(())
    }

    /// Makes room for slot `index` and returns it as a position; `None` when that is not
    /// possible.
    fn grow_to(&mut self, index: u32) -> Option<usize> {
        let index = usize::try_from(index)
            .ok()
            .filter(|index| *index < MAX_SLOTS)?;
        if index >= self.slots.len() {
            let more = index + 1 - self.slots.len();
            self.slots.try_reserve(more).ok()?;
            self.slots.resize_with(index + 1, || None);
        }
        Some(index)
    }

    /// The record of a live handle.
    pub fn get<M>(&self, handle: Handle<M>) -> Option<&R> {
        let slot = self.slots.get(usize::try_from(handle.index()).ok()?)?;
        match slot {
            Some((generation, record)) if *generation == handle.generation() => Some(record),
            _ => None,
        }
    }

    /// The record of a live handle, to change.
    pub fn get_mut<M>(&mut self, handle: Handle<M>) -> Option<&mut R> {
        let slot = self.slots.get_mut(usize::try_from(handle.index()).ok()?)?;
        match slot {
            Some((generation, record)) if *generation == handle.generation() => Some(record),
            _ => None,
        }
    }

    /// Takes the record of a live handle out; a stale handle leaves the table as it is.
    pub fn remove<M>(&mut self, handle: Handle<M>) -> Option<R> {
        self.get::<M>(handle)?;
        let slot = self.slots.get_mut(usize::try_from(handle.index()).ok()?)?;
        slot.take().map(|(_, record)| record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Marker;

    fn handle(index: u32, generation: u32) -> Handle<Marker> {
        Handle::from_raw(index, generation)
    }

    #[test]
    fn a_stored_record_is_found_by_its_handle_only() {
        let mut table = Table::new();
        assert!(table.insert(handle(2, 1), "two").is_ok());
        assert_eq!(table.get(handle(2, 1)), Some(&"two"));
        assert_eq!(table.get(handle(2, 2)), None, "another generation");
        assert_eq!(table.get(handle(1, 1)), None, "an empty slot");
        assert_eq!(table.get(handle(9, 1)), None, "past the end");
    }

    #[test]
    fn a_reused_slot_hides_the_old_generation() {
        let mut table = Table::new();
        assert!(table.insert(handle(0, 1), 10).is_ok());
        assert_eq!(table.remove(handle(0, 1)), Some(10));
        assert!(table.insert(handle(0, 2), 20).is_ok());
        assert_eq!(table.get(handle(0, 1)), None);
        assert_eq!(table.get(handle(0, 2)), Some(&20));
    }

    #[test]
    fn removing_a_stale_handle_changes_nothing() {
        let mut table = Table::new();
        assert!(table.insert(handle(0, 2), 5).is_ok());
        assert_eq!(table.remove(handle(0, 1)), None);
        assert_eq!(table.get(handle(0, 2)), Some(&5));
        assert_eq!(table.remove(handle(7, 1)), None);
    }

    #[test]
    fn a_record_can_be_changed_in_place() {
        let mut table = Table::new();
        assert!(table.insert(handle(1, 1), 1).is_ok());
        if let Some(value) = table.get_mut(handle(1, 1)) {
            *value = 9;
        }
        assert_eq!(table.get(handle(1, 1)), Some(&9));
        assert!(table.get_mut(handle(1, 4)).is_none());
    }

    #[test]
    fn an_index_beyond_any_pool_is_out_of_memory() {
        let mut table = Table::new();
        assert_eq!(
            table.insert(handle(u32::MAX, 1), 7),
            Err((RenderError::OutOfMemory, 7)),
            "the record comes back"
        );
        assert_eq!(
            table.insert(handle(1 << 20, 1), 8),
            Err((RenderError::OutOfMemory, 8))
        );
        assert!(table.insert(handle((1 << 20) - 1, 1), 0).is_ok());
    }
}
