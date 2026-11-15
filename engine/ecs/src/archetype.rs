//! An archetype table: one dense column per component type of one set of components, and the
//! entities that own the rows. Rows are in spawn order, with `swap_remove` on despawn and on a
//! structural change (ADR-0003).

use crate::column::{Column, typed, typed_mut};
use crate::component::{Component, ComponentId};
use crate::entity::Entity;
use crate::error::EcsError;

pub(crate) struct Archetype {
    /// Sorted ascending, no duplicates; `columns[i]` holds `components[i]`.
    components: Vec<ComponentId>,
    columns: Vec<Box<dyn Column>>,
    entities: Vec<Entity>,
    /// Cache: the table reached by adding component `i` (indexed by component id).
    add_edges: Vec<Option<u32>>,
    /// Cache: the table reached by removing component `i`.
    remove_edges: Vec<Option<u32>>,
}

impl Archetype {
    pub(crate) fn new(components: Vec<ComponentId>, columns: Vec<Box<dyn Column>>) -> Self {
        Self {
            components,
            columns,
            entities: Vec::new(),
            add_edges: Vec::new(),
            remove_edges: Vec::new(),
        }
    }

    pub(crate) fn components(&self) -> &[ComponentId] {
        &self.components
    }

    pub(crate) fn entities(&self) -> &[Entity] {
        &self.entities
    }

    pub(crate) fn len(&self) -> usize {
        self.entities.len()
    }

    pub(crate) fn has(&self, id: ComponentId) -> bool {
        self.column_index(id).is_some()
    }

    fn column_index(&self, id: ComponentId) -> Option<usize> {
        self.components.binary_search(&id).ok()
    }

    /// The rows of component `id` as `&Vec<T>`.
    pub(crate) fn column<T: Component>(&self, id: ComponentId) -> Option<&Vec<T>> {
        let column = self.columns.get(self.column_index(id)?)?;
        typed::<T>(column.as_ref())
    }

    /// The rows of component `id` as `&mut Vec<T>`.
    pub(crate) fn column_mut<T: Component>(&mut self, id: ComponentId) -> Option<&mut Vec<T>> {
        let index = self.column_index(id)?;
        typed_mut::<T>(self.columns.get_mut(index)?.as_mut())
    }

    /// Appends `entity` as a new row of an entity with no column values yet (the caller pushes
    /// one value per column). Returns the row.
    pub(crate) fn push_entity(&mut self, entity: Entity) -> Result<u32, EcsError> {
        let row = u32::try_from(self.entities.len()).map_err(|_| EcsError::TooManyEntities)?;
        self.entities.push(entity);
        Ok(row)
    }

    /// Drops every component of row `row` and its entity entry. Returns the entity that was moved
    /// into the hole, if any.
    pub(crate) fn remove_row(&mut self, row: usize) -> Result<Option<Entity>, EcsError> {
        self.check_row(row)?;
        for column in &mut self.columns {
            column.swap_remove_drop(row)?;
        }
        Ok(self.fill_hole(row))
    }

    /// Moves row `row` into `dst`: components both tables have move over, the others are dropped.
    /// The caller adds values for components only `dst` has. Returns the entity that was moved
    /// into the hole, if any.
    pub(crate) fn move_row_into(
        &mut self,
        row: usize,
        dst: &mut Self,
    ) -> Result<Option<Entity>, EcsError> {
        let entity = *self.entities.get(row).ok_or(EcsError::StorageMismatch)?;
        for (id, column) in self.components.iter().zip(self.columns.iter_mut()) {
            match dst.column_index(*id).and_then(|i| dst.columns.get_mut(i)) {
                Some(target) => column.swap_remove_into(row, target.as_mut())?,
                None => column.swap_remove_drop(row)?,
            }
        }
        dst.entities.push(entity);
        Ok(self.fill_hole(row))
    }

    fn check_row(&self, row: usize) -> Result<(), EcsError> {
        if row < self.entities.len() {
            Ok(())
        } else {
            Err(EcsError::StorageMismatch)
        }
    }

    fn fill_hole(&mut self, row: usize) -> Option<Entity> {
        self.entities.swap_remove(row);
        self.entities.get(row).copied()
    }

    pub(crate) fn add_edge(&self, id: ComponentId) -> Option<u32> {
        edge(&self.add_edges, id)
    }

    pub(crate) fn remove_edge(&self, id: ComponentId) -> Option<u32> {
        edge(&self.remove_edges, id)
    }

    pub(crate) fn set_add_edge(&mut self, id: ComponentId, target: u32) {
        set_edge(&mut self.add_edges, id, target);
    }

    pub(crate) fn set_remove_edge(&mut self, id: ComponentId, target: u32) {
        set_edge(&mut self.remove_edges, id, target);
    }
}

fn edge(edges: &[Option<u32>], id: ComponentId) -> Option<u32> {
    edges
        .get(usize::try_from(id.index()).ok()?)
        .copied()
        .flatten()
}

fn set_edge(edges: &mut Vec<Option<u32>>, id: ComponentId, target: u32) {
    let Ok(index) = usize::try_from(id.index()) else {
        return;
    };
    if edges.len() <= index {
        edges.resize(index + 1, None);
    }
    if let Some(slot) = edges.get_mut(index) {
        *slot = Some(target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::column::new_column;

    fn table_with_one_row() -> Archetype {
        let mut table = Archetype::new(vec![ComponentId(0)], vec![new_column::<u32>()]);
        let entity = Entity::from_raw(0, 0);
        assert_eq!(table.push_entity(entity), Ok(0));
        if let Some(rows) = table.column_mut::<u32>(ComponentId(0)) {
            rows.push(7);
        }
        table
    }

    #[test]
    fn remove_row_refuses_a_row_past_the_end() {
        let mut empty = Archetype::new(Vec::new(), Vec::new());
        assert_eq!(empty.remove_row(0), Err(EcsError::StorageMismatch));
        let mut table = table_with_one_row();
        assert_eq!(table.remove_row(1), Err(EcsError::StorageMismatch));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn remove_row_drops_the_last_row_without_moving_another() {
        let mut table = table_with_one_row();
        assert_eq!(table.remove_row(0), Ok(None));
        assert_eq!(table.len(), 0);
    }

    #[test]
    fn a_table_finds_its_columns_by_component_id() {
        let table = table_with_one_row();
        assert!(table.has(ComponentId(0)));
        assert!(!table.has(ComponentId(1)));
        assert_eq!(table.column::<u32>(ComponentId(0)), Some(&vec![7]));
        assert!(table.column::<u8>(ComponentId(0)).is_none());
        assert_eq!(table.components(), &[ComponentId(0)]);
    }

    #[test]
    fn edges_start_empty_and_remember_what_was_set() {
        let mut table = table_with_one_row();
        assert_eq!(
            (
                table.add_edge(ComponentId(3)),
                table.remove_edge(ComponentId(3))
            ),
            (None, None)
        );
        table.set_add_edge(ComponentId(3), 9);
        table.set_remove_edge(ComponentId(1), 4);
        assert_eq!(table.add_edge(ComponentId(3)), Some(9));
        assert_eq!(table.add_edge(ComponentId(2)), None);
        assert_eq!(table.remove_edge(ComponentId(1)), Some(4));
        assert_eq!(table.remove_edge(ComponentId(3)), None);
    }
}
