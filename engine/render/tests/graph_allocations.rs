//! Declaring, compiling and recording a render graph makes no allocator call once the command
//! stream has its capacity (zero heap allocations per frame on the hot path).

use base::alloc::{CountingAllocator, count_allocations};
use render::desc::{TextureDesc, TextureFormat};
use render::graph::RenderGraph;
use render::ids::{Texture, TextureId};
use render::stream::CommandStream;
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const CLEAR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

fn desc() -> TextureDesc {
    TextureDesc {
        width: 64,
        height: 64,
        mip_count: 1,
        format: TextureFormat::Rgba8,
    }
}

fn texture() -> TextureId {
    let mut pool = base::pool::Pool::<Texture>::with_capacity(1);
    pool.insert(Texture).unwrap_or_else(|_| unreachable!())
}

/// One whole frame: declare, compile, record.
fn frame(texture: TextureId, stream: &mut CommandStream) -> bool {
    let mut graph = RenderGraph::new();
    let Ok(back) = graph.import_backbuffer() else {
        return false;
    };
    let Ok(scene) = graph.transient(desc()) else {
        return false;
    };
    let declared = graph.add_pass("scene", scene, Some(CLEAR), &[]).is_ok()
        && graph.add_pass("main", back, None, &[scene]).is_ok();
    let compiled = graph.compile();
    let recorded = compiled
        .record(&[(scene, texture)], stream, |_, s| s.draw(3, 1))
        .is_ok();
    declared && recorded
}

#[test]
fn a_frame_of_the_graph_makes_no_allocator_call() {
    let texture = texture();
    let mut stream = CommandStream::new();
    assert!(frame(texture, &mut stream));
    stream.clear();
    let (ok, counts) = count_allocations(|| frame(texture, &mut stream));
    assert!(ok);
    assert_eq!(counts.calls(), 0);
}
