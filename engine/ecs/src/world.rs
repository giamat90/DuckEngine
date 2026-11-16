//! The world: entities, components and the archetype tables that store them (ADR-0003).

use crate::archetype::Archetype;
use crate::column::{Column, new_column};
use crate::component::{Component, ComponentId};
use crate::entity::{Entity, EntityTable, Location};
use crate::error::EcsError;
use std::any::TypeId;
use std::collections::BTreeMap;

/// Builds an empty column for one registered component type.
type ColumnFactory = fn() -> Box<dyn Column>;

/// Index of the table with no component, where every entity is spawned.
const EMPTY_TABLE: usize = 0;

/// All entities and their components.
///
/// Components live in archetype tables: one dense table per set of component types, one column
/// per type, rows in spawn order. A structural change (adding or removing a component) moves the
/// entity's row to another table with `swap_remove`. Iteration order is table creation order, then
/// row order; no hash is iterated, so the order is a function of the history of calls.
///
/// A world with `K` component types can reach up to `2^K` tables (one per set that really occurs),
/// each with two edge caches of `K` entries; the sandbox's handful of types stays far from that.
///
/// After a warm-up that has created the tables and the transitions between them and has grown the
/// columns, spawning, despawning, inserting and removing make no allocator call. Registering every
/// component type up front with [`World::register`] keeps the first use out of the hot path.
pub struct World {
    entities: EntityTable,
    ids: BTreeMap<TypeId, ComponentId>,
    factories: Vec<ColumnFactory>,
    archetypes: Vec<Archetype>,
    tables: BTreeMap<Vec<ComponentId>, u32>,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    /// An empty world with its empty table.
    #[must_use]
    pub fn new() -> Self {
        let mut tables = BTreeMap::new();
        tables.insert(Vec::new(), 0);
        Self {
            entities: EntityTable::new(),
            ids: BTreeMap::new(),
            factories: Vec::new(),
            archetypes: vec![Archetype::new(Vec::new(), Vec::new())],
            tables,
        }
    }

    /// Registers component type `T` and returns its id; registering again returns the same id.
    /// Ids follow the order of first use.
    ///
    /// # Errors
    /// [`EcsError::TooManyComponents`] when ids no longer fit 32 bits.
    pub fn register<T: Component>(&mut self) -> Result<ComponentId, EcsError> {
        if let Some(id) = self.ids.get(&TypeId::of::<T>()) {
            return Ok(*id);
        }
        let id = ComponentId(
            u32::try_from(self.factories.len()).map_err(|_| EcsError::TooManyComponents)?,
        );
        self.factories.push(new_column::<T>);
        self.ids.insert(TypeId::of::<T>(), id);
        Ok(id)
    }

    /// Spawns an entity with no component.
    ///
    /// # Errors
    /// [`EcsError::TooManyEntities`] when slot indices no longer fit 32 bits.
    pub fn spawn(&mut self) -> Result<Entity, EcsError> {
        let row =
            u32::try_from(self.table(EMPTY_TABLE)?.len()).map_err(|_| EcsError::TooManyEntities)?;
        let entity = self.entities.allocate(Location { archetype: 0, row })?;
        self.table_mut(EMPTY_TABLE)?.push_entity(entity)?;
        Ok(entity)
    }

    /// Despawns an entity and drops its components. Its handle is stale from now on.
    ///
    /// # Errors
    /// [`EcsError::StaleEntity`] when the entity is not alive.
    pub fn despawn(&mut self, entity: Entity) -> Result<(), EcsError> {
        let at = self
            .entities
            .location(entity)
            .ok_or(EcsError::StaleEntity)?;
        let moved = self
            .table_mut(index(at.archetype)?)?
            .remove_row(index(at.row)?)?;
        self.entities.release(entity)?;
        self.relocate_moved(moved, at)
    }

    /// Adds component `value` to `entity`, or replaces the one it has (in place, no table move).
    ///
    /// # Errors
    /// [`EcsError::StaleEntity`] when the entity is not alive; [`EcsError::TooManyComponents`] or
    /// [`EcsError::TooManyEntities`] at the 32-bit limits.
    pub fn insert<T: Component>(&mut self, entity: Entity, value: T) -> Result<(), EcsError> {
        let at = self
            .entities
            .location(entity)
            .ok_or(EcsError::StaleEntity)?;
        let id = self.register::<T>()?;
        if self.table(index(at.archetype)?)?.has(id) {
            return self.replace(at, id, value);
        }
        let target = self.add_target(at.archetype, id)?;
        let new_row = self.move_entity(at, target)?;
        self.table_mut(index(target)?)?
            .column_mut::<T>(id)
            .ok_or(EcsError::StorageMismatch)?
            .push(value);
        self.entities.relocate(
            entity,
            Location {
                archetype: target,
                row: new_row,
            },
        )
    }

    /// Removes component `T` from `entity` and drops its value.
    ///
    /// # Errors
    /// [`EcsError::StaleEntity`] when the entity is not alive; [`EcsError::MissingComponent`]
    /// when it has no `T`.
    pub fn remove<T: Component>(&mut self, entity: Entity) -> Result<(), EcsError> {
        let at = self
            .entities
            .location(entity)
            .ok_or(EcsError::StaleEntity)?;
        let id = *self
            .ids
            .get(&TypeId::of::<T>())
            .ok_or(EcsError::MissingComponent)?;
        if !self.table(index(at.archetype)?)?.has(id) {
            return Err(EcsError::MissingComponent);
        }
        let target = self.remove_target(at.archetype, id)?;
        let new_row = self.move_entity(at, target)?;
        self.entities.relocate(
            entity,
            Location {
                archetype: target,
                row: new_row,
            },
        )
    }

    /// The `T` component of `entity`, if it is alive and has one.
    #[must_use]
    pub fn get<T: Component>(&self, entity: Entity) -> Option<&T> {
        let at = self.entities.location(entity)?;
        let id = *self.ids.get(&TypeId::of::<T>())?;
        self.archetypes
            .get(index(at.archetype).ok()?)?
            .column::<T>(id)?
            .get(index(at.row).ok()?)
    }

    /// The `T` component of `entity`, mutably.
    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Option<&mut T> {
        let at = self.entities.location(entity)?;
        let id = *self.ids.get(&TypeId::of::<T>())?;
        self.archetypes
            .get_mut(index(at.archetype).ok()?)?
            .column_mut::<T>(id)?
            .get_mut(index(at.row).ok()?)
    }

    /// Whether `entity` is alive and has a `T`.
    #[must_use]
    pub fn has<T: Component>(&self, entity: Entity) -> bool {
        self.get::<T>(entity).is_some()
    }

    /// Whether `entity` is alive.
    #[must_use]
    pub fn is_alive(&self, entity: Entity) -> bool {
        self.entities.is_alive(entity)
    }

    /// Number of live entities.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Whether there is no live entity.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.len() == 0
    }

    /// Number of archetype tables, the empty one included.
    #[must_use]
    pub fn archetype_count(&self) -> usize {
        self.archetypes.len()
    }

    /// Live entities in storage order: table creation order, then row order.
    pub fn entities(&self) -> impl Iterator<Item = Entity> + '_ {
        self.archetypes
            .iter()
            .flat_map(|table| table.entities().iter().copied())
    }

    /// The id of a registered component type, if any.
    pub(crate) fn component_id(&self, type_id: TypeId) -> Option<ComponentId> {
        self.ids.get(&type_id).copied()
    }

    /// Every table, mutably, in creation order.
    pub(crate) fn tables_mut(&mut self) -> &mut [Archetype] {
        &mut self.archetypes
    }

    fn table(&self, at: usize) -> Result<&Archetype, EcsError> {
        self.archetypes.get(at).ok_or(EcsError::StorageMismatch)
    }

    fn table_mut(&mut self, at: usize) -> Result<&mut Archetype, EcsError> {
        self.archetypes.get_mut(at).ok_or(EcsError::StorageMismatch)
    }

    fn replace<T: Component>(
        &mut self,
        at: Location,
        id: ComponentId,
        value: T,
    ) -> Result<(), EcsError> {
        let slot = self
            .table_mut(index(at.archetype)?)?
            .column_mut::<T>(id)
            .and_then(|rows| rows.get_mut(index(at.row).ok()?))
            .ok_or(EcsError::StorageMismatch)?;
        *slot = value;
        Ok(())
    }

    /// Moves the row at `at` from its table to table `target` and fixes the location of the
    /// entity that fills the hole. Returns the new row.
    fn move_entity(&mut self, at: Location, target: u32) -> Result<u32, EcsError> {
        let (from, to) = two_mut(&mut self.archetypes, index(at.archetype)?, index(target)?)
            .ok_or(EcsError::StorageMismatch)?;
        let moved = from.move_row_into(index(at.row)?, to)?;
        let new_row = u32::try_from(to.len() - 1).map_err(|_| EcsError::TooManyEntities)?;
        self.relocate_moved(moved, at)?;
        Ok(new_row)
    }

    /// An entity that was moved into a freed row now lives at `hole`.
    fn relocate_moved(&mut self, moved: Option<Entity>, hole: Location) -> Result<(), EcsError> {
        match moved {
            Some(other) => self.entities.relocate(other, hole),
            None => Ok(()),
        }
    }

    fn add_target(&mut self, from: u32, id: ComponentId) -> Result<u32, EcsError> {
        let source = self.table(index(from)?)?;
        if let Some(target) = source.add_edge(id) {
            return Ok(target);
        }
        let mut components = source.components().to_vec();
        if let Err(at) = components.binary_search(&id) {
            components.insert(at, id);
        }
        let target = self.table_for(components)?;
        self.table_mut(index(from)?)?.set_add_edge(id, target);
        Ok(target)
    }

    fn remove_target(&mut self, from: u32, id: ComponentId) -> Result<u32, EcsError> {
        let source = self.table(index(from)?)?;
        if let Some(target) = source.remove_edge(id) {
            return Ok(target);
        }
        let mut components = source.components().to_vec();
        components.retain(|have| *have != id);
        let target = self.table_for(components)?;
        self.table_mut(index(from)?)?.set_remove_edge(id, target);
        Ok(target)
    }

    /// The table for a sorted set of components; created when it does not exist yet.
    fn table_for(&mut self, components: Vec<ComponentId>) -> Result<u32, EcsError> {
        if let Some(found) = self.tables.get(&components) {
            return Ok(*found);
        }
        let mut columns = Vec::with_capacity(components.len());
        for id in &components {
            let make = self
                .factories
                .get(index(id.index())?)
                .ok_or(EcsError::StorageMismatch)?;
            columns.push(make());
        }
        let at = u32::try_from(self.archetypes.len()).map_err(|_| EcsError::TooManyComponents)?;
        self.archetypes
            .push(Archetype::new(components.clone(), columns));
        self.tables.insert(components, at);
        Ok(at)
    }
}

fn index(value: u32) -> Result<usize, EcsError> {
    usize::try_from(value).map_err(|_| EcsError::TooManyEntities)
}

/// Two distinct elements of a slice, both mutable.
fn two_mut<T>(items: &mut [T], a: usize, b: usize) -> Option<(&mut T, &mut T)> {
    if a.max(b) >= items.len() {
        None
    } else if a < b {
        let (low, high) = items.split_at_mut(b);
        Some((low.get_mut(a)?, high.first_mut()?))
    } else if b < a {
        let (low, high) = items.split_at_mut(a);
        Some((high.first_mut()?, low.get_mut(b)?))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::two_mut;

    #[test]
    fn two_mut_returns_both_in_the_order_asked() {
        let mut items = [10, 20, 30];
        assert_eq!(
            two_mut(&mut items, 0, 2).map(|(a, b)| (*a, *b)),
            Some((10, 30))
        );
        assert_eq!(
            two_mut(&mut items, 2, 0).map(|(a, b)| (*a, *b)),
            Some((30, 10))
        );
        assert_eq!(
            two_mut(&mut items, 1, 0).map(|(a, b)| (*a, *b)),
            Some((20, 10))
        );
    }

    #[test]
    fn two_mut_refuses_one_element_twice_or_out_of_range() {
        let mut items = [1, 2];
        assert!(two_mut(&mut items, 1, 1).is_none());
        assert!(two_mut(&mut items, 0, 5).is_none());
        assert!(two_mut(&mut items, 5, 0).is_none());
    }
}
