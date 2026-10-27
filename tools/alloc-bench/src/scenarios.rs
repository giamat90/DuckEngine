//! The workloads. Each function does a fixed amount of allocation work and returns a checksum, so
//! the work cannot be optimised away and a test can check that it did what it says.

use std::collections::HashMap;
use std::hint::black_box;
use std::sync::mpsc;
use std::thread;

/// Small deterministic random numbers (xorshift64); no dependency, same sequence everywhere.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Allocates `count` boxes of `N` bytes, writes the first byte of each, then frees them all.
/// Returns the sum of the first bytes.
#[must_use]
pub fn churn<const N: usize>(count: usize) -> u64 {
    let mut boxes: Vec<Box<[u8; N]>> = Vec::with_capacity(count);
    for i in 0..count {
        let mut item = Box::new([0_u8; N]);
        item[0] = u8::try_from(i % 251).unwrap_or(0);
        boxes.push(item);
    }
    boxes.iter().map(|b| u64::from(black_box(b)[0])).sum()
}

/// Builds `count` lists of 1 to 8 `u32` by pushing (so each list grows), then frees them.
/// Returns the total number of items.
#[must_use]
pub fn lists(count: usize, seed: u64) -> u64 {
    let mut rng = Rng(seed);
    let mut all: Vec<Vec<u32>> = Vec::with_capacity(count);
    for _ in 0..count {
        let len = rng.next() % 8 + 1;
        let mut list = Vec::new();
        for value in 0..len {
            list.push(u32::try_from(value).unwrap_or(0));
        }
        all.push(list);
    }
    all.iter().map(|l| black_box(l).len() as u64).sum()
}

/// Builds a complete binary tree of `depth` levels out of boxed nodes, counts the nodes, frees it.
#[must_use]
pub fn tree(depth: u32) -> u64 {
    let root = build(depth);
    black_box(count(root.as_deref()))
}

struct Node {
    left: Option<Box<Node>>,
    right: Option<Box<Node>>,
}

fn build(depth: u32) -> Option<Box<Node>> {
    if depth == 0 {
        return None;
    }
    Some(Box::new(Node {
        left: build(depth - 1),
        right: build(depth - 1),
    }))
}

fn count(node: Option<&Node>) -> u64 {
    node.map_or(0, |n| {
        1 + count(n.left.as_deref()) + count(n.right.as_deref())
    })
}

/// Pushes `0..n` into a `Vec<u32>` that starts empty. Returns the sum of the values.
#[must_use]
pub fn grow_vec(n: u32) -> u64 {
    let mut v: Vec<u32> = Vec::new();
    for i in 0..n {
        v.push(black_box(i));
    }
    v.iter().map(|&x| u64::from(x)).sum()
}

/// A map of `live` boxed 64-byte values; `ops` times removes the oldest key and inserts a new one.
/// Returns the final number of entries.
#[must_use]
pub fn map_churn(live: usize, ops: usize) -> u64 {
    let mut map: HashMap<u64, Box<[u8; 64]>> = HashMap::with_capacity(live + 1);
    for key in 0..live as u64 {
        map.insert(key, Box::new([0_u8; 64]));
    }
    for op in 0..ops as u64 {
        map.remove(&op);
        map.insert(live as u64 + op, Box::new([1_u8; 64]));
    }
    black_box(map.len() as u64)
}

/// `threads` threads each allocate and free `pairs` boxes of 64 bytes, in batches. Returns the
/// number of pairs done in total.
#[must_use]
pub fn threads_same(threads: usize, pairs: usize) -> u64 {
    let handles: Vec<_> = (0..threads)
        .map(|_| thread::spawn(move || pairs_in_batches(pairs)))
        .collect();
    handles.into_iter().map(|h| h.join().unwrap_or(0)).sum()
}

const BATCH: usize = 1024;

fn pairs_in_batches(pairs: usize) -> u64 {
    let mut done = 0;
    let mut batch: Vec<Box<[u8; 64]>> = Vec::with_capacity(BATCH);
    while done < pairs {
        let n = BATCH.min(pairs - done);
        batch.extend((0..n).map(|_| Box::new([1_u8; 64])));
        black_box(&batch);
        done += batch.len();
        batch.clear();
    }
    done as u64
}

/// `threads` threads in a ring: each allocates `batches` batches of `batch` boxes and sends each
/// batch to the next thread, which frees it. Returns the number of boxes freed in total.
#[must_use]
pub fn threads_cross(threads: usize, batches: usize, batch: usize) -> u64 {
    let (senders, receivers): (Vec<_>, Vec<_>) =
        (0..threads).map(|_| mpsc::channel::<Vec<Block>>()).unzip();
    let mut handles = Vec::with_capacity(threads);
    for (index, inbox) in receivers.into_iter().enumerate() {
        let next = senders[(index + 1) % threads].clone();
        handles.push(thread::spawn(move || relay(&next, &inbox, batches, batch)));
    }
    drop(senders);
    handles.into_iter().map(|h| h.join().unwrap_or(0)).sum()
}

/// One separately allocated 64-byte block (a wrapper, so a `Vec` of them is a `Vec` of allocations).
struct Block {
    _bytes: Box<[u8; 64]>,
}

/// Sends `batches` fresh batches to the next thread and frees the same number received.
fn relay(
    next: &mpsc::Sender<Vec<Block>>,
    inbox: &mpsc::Receiver<Vec<Block>>,
    batches: usize,
    batch: usize,
) -> u64 {
    let mut freed = 0;
    for _ in 0..batches {
        let fresh: Vec<Block> = (0..batch)
            .map(|_| Block {
                _bytes: Box::new([1_u8; 64]),
            })
            .collect();
        black_box(&fresh);
        if next.send(fresh).is_err() {
            return freed;
        }
        freed += inbox.recv().map_or(0, |received| received.len() as u64);
    }
    freed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_random_sequence_is_fixed() {
        let mut rng = Rng(1);
        assert_eq!(rng.next(), 0x4082_2041);
        assert_ne!(rng.next(), 0);
    }

    #[test]
    fn churn_sums_the_first_bytes_it_wrote() {
        let expected: u64 = (0..1000_u64).map(|i| i % 251).sum();
        assert_eq!(churn::<16>(1000), expected);
        assert_eq!(churn::<512>(1000), expected);
        assert_eq!(churn::<64>(0), 0);
    }

    #[test]
    fn lists_counts_the_items_of_the_lengths_the_generator_gives() {
        let mut rng = Rng(7);
        let expected: u64 = (0..300).map(|_| rng.next() % 8 + 1).sum();
        assert_eq!(lists(300, 7), expected);
        assert!((300..=2400).contains(&expected));
    }

    #[test]
    fn a_tree_of_d_levels_has_two_to_the_d_minus_one_nodes() {
        assert_eq!(tree(1), 1);
        assert_eq!(tree(5), 31);
        assert_eq!(tree(10), 1023);
    }

    #[test]
    fn grow_vec_sums_zero_to_n_minus_one() {
        assert_eq!(grow_vec(0), 0);
        assert_eq!(grow_vec(1000), 499_500);
    }

    #[test]
    fn map_churn_keeps_the_number_of_live_entries() {
        assert_eq!(map_churn(100, 1000), 100);
        assert_eq!(map_churn(10, 0), 10);
    }

    #[test]
    fn threads_same_does_every_pair_on_every_thread() {
        assert_eq!(threads_same(3, 2048), 3 * 2048);
        assert_eq!(threads_same(1, 10), 10);
    }

    #[test]
    fn threads_cross_frees_every_box_that_was_sent() {
        assert_eq!(threads_cross(4, 3, 50), 4 * 3 * 50);
        assert_eq!(threads_cross(2, 1, 1), 2);
    }
}
