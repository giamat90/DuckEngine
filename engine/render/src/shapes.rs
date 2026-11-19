//! Generated primitive meshes (card 8-9; design: `wiki/architecture/phase8-3d-v0.md` section 4):
//! sphere, capsule, box and plane, with normals, next to the built-in [`crate::mesh::cube_vertices`].
//!
//! Each generator writes into caller-supplied slices (so a mesh is built once at load, with no
//! allocation here) and returns how many vertices and 16-bit indices it wrote, or `None` and
//! writes nothing when a parameter is not positive and finite, a mesh would need more than 65 536
//! vertices, or a slice is too small. Every triangle is counter-clockwise seen from outside. The
//! sphere and the capsule have their axis along Y, centred on the origin; the plane lies in XZ
//! facing +Y.

use crate::mesh::{CUBE_INDEX_COUNT, CUBE_VERTEX_COUNT, MeshVertex, cube_indices, cube_vertices};
use base::dmath::sin_cos;
use base::math::Vec3;
use core::f32::consts::{FRAC_PI_2, PI, TAU};

/// How many vertices and indices a mesh has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshCounts {
    /// Entries of the vertex slice.
    pub vertices: usize,
    /// Entries of the index slice.
    pub indices: usize,
}

/// Fewest segments around the axis.
const MIN_SEGMENTS: u32 = 3;
/// Vertices a 16-bit index can address.
const MAX_VERTICES: usize = 1 << 16;
const TRIANGLE: usize = 3;
const PLANE_VERTICES: usize = 4;
const PLANE_INDICES: usize = 6;
/// The two triangles of the plane's quad (corners -x-z, +x-z, -x+z, +x+z).
const PLANE_QUAD: [u16; PLANE_INDICES] = [0, 2, 1, 1, 2, 3];

fn positive(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

/// Counts of a surface of revolution with `segments` around and `rows` bands between rings of
/// vertices (a pole at each end, so the first and the last band are triangles).
fn lathe_counts(segments: u32, rows: u32) -> Option<MeshCounts> {
    if segments < MIN_SEGMENTS || rows < 2 {
        return None;
    }
    let segments = usize::try_from(segments).ok()?;
    let rows = usize::try_from(rows).ok()?;
    let vertices = rows.checked_add(1)?.checked_mul(segments.checked_add(1)?)?;
    if vertices > MAX_VERTICES {
        return None;
    }
    let triangles = segments.checked_mul(2 * rows - 2)?;
    Some(MeshCounts {
        vertices,
        indices: triangles * TRIANGLE,
    })
}

/// The counts of [`write_sphere`]; `None` for fewer than three segments or two rings, or more
/// than 65 536 vertices.
#[must_use]
pub fn sphere_counts(segments: u32, rings: u32) -> Option<MeshCounts> {
    lathe_counts(segments, rings)
}

/// The counts of [`write_capsule`]; `None` as [`sphere_counts`], and for no cap rings.
#[must_use]
pub fn capsule_counts(segments: u32, cap_rings: u32) -> Option<MeshCounts> {
    if cap_rings == 0 {
        return None;
    }
    lathe_counts(segments, cap_rings.checked_mul(2)?.checked_add(1)?)
}

/// Whether the slices can hold `counts`.
fn fits(counts: MeshCounts, vertices: &[MeshVertex], indices: &[u16]) -> bool {
    vertices.len() >= counts.vertices && indices.len() >= counts.indices
}

/// `k / n` as a float; both are at most 65 535 in any mesh that passed [`lathe_counts`].
fn fraction(k: u32, n: u32) -> f32 {
    let small = |v: u32| f32::from(u16::try_from(v).unwrap_or(u16::MAX));
    small(k) / small(n)
}

/// Where a ring of vertices sits: its angle from the +Y pole and its height shift.
type Ring = (f32, f32);

/// Writes a surface of revolution: ring `k` of `rows + 1` is at `ring(k)`.
fn write_lathe(
    radius: f32,
    segments: u32,
    rows: u32,
    ring: impl Fn(u32) -> Ring,
    out: (&mut [MeshVertex], &mut [u16]),
) {
    let (vertices, indices) = out;
    let mut at = 0;
    for k in 0..=rows {
        let (theta, lift) = ring(k);
        let (sin_t, cos_t) = sin_cos(theta);
        for s in 0..=segments {
            let (sin_p, cos_p) = sin_cos(TAU * fraction(s, segments));
            let normal = [sin_t * cos_p, cos_t, sin_t * sin_p];
            vertices[at] = MeshVertex {
                position: [
                    normal[0] * radius,
                    normal[1] * radius + lift,
                    normal[2] * radius,
                ],
                normal,
                uv: [fraction(s, segments), fraction(k, rows)],
            };
            at += 1;
        }
    }
    write_lathe_indices(segments, rows, indices);
}

/// The triangles of a surface of revolution: one per segment in the two pole bands, two in the
/// others, each counter-clockwise seen from outside.
fn write_lathe_indices(segments: u32, rows: u32, indices: &mut [u16]) {
    let stride = segments + 1;
    let mut at = 0;
    for k in 0..rows {
        for s in 0..segments {
            let a = u16::try_from(k * stride + s).unwrap_or(0);
            let (b, c) = (a + 1, a + u16::try_from(stride).unwrap_or(0));
            let d = c + 1;
            let bands: &[[u16; 3]] = match k {
                0 => &[[c, b, d]],
                last if last + 1 == rows => &[[a, b, c]],
                _ => &[[a, b, c], [c, b, d]],
            };
            at = write_triangles(indices, at, bands);
        }
    }
}

/// Copies `triangles` into `indices` from `at`; returns the next free place.
fn write_triangles(indices: &mut [u16], at: usize, triangles: &[[u16; 3]]) -> usize {
    let mut at = at;
    for tri in triangles {
        indices[at..at + TRIANGLE].copy_from_slice(tri);
        at += TRIANGLE;
    }
    at
}

/// Writes a sphere of `radius` with `segments` around Y and `rings` bands from pole to pole.
#[must_use]
pub fn write_sphere(
    radius: f32,
    segments: u32,
    rings: u32,
    vertices: &mut [MeshVertex],
    indices: &mut [u16],
) -> Option<MeshCounts> {
    let counts = sphere_counts(segments, rings)?;
    if !positive(radius) || !fits(counts, vertices, indices) {
        return None;
    }
    let ring = |k: u32| (PI * fraction(k, rings), 0.0);
    write_lathe(radius, segments, rings, ring, (vertices, indices));
    Some(counts)
}

/// The shape of a capsule: a cylinder along Y closed by a hemisphere at each end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Capsule {
    /// Radius of the cylinder and of the caps.
    pub radius: f32,
    /// Half the length of the cylinder part, zero or more.
    pub half_height: f32,
    /// Segments around the axis.
    pub segments: u32,
    /// Bands in each hemisphere.
    pub cap_rings: u32,
}

/// Writes `capsule`: the cylinder runs from `-half_height` to `half_height`.
#[must_use]
pub fn write_capsule(
    capsule: Capsule,
    vertices: &mut [MeshVertex],
    indices: &mut [u16],
) -> Option<MeshCounts> {
    let Capsule {
        radius,
        half_height,
        segments,
        cap_rings,
    } = capsule;
    let counts = capsule_counts(segments, cap_rings)?;
    let half_ok = half_height.is_finite() && half_height >= 0.0;
    if !positive(radius) || !half_ok || !fits(counts, vertices, indices) {
        return None;
    }
    let ring = |k: u32| {
        let angle = |j: u32| FRAC_PI_2 * fraction(j, cap_rings);
        if k <= cap_rings {
            (angle(k), half_height)
        } else {
            (FRAC_PI_2 + angle(k - cap_rings - 1), -half_height)
        }
    };
    write_lathe(
        radius,
        segments,
        2 * cap_rings + 1,
        ring,
        (vertices, indices),
    );
    Some(counts)
}

/// Writes a box with the given half extents: the unit cube scaled, 24 vertices and 36 indices.
#[must_use]
pub fn write_box(
    half_extents: Vec3,
    vertices: &mut [MeshVertex],
    indices: &mut [u16],
) -> Option<MeshCounts> {
    let counts = MeshCounts {
        vertices: CUBE_VERTEX_COUNT,
        indices: CUBE_INDEX_COUNT as usize,
    };
    let sizes = [half_extents.x, half_extents.y, half_extents.z];
    if !sizes.iter().all(|h| positive(*h)) || !fits(counts, vertices, indices) {
        return None;
    }
    for (out, cube) in vertices.iter_mut().zip(cube_vertices()) {
        let scaled = |axis: usize| cube.position[axis] * 2.0 * sizes[axis];
        *out = MeshVertex {
            position: [scaled(0), scaled(1), scaled(2)],
            ..cube
        };
    }
    indices[..counts.indices].copy_from_slice(&cube_indices());
    Some(counts)
}

/// Writes a plane in XZ facing +Y, `2 * half_x` by `2 * half_z`: four vertices, six indices.
#[must_use]
pub fn write_plane(
    half_x: f32,
    half_z: f32,
    vertices: &mut [MeshVertex],
    indices: &mut [u16],
) -> Option<MeshCounts> {
    let counts = MeshCounts {
        vertices: PLANE_VERTICES,
        indices: PLANE_INDICES,
    };
    if !positive(half_x) || !positive(half_z) || !fits(counts, vertices, indices) {
        return None;
    }
    for (i, out) in vertices.iter_mut().take(PLANE_VERTICES).enumerate() {
        let (right, far) = (i % 2 == 1, i / 2 == 1);
        *out = MeshVertex {
            position: [
                if right { half_x } else { -half_x },
                0.0,
                if far { half_z } else { -half_z },
            ],
            normal: [0.0, 1.0, 0.0],
            uv: [if right { 1.0 } else { 0.0 }, if far { 0.0 } else { 1.0 }],
        };
    }
    indices[..PLANE_INDICES].copy_from_slice(&PLANE_QUAD);
    Some(counts)
}

#[cfg(test)]
mod tests;
