//! The copy from the ECS into the render scene.

use crate::error::SyncError;
use crate::sprite::SpriteRenderer;
use base::math::{Mat3, Vec2};
use ecs::{Read, World};
use render::scene::RenderScene;
use render::sprite::Sprite;
use scene::WorldTransform2D;

/// What an extraction did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ExtractReport {
    /// Sprites put into the scene.
    pub extracted: u32,
    /// Entities passed over: a value that is not finite or a size that is not positive.
    pub skipped: u32,
    /// Sprites that did not fit: the scene was full.
    pub dropped: u32,
}

/// Replaces the sprites of `scene` with one sprite per entity that has a [`crate::SpriteRenderer`]
/// and a `WorldTransform2D`, in the storage order of the ECS. The camera is not touched.
///
/// # Errors
/// [`SyncError::Ecs`] when the query cannot run.
pub fn extract_sprites(
    world: &mut World,
    scene: &mut RenderScene,
) -> Result<ExtractReport, SyncError> {
    scene.clear();
    let mut report = ExtractReport::default();
    world.for_each::<(Read<WorldTransform2D>, Read<SpriteRenderer>), _>(
        |(transform, renderer)| place(scene, to_sprite(renderer, &transform.matrix), &mut report),
    )?;
    Ok(report)
}

/// Adds one to a counter without overflow.
pub(crate) fn bump(counter: &mut u32) {
    *counter = counter.saturating_add(1);
}

/// Puts a sprite into the scene, or counts why it did not go in.
fn place(scene: &mut RenderScene, sprite: Option<Sprite>, report: &mut ExtractReport) {
    let Some(sprite) = sprite else {
        bump(&mut report.skipped);
        return;
    };
    if scene.push(sprite).is_ok() {
        bump(&mut report.extracted);
    } else {
        bump(&mut report.dropped);
    }
}

/// The sprite of a renderer at a world matrix: the position is the translation, the angle is the
/// turn of the x axis, and the size is the size at scale one times the length of each axis.
/// `None` for a value that is not finite or a size that is not positive. A mirror is not drawn
/// mirrored: the lengths of the axes carry no sign, so a negative scale on x shows the sprite
/// turned by half a circle (an asymmetric image looks wrong).
fn to_sprite(renderer: &SpriteRenderer, matrix: &Mat3) -> Option<Sprite> {
    let [x_axis, y_axis, origin] = matrix.cols;
    let position = origin.truncate();
    let angle = x_axis.y.atan2(x_axis.x);
    let size = Vec2::new(
        renderer.size.x * x_axis.truncate().length(),
        renderer.size.y * y_axis.truncate().length(),
    );
    let finite = matrix.is_finite() && size.x.is_finite() && size.y.is_finite();
    (finite && size.x > 0.0 && size.y > 0.0).then_some(Sprite {
        texture: renderer.texture,
        position,
        size,
        angle,
        uv: renderer.uv,
        tint: renderer.tint,
        layer: renderer.layer,
    })
}
