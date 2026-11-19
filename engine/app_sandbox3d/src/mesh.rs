//! The three meshes every object of the sample is drawn from, made once at start: a sphere of
//! radius 1, a box of half extents 1 and a capsule of the radius and half length the bodies
//! scale from ([`CAPSULE_RADIUS`], [`CAPSULE_HALF_HEIGHT`]). An object's `Transform3D` scale turns
//! the unit mesh into its own size.

use crate::spawn::{CAPSULE_HALF_HEIGHT, CAPSULE_RADIUS};
use base::math::Vec3;
use render::mesh::{MESH_VERTEX_STRIDE, MeshVertex, encode_mesh_vertices};
use render::shapes::{
    Capsule, MeshCounts, capsule_counts, sphere_counts, write_box, write_capsule, write_sphere,
};

/// The [`MeshRef`] number of the sphere.
pub const SPHERE: u32 = 0;
/// The [`MeshRef`] number of the box.
pub const BOX: u32 = 1;
/// The [`MeshRef`] number of the capsule.
pub const CAPSULE: u32 = 2;
/// Segments around the axis of a round mesh.
const SEGMENTS: u32 = 24;
/// Bands from pole to pole of the sphere.
const RINGS: u32 = 12;
/// Bands in each hemisphere of the capsule.
const CAP_RINGS: u32 = 6;

/// The vertices and indices of one mesh.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    /// Vertices.
    pub vertices: Vec<MeshVertex>,
    /// 16-bit indices, three to a triangle.
    pub indices: Vec<u16>,
}

impl MeshData {
    /// The vertices as the bytes of the vertex buffer; `None` if they cannot be encoded.
    #[must_use]
    pub fn vertex_bytes(&self) -> Option<Vec<u8>> {
        let mut bytes = vec![0u8; self.vertices.len() * MESH_VERTEX_STRIDE as usize];
        encode_mesh_vertices(&self.vertices, &mut bytes).then_some(bytes)
    }

    /// The indices as the bytes of the index buffer.
    #[must_use]
    pub fn index_bytes(&self) -> Vec<u8> {
        self.indices.iter().flat_map(|i| i.to_le_bytes()).collect()
    }
}

/// Room for `counts`, then the mesh a writer makes in it.
fn made(
    counts: Option<MeshCounts>,
    write: impl FnOnce(&mut [MeshVertex], &mut [u16]) -> Option<MeshCounts>,
) -> Option<MeshData> {
    let counts = counts?;
    let mut mesh = MeshData {
        vertices: vec![MeshVertex::default(); counts.vertices],
        indices: vec![0; counts.indices],
    };
    let written = write(&mut mesh.vertices, &mut mesh.indices)?;
    (written == counts).then_some(mesh)
}

/// The sphere, the box and the capsule, in the order of [`SPHERE`], [`BOX`] and [`CAPSULE`];
/// `None` if a mesh cannot be made.
#[must_use]
pub fn unit_meshes() -> Option<[MeshData; 3]> {
    let sphere = made(sphere_counts(SEGMENTS, RINGS), |v, i| {
        write_sphere(1.0, SEGMENTS, RINGS, v, i)
    })?;
    let cuboid = made(
        Some(MeshCounts {
            vertices: render::mesh::CUBE_VERTEX_COUNT,
            indices: render::mesh::CUBE_INDEX_COUNT as usize,
        }),
        |v, i| write_box(Vec3::ONE, v, i),
    )?;
    let capsule = made(capsule_counts(SEGMENTS, CAP_RINGS), |v, i| {
        write_capsule(
            Capsule {
                radius: CAPSULE_RADIUS,
                half_height: CAPSULE_HALF_HEIGHT,
                segments: SEGMENTS,
                cap_rings: CAP_RINGS,
            },
            v,
            i,
        )
    })?;
    Some([sphere, cuboid, capsule])
}

#[cfg(test)]
mod tests;
