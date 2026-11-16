use super::*;
use crate::ids::{Texture, TextureId};
use crate::sprite::{MAX_SPRITES, SpriteBatcher, SpriteVertex, UvRect};
use base::math::Vec2;
use base::pool::{Handle, Pool};

fn texture() -> TextureId {
    Pool::<Texture>::with_capacity(1)
        .insert(Texture)
        .unwrap_or_else(|_| Handle::from_raw(0, 0))
}

fn camera() -> OrthographicCamera {
    OrthographicCamera {
        position: Vec2::new(1.0, 2.0),
        height: 10.0,
        aspect: 2.0,
    }
}

fn sprite(x: f32, layer: i16) -> Sprite {
    Sprite {
        texture: texture(),
        position: Vec2::new(x, 0.0),
        size: Vec2::new(1.0, 1.0),
        angle: 0.0,
        uv: UvRect::FULL,
        tint: [255; 4],
        layer,
    }
}

fn scene(capacity: usize) -> RenderScene {
    RenderScene::new(capacity, camera()).unwrap_or_else(|_| unreachable!())
}

#[test]
fn a_new_scene_is_empty_and_keeps_its_capacity_and_camera() {
    let scene = scene(8);
    assert!(scene.sprites().is_empty());
    assert_eq!(scene.capacity(), 8);
    assert_eq!(*scene.camera(), camera());
}

#[test]
fn a_capacity_above_the_maximum_is_refused() {
    assert!(RenderScene::new(MAX_SPRITES, camera()).is_ok());
    assert_eq!(
        RenderScene::new(MAX_SPRITES + 1, camera()).err(),
        Some(BatchError::TooManySprites)
    );
}

#[test]
fn sprites_keep_the_order_they_were_pushed_in() {
    let mut scene = scene(4);
    for x in [3.0, 1.0, 2.0] {
        assert_eq!(scene.push(sprite(x, 0)), Ok(()));
    }
    let xs: Vec<f32> = scene.sprites().iter().map(|s| s.position.x).collect();
    assert_eq!(xs, vec![3.0, 1.0, 2.0]);
}

#[test]
fn a_full_scene_refuses_a_sprite_and_stays_as_it_was() {
    let mut scene = scene(2);
    assert_eq!(scene.push(sprite(1.0, 0)), Ok(()));
    assert_eq!(scene.push(sprite(2.0, 0)), Ok(()));
    assert_eq!(scene.push(sprite(3.0, 0)), Err(BatchError::TooManySprites));
    assert_eq!(scene.sprites().len(), 2);
    assert_eq!(scene.sprites().get(1), Some(&sprite(2.0, 0)));
}

#[test]
fn a_scene_of_capacity_zero_takes_nothing() {
    let mut scene = scene(0);
    assert_eq!(scene.push(sprite(1.0, 0)), Err(BatchError::TooManySprites));
}

#[test]
fn clear_empties_the_scene_and_it_can_be_filled_again() {
    let mut scene = scene(2);
    assert_eq!(scene.push(sprite(1.0, 0)), Ok(()));
    assert_eq!(scene.push(sprite(2.0, 0)), Ok(()));
    scene.clear();
    assert!(scene.sprites().is_empty());
    assert_eq!(scene.capacity(), 2);
    assert_eq!(scene.push(sprite(5.0, 0)), Ok(()));
    assert_eq!(scene.sprites().len(), 1);
}

#[test]
fn the_camera_can_be_replaced_and_gives_its_matrix() {
    let mut scene = scene(1);
    let other = OrthographicCamera {
        position: Vec2::new(-4.0, 0.5),
        height: 6.0,
        aspect: 1.5,
    };
    scene.set_camera(other);
    assert_eq!(*scene.camera(), other);
    assert_eq!(scene.view_projection(), other.view_projection());
    assert!(scene.view_projection().is_some());
}

#[test]
fn the_batcher_prepares_a_scene_like_its_slice_of_sprites() {
    let mut scene = scene(4);
    for (x, layer) in [(1.0, 2), (2.0, 0), (3.0, 1)] {
        assert_eq!(scene.push(sprite(x, layer)), Ok(()));
    }
    let mut a = SpriteBatcher::new(4).unwrap_or_else(|_| unreachable!());
    let mut b = SpriteBatcher::new(4).unwrap_or_else(|_| unreachable!());
    let mut from_scene = vec![SpriteVertex::default(); 16];
    let mut from_slice = vec![SpriteVertex::default(); 16];
    let batches_scene = a.prepare_scene(&scene, &mut from_scene).map(<[_]>::to_vec);
    let batches_slice = b
        .prepare(scene.sprites(), &mut from_slice)
        .map(<[_]>::to_vec);
    assert_eq!(batches_scene, batches_slice);
    assert_eq!(from_scene, from_slice);
    // All three share one texture handle, so they form one batch.
    assert_eq!(batches_scene.map(|b| b.len()), Ok(1));
}

#[test]
fn an_empty_scene_prepares_no_batches() {
    let scene = scene(2);
    let mut batcher = SpriteBatcher::new(2).unwrap_or_else(|_| unreachable!());
    let mut staging = vec![SpriteVertex::default(); 8];
    assert_eq!(
        batcher.prepare_scene(&scene, &mut staging).map(<[_]>::len),
        Ok(0)
    );
}
