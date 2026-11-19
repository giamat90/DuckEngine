//! The three meshes every object of the sample is drawn from, made once at start: a sphere of
//! radius 1, a box of half extents 1 and a capsule of the radius and half length the bodies
//! scale from ([`CAPSULE_RADIUS`], [`CAPSULE_HALF_HEIGHT`]). An object's `Transform3D` scale turns
//! the unit mesh into its own size.

use crate::spawn::{Block, CAPSULE_HALF_HEIGHT, CAPSULE_RADIUS, scenery};
use base::math::Vec3;
use render::mesh::{
    MESH_VERTEX_STRIDE, MeshVertex, cube_indices, cube_vertices, encode_mesh_vertices,
};
use render::shapes::{
    Capsule, MeshCounts, capsule_counts, sphere_counts, write_box, write_capsule, write_sphere,
};

/// The [`MeshRef`] number of the sphere.
pub const SPHERE: u32 = 0;
/// The [`MeshRef`] number of the box.
pub const BOX: u32 = 1;
/// The [`MeshRef`] number of the capsule.
pub const CAPSULE: u32 = 2;
/// The [`MeshRef`] number of the first block of the scenery: the floor, then the four walls.
pub const SCENERY_FIRST: u32 = 3;
/// Metres one tile of the texture covers on the floor and the walls, in both directions (card
/// 8-31). The whole texture used to be stretched over each face, so a long wall and its thin
/// rim sampled it at very different rates. 12 m makes the squares 1.5 m, about what the floor
/// had (13 m over eight texels): no face is denser than before, because the texture has one
/// mip level and a denser pattern would alias more when far.
pub const TILE_METRES: f32 = 12.0;
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

/// The mesh of one block of the scenery: the box of half extents 1 (the block's own half extents
/// scale it, as for the pieces) with texture coordinates that run one unit per [`TILE_METRES`] of
/// the scaled face, so the texture has the same size in metres on every face.
fn scenery_mesh(block: &Block) -> MeshData {
    let mut vertices = cube_vertices().to_vec();
    let half = [block.half.x, block.half.y, block.half.z];
    for face in vertices.chunks_exact_mut(4) {
        // The corners of a face run: 0 top-left, 1 top-right, 2 bottom-left (see `cube_vertices`).
        let world =
            |v: &MeshVertex| std::array::from_fn::<f32, 3, _>(|k| v.position[k] * 2.0 * half[k]);
        let apart = |a: [f32; 3], b: [f32; 3]| {
            ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
        };
        let across = apart(world(&face[1]), world(&face[0])) / TILE_METRES;
        let down = apart(world(&face[0]), world(&face[2])) / TILE_METRES;
        for vertex in face {
            vertex.uv = [vertex.uv[0] * across, vertex.uv[1] * down];
            vertex.position = vertex.position.map(|p| p * 2.0);
        }
    }
    MeshData {
        vertices,
        indices: cube_indices().to_vec(),
    }
}

/// One mesh per block of [`scenery`], in its order, to be drawn from [`SCENERY_FIRST`] on:
/// the mesh of block `i` is [`SCENERY_FIRST`] + `i`.
#[must_use]
pub fn scenery_meshes() -> Vec<MeshData> {
    scenery().iter().map(scenery_mesh).collect()
}

#[cfg(test)]
mod tests;
