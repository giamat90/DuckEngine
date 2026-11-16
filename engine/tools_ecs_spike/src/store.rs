//! The workload API every storage implements. A storage answers the same operations with the
//! same results (the tests compare checksums), so the cost of each can be compared.

use crate::components::Kind;
use crate::entity::Entity;

/// One ECS storage on the sandbox shape.
pub trait Store: Sized {
    /// Name in the report.
    fn name(&self) -> &'static str;
    /// An empty world with room for `entities` entities.
    fn with_capacity(entities: usize) -> Self;
    /// Spawns the `serial`th entity of `kind` with its components.
    fn spawn(&mut self, kind: Kind, serial: u32) -> Entity;
    /// Despawns `entity`; false when it is stale.
    fn despawn(&mut self, entity: Entity) -> bool;
    /// Adds (`on`) or removes the `Hit` marker; false when `entity` is stale.
    fn set_hit(&mut self, entity: Entity, on: bool) -> bool;
    /// Whether `entity` is alive.
    fn is_alive(&self, entity: Entity) -> bool;
    /// Number of live entities.
    fn len(&self) -> usize;
    /// Whether no entity is alive.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Query of one component: sum of the `x` of every `Transform`.
    fn sum_x(&self) -> f64;
    /// Query of two components, one written: `Transform.pos += Velocity`.
    fn integrate(&mut self);
    /// Query of three components, read only: checksum over `Transform`, `Sprite` and `Body`.
    fn extract(&self) -> u64;
    /// Query over the marker and one component: `Transform.rot += 1` where `Hit` is present.
    fn bump_hit(&mut self);
    /// Order-independent digest of every field of every component of every entity (the slot
    /// numbers included), so two storages in the same state give the same value.
    fn digest(&self) -> u64;
    /// Hash of the transforms in storage iteration order (equal across two identical runs of the
    /// same storage; different storages may differ).
    fn order_hash(&self) -> u64;
    /// Bytes held by component data and bookkeeping, by length (the sparse arrays of a sparse set by entity slot).
    fn data_bytes(&self) -> usize;
}
