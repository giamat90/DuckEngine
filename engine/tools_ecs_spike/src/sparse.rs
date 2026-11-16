//! Sparse sets, one per component, and the world built on them.

use crate::components::{
    BODY, Body, HIT, Hit, Kind, RADIUS, Radius, SPRITE, Sprite, TRANSFORM, Transform, VELOCITY,
    Velocity, Words, body_of, digest_of, mix, radius_of, sprite_of, transform_of, velocity_of,
};
use crate::entity::{Entity, EntityTable};
use crate::store::Store;

/// Marks a sparse slot with no dense row.
const ABSENT: u32 = u32::MAX;

/// Values of one component type, dense, plus an index from the entity slot to the dense row.
#[derive(Debug)]
pub struct SparseSet<T> {
    sparse: Vec<u32>,
    indices: Vec<u32>,
    data: Vec<T>,
}

impl<T> Default for SparseSet<T> {
    fn default() -> Self {
        Self {
            sparse: Vec::new(),
            indices: Vec::new(),
            data: Vec::new(),
        }
    }
}

impl<T> SparseSet<T> {
    /// An empty set with room for `slots` entity slots and `rows` values.
    #[must_use]
    pub fn with_capacity(slots: usize, rows: usize) -> Self {
        Self {
            sparse: vec![ABSENT; slots],
            indices: Vec::with_capacity(rows),
            data: Vec::with_capacity(rows),
        }
    }

    /// Number of values.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether the set holds no value.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    fn row_of(&self, index: u32) -> Option<usize> {
        let row = *self.sparse.get(index as usize)?;
        (row != ABSENT).then_some(row as usize)
    }

    /// Sets the value of slot `index`, replacing a previous one.
    pub fn insert(&mut self, index: u32, value: T) {
        if let Some(row) = self.row_of(index) {
            self.replace(row, value);
            return;
        }
        if self.sparse.len() <= index as usize {
            self.sparse.resize(index as usize + 1, ABSENT);
        }
        self.point_to(index, self.data.len());
        self.indices.push(index);
        self.data.push(value);
    }

    fn replace(&mut self, row: usize, value: T) {
        if let Some(slot) = self.data.get_mut(row) {
            *slot = value;
        }
    }

    fn point_to(&mut self, index: u32, row: usize) {
        if let Some(slot) = self.sparse.get_mut(index as usize) {
            *slot = u32::try_from(row).unwrap_or(ABSENT);
        }
    }

    /// Removes the value of slot `index`; false when there was none.
    pub fn remove(&mut self, index: u32) -> bool {
        let Some(row) = self.row_of(index) else {
            return false;
        };
        self.point_to(index, usize::MAX);
        self.indices.swap_remove(row);
        drop(self.data.swap_remove(row));
        let moved = self.indices.get(row).copied().unwrap_or(ABSENT);
        self.point_to(moved, row);
        true
    }

    /// The value of slot `index`.
    #[must_use]
    pub fn get(&self, index: u32) -> Option<&T> {
        self.row_of(index).and_then(|row| self.data.get(row))
    }

    /// The value of slot `index`, writable.
    pub fn get_mut(&mut self, index: u32) -> Option<&mut T> {
        let row = self.row_of(index)?;
        self.data.get_mut(row)
    }

    /// Entity slots in dense order.
    #[must_use]
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Values in dense order.
    #[must_use]
    pub fn data(&self) -> &[T] {
        &self.data
    }

    /// Values in dense order, writable.
    pub fn data_mut(&mut self) -> &mut [T] {
        &mut self.data
    }

    /// Bytes of the three arrays, by length.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.sparse.len() * 4 + self.indices.len() * 4 + self.data.len() * size_of::<T>()
    }
}

/// Digest of one sparse set (see [`Store::digest`]).
pub(crate) fn set_digest<T: Words>(id: usize, set: &SparseSet<T>) -> u64 {
    set.data()
        .iter()
        .zip(set.indices())
        .fold(0, |sum, (value, &index)| {
            sum.wrapping_add(digest_of(id, index, value))
        })
}

fn apply_velocity(transform: Option<&mut Transform>, velocity: Velocity) {
    if let Some(t) = transform {
        t.x += velocity.x;
        t.y += velocity.y;
    }
}

fn extract_one(transform: Option<&Transform>, sprite: Option<&Sprite>, body: Body) -> u64 {
    match (transform, sprite) {
        (Some(t), Some(s)) => mix(t, s, &body),
        _ => 0,
    }
}

fn bump(transform: Option<&mut Transform>) {
    if let Some(t) = transform {
        t.rot += 1.0;
    }
}

/// A world with one sparse set per component. A query is driven by the set the author judges
/// the smallest and looks the other components up by entity slot.
#[derive(Debug)]
pub struct SparseWorld {
    entities: EntityTable,
    transforms: SparseSet<Transform>,
    velocities: SparseSet<Velocity>,
    sprites: SparseSet<Sprite>,
    bodies: SparseSet<Body>,
    radii: SparseSet<Radius>,
    hits: SparseSet<Hit>,
    live: usize,
}

impl Store for SparseWorld {
    fn name(&self) -> &'static str {
        "sparse-set"
    }

    fn with_capacity(entities: usize) -> Self {
        Self {
            entities: EntityTable::with_capacity(entities),
            transforms: SparseSet::with_capacity(entities, entities),
            velocities: SparseSet::with_capacity(entities, entities),
            sprites: SparseSet::with_capacity(entities, entities),
            bodies: SparseSet::with_capacity(entities, entities),
            radii: SparseSet::with_capacity(entities, entities),
            hits: SparseSet::with_capacity(entities, entities),
            live: 0,
        }
    }

    fn spawn(&mut self, kind: Kind, serial: u32) -> Entity {
        let entity = self.entities.allocate();
        let index = entity.index;
        self.transforms.insert(index, transform_of(serial));
        self.sprites.insert(index, sprite_of(serial));
        if kind != Kind::Wall {
            self.velocities.insert(index, velocity_of(serial));
        }
        if matches!(kind, Kind::Box | Kind::Disc) {
            self.bodies.insert(index, body_of(serial));
        }
        if kind == Kind::Disc {
            self.radii.insert(index, radius_of(serial));
        }
        self.live += 1;
        entity
    }

    fn despawn(&mut self, entity: Entity) -> bool {
        if !self.entities.release(entity) {
            return false;
        }
        let index = entity.index;
        self.transforms.remove(index);
        self.velocities.remove(index);
        self.sprites.remove(index);
        self.bodies.remove(index);
        self.radii.remove(index);
        self.hits.remove(index);
        self.live -= 1;
        true
    }

    fn set_hit(&mut self, entity: Entity, on: bool) -> bool {
        if !self.entities.is_alive(entity) {
            return false;
        }
        if on {
            self.hits.insert(entity.index, Hit(entity.index));
        } else {
            self.hits.remove(entity.index);
        }
        true
    }

    fn is_alive(&self, entity: Entity) -> bool {
        self.entities.is_alive(entity)
    }

    fn len(&self) -> usize {
        self.live
    }

    fn sum_x(&self) -> f64 {
        self.transforms.data().iter().map(|t| f64::from(t.x)).sum()
    }

    fn integrate(&mut self) {
        let velocities = &self.velocities;
        for (velocity, &index) in velocities.data().iter().zip(velocities.indices()) {
            apply_velocity(self.transforms.get_mut(index), *velocity);
        }
    }

    fn extract(&self) -> u64 {
        let mut sum = 0_u64;
        for (body, &index) in self.bodies.data().iter().zip(self.bodies.indices()) {
            let (t, s) = (self.transforms.get(index), self.sprites.get(index));
            sum = sum.wrapping_add(extract_one(t, s, *body));
        }
        sum
    }

    fn bump_hit(&mut self) {
        for &index in self.hits.indices() {
            bump(self.transforms.get_mut(index));
        }
    }

    fn digest(&self) -> u64 {
        set_digest(TRANSFORM, &self.transforms)
            .wrapping_add(set_digest(VELOCITY, &self.velocities))
            .wrapping_add(set_digest(SPRITE, &self.sprites))
            .wrapping_add(set_digest(BODY, &self.bodies))
            .wrapping_add(set_digest(RADIUS, &self.radii))
            .wrapping_add(set_digest(HIT, &self.hits))
    }

    fn order_hash(&self) -> u64 {
        let mut hash = 0xCBF2_9CE4_8422_2325_u64;
        for (t, &index) in self.transforms.data().iter().zip(self.transforms.indices()) {
            hash =
                (hash ^ u64::from(index) ^ u64::from(t.x.to_bits())).wrapping_mul(0x0100_0000_01B3);
        }
        hash
    }

    fn data_bytes(&self) -> usize {
        self.transforms.bytes()
            + self.velocities.bytes()
            + self.sprites.bytes()
            + self.bodies.bytes()
            + self.radii.bytes()
            + self.hits.bytes()
            + self.entities.slots() * 5
    }
}
