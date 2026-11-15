//! The command buffer: structural changes (spawn, despawn, insert, remove) recorded while a system
//! iterates and applied later, in order, at a fixed point, so no system sees storage move under it
//! (design note section 3).
//!
//! Values are kept in one queue per component type (typed, no boxing per command), so after a
//! warm-up that has seen each type once, recording and applying make no allocator call.

use crate::component::Component;
use crate::entity::Entity;
use crate::error::EcsError;
use crate::world::World;
use std::any::{Any, TypeId};
use std::collections::{BTreeMap, VecDeque};

/// Which entity a command is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// An entity that exists now.
    Existing(Entity),
    /// The `n`th entity spawned by this buffer, which does not exist until [`Commands::apply`].
    /// The number is only meaningful for the buffer that returned it and until its next apply: a
    /// number kept longer, or taken from another buffer, names whatever that buffer spawned at the
    /// same position, or nothing.
    Pending(u32),
}

impl From<Entity> for Target {
    fn from(entity: Entity) -> Self {
        Self::Existing(entity)
    }
}

/// What [`Commands::apply`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ApplyReport {
    /// Commands that took effect.
    pub applied: usize,
    /// Commands refused: a stale or unknown target, a missing component, a failed spawn.
    pub refused: usize,
}

impl ApplyReport {
    fn count(&mut self, done: Result<(), EcsError>) {
        if done.is_ok() {
            self.applied = self.applied.saturating_add(1);
        } else {
            self.refused = self.refused.saturating_add(1);
        }
    }
}

type RemoveFn = fn(&mut World, Entity) -> Result<(), EcsError>;

enum Op {
    Spawn,
    Despawn(Target),
    Insert(Target, TypeId),
    Remove(Target, RemoveFn),
}

/// The queued values of one component type, applied in the order they were recorded.
trait Queue: Any + Send + Sync {
    /// Takes the next value and inserts it into `entity` of `world`.
    fn apply_next(&mut self, world: &mut World, entity: Entity) -> Result<(), EcsError>;
    /// Drops the next value without inserting it.
    fn discard_next(&mut self);
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

struct Typed<T>(VecDeque<T>);

impl<T: Component> Queue for Typed<T> {
    fn apply_next(&mut self, world: &mut World, entity: Entity) -> Result<(), EcsError> {
        match self.0.pop_front() {
            Some(value) => world.insert(entity, value),
            None => Err(EcsError::StorageMismatch),
        }
    }

    fn discard_next(&mut self) {
        drop(self.0.pop_front());
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Recorded structural changes, applied to a [`World`] by [`Commands::apply`].
#[derive(Default)]
pub struct Commands {
    ops: Vec<Op>,
    queues: BTreeMap<TypeId, Box<dyn Queue>>,
    spawns: u32,
    spawned: Vec<Option<Entity>>,
}

impl Commands {
    /// An empty buffer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a spawn and returns the target to give to later commands of this buffer.
    pub fn spawn(&mut self) -> Target {
        self.ops.push(Op::Spawn);
        let pending = Target::Pending(self.spawns);
        // At 2^32 spawns the number saturates and no longer names a spawn: later commands on it
        // are refused instead of aliasing another entity.
        self.spawns = self.spawns.saturating_add(1);
        pending
    }

    /// Records a despawn.
    pub fn despawn(&mut self, target: impl Into<Target>) {
        self.ops.push(Op::Despawn(target.into()));
    }

    /// Records adding (or replacing) component `value`.
    pub fn insert<T: Component>(&mut self, target: impl Into<Target>, value: T) {
        let key = TypeId::of::<T>();
        let queue = self
            .queues
            .entry(key)
            .or_insert_with(|| Box::new(Typed::<T>(VecDeque::new())));
        if let Some(typed) = queue.as_any_mut().downcast_mut::<Typed<T>>() {
            typed.0.push_back(value);
            self.ops.push(Op::Insert(target.into(), key));
        }
    }

    /// Records removing component `T`.
    pub fn remove<T: Component>(&mut self, target: impl Into<Target>) {
        self.ops
            .push(Op::Remove(target.into(), World::remove::<T> as RemoveFn));
    }

    /// Number of recorded commands.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Whether nothing is recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// The entities spawned by the last [`Commands::apply`], by pending number; `None` where the
    /// spawn failed.
    #[must_use]
    pub fn spawned(&self) -> &[Option<Entity>] {
        &self.spawned
    }

    /// Applies the commands in the order they were recorded and empties the buffer. A command that
    /// cannot take effect is counted as refused and does not stop the others.
    pub fn apply(&mut self, world: &mut World) -> ApplyReport {
        self.spawned.clear();
        let mut report = ApplyReport::default();
        let ops = std::mem::take(&mut self.ops);
        for op in &ops {
            let done = self.run(op, world);
            report.count(done);
        }
        self.ops = ops;
        self.ops.clear();
        self.spawns = 0;
        report
    }

    fn run(&mut self, op: &Op, world: &mut World) -> Result<(), EcsError> {
        match op {
            Op::Spawn => {
                let spawned = world.spawn();
                self.spawned.push(spawned.ok());
                spawned.map(|_| ())
            }
            Op::Despawn(target) => world.despawn(self.resolve(*target)?),
            Op::Remove(target, remove) => remove(world, self.resolve(*target)?),
            Op::Insert(target, key) => self.run_insert(*target, *key, world),
        }
    }

    fn run_insert(
        &mut self,
        target: Target,
        key: TypeId,
        world: &mut World,
    ) -> Result<(), EcsError> {
        let entity = self.resolve(target);
        let queue = self.queues.get_mut(&key).ok_or(EcsError::StorageMismatch)?;
        match entity {
            Ok(entity) => queue.apply_next(world, entity),
            Err(refused) => {
                queue.discard_next();
                Err(refused)
            }
        }
    }

    fn resolve(&self, target: Target) -> Result<Entity, EcsError> {
        match target {
            Target::Existing(entity) => Ok(entity),
            Target::Pending(number) => usize::try_from(number)
                .ok()
                .and_then(|at| self.spawned.get(at).copied().flatten())
                .ok_or(EcsError::StaleEntity),
        }
    }
}
