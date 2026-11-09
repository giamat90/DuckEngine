//! Tests of what the device callback does with the renderer, without a sound device: they call
//! [`Shared::render`] the way the C side does.

use crate::shared::{Counters, Shared};
use audio::device::AudioRenderer;
use base::alloc::{CountingAllocator, count_allocations};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

#[global_allocator]
static ALLOCATOR: CountingAllocator<std::alloc::System> =
    CountingAllocator::new(std::alloc::System);

/// Writes `value` to every sample and notes each call (frames asked) and its drop.
struct Probe {
    value: f32,
    calls: Arc<Mutex<Vec<u32>>>,
    drops: Arc<AtomicU32>,
}

impl AudioRenderer for Probe {
    fn render(&mut self, out: &mut [f32], frames: u32) {
        out.fill(self.value);
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(frames);
        }
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::AcqRel);
    }
}

struct Rig {
    shared: Shared,
    counters: Arc<Counters>,
    calls: Arc<Mutex<Vec<u32>>>,
    drops: Arc<AtomicU32>,
}

fn rig(channels: usize, max_block: u32) -> Rig {
    let counters = Arc::new(Counters::default());
    Rig {
        shared: Shared::new(channels, max_block, Arc::clone(&counters)),
        counters,
        calls: Arc::new(Mutex::new(Vec::new())),
        drops: Arc::new(AtomicU32::new(0)),
    }
}

impl Rig {
    fn renderer(&self, value: f32) -> Box<dyn AudioRenderer> {
        Box::new(Probe {
            value,
            calls: Arc::clone(&self.calls),
            drops: Arc::clone(&self.drops),
        })
    }

    fn frames_asked(&self) -> Vec<u32> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }
}

#[test]
fn without_a_renderer_the_callback_writes_silence() {
    let rig = rig(2, 64);
    let mut out = vec![0.7_f32; 20];
    rig.shared.render(&mut out, 10);
    assert!(out.iter().all(|s| s.to_bits() == 0.0_f32.to_bits()));
    assert_eq!(rig.counters.callbacks(), 0);
}

#[test]
fn a_block_within_the_limit_is_one_call() {
    let rig = rig(2, 64);
    rig.shared.put(rig.renderer(0.5));
    let mut out = vec![0.0_f32; 2 * 64];
    rig.shared.render(&mut out, 64);
    assert_eq!(rig.frames_asked(), vec![64]);
    assert!(out.iter().all(|s| (s - 0.5).abs() < 1e-6));
    assert_eq!(rig.counters.callbacks(), 1);
    assert_eq!(rig.counters.frames(), 64);
}

#[test]
fn a_longer_block_is_cut_into_pieces_of_at_most_the_limit() {
    let rig = rig(2, 64);
    rig.shared.put(rig.renderer(0.25));
    let mut out = vec![0.0_f32; 2 * 150];
    rig.shared.render(&mut out, 150);
    assert_eq!(rig.frames_asked(), vec![64, 64, 22]);
    assert!(out.iter().all(|s| (s - 0.25).abs() < 1e-6));
    assert_eq!(rig.counters.callbacks(), 3);
    assert_eq!(rig.counters.frames(), 150);
}

#[test]
fn the_pieces_follow_the_channel_count() {
    let rig = rig(1, 4);
    rig.shared.put(rig.renderer(1.0));
    let mut out = vec![0.0_f32; 10];
    rig.shared.render(&mut out, 10);
    assert_eq!(rig.frames_asked(), vec![4, 4, 2]);
}

#[test]
fn a_slice_of_the_wrong_length_is_silenced_and_not_rendered() {
    let rig = rig(2, 64);
    rig.shared.put(rig.renderer(0.5));
    let mut out = vec![0.9_f32; 7];
    rig.shared.render(&mut out, 10);
    assert!(out.iter().all(|s| s.to_bits() == 0.0_f32.to_bits()));
    assert!(rig.frames_asked().is_empty());
}

#[test]
fn zero_frames_call_nothing() {
    let rig = rig(2, 64);
    rig.shared.put(rig.renderer(0.5));
    rig.shared.render(&mut [], 0);
    assert!(rig.frames_asked().is_empty());
    assert_eq!(rig.counters.callbacks(), 0);
}

#[test]
fn take_gives_the_renderer_back_without_dropping_it() {
    let rig = rig(2, 64);
    rig.shared.put(rig.renderer(0.5));
    let back = rig.shared.take();
    assert!(back.is_some());
    assert_eq!(rig.drops.load(Ordering::Acquire), 0);
    drop(back);
    assert_eq!(rig.drops.load(Ordering::Acquire), 1);
    assert!(rig.shared.take().is_none());
}

#[test]
fn after_take_the_callback_is_silent_again() {
    let rig = rig(2, 64);
    rig.shared.put(rig.renderer(0.5));
    drop(rig.shared.take());
    let mut out = vec![0.3_f32; 8];
    rig.shared.render(&mut out, 4);
    assert!(out.iter().all(|s| s.to_bits() == 0.0_f32.to_bits()));
}

#[test]
fn a_zero_limit_or_channel_count_does_not_break_the_cutting() {
    let rig = rig(0, 0);
    rig.shared.put(rig.renderer(0.5));
    let mut out = vec![0.0_f32; 4];
    rig.shared.render(&mut out, 4);
    // Both are raised to one: a frame at a time, never a division by zero or an endless loop.
    assert_eq!(rig.frames_asked(), vec![1, 1, 1, 1]);
}

#[test]
fn a_callback_that_finds_the_slot_taken_writes_silence_and_counts_a_miss() {
    let rig = rig(2, 64);
    rig.shared.put(rig.renderer(0.5));
    let mut out = vec![0.9_f32; 8];
    rig.shared
        .with_slot_taken(|| rig.shared.render(&mut out, 4));
    assert!(out.iter().all(|s| s.to_bits() == 0.0_f32.to_bits()));
    assert!(rig.frames_asked().is_empty());
    assert_eq!(rig.counters.misses(), 1);
    assert_eq!(rig.counters.callbacks(), 0);
    rig.shared.render(&mut out, 4);
    assert_eq!(rig.counters.misses(), 1, "a normal callback is not a miss");
}

#[test]
fn the_callback_allocates_nothing() {
    let rig = rig(2, 64);
    rig.shared.put(Box::new(Silent));
    let mut out = vec![0.0_f32; 2 * 150];
    let ((), counts) = count_allocations(|| {
        for _ in 0..100 {
            rig.shared.render(&mut out, 150);
        }
    });
    assert_eq!(counts.calls(), 0, "allocator calls");
    assert_eq!(rig.counters.callbacks(), 300);
}

/// A renderer that touches nothing else, so the test counts only the adapter's own calls.
struct Silent;

impl AudioRenderer for Silent {
    fn render(&mut self, out: &mut [f32], _frames: u32) {
        out.fill(0.0);
    }
}
