//! The render scene: the plain snapshot of what to draw this frame (design:
//! `wiki/architecture/ecs-and-sandbox-v0.md` section 4). A camera and a list of [`Sprite`]s with a
//! fixed capacity. The renderer consumes only this type, so it needs no knowledge of the ECS; a
//! glue crate fills it. After `new`, nothing here allocates.

use crate::camera::OrthographicCamera;
use crate::sprite::{BatchError, MAX_SPRITES, Sprite};
use base::math::Mat4;

/// A camera and the sprites to draw, in the order they were pushed.
pub struct RenderScene {
    camera: OrthographicCamera,
    sprites: Vec<Sprite>,
    capacity: usize,
}

impl RenderScene {
    /// A scene for up to `capacity` sprites. Allocates its space once.
    ///
    /// # Errors
    /// [`BatchError::TooManySprites`] when `capacity` exceeds [`crate::sprite::MAX_SPRITES`].
    pub fn new(capacity: usize, camera: OrthographicCamera) -> Result<Self, BatchError> {
        if capacity > MAX_SPRITES {
            return Err(BatchError::TooManySprites);
        }
        Ok(Self {
            camera,
            sprites: Vec::with_capacity(capacity),
            capacity,
        })
    }

    /// The camera.
    #[must_use]
    pub fn camera(&self) -> &OrthographicCamera {
        &self.camera
    }

    /// Replaces the camera.
    pub fn set_camera(&mut self, camera: OrthographicCamera) {
        self.camera = camera;
    }

    /// The matrix from world to clip space; `None` as [`OrthographicCamera::view_projection`].
    #[must_use]
    pub fn view_projection(&self) -> Option<Mat4> {
        self.camera.view_projection()
    }

    /// Removes every sprite; the capacity stays.
    pub fn clear(&mut self) {
        self.sprites.clear();
    }

    /// Adds a sprite after the ones already there.
    ///
    /// # Errors
    /// [`BatchError::TooManySprites`] when the scene is full; the scene is unchanged.
    pub fn push(&mut self, sprite: Sprite) -> Result<(), BatchError> {
        if self.sprites.len() >= self.capacity {
            return Err(BatchError::TooManySprites);
        }
        self.sprites.push(sprite);
        Ok(())
    }

    /// The sprites, in the order they were pushed.
    #[must_use]
    pub fn sprites(&self) -> &[Sprite] {
        &self.sprites
    }

    /// How many sprites the scene holds at most.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests;
