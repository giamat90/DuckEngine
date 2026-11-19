//! The mesh scene: the plain snapshot of what to draw this frame in 3D (card 8-10; design:
//! `wiki/architecture/phase8-3d-v0.md` section 5), the counterpart of [`crate::scene::RenderScene`]
//! for lit meshes. A perspective camera, one directional light and a list of [`MeshInstance`]s
//! with a fixed capacity. The renderer consumes only this type, so it needs no knowledge of the
//! ECS; a glue crate fills it. A mesh is named by a [`MeshRef`], an index into the caller's table
//! of uploaded meshes (vertex and index buffers, index count), so the snapshot holds no buffer
//! handle of a device. After `new`, nothing here allocates.

use crate::camera::PerspectiveCamera;
use crate::ids::TextureId;
use crate::lit::{DirectionalLight, Material};
use base::math::Mat4;

/// Most instances per scene.
pub const MAX_MESH_INSTANCES: usize = 16_384;

/// A mesh of the caller's table: its meaning belongs to whoever draws the scene.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshRef(pub u32);

/// One mesh to draw: which mesh, where, and how it looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshInstance {
    /// The mesh, as the caller numbers its meshes.
    pub mesh: MeshRef,
    /// Model space to world space; it must give a normal matrix (see
    /// [`crate::lit::normal_matrix`]) or the draw has no transform block.
    pub model: Mat4,
    /// Surface colour.
    pub material: Material,
    /// The texture sampled by the fragment shader.
    pub texture: TextureId,
}

/// Why a mesh scene call was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshSceneError {
    /// The capacity is above [`MAX_MESH_INSTANCES`], or the scene is full.
    TooManyInstances,
}

/// A camera, a light and the mesh instances to draw, in the order they were pushed.
pub struct MeshScene {
    camera: PerspectiveCamera,
    light: DirectionalLight,
    instances: Vec<MeshInstance>,
    capacity: usize,
}

impl MeshScene {
    /// A scene for up to `capacity` instances. Allocates its space once.
    ///
    /// # Errors
    /// [`MeshSceneError::TooManyInstances`] when `capacity` exceeds [`MAX_MESH_INSTANCES`].
    pub fn new(
        capacity: usize,
        camera: PerspectiveCamera,
        light: DirectionalLight,
    ) -> Result<Self, MeshSceneError> {
        if capacity > MAX_MESH_INSTANCES {
            return Err(MeshSceneError::TooManyInstances);
        }
        Ok(Self {
            camera,
            light,
            instances: Vec::with_capacity(capacity),
            capacity,
        })
    }

    /// The camera.
    #[must_use]
    pub fn camera(&self) -> &PerspectiveCamera {
        &self.camera
    }

    /// Replaces the camera.
    pub fn set_camera(&mut self, camera: PerspectiveCamera) {
        self.camera = camera;
    }

    /// The light.
    #[must_use]
    pub fn light(&self) -> &DirectionalLight {
        &self.light
    }

    /// Replaces the light.
    pub fn set_light(&mut self, light: DirectionalLight) {
        self.light = light;
    }

    /// The matrix from world to clip space; `None` as [`PerspectiveCamera::view_projection`].
    #[must_use]
    pub fn view_projection(&self) -> Option<Mat4> {
        self.camera.view_projection()
    }

    /// Removes every instance; the capacity, the camera and the light stay.
    pub fn clear(&mut self) {
        self.instances.clear();
    }

    /// Adds an instance after the ones already there.
    ///
    /// # Errors
    /// [`MeshSceneError::TooManyInstances`] when the scene is full; the scene is unchanged.
    pub fn push(&mut self, instance: MeshInstance) -> Result<(), MeshSceneError> {
        if self.instances.len() >= self.capacity {
            return Err(MeshSceneError::TooManyInstances);
        }
        self.instances.push(instance);
        Ok(())
    }

    /// The instances, in the order they were pushed.
    #[must_use]
    pub fn instances(&self) -> &[MeshInstance] {
        &self.instances
    }

    /// How many instances the scene holds at most.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests;
