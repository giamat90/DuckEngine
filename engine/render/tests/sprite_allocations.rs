//! Preparing and recording a sprite frame makes no allocator call once the batcher and the
//! command stream are sized (zero heap allocations per frame on the hot path).

use base::alloc::{CountingAllocator, count_allocations};
use base::math::Vec2;
use base::pool::{Handle, Pool};
use render::ids::{Buffer, Pipeline, Sampler, Texture, TextureId};
use render::sprite::{Sprite, SpriteBatcher, SpriteBindings, SpriteVertex, UvRect};
use render::stream::CommandStream;
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

const SPRITES: usize = 512;

fn handle<T>(value: T) -> Handle<T> {
    Pool::<T>::with_capacity(1)
        .insert(value)
        .unwrap_or_else(|_| Handle::from_raw(0, 0))
}

fn scene(textures: &[TextureId]) -> Vec<Sprite> {
    (0..SPRITES)
        .map(|i| Sprite {
            texture: textures[i % textures.len()],
            position: Vec2::new(f32::from(u16::try_from(i).unwrap_or(0)), 0.0),
            size: Vec2::new(1.0, 1.0),
            angle: 0.1,
            uv: UvRect::FULL,
            tint: [255; 4],
            layer: i16::try_from(i % 4).unwrap_or(0),
        })
        .collect()
}

#[test]
fn a_sprite_frame_makes_no_allocator_call() {
    let mut pool = Pool::<Texture>::with_capacity(3);
    let textures: Vec<TextureId> = (0..3)
        .map(|_| {
            pool.insert(Texture)
                .unwrap_or_else(|_| Handle::from_raw(0, 0))
        })
        .collect();
    let sprites = scene(&textures);
    let bindings = SpriteBindings {
        pipeline: handle(Pipeline),
        uniforms: handle(Buffer),
        vertices: handle(Buffer),
        indices: handle(Buffer),
        sampler: handle(Sampler),
    };
    let mut batcher = SpriteBatcher::new(SPRITES).unwrap_or_else(|_| unreachable!());
    let mut staging = vec![SpriteVertex::default(); SPRITES * 4];
    let mut stream = CommandStream::new();
    for _ in 0..2 {
        assert!(batcher.prepare(&sprites, &mut staging).is_ok());
        batcher.record(&mut stream, &bindings);
        stream.clear();
    }
    let (ok, counts) = count_allocations(|| {
        let ready = batcher.prepare(&sprites, &mut staging).is_ok();
        batcher.record(&mut stream, &bindings);
        ready
    });
    assert!(ok);
    assert_eq!(counts.calls(), 0);
}
