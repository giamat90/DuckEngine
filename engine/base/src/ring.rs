//! A fixed-capacity FIFO ring of plain values.
//!
//! [`Ring`] stores `Copy` values (so there is no drop glue to run) in one array that is allocated
//! when the ring is built. `push` at the back and `pop` at the front make no allocator call and
//! never move other values; a push into a full ring returns the value instead of growing.
//! The array length is a power of two so that an index wraps with a mask; the ring still holds
//! exactly `capacity` values. Safe code. The values must be `Default` only to fill the array once.

/// FIFO queue of at most `capacity` values; see the module documentation.
#[derive(Clone, Debug)]
pub struct Ring<T> {
    items: Vec<T>,
    head: usize,
    len: usize,
    capacity: usize,
}

impl<T: Copy + Default> Ring<T> {
    /// A ring that holds at most `capacity` values. The array has the next power of two of
    /// `capacity` slots (up to twice as many) and every slot is filled once with `T::default()`,
    /// so building costs time and memory in proportion to that. Like `vec!`, this aborts or panics
    /// when the system cannot provide the storage (a capacity near `u32::MAX` asks for 2^32 slots).
    #[must_use]
    pub fn with_capacity(capacity: u32) -> Self {
        let slots = u64::from(capacity).next_power_of_two();
        Self {
            items: vec![T::default(); usize::try_from(slots).unwrap_or(usize::MAX)],
            head: 0,
            len: 0,
            capacity: capacity as usize,
        }
    }

    /// Number of values the ring can hold at once.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Number of values.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// True when the ring holds no value.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// True when a push would be refused.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.len == self.capacity
    }

    /// Adds `value` at the back.
    ///
    /// # Errors
    /// Returns the value back when the ring is full.
    pub fn push(&mut self, value: T) -> Result<(), T> {
        if self.is_full() {
            return Err(value);
        }
        let at = self.slot(self.len);
        self.items[at] = value;
        self.len += 1;
        Ok(())
    }

    /// Removes and returns the value at the front.
    pub fn pop(&mut self) -> Option<T> {
        let value = *self.peek()?;
        self.head = self.slot(1);
        self.len -= 1;
        Some(value)
    }

    /// The value at the front.
    #[must_use]
    pub fn peek(&self) -> Option<&T> {
        self.get(0)
    }

    /// The value `index` places after the front (0 is the front).
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&T> {
        (index < self.len).then(|| &self.items[self.slot(index)])
    }

    /// Removes every value; the capacity stays.
    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }

    /// The values from front to back.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        (0..self.len).map(|index| &self.items[self.slot(index)])
    }

    /// Array index of the value `offset` places after the front.
    fn slot(&self, offset: usize) -> usize {
        (self.head + offset) & (self.items.len() - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Pcg32;
    use std::collections::VecDeque;

    fn drained(ring: &mut Ring<u32>) -> Vec<u32> {
        std::iter::from_fn(|| ring.pop()).collect()
    }

    fn filled(capacity: u32, values: &[u32]) -> Ring<u32> {
        let mut ring = Ring::with_capacity(capacity);
        for &v in values {
            assert_eq!(ring.push(v), Ok(()));
        }
        ring
    }

    #[test]
    fn a_new_ring_is_empty_and_reports_its_capacity() {
        let ring = Ring::<u32>::with_capacity(5);
        assert!(ring.is_empty());
        assert!(!ring.is_full());
        assert_eq!(ring.len(), 0);
        assert_eq!(ring.capacity(), 5);
        assert_eq!(ring.peek(), None);
        assert_eq!(ring.iter().count(), 0);
    }

    #[test]
    fn values_come_out_in_the_order_they_went_in() {
        let mut ring = filled(8, &[3, 1, 2]);
        assert_eq!(ring.len(), 3);
        assert_eq!(ring.peek(), Some(&3));
        assert_eq!(drained(&mut ring), vec![3, 1, 2]);
        assert!(ring.is_empty());
        assert_eq!(ring.pop(), None);
    }

    #[test]
    fn a_full_ring_refuses_a_push_and_hands_the_value_back() {
        let mut ring = filled(3, &[1, 2, 3]);
        assert!(ring.is_full());
        assert_eq!(ring.push(4), Err(4));
        assert_eq!(ring.len(), 3);
        assert_eq!(drained(&mut ring), vec![1, 2, 3]);
    }

    fn holds_exactly(capacity: u32) {
        let mut ring = Ring::<u32>::with_capacity(capacity);
        for v in 0..capacity {
            assert_eq!(ring.push(v), Ok(()));
        }
        assert_eq!(ring.push(0), Err(0));
        assert_eq!(ring.len(), capacity as usize);
    }

    #[test]
    fn a_capacity_that_is_not_a_power_of_two_holds_exactly_that_many() {
        for capacity in [1_u32, 2, 3, 5, 6, 7, 9, 100] {
            holds_exactly(capacity);
        }
    }

    #[test]
    fn a_ring_with_no_capacity_refuses_everything() {
        let mut ring = Ring::<u32>::with_capacity(0);
        assert!(ring.is_empty());
        assert!(ring.is_full());
        assert_eq!(ring.push(1), Err(1));
        assert_eq!(ring.pop(), None);
        assert_eq!(ring.peek(), None);
    }

    #[test]
    fn the_ring_wraps_around_the_end_of_its_storage() {
        let mut ring = filled(3, &[1, 2, 3]);
        assert_eq!(ring.pop(), Some(1));
        assert_eq!(ring.pop(), Some(2));
        assert_eq!(ring.push(4), Ok(()));
        assert_eq!(ring.push(5), Ok(()));
        assert!(ring.is_full());
        assert_eq!(ring.iter().copied().collect::<Vec<_>>(), vec![3, 4, 5]);
        assert_eq!(drained(&mut ring), vec![3, 4, 5]);
    }

    #[test]
    fn iteration_and_get_follow_fifo_order_without_consuming() {
        let mut ring = filled(4, &[1, 2, 3, 4]);
        ring.pop();
        ring.push(5).unwrap_or_else(|_| panic!("fits"));
        assert_eq!(ring.iter().copied().collect::<Vec<_>>(), vec![2, 3, 4, 5]);
        assert_eq!(ring.get(0), Some(&2));
        assert_eq!(ring.get(3), Some(&5));
        assert_eq!(ring.get(4), None);
        assert_eq!(ring.len(), 4);
    }

    #[test]
    fn clear_empties_the_ring_and_keeps_the_capacity() {
        let mut ring = filled(4, &[1, 2, 3]);
        ring.pop();
        ring.clear();
        assert!(ring.is_empty());
        assert_eq!(ring.capacity(), 4);
        assert_eq!(ring.peek(), None);
        assert_eq!(filled_after_clear(&mut ring), vec![7, 8, 9, 10]);
    }

    fn filled_after_clear(ring: &mut Ring<u32>) -> Vec<u32> {
        for v in 7..=10 {
            assert_eq!(ring.push(v), Ok(()));
        }
        drained(ring)
    }

    #[test]
    fn a_long_random_run_agrees_with_a_vec_deque() {
        let mut ring = Ring::<u32>::with_capacity(13);
        let mut model = VecDeque::new();
        let mut dice = Pcg32::new(11, 5);
        for _ in 0..20_000 {
            random_step(&mut ring, &mut model, &mut dice);
            assert_eq!(ring.len(), model.len());
            assert_eq!(ring.peek(), model.front());
        }
        assert_eq!(drained(&mut ring), model.into_iter().collect::<Vec<_>>());
    }

    fn random_step(ring: &mut Ring<u32>, model: &mut VecDeque<u32>, dice: &mut Pcg32) {
        if dice.next_u32() % 5 < 3 {
            step_push(ring, model, dice.next_u32());
        } else {
            assert_eq!(ring.pop(), model.pop_front());
        }
    }

    fn step_push(ring: &mut Ring<u32>, model: &mut VecDeque<u32>, value: u32) {
        if model.len() < 13 {
            model.push_back(value);
            assert_eq!(ring.push(value), Ok(()));
        } else {
            assert_eq!(ring.push(value), Err(value));
        }
    }
}
