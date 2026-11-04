//! An in-place heap sort. The standard stable sort allocates a buffer for slices of more than a
//! few dozen items and the standard unstable sort is banned in this crate (equal keys would end up
//! in an unspecified order), so the canonical pair order uses this one: no allocation, no recursion
//! and `O(n log n)` for every input. It is not stable; callers use it on keys that are unique.

/// Sorts `items` by ascending `key`. Items with equal keys end up in an unspecified (but, for the
/// same input, always the same) order.
pub(super) fn heap_sort_by_key<T, K: Ord>(items: &mut [T], key: impl Fn(&T) -> K) {
    let len = items.len();
    for start in (0..len / 2).rev() {
        sift_down(items, start, len, &key);
    }
    for end in (1..len).rev() {
        items.swap(0, end);
        sift_down(items, 0, end, &key);
    }
}

/// The key of the item at `index`; `None` past the end (never asked for inside the sort).
fn key_at<T, K>(items: &[T], index: usize, key: &impl Fn(&T) -> K) -> Option<K> {
    items.get(index).map(key)
}

/// Restores the heap property below `root` in `items[..end]`.
fn sift_down<T, K: Ord>(items: &mut [T], mut root: usize, end: usize, key: &impl Fn(&T) -> K) {
    loop {
        let left = 2 * root + 1;
        if left >= end {
            return;
        }
        let right = left + 1;
        let larger = if right < end && key_at(items, right, key) > key_at(items, left, key) {
            right
        } else {
            left
        };
        if key_at(items, larger, key) <= key_at(items, root, key) {
            return;
        }
        items.swap(root, larger);
        root = larger;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base::rng::Pcg32;

    fn sorted_by_heap(mut items: Vec<u64>) -> Vec<u64> {
        heap_sort_by_key(&mut items, |&value| value);
        items
    }

    #[test]
    fn empty_and_single_slices_are_left_alone() {
        assert_eq!(sorted_by_heap(vec![]), Vec::<u64>::new());
        assert_eq!(sorted_by_heap(vec![5]), vec![5]);
    }

    #[test]
    fn small_slices_in_any_order_come_out_sorted() {
        assert_eq!(sorted_by_heap(vec![2, 1]), vec![1, 2]);
        assert_eq!(sorted_by_heap(vec![3, 1, 2]), vec![1, 2, 3]);
        assert_eq!(sorted_by_heap(vec![1, 2, 3, 4, 5]), vec![1, 2, 3, 4, 5]);
        assert_eq!(sorted_by_heap(vec![5, 4, 3, 2, 1]), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn random_slices_of_every_length_equal_the_standard_sort() {
        let mut rng = Pcg32::new(21, 1);
        for len in 0..80 {
            // Few distinct values (duplicates) and many.
            check_random(&mut rng, len, 4);
            check_random(&mut rng, len, 1_000_000);
        }
    }

    fn check_random(rng: &mut Pcg32, len: usize, spread: u32) {
        let items: Vec<u64> = (0..len)
            .map(|_| u64::from(rng.below(spread).unwrap_or(0)))
            .collect();
        let mut expected = items.clone();
        expected.sort_by_key(|&value| value);
        assert_eq!(sorted_by_heap(items), expected, "len {len}");
    }

    #[test]
    fn items_are_ordered_by_the_key_and_travel_with_their_payload() {
        let mut items = vec![(30_u32, 'c'), (10, 'a'), (20, 'b'), (40, 'd')];
        heap_sort_by_key(&mut items, |item| item.0);
        assert_eq!(items, vec![(10, 'a'), (20, 'b'), (30, 'c'), (40, 'd')]);
    }

    const PIN_FIRST: u32 = 39;
    const PIN_LAST: u32 = 32;

    #[test]
    fn the_same_input_with_equal_keys_always_gives_the_same_order() {
        let input: Vec<(u32, u32)> = (0..50).map(|i| (i % 3, i)).collect();
        let run = || {
            let mut items = input.clone();
            heap_sort_by_key(&mut items, |item| item.0);
            items
        };
        assert_eq!(run(), run());
        assert_eq!(run().first(), Some(&(0, PIN_FIRST)));
        assert_eq!(run().last(), Some(&(2, PIN_LAST)));
        assert!(run().windows(2).all(|pair| pair[0].0 <= pair[1].0));
    }

    #[test]
    fn a_key_past_the_end_is_none() {
        assert_eq!(key_at(&[1_u8], 1, &|&v| v), None);
        assert_eq!(key_at(&[1_u8], 0, &|&v| v), Some(1));
    }
}
