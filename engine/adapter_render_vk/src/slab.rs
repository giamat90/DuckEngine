//! A table of values addressed by generational keys: a key whose value was removed never finds
//! another value, even when the slot is reused. Pure data, no `unsafe`.

/// Names one value of a [`Slab`]. The zero key (generation 0) names nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    index: u32,
    generation: u32,
}

impl Key {
    /// The key as one number (generation in the high half), for handles that cross a boundary.
    #[must_use]
    pub fn to_bits(self) -> u64 {
        (u64::from(self.generation) << 32) | u64::from(self.index)
    }

    /// The key a number from [`Key::to_bits`] stands for.
    #[must_use]
    pub fn from_bits(bits: u64) -> Self {
        Self {
            index: u32::try_from(bits & u64::from(u32::MAX)).unwrap_or(0),
            generation: u32::try_from(bits >> 32).unwrap_or(0),
        }
    }
}

struct Entry<T> {
    generation: u32,
    value: Option<T>,
}

/// Values in slots; a freed slot is reused with a new generation.
pub struct Slab<T> {
    entries: Vec<Entry<T>>,
    free: Vec<u32>,
}

impl<T> Default for Slab<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Slab<T> {
    /// An empty table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            free: Vec::new(),
        }
    }

    /// Stores `value` and returns its key; the value comes back when the table has no index left.
    ///
    /// # Errors
    /// The value itself, when no slot can be made.
    pub fn insert(&mut self, value: T) -> Result<Key, T> {
        match self.free.pop() {
            Some(index) => self.reuse(index, value),
            None => self.push(value),
        }
    }

    fn reuse(&mut self, index: u32, value: T) -> Result<Key, T> {
        let slot = usize::try_from(index)
            .ok()
            .and_then(|at| self.entries.get_mut(at));
        let Some(entry) = slot else {
            return Err(value);
        };
        entry.value = Some(value);
        Ok(Key {
            index,
            generation: entry.generation,
        })
    }

    fn push(&mut self, value: T) -> Result<Key, T> {
        let Ok(index) = u32::try_from(self.entries.len()) else {
            return Err(value);
        };
        self.entries.push(Entry {
            generation: 1,
            value: Some(value),
        });
        Ok(Key {
            index,
            generation: 1,
        })
    }

    fn entry(&self, key: Key) -> Option<&Entry<T>> {
        self.entries
            .get(usize::try_from(key.index).ok()?)
            .filter(|entry| entry.generation == key.generation)
    }

    /// The value `key` names, if it is still there.
    #[must_use]
    pub fn get(&self, key: Key) -> Option<&T> {
        self.entry(key)?.value.as_ref()
    }

    /// The value `key` names, if it is still there.
    pub fn get_mut(&mut self, key: Key) -> Option<&mut T> {
        let entry = self.entries.get_mut(usize::try_from(key.index).ok()?)?;
        if entry.generation == key.generation {
            entry.value.as_mut()
        } else {
            None
        }
    }

    /// Takes the value out; the key and every copy of it stop naming anything.
    pub fn remove(&mut self, key: Key) -> Option<T> {
        let entry = self.entries.get_mut(usize::try_from(key.index).ok()?)?;
        if entry.generation != key.generation {
            return None;
        }
        let value = entry.value.take()?;
        // Generation 0 is kept for "no key", so the counter skips it when it wraps.
        entry.generation = entry.generation.wrapping_add(1).max(1);
        self.free.push(key.index);
        Some(value)
    }

    /// How many values are stored.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len() - self.free.len()
    }

    /// Whether nothing is stored.
    #[must_use]
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Takes every value out, newest slot first; all keys go stale.
    pub fn drain(&mut self) -> Vec<T> {
        let free = &mut self.free;
        self.entries
            .iter_mut()
            .enumerate()
            .rev()
            .filter_map(|(index, entry)| {
                let value = entry.value.take()?;
                entry.generation = entry.generation.wrapping_add(1).max(1);
                free.extend(u32::try_from(index).ok());
                Some(value)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(slab: &mut Slab<&'static str>, value: &'static str) -> Key {
        slab.insert(value).unwrap_or(Key {
            index: 0,
            generation: 0,
        })
    }

    #[test]
    fn a_stored_value_is_found_by_its_key() {
        let mut slab = Slab::new();
        let a = key(&mut slab, "a");
        let b = key(&mut slab, "b");
        assert_eq!(slab.get(a), Some(&"a"));
        assert_eq!(slab.get(b), Some(&"b"));
        assert_eq!(slab.len(), 2);
    }

    #[test]
    fn a_removed_value_is_gone_and_a_second_remove_finds_nothing() {
        let mut slab = Slab::new();
        let a = key(&mut slab, "a");
        assert_eq!(slab.remove(a), Some("a"));
        assert_eq!(slab.get(a), None);
        assert_eq!(slab.remove(a), None);
        assert!(slab.is_empty());
    }

    #[test]
    fn a_reused_slot_does_not_answer_to_the_old_key() {
        let mut slab = Slab::new();
        let old = key(&mut slab, "old");
        assert_eq!(slab.remove(old), Some("old"));
        let new = key(&mut slab, "new");
        assert_ne!(old, new);
        assert_eq!(slab.get(old), None);
        assert_eq!(slab.get_mut(old), None);
        assert_eq!(slab.remove(old), None);
        assert_eq!(slab.get(new), Some(&"new"));
    }

    #[test]
    fn a_value_can_be_changed_through_its_key() {
        let mut slab = Slab::new();
        let a = key(&mut slab, "a");
        if let Some(value) = slab.get_mut(a) {
            *value = "changed";
        }
        assert_eq!(slab.get(a), Some(&"changed"));
    }

    #[test]
    fn the_zero_key_and_a_key_from_nowhere_name_nothing() {
        let mut slab = Slab::new();
        let _ = key(&mut slab, "a");
        assert_eq!(slab.get(Key::from_bits(0)), None);
        assert_eq!(slab.get(Key::from_bits((1_u64 << 32) | 0x63)), None);
    }

    #[test]
    fn a_key_survives_the_trip_through_a_number() {
        let mut slab = Slab::new();
        let _ = key(&mut slab, "a");
        let b = key(&mut slab, "b");
        assert_eq!(Key::from_bits(b.to_bits()), b);
        assert_eq!(slab.get(Key::from_bits(b.to_bits())), Some(&"b"));
    }

    #[test]
    fn draining_returns_every_value_and_makes_every_key_stale() {
        let mut slab = Slab::new();
        let a = key(&mut slab, "a");
        let b = key(&mut slab, "b");
        let c = key(&mut slab, "c");
        assert_eq!(slab.remove(b), Some("b"));
        let mut all = slab.drain();
        all.sort_unstable();
        assert_eq!(all, vec!["a", "c"]);
        assert!(slab.is_empty());
        assert_eq!((slab.get(a), slab.get(b), slab.get(c)), (None, None, None));
        // The slots can be used again.
        let d = key(&mut slab, "d");
        assert_eq!(slab.get(d), Some(&"d"));
        assert_eq!(slab.len(), 1);
    }

    #[test]
    fn the_generation_never_wraps_to_the_zero_key() {
        let mut slab = Slab::new();
        let first = key(&mut slab, "x");
        // Put the slot one step before the wrap.
        slab.entries[0].generation = u32::MAX;
        let wrapped = Key {
            index: first.index,
            generation: u32::MAX,
        };
        assert_eq!(slab.remove(wrapped), Some("x"));
        assert_eq!(slab.entries[0].generation, 1, "0 is skipped");
    }
}
