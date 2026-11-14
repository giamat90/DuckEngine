//! Entity ids: an index and a generation, eight bytes (the shape of `base::pool::Handle`; the
//! spike keeps its own so that every storage pays the same).

/// An entity: slot index plus generation. A stale value is refused, never dangling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Entity {
    /// Slot index.
    pub index: u32,
    /// Generation of the slot when the entity was created.
    pub generation: u32,
}

/// Hands out entities and tells whether one is alive. The free list is last-in first-out, so the
/// same sequence of calls gives the same ids.
#[derive(Debug, Default)]
pub struct EntityTable {
    generations: Vec<u32>,
    alive: Vec<bool>,
    free: Vec<u32>,
}

impl EntityTable {
    /// A table with room for `capacity` entities.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            generations: Vec::with_capacity(capacity),
            alive: Vec::with_capacity(capacity),
            free: Vec::new(),
        }
    }

    /// A new entity.
    pub fn allocate(&mut self) -> Entity {
        match self.free.pop() {
            Some(index) => self.reuse(index),
            None => self.fresh(),
        }
    }

    fn reuse(&mut self, index: u32) -> Entity {
        if let Some(alive) = self.alive.get_mut(index as usize) {
            *alive = true;
        }
        let generation = self.generations.get(index as usize).copied().unwrap_or(0);
        Entity { index, generation }
    }

    fn fresh(&mut self) -> Entity {
        let index = u32::try_from(self.generations.len()).unwrap_or(u32::MAX);
        self.generations.push(0);
        self.alive.push(true);
        Entity {
            index,
            generation: 0,
        }
    }

    /// Whether `entity` is alive.
    #[must_use]
    pub fn is_alive(&self, entity: Entity) -> bool {
        let index = entity.index as usize;
        self.alive.get(index).copied().unwrap_or(false)
            && self.generations.get(index) == Some(&entity.generation)
    }

    /// Frees `entity`; false (and nothing done) when it is stale.
    pub fn release(&mut self, entity: Entity) -> bool {
        if !self.is_alive(entity) {
            return false;
        }
        let index = entity.index as usize;
        if let (Some(generation), Some(alive)) =
            (self.generations.get_mut(index), self.alive.get_mut(index))
        {
            *generation = generation.wrapping_add(1);
            *alive = false;
        }
        self.free.push(entity.index);
        true
    }

    /// Number of slots ever used (alive or free).
    #[must_use]
    pub fn slots(&self) -> usize {
        self.generations.len()
    }

    /// The live entity of slot `index`, if any.
    #[must_use]
    pub fn entity_at(&self, index: u32) -> Option<Entity> {
        let alive = self.alive.get(index as usize).copied()?;
        let generation = self.generations.get(index as usize).copied()?;
        alive.then_some(Entity { index, generation })
    }
}
