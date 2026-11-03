//! The frame scene is deterministic, valid for the Null device, and its CPU stages make no
//! allocator call once warm (zero heap allocations per frame on the hot path).

use base::alloc::{CountingAllocator, count_allocations};
use std::alloc::System;
use tools_render_bench::{BATCH_FLOOR, FrameScene, SEED, SPRITES, fnv1a};

/// Pinned hashes of the frame of the default seed (FNV-1a of the recorded command stream, and of the
/// encoded vertex bytes of the unrotated scene, which calls no sin or cos so it is the same on every platform). A change here means the frame changed: review it, then re-pin.
const VERTEX_HASH: u64 = 1_127_670_744_278_913_829;
const STREAM_HASH: u64 = 16_668_592_047_273_128_176;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

fn warm() -> FrameScene {
    let mut scene = FrameScene::new().unwrap_or_else(|_| unreachable!());
    for _ in 0..2 {
        assert!(scene.cpu_frame().is_ok());
    }
    scene
}

#[test]
fn the_cpu_stages_of_a_frame_make_no_allocator_call() {
    let mut scene = warm();
    let (done, counts) = count_allocations(|| scene.cpu_frame().is_ok());
    assert!(done);
    assert_eq!(counts.calls(), 0);
}

#[test]
fn the_scene_has_the_declared_size_and_many_batches() {
    let scene = warm();
    assert_eq!(scene.sprite_count(), SPRITES);
    assert!(scene.batch_count() >= BATCH_FLOOR);
}

#[test]
fn the_default_seed_gives_the_pinned_stream_bytes() {
    assert_eq!(fnv1a(warm().stream_data()), STREAM_HASH);
}

#[test]
fn the_unrotated_scene_gives_the_pinned_vertex_bytes() {
    let mut scene = FrameScene::with_options(SEED, false).unwrap_or_else(|_| unreachable!());
    assert!(scene.cpu_frame().is_ok());
    assert_eq!(fnv1a(scene.vertex_data()), VERTEX_HASH);
}

#[test]
fn rotation_changes_the_vertex_bytes_but_not_the_stream() {
    let mut flat = FrameScene::with_options(SEED, false).unwrap_or_else(|_| unreachable!());
    assert!(flat.cpu_frame().is_ok());
    let turned = warm();
    assert_ne!(fnv1a(flat.vertex_data()), fnv1a(turned.vertex_data()));
    assert_eq!(flat.stream_data(), turned.stream_data());
}

#[test]
fn the_same_seed_twice_gives_the_same_bytes() {
    let (a, b) = (warm(), warm());
    assert_eq!(fnv1a(a.vertex_data()), fnv1a(b.vertex_data()));
    assert_eq!(a.stream_data(), b.stream_data());
}

#[test]
fn a_different_seed_gives_different_vertex_bytes() {
    let mut other = FrameScene::with_seed(SEED + 1).unwrap_or_else(|_| unreachable!());
    assert!(other.cpu_frame().is_ok());
    assert_ne!(fnv1a(other.vertex_data()), fnv1a(warm().vertex_data()));
}

#[test]
fn the_null_device_accepts_the_stream_and_counts_one_draw_per_batch() {
    let mut scene = warm();
    let stats = scene.submit();
    assert!(stats.is_ok());
    let draws = stats.map(|s| s.draws).unwrap_or(0);
    assert_eq!(usize::try_from(draws).unwrap_or(0), scene.batch_count());
}

#[test]
fn the_byte_counts_match_the_data() {
    let scene = warm();
    assert_eq!(scene.stream_bytes(), scene.stream_data().len());
    assert!(scene.stream_bytes() > 1);
    assert_eq!(scene.vertex_upload_bytes(), scene.vertex_data().len());
    assert_eq!(scene.vertex_upload_bytes(), SPRITES * 80);
}
