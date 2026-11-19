//! The 3D copy of the render glue (card 8-10): an entity with a [`MeshRenderer`] and a
//! [`scene::WorldTransform3D`] becomes one instance of the [`render::mesh_scene::MeshScene`], the
//! plain snapshot the renderer consumes. The renderer never sees the ECS.

use crate::error::SyncError;
use crate::extract::{ExtractReport, bump};
use base::math::Mat4;
use ecs::{Read, World};
use render::ids::TextureId;
use render::lit::{Material, normal_matrix};
use render::mesh_scene::{MeshInstance, MeshRef, MeshScene};
use scene::WorldTransform3D;

/// How an entity is drawn as a lit mesh. The place, the turn and the scale come from the world
/// transform of the entity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshRenderer {
    /// The mesh, as the caller numbers its uploaded meshes.
    pub mesh: MeshRef,
    /// Surface colour.
    pub material: Material,
    /// The texture sampled by the fragment shader.
    pub texture: TextureId,
}

impl MeshRenderer {
    /// A white material.
    #[must_use]
    pub fn new(mesh: MeshRef, texture: TextureId) -> Self {
        Self {
            mesh,
            material: Material { colour: [1.0; 4] },
            texture,
        }
    }
}

/// Replaces the instances of `scene` with one per entity that has a [`MeshRenderer`] and a
/// `WorldTransform3D`, in the storage order of the ECS. The camera and the light are not touched.
/// An entity whose matrix is not finite, has no normal matrix (a zero scale) or overflows when the
/// camera's view-projection is applied, or whose colour is not finite, is counted in `skipped`: every
/// instance of the scene has a transform block for the camera it was extracted with.
///
/// # Errors
/// [`SyncError::Ecs`] when the query cannot run.
pub fn extract_meshes(
    world: &mut World,
    scene: &mut MeshScene,
) -> Result<ExtractReport, SyncError> {
    scene.clear();
    let mut report = ExtractReport::default();
    let view_projection = scene.view_projection();
    world.for_each::<(Read<WorldTransform3D>, Read<MeshRenderer>), _>(
        |(transform, renderer)| match to_instance(renderer, transform, view_projection.as_ref()) {
            Some(instance) if scene.push(instance).is_ok() => bump(&mut report.extracted),
            Some(_) => bump(&mut report.dropped),
            None => bump(&mut report.skipped),
        },
    )?;
    Ok(report)
}

/// The instance of a renderer at a world transform; `None` when it could not be drawn.
fn to_instance(
    renderer: &MeshRenderer,
    transform: &WorldTransform3D,
    view_projection: Option<&Mat4>,
) -> Option<MeshInstance> {
    let colour_ok = renderer.material.colour.iter().all(|c| c.is_finite());
    let seen = view_projection.is_none_or(|vp| (*vp * transform.matrix).is_finite());
    let drawable = colour_ok
        && seen
        && transform.matrix.is_finite()
        && normal_matrix(&transform.matrix).is_some();
    drawable.then_some(MeshInstance {
        mesh: renderer.mesh,
        model: transform.matrix,
        material: renderer.material,
        texture: renderer.texture,
    })
}
