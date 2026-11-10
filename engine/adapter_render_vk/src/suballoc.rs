//! The sub-allocator's bookkeeping for one block of device memory: which byte ranges are free.
//!
//! Pure data, no Vulkan and no `unsafe`: the tests run without a GPU. A block of memory is one
//! `vkAllocateMemory` call; resources are bound at offsets inside it (card 6-8, choice O2).
//! Allocation is best fit with alignment, freeing coalesces neighbours.

/// A free byte range: `start..end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Range {
    start: u64,
    end: u64,
}

/// The free ranges of one block, sorted by start, never touching each other.
#[derive(Debug)]
pub struct FreeList {
    capacity: u64,
    free: Vec<Range>,
}

impl FreeList {
    /// A block of `capacity` bytes, all free.
    #[must_use]
    pub fn new(capacity: u64) -> Self {
        let free = if capacity == 0 {
            Vec::new()
        } else {
            vec![Range {
                start: 0,
                end: capacity,
            }]
        };
        Self { capacity, free }
    }

    /// The block size.
    #[must_use]
    #[cfg(test)]
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    /// Bytes not handed out (including bytes lost to alignment gaps that are still free).
    #[must_use]
    pub fn free_bytes(&self) -> u64 {
        self.free.iter().map(|r| r.end - r.start).sum()
    }

    /// Whether nothing is handed out.
    #[must_use]
    pub fn is_unused(&self) -> bool {
        self.free_bytes() == self.capacity
    }

    /// Hands out `size` bytes at a multiple of `alignment`, from the smallest free range that
    /// fits; `None` when nothing fits, `size` is zero or `alignment` is not a power of two.
    pub fn allocate(&mut self, size: u64, alignment: u64) -> Option<u64> {
        if size == 0 || !alignment.is_power_of_two() {
            return None;
        }
        // (index, aligned start, length) of the smallest range that fits; the first of equals wins.
        let best = self
            .free
            .iter()
            .enumerate()
            .filter_map(|(index, range)| {
                let start = fit(range, size, alignment)?;
                Some((index, start, range.end - range.start))
            })
            .min_by_key(|&(_, _, length)| length);
        let (index, start, _) = best?;
        let range = self.free.remove(index);
        let mut pieces = Vec::with_capacity(2);
        if start > range.start {
            pieces.push(Range {
                start: range.start,
                end: start,
            });
        }
        if start + size < range.end {
            pieces.push(Range {
                start: start + size,
                end: range.end,
            });
        }
        for (at, piece) in pieces.into_iter().enumerate() {
            self.free.insert(index + at, piece);
        }
        Some(start)
    }

    /// Takes back `size` bytes at `offset` that [`FreeList::allocate`] handed out. Returns whether
    /// it did: a range outside the block or overlapping bytes that are already free (a double
    /// free) is refused and changes nothing.
    pub fn release(&mut self, offset: u64, size: u64) -> bool {
        let Some(end) = offset.checked_add(size) else {
            return false;
        };
        if size == 0 || end > self.capacity {
            return false;
        }
        let at = self.free.partition_point(|range| range.start < offset);
        let before_overlaps = at
            .checked_sub(1)
            .and_then(|i| self.free.get(i))
            .is_some_and(|range| range.end > offset);
        let after_overlaps = self.free.get(at).is_some_and(|range| range.start < end);
        if before_overlaps || after_overlaps {
            return false;
        }
        let joins_before = at
            .checked_sub(1)
            .and_then(|i| self.free.get(i))
            .is_some_and(|range| range.end == offset);
        let joins_after = self.free.get(at).is_some_and(|range| range.start == end);
        match (joins_before, joins_after) {
            (true, true) => self.join_both(at),
            (true, false) => self.set_end(at - 1, end),
            (false, true) => self.set_start(at, offset),
            (false, false) => self.free.insert(at, Range { start: offset, end }),
        }
        true
    }

    /// The freed range joins the range before it and the one after it into one.
    fn join_both(&mut self, at: usize) {
        let after = self.free.remove(at);
        self.set_end(at - 1, after.end);
    }

    fn set_end(&mut self, index: usize, end: u64) {
        if let Some(range) = self.free.get_mut(index) {
            range.end = end;
        }
    }

    fn set_start(&mut self, index: usize, start: u64) {
        if let Some(range) = self.free.get_mut(index) {
            range.start = start;
        }
    }
}

/// Where in `range` an aligned block of `size` bytes would start, if it fits.
fn fit(range: &Range, size: u64, alignment: u64) -> Option<u64> {
    let start = align_up(range.start, alignment)?;
    (start.checked_add(size)? <= range.end).then_some(start)
}

/// `value` rounded up to a multiple of `alignment` (a power of two); `None` on overflow.
fn align_up(value: u64, alignment: u64) -> Option<u64> {
    let mask = alignment - 1;
    value.checked_add(mask).map(|sum| sum & !mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One step of the mixed run: allocate or release, chosen by the numbers drawn.
    fn churn(list: &mut FreeList, live: &mut Vec<(u64, u64)>, roll: [u64; 3]) {
        if live.len() < 40 && !roll[0].is_multiple_of(3) {
            take_and_check(list, live, roll[1] % 700 + 1, 1_u64 << (roll[2] % 9));
        } else if !live.is_empty() {
            let at = usize::try_from(roll[1]).unwrap_or(0) % live.len();
            let (offset, size) = live.swap_remove(at);
            assert!(list.release(offset, size));
        }
    }

    /// Allocates, and when it works checks alignment, bounds and that nothing live overlaps.
    fn take_and_check(list: &mut FreeList, live: &mut Vec<(u64, u64)>, size: u64, alignment: u64) {
        let Some(offset) = list.allocate(size, alignment) else {
            return;
        };
        assert_eq!(offset % alignment, 0);
        assert!(offset + size <= list.capacity());
        for (other, length) in live.iter() {
            assert!(
                offset + size <= *other || *other + *length <= offset,
                "{offset}+{size} overlaps {other}+{length}"
            );
        }
        live.push((offset, size));
    }

    #[test]
    fn a_new_block_is_all_free_and_unused() {
        let list = FreeList::new(1024);
        assert_eq!(list.capacity(), 1024);
        assert_eq!(list.free_bytes(), 1024);
        assert!(list.is_unused());
    }

    #[test]
    fn the_first_allocation_starts_at_zero() {
        let mut list = FreeList::new(1024);
        assert_eq!(list.allocate(100, 1), Some(0));
        assert_eq!(list.free_bytes(), 924);
        assert!(!list.is_unused());
    }

    #[test]
    fn allocations_do_not_overlap() {
        let mut list = FreeList::new(1024);
        assert_eq!(list.allocate(100, 1), Some(0));
        assert_eq!(list.allocate(50, 1), Some(100));
        assert_eq!(list.allocate(10, 1), Some(150));
    }

    #[test]
    fn an_offset_is_a_multiple_of_the_alignment() {
        let mut list = FreeList::new(4096);
        assert_eq!(list.allocate(10, 1), Some(0));
        assert_eq!(list.allocate(10, 256), Some(256));
        // The smallest range that fits is the gap 10..256, so 64 is the first aligned offset in it.
        assert_eq!(list.allocate(10, 64), Some(64));
    }

    #[test]
    fn the_gap_before_an_aligned_allocation_stays_usable() {
        let mut list = FreeList::new(4096);
        assert_eq!(list.allocate(10, 1), Some(0));
        assert_eq!(list.allocate(10, 256), Some(256));
        // The bytes 10..256 are still free.
        assert_eq!(list.allocate(200, 1), Some(10));
    }

    #[test]
    fn an_allocation_that_does_not_fit_is_refused_and_changes_nothing() {
        let mut list = FreeList::new(100);
        assert_eq!(list.allocate(101, 1), None);
        assert_eq!(list.free_bytes(), 100);
        assert_eq!(list.allocate(100, 1), Some(0));
        assert_eq!(list.allocate(1, 1), None);
    }

    #[test]
    fn an_alignment_that_pushes_the_end_past_the_block_is_refused() {
        let mut list = FreeList::new(300);
        assert_eq!(list.allocate(10, 1), Some(0));
        // The next 256-aligned offset is 256; 256 + 100 > 300.
        assert_eq!(list.allocate(100, 256), None);
        assert_eq!(list.allocate(44, 256), Some(256));
    }

    #[test]
    fn a_zero_size_or_a_bad_alignment_is_refused() {
        let mut list = FreeList::new(100);
        assert_eq!(list.allocate(0, 1), None);
        assert_eq!(list.allocate(10, 0), None);
        assert_eq!(list.allocate(10, 3), None);
        assert_eq!(list.free_bytes(), 100);
    }

    #[test]
    fn the_smallest_range_that_fits_is_used() {
        let mut list = FreeList::new(1000);
        // Layout: [A 0..100][B 100..200][C 200..500][D 500..520][rest 520..1000].
        let a = list.allocate(100, 1);
        let b = list.allocate(100, 1);
        let c = list.allocate(300, 1);
        let d = list.allocate(20, 1);
        assert_eq!((a, b, c, d), (Some(0), Some(100), Some(200), Some(500)));
        // Free A (100 bytes) and C (300 bytes): two holes, 0..100 and 200..500.
        assert!(list.release(0, 100));
        assert!(list.release(200, 300));
        // 90 bytes fit both holes and the tail; the smallest hole (0..100) wins.
        assert_eq!(list.allocate(90, 1), Some(0));
    }

    #[test]
    fn freeing_joins_neighbours_into_one_range() {
        let mut list = FreeList::new(300);
        assert_eq!(list.allocate(100, 1), Some(0));
        assert_eq!(list.allocate(100, 1), Some(100));
        assert_eq!(list.allocate(100, 1), Some(200));
        // Free in an order that needs joins on both sides: first, last, then the middle.
        assert!(list.release(0, 100));
        assert!(list.release(200, 100));
        assert!(list.release(100, 100));
        assert!(list.is_unused());
        // One range again: the whole block fits.
        assert_eq!(list.allocate(300, 1), Some(0));
    }

    #[test]
    fn a_double_free_is_refused() {
        let mut list = FreeList::new(300);
        assert_eq!(list.allocate(100, 1), Some(0));
        assert!(list.release(0, 100));
        assert!(!list.release(0, 100));
        assert_eq!(list.free_bytes(), 300);
    }

    #[test]
    fn a_free_that_overlaps_free_bytes_is_refused() {
        let mut list = FreeList::new(300);
        assert_eq!(list.allocate(100, 1), Some(0));
        // 0..150 covers the allocation and 50 bytes that are free.
        assert!(!list.release(0, 150));
        assert_eq!(list.free_bytes(), 200);
    }

    #[test]
    fn a_free_outside_the_block_or_of_zero_size_is_refused() {
        let mut list = FreeList::new(300);
        assert_eq!(list.allocate(300, 1), Some(0));
        assert!(!list.release(200, 101));
        assert!(!list.release(u64::MAX, 2));
        assert!(!list.release(0, 0));
        assert_eq!(list.free_bytes(), 0);
    }

    #[test]
    fn a_zero_sized_block_hands_out_nothing() {
        let mut list = FreeList::new(0);
        assert_eq!(list.allocate(1, 1), None);
        assert!(list.is_unused());
    }

    #[test]
    fn a_long_mixed_run_never_hands_out_overlapping_ranges() {
        // A small deterministic generator: no clock, no external crate.
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut list = FreeList::new(1 << 16);
        let mut live: Vec<(u64, u64)> = Vec::new();
        for _ in 0..2000 {
            churn(&mut list, &mut live, [next(), next(), next()]);
            let used: u64 = live.iter().map(|(_, size)| size).sum();
            assert!(list.free_bytes() + used <= list.capacity());
        }
        for (offset, size) in live {
            assert!(list.release(offset, size));
        }
        assert!(list.is_unused());
        assert_eq!(list.allocate(1 << 16, 1), Some(0), "all joined back");
    }
}
