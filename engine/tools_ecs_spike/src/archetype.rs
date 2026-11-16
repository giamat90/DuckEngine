//! Archetype tables: one dense table (a column per component plus the entity list) for each set
//! of components, generic over the column implementation, so the safe and the type-erased
//! variants run the same code. With `hit_in_table` false the `Hit` marker lives in a sparse set
//! beside the tables: that is the hybrid.

use crate::column::{ColumnOps, SafeColumn};
use crate::components::{
    BODY, Body, COMPONENT_COUNT, HIT, Hit, Kind, RADIUS, Radius, SPRITE, Sprite, TRANSFORM,
    Transform, VELOCITY, Velocity, Words, bit, body_of, digest_of, mix, radius_of, sprite_of,
    transform_of, velocity_of,
};
use crate::entity::{Entity, EntityTable};
use crate::raw_column::RawColumn;
use crate::sparse::{SparseSet, set_digest};
use crate::store::Store;

/// One table: the components of one mask, one row per entity.
struct Archetype<C> {
    mask: u8,
    ids: Vec<usize>,
    cols: Vec<C>,
    entities: Vec<Entity>,
}

fn make_column<C: ColumnOps>(id: usize) -> Option<C> {
    match id {
        TRANSFORM => Some(C::new::<Transform>()),
        VELOCITY => Some(C::new::<Velocity>()),
        SPRITE => Some(C::new::<Sprite>()),
        BODY => Some(C::new::<Body>()),
        RADIUS => Some(C::new::<Radius>()),
        HIT => Some(C::new::<Hit>()),
        _ => None,
    }
}

impl<C: ColumnOps> Archetype<C> {
    fn new(mask: u8) -> Self {
        let (ids, cols): (Vec<usize>, Vec<C>) = (0..COMPONENT_COUNT)
            .filter(|&id| mask & bit(id) != 0)
            .filter_map(|id| make_column::<C>(id).map(|column| (id, column)))
            .unzip();
        Self {
            mask,
            ids,
            cols,
            entities: Vec::new(),
        }
    }

    fn has(&self, need: u8) -> bool {
        self.mask & need == need
    }

    fn col_index(&self, id: usize) -> Option<usize> {
        self.ids.iter().position(|&candidate| candidate == id)
    }

    fn slice<T: 'static>(&self, id: usize) -> &[T] {
        self.col_index(id)
            .and_then(|i| self.cols.get(i))
            .map_or(&[], C::as_slice::<T>)
    }

    fn slice_mut<T: 'static>(&mut self, id: usize) -> &mut [T] {
        match self.col_index(id).and_then(|i| self.cols.get_mut(i)) {
            Some(column) => column.as_mut_slice::<T>(),
            None => &mut [],
        }
    }

    /// One column written and one read, found by id.
    fn write_read<W: 'static, R: 'static>(
        &mut self,
        write: usize,
        read: usize,
    ) -> (&mut [W], &[R]) {
        let (Some(w), Some(r)) = (self.col_index(write), self.col_index(read)) else {
            return (&mut [], &[]);
        };
        match self.cols.get_disjoint_mut([w, r]) {
            Ok([w, r]) => (w.as_mut_slice::<W>(), r.as_slice::<R>()),
            Err(_) => (&mut [], &[]),
        }
    }

    fn push<T: 'static>(&mut self, id: usize, value: T) {
        if let Some(column) = self.col_index(id).and_then(|i| self.cols.get_mut(i)) {
            column.push(value);
        }
    }

    fn push_entity(&mut self, entity: Entity, serial: u32) {
        self.entities.push(entity);
        self.push(TRANSFORM, transform_of(serial));
        self.push(SPRITE, sprite_of(serial));
        if self.mask & bit(VELOCITY) != 0 {
            self.push(VELOCITY, velocity_of(serial));
        }
        if self.mask & bit(BODY) != 0 {
            self.push(BODY, body_of(serial));
        }
        if self.mask & bit(RADIUS) != 0 {
            self.push(RADIUS, radius_of(serial));
        }
    }

    fn bytes(&self) -> usize {
        let rows = self.entities.len();
        self.cols
            .iter()
            .map(|c| c.row_bytes() * rows)
            .sum::<usize>()
            + rows * size_of::<Entity>()
    }
}

/// The index of the table of `kind` (`with_hit` selects the twin that holds the marker).
const fn table_of(kind: Kind, with_hit: bool) -> usize {
    let base = match kind {
        Kind::Box => 0,
        Kind::Disc => 1,
        Kind::Particle => 2,
        Kind::Wall => 3,
    };
    if with_hit {
        base + Kind::ALL.len()
    } else {
        base
    }
}

fn integrate_table<C: ColumnOps>(table: &mut Archetype<C>) {
    let (transforms, velocities) = table.write_read::<Transform, Velocity>(TRANSFORM, VELOCITY);
    for (t, v) in transforms.iter_mut().zip(velocities) {
        t.x += v.x;
        t.y += v.y;
    }
}

fn extract_table<C: ColumnOps>(table: &Archetype<C>) -> u64 {
    let transforms = table.slice::<Transform>(TRANSFORM);
    let sprites = table.slice::<Sprite>(SPRITE);
    let bodies = table.slice::<Body>(BODY);
    let rows = transforms.iter().zip(sprites).zip(bodies);
    rows.fold(0, |sum, ((t, s), b)| sum.wrapping_add(mix(t, s, b)))
}

fn column_digest<T: Words + 'static, C: ColumnOps>(table: &Archetype<C>, id: usize) -> u64 {
    let rows = table.slice::<T>(id).iter().zip(&table.entities);
    rows.fold(0, |sum, (value, e)| {
        sum.wrapping_add(digest_of(id, e.index, value))
    })
}

fn table_digest<C: ColumnOps>(table: &Archetype<C>) -> u64 {
    column_digest::<Transform, C>(table, TRANSFORM)
        .wrapping_add(column_digest::<Velocity, C>(table, VELOCITY))
        .wrapping_add(column_digest::<Sprite, C>(table, SPRITE))
        .wrapping_add(column_digest::<Body, C>(table, BODY))
        .wrapping_add(column_digest::<Radius, C>(table, RADIUS))
        .wrapping_add(column_digest::<Hit, C>(table, HIT))
}

fn bump_table<C: ColumnOps>(table: &mut Archetype<C>) {
    for t in table.slice_mut::<Transform>(TRANSFORM) {
        t.rot += 1.0;
    }
}

fn hash_table<C: ColumnOps>(table: &Archetype<C>, seed: u64) -> u64 {
    let transforms = table.slice::<Transform>(TRANSFORM);
    table
        .entities
        .iter()
        .zip(transforms)
        .fold(seed, |hash, (e, t)| {
            (hash ^ u64::from(e.index) ^ u64::from(t.x.to_bits())).wrapping_mul(0x0100_0000_01B3)
        })
}

/// A world of archetype tables over the column implementation `C`.
pub struct ArchWorld<C: ColumnOps> {
    label: &'static str,
    entities: EntityTable,
    locs: Vec<(u32, u32)>,
    tables: Vec<Archetype<C>>,
    hit_in_table: bool,
    hits: SparseSet<Hit>,
    live: usize,
}

/// Archetype tables with typed vectors behind a trait object: no `unsafe`.
pub type ArchetypeSafe = ArchWorld<SafeColumn>;
/// Archetype tables with type-erased byte columns: `unsafe` in `raw_column`.
pub type ArchetypeRaw = ArchWorld<RawColumn>;

impl<C: ColumnOps> ArchWorld<C> {
    fn build(capacity: usize, hit_in_table: bool, label: &'static str) -> Self {
        let mut tables: Vec<Archetype<C>> = Kind::ALL
            .iter()
            .map(|kind| Archetype::new(kind.mask()))
            .collect();
        if hit_in_table {
            tables.extend(
                Kind::ALL
                    .iter()
                    .map(|kind| Archetype::new(kind.mask() | bit(HIT))),
            );
        }
        for table in &mut tables {
            table.entities.reserve(capacity);
        }
        Self {
            label,
            entities: EntityTable::with_capacity(capacity),
            locs: Vec::with_capacity(capacity),
            tables,
            hit_in_table,
            hits: if hit_in_table {
                SparseSet::default()
            } else {
                SparseSet::with_capacity(capacity, capacity)
            },
            live: 0,
        }
    }

    fn set_loc(&mut self, index: u32, table: usize, row: usize) {
        let value = (
            u32::try_from(table).unwrap_or(u32::MAX),
            u32::try_from(row).unwrap_or(u32::MAX),
        );
        if self.locs.len() <= index as usize {
            self.locs.resize(index as usize + 1, (0, 0));
        }
        if let Some(slot) = self.locs.get_mut(index as usize) {
            *slot = value;
        }
    }

    fn loc(&self, entity: Entity) -> Option<(usize, usize)> {
        if !self.entities.is_alive(entity) {
            return None;
        }
        let (table, row) = *self.locs.get(entity.index as usize)?;
        Some((table as usize, row as usize))
    }

    fn set_hit_sparse(&mut self, entity: Entity, on: bool) {
        if on {
            self.hits.insert(entity.index, Hit(entity.index));
        } else {
            self.hits.remove(entity.index);
        }
    }

    fn bump_tables(&mut self) {
        let need = bit(TRANSFORM) | bit(HIT);
        for table in self.tables.iter_mut().filter(|t| t.has(need)) {
            bump_table(table);
        }
    }

    fn bump_marked(&mut self) {
        for row in 0..self.hits.len() {
            let index = self.hits.indices().get(row).copied().unwrap_or(u32::MAX);
            self.bump_one(index);
        }
    }

    fn bump_one(&mut self, index: u32) {
        let Some(&(table, row)) = self.locs.get(index as usize) else {
            return;
        };
        let transform = self
            .tables
            .get_mut(table as usize)
            .and_then(|t| t.slice_mut::<Transform>(TRANSFORM).get_mut(row as usize));
        if let Some(t) = transform {
            t.rot += 1.0;
        }
    }

    /// Moves the entity at (`from`, `row`) to table `to`, keeping the components both share.
    fn move_to(&mut self, entity: Entity, at: (usize, usize), to: usize, adds_hit: bool) -> bool {
        let (from, row) = at;
        let Ok([src, dst]) = self.tables.get_disjoint_mut([from, to]) else {
            return false;
        };
        for (id, column) in src.ids.iter().zip(src.cols.iter_mut()) {
            match dst.col_index(*id).and_then(|i| dst.cols.get_mut(i)) {
                Some(target) => column.swap_remove_into(row, target),
                None => column.swap_remove_drop(row),
            }
        }
        src.entities.swap_remove(row);
        dst.entities.push(entity);
        if adds_hit {
            dst.push(HIT, Hit(entity.index));
        }
        let new_row = dst.entities.len() - 1;
        let moved = src.entities.get(row).copied();
        self.set_loc(entity.index, to, new_row);
        if let Some(moved) = moved {
            self.set_loc(moved.index, from, row);
        }
        true
    }
}

impl<C: ColumnOps> Store for ArchWorld<C> {
    fn name(&self) -> &'static str {
        self.label
    }

    fn with_capacity(entities: usize) -> Self {
        Self::build(entities, true, C::LABEL)
    }

    fn spawn(&mut self, kind: Kind, serial: u32) -> Entity {
        let entity = self.entities.allocate();
        let table = table_of(kind, false);
        let row = self.tables.get(table).map_or(0, |t| t.entities.len());
        if let Some(t) = self.tables.get_mut(table) {
            t.push_entity(entity, serial);
        }
        self.set_loc(entity.index, table, row);
        self.live += 1;
        entity
    }

    fn despawn(&mut self, entity: Entity) -> bool {
        let Some((table, row)) = self.loc(entity) else {
            return false;
        };
        let Some(t) = self.tables.get_mut(table) else {
            return false;
        };
        for column in &mut t.cols {
            column.swap_remove_drop(row);
        }
        t.entities.swap_remove(row);
        let moved = t.entities.get(row).copied();
        if let Some(moved) = moved {
            self.set_loc(moved.index, table, row);
        }
        self.entities.release(entity);
        if !self.hit_in_table {
            self.hits.remove(entity.index);
        }
        self.live -= 1;
        true
    }

    fn set_hit(&mut self, entity: Entity, on: bool) -> bool {
        let Some((table, row)) = self.loc(entity) else {
            return false;
        };
        if !self.hit_in_table {
            self.set_hit_sparse(entity, on);
            return true;
        }
        let twins = Kind::ALL.len();
        let has_hit = table >= twins;
        if on == has_hit {
            return true;
        }
        let to = if on { table + twins } else { table - twins };
        self.move_to(entity, (table, row), to, on)
    }

    fn is_alive(&self, entity: Entity) -> bool {
        self.entities.is_alive(entity)
    }

    fn len(&self) -> usize {
        self.live
    }

    fn sum_x(&self) -> f64 {
        let mut sum = 0.0_f64;
        for table in self.tables.iter().filter(|t| t.has(bit(TRANSFORM))) {
            sum += table
                .slice::<Transform>(TRANSFORM)
                .iter()
                .map(|t| f64::from(t.x))
                .sum::<f64>();
        }
        sum
    }

    fn integrate(&mut self) {
        let need = bit(TRANSFORM) | bit(VELOCITY);
        for table in self.tables.iter_mut().filter(|t| t.has(need)) {
            integrate_table(table);
        }
    }

    fn extract(&self) -> u64 {
        let need = bit(TRANSFORM) | bit(SPRITE) | bit(BODY);
        let mut sum = 0_u64;
        for table in self.tables.iter().filter(|t| t.has(need)) {
            sum = sum.wrapping_add(extract_table(table));
        }
        sum
    }

    fn bump_hit(&mut self) {
        if self.hit_in_table {
            self.bump_tables();
        } else {
            self.bump_marked();
        }
    }

    fn digest(&self) -> u64 {
        let tables = self.tables.iter().map(table_digest);
        tables
            .fold(0, u64::wrapping_add)
            .wrapping_add(set_digest(HIT, &self.hits))
    }

    fn order_hash(&self) -> u64 {
        let mut hash = 0xCBF2_9CE4_8422_2325_u64;
        for table in &self.tables {
            hash = hash_table(table, hash);
        }
        hash
    }

    fn data_bytes(&self) -> usize {
        self.tables.iter().map(Archetype::bytes).sum::<usize>()
            + self.locs.len() * size_of::<(u32, u32)>()
            + self.hits.bytes()
            + self.entities.slots() * 5
    }
}

/// Archetype tables for the stable components and a sparse set for the marker that is added and
/// removed often.
pub struct Hybrid(ArchWorld<SafeColumn>);

impl Store for Hybrid {
    fn name(&self) -> &'static str {
        "hybrid"
    }

    fn with_capacity(entities: usize) -> Self {
        Self(ArchWorld::build(entities, false, "hybrid"))
    }

    fn spawn(&mut self, kind: Kind, serial: u32) -> Entity {
        self.0.spawn(kind, serial)
    }

    fn despawn(&mut self, entity: Entity) -> bool {
        self.0.despawn(entity)
    }

    fn set_hit(&mut self, entity: Entity, on: bool) -> bool {
        self.0.set_hit(entity, on)
    }

    fn is_alive(&self, entity: Entity) -> bool {
        self.0.is_alive(entity)
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    fn sum_x(&self) -> f64 {
        self.0.sum_x()
    }

    fn integrate(&mut self) {
        self.0.integrate();
    }

    fn extract(&self) -> u64 {
        self.0.extract()
    }

    fn bump_hit(&mut self) {
        self.0.bump_hit();
    }

    fn digest(&self) -> u64 {
        self.0.digest()
    }

    fn order_hash(&self) -> u64 {
        self.0.order_hash()
    }

    fn data_bytes(&self) -> usize {
        self.0.data_bytes()
    }
}
