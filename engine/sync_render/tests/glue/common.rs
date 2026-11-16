//! Shared set-up: textures, a camera, a render scene and entities that can be drawn.

use base::math::Vec2;
use base::pool::{Handle, Pool};
use ecs::{Entity, World};
use render::camera::OrthographicCamera;
use render::ids::{Texture, TextureId};
use render::scene::RenderScene;
use scene::{Transform2D, TransformUpdater};
use sync_render::{SpriteRenderer, SyncError};

pub(crate) fn textures(count: u32) -> Vec<TextureId> {
    let mut pool = Pool::<Texture>::with_capacity(count);
    (0..count)
        .map(|_| {
            pool.insert(Texture)
                .unwrap_or_else(|_| Handle::from_raw(0, 0))
        })
        .collect()
}

pub(crate) fn camera() -> OrthographicCamera {
    OrthographicCamera {
        position: Vec2::new(0.0, 0.0),
        height: 10.0,
        aspect: 1.0,
    }
}

pub(crate) fn render_scene(capacity: usize) -> RenderScene {
    RenderScene::new(capacity, camera()).unwrap_or_else(|_| unreachable!())
}

pub(crate) fn drawable(
    world: &mut World,
    local: Transform2D,
    renderer: SpriteRenderer,
) -> Result<Entity, SyncError> {
    let entity = world.spawn()?;
    world.insert(entity, local)?;
    world.insert(entity, renderer)?;
    Ok(entity)
}

pub(crate) fn at(x: f32, y: f32) -> Transform2D {
    Transform2D::from_position(Vec2::new(x, y))
}

pub(crate) fn unit(texture: TextureId) -> SpriteRenderer {
    SpriteRenderer::new(texture, Vec2::new(1.0, 1.0))
}

/// Updates the world transforms, as the frame does before the extraction.
pub(crate) fn refresh(world: &mut World) {
    assert!(TransformUpdater::new().update(world).is_ok());
}
