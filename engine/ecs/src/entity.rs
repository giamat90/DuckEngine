//! Entities: generation-checked handles and the table that issues them.

use crate::error::EcsError;
use base::pool::Handle;

/// Marker that makes an [`Entity`] a different type from every other `Handle`.
#[derive(Debug, Clone, Copy)]
pub enum EntityTag {}

/// An entity: slot index plus generation (eight bytes, `Copy`). A handle kept after the entity was
/// despawned is refused by every world call, never dereferenced.
pub type Entity = Handle<EntityTag>;

/// Where an entity's components live: an archetype and a row in its table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Location {
    pub(crate) archetype: u32,
    pub(crate) row: u32,
}

#[derive(Debug)]
struct Slot {
    generation: u32,
    location: Option<Location>,
}

/// Issues entities and remembers where each live one is stored. Freed slots are reused last in,
/// first out, so the index sequence is a function of the spawn and despawn history.
#[derive(Debug, Default)]
pub(crate) struct EntityTable {
    slots: Vec<Slot>,
    free: Vec<u32>,
    live: usize,
}

impl EntityTable {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Number of live entities.
    pub(crate) fn len(&self) -> usize {
        self.live
    }

    /// Issues an entity stored at `location`.
    pub(crate) fn allocate(&mut self, location: Location) -> Result<Entity, EcsError> {
        let index = match self.free.pop() {
            Some(index) => index,
            None => self.new_slot()?,
        };
        let slot = self.slot_mut(index)?;
        slot.location = Some(location);
        let generation = slot.generation;
        self.live += 1;
        Ok(Entity::from_raw(index, generation))
    }

    fn new_slot(&mut self) -> Result<u32, EcsError> {
        let index = u32::try_from(self.slots.len()).map_err(|_| EcsError::TooManyEntities)?;
        self.slots.push(Slot {
            generation: 0,
            location: None,
        });
        Ok(index)
    }

    /// The location of a live entity; `None` when the handle is stale.
    pub(crate) fn location(&self, entity: Entity) -> Option<Location> {
        let slot = self.slots.get(usize::try_from(entity.index()).ok()?)?;
        if slot.generation == entity.generation() {
            slot.location
        } else {
            None
        }
    }

    /// Whether `entity` is alive.
    pub(crate) fn is_alive(&self, entity: Entity) -> bool {
        self.location(entity).is_some()
    }

    /// Records a new location for a live entity (its row moved).
    pub(crate) fn relocate(&mut self, entity: Entity, location: Location) -> Result<(), EcsError> {
        if !self.is_alive(entity) {
            return Err(EcsError::StaleEntity);
        }
        self.slot_mut(entity.index())?.location = Some(location);
        Ok(())
    }

    /// Frees the slot of a live entity and bumps its generation. A slot whose generation would
    /// overflow is retired (never reused) so an old handle can never match again.
    pub(crate) fn release(&mut self, entity: Entity) -> Result<Location, EcsError> {
        let location = self.location(entity).ok_or(EcsError::StaleEntity)?;
        let slot = self.slot_mut(entity.index())?;
        slot.location = None;
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
            self.free.push(entity.index());
        }
        self.live -= 1;
        Ok(location)
    }

    /// The live entity in slot `index`, if any, for tests that walk the table.
    #[cfg(test)]
    fn set_generation(&mut self, index: u32, generation: u32) {
        if let Some(slot) = self.slots.get_mut(index as usize) {
            slot.generation = generation;
        }
    }

    fn slot_mut(&mut self, index: u32) -> Result<&mut Slot, EcsError> {
        let index = usize::try_from(index).map_err(|_| EcsError::TooManyEntities)?;
        self.slots.get_mut(index).ok_or(EcsError::StaleEntity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HERE: Location = Location {
        archetype: 0,
        row: 0,
    };

    fn issue(table: &mut EntityTable) -> Entity {
        table
            .allocate(HERE)
            .unwrap_or_else(|e| panic!("allocate: {e}"))
    }

    #[test]
    fn the_first_entity_is_slot_zero_generation_zero() {
        let mut table = EntityTable::new();
        let entity = issue(&mut table);
        assert_eq!((entity.index(), entity.generation()), (0, 0));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn entities_get_consecutive_slots() {
        let mut table = EntityTable::new();
        let a = issue(&mut table);
        let b = issue(&mut table);
        assert_eq!((a.index(), b.index()), (0, 1));
    }

    #[test]
    fn a_live_entity_reports_its_location() {
        let mut table = EntityTable::new();
        let there = Location {
            archetype: 3,
            row: 7,
        };
        let entity = table
            .allocate(there)
            .unwrap_or_else(|e| panic!("allocate: {e}"));
        assert_eq!(table.location(entity), Some(there));
        assert!(table.is_alive(entity));
    }

    #[test]
    fn a_released_entity_is_stale() {
        let mut table = EntityTable::new();
        let entity = issue(&mut table);
        assert_eq!(table.release(entity), Ok(HERE));
        assert!(!table.is_alive(entity));
        assert_eq!(table.location(entity), None);
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn releasing_twice_is_refused() {
        let mut table = EntityTable::new();
        let entity = issue(&mut table);
        assert_eq!(table.release(entity), Ok(HERE));
        assert_eq!(table.release(entity), Err(EcsError::StaleEntity));
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn a_reused_slot_has_a_new_generation_and_the_old_handle_stays_stale() {
        let mut table = EntityTable::new();
        let old = issue(&mut table);
        assert_eq!(table.release(old), Ok(HERE));
        let new = issue(&mut table);
        assert_eq!((new.index(), new.generation()), (0, 1));
        assert!(table.is_alive(new));
        assert!(!table.is_alive(old));
    }

    #[test]
    fn freed_slots_are_reused_last_in_first_out() {
        let mut table = EntityTable::new();
        let a = issue(&mut table);
        let b = issue(&mut table);
        assert_eq!(table.release(a), Ok(HERE));
        assert_eq!(table.release(b), Ok(HERE));
        assert_eq!(issue(&mut table).index(), b.index());
        assert_eq!(issue(&mut table).index(), a.index());
    }

    #[test]
    fn a_handle_that_was_never_issued_is_stale() {
        let table = EntityTable::new();
        assert!(!table.is_alive(Entity::from_raw(5, 0)));
    }

    #[test]
    fn a_forged_generation_is_refused() {
        let mut table = EntityTable::new();
        let entity = issue(&mut table);
        let forged = Entity::from_raw(entity.index(), entity.generation() + 1);
        assert!(!table.is_alive(forged));
    }

    #[test]
    fn relocate_moves_a_live_entity_and_refuses_a_stale_one() {
        let mut table = EntityTable::new();
        let entity = issue(&mut table);
        let there = Location {
            archetype: 1,
            row: 2,
        };
        assert_eq!(table.relocate(entity, there), Ok(()));
        assert_eq!(table.location(entity), Some(there));
        assert_eq!(table.release(entity), Ok(there));
        assert_eq!(table.relocate(entity, HERE), Err(EcsError::StaleEntity));
    }

    #[test]
    fn a_slot_at_the_last_generation_is_retired_not_reused() {
        let mut table = EntityTable::new();
        let first = issue(&mut table);
        assert_eq!(table.release(first), Ok(HERE));
        table.set_generation(first.index(), u32::MAX);
        let last = issue(&mut table);
        assert_eq!(last.generation(), u32::MAX);
        assert_eq!(table.release(last), Ok(HERE));
        let next = issue(&mut table);
        assert_ne!(next.index(), last.index());
        assert!(!table.is_alive(last));
    }
}
