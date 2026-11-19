use super::*;
use crate::mesh::{CUBE_INDEX_COUNT, CUBE_VERTEX_COUNT};
use base::math::Vec3;

const EPS: f32 = 1e-5;

fn vec(a: [f32; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

/// A mesh built into fresh buffers, trimmed to the counts the generator reports.
struct Built {
    vertices: Vec<MeshVertex>,
    indices: Vec<u16>,
}

fn build(
    counts: Option<MeshCounts>,
    write: impl Fn(&mut [MeshVertex], &mut [u16]) -> Option<MeshCounts>,
) -> Built {
    let c = counts.unwrap_or(MeshCounts {
        vertices: 0,
        indices: 0,
    });
    let mut vertices = vec![MeshVertex::default(); c.vertices];
    let mut indices = vec![0u16; c.indices];
    assert_eq!(write(&mut vertices, &mut indices), Some(c));
    Built { vertices, indices }
}

fn sphere(radius: f32, segments: u32, rings: u32) -> Built {
    build(sphere_counts(segments, rings), |v, i| {
        write_sphere(radius, segments, rings, v, i)
    })
}

fn capsule(radius: f32, half: f32, segments: u32, rings: u32) -> Built {
    build(capsule_counts(segments, rings), |v, i| {
        write_capsule(
            Capsule {
                radius,
                half_height: half,
                segments,
                cap_rings: rings,
            },
            v,
            i,
        )
    })
}

/// Indices in range, normals of unit length, every triangle counter-clockwise seen from outside
/// (its face normal agrees with the vertex normals) and with an area.
fn check_closed_surface(mesh: &Built) {
    assert_eq!(mesh.indices.len() % 3, 0);
    for v in &mesh.vertices {
        assert!((vec(v.normal).length() - 1.0).abs() < EPS, "unit normal");
        assert!(
            v.uv.iter().all(|t| (0.0..=1.0).contains(t)),
            "uv inside the image"
        );
    }
    for tri in mesh.indices.chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|k| mesh.vertices.get(usize::from(tri[k])));
        let (Some(a), Some(b), Some(c)) = (a, b, c) else {
            panic!("index out of range in {tri:?}")
        };
        let face = (vec(b.position) - vec(a.position)).cross(vec(c.position) - vec(a.position));
        assert!(face.length() > 1e-9, "no degenerate triangle: {tri:?}");
        let outward = vec(a.normal) + vec(b.normal) + vec(c.normal);
        assert!(
            face.dot(outward) > 0.0,
            "counter-clockwise from outside: {tri:?}"
        );
    }
}

#[test]
fn a_sphere_has_the_counts_its_rings_and_segments_give() {
    assert_eq!(
        sphere_counts(8, 4),
        Some(MeshCounts {
            vertices: 5 * 9,
            indices: 8 * (2 * 4 - 2) * 3
        })
    );
}

#[test]
fn every_vertex_of_a_sphere_is_on_it_with_the_normal_pointing_out() {
    let mesh = sphere(2.0, 12, 6);
    for v in &mesh.vertices {
        let p = vec(v.position);
        assert!((p.length() - 2.0).abs() < 1e-4);
        assert!((vec(v.normal) - p * 0.5).length() < 1e-4);
    }
    check_closed_surface(&mesh);
}

#[test]
fn a_sphere_reaches_both_poles_and_wraps_its_uv_once() {
    let mesh = sphere(1.0, 8, 4);
    let top = mesh
        .vertices
        .iter()
        .map(|v| v.position[1])
        .fold(f32::MIN, f32::max);
    let bottom = mesh
        .vertices
        .iter()
        .map(|v| v.position[1])
        .fold(f32::MAX, f32::min);
    assert!((top - 1.0).abs() < EPS && (bottom + 1.0).abs() < EPS);
    let first = mesh.vertices[0].uv;
    let last_of_ring = mesh.vertices[8].uv;
    assert!(first[0].abs() < EPS && (last_of_ring[0] - 1.0).abs() < EPS);
}

#[test]
fn a_capsule_is_a_cylinder_of_the_radius_with_a_hemisphere_on_each_end() {
    let mesh = capsule(0.5, 1.0, 12, 4);
    for v in &mesh.vertices {
        let p = vec(v.position);
        let on_axis = Vec3::new(0.0, p.y.clamp(-1.0, 1.0), 0.0);
        assert!(((p - on_axis).length() - 0.5).abs() < 1e-4);
        assert!((vec(v.normal) - (p - on_axis) * 2.0).length() < 1e-4);
    }
    let top = mesh
        .vertices
        .iter()
        .map(|v| v.position[1])
        .fold(f32::MIN, f32::max);
    assert!(
        (top - 1.5).abs() < EPS,
        "the cap reaches half height plus radius"
    );
    check_closed_surface(&mesh);
}

#[test]
fn a_capsule_has_the_counts_its_rings_and_segments_give() {
    assert_eq!(
        capsule_counts(8, 3),
        Some(MeshCounts {
            vertices: 8 * 9,
            indices: 12 * 8 * 3
        })
    );
}

#[test]
fn a_box_is_the_cube_scaled_to_its_half_extents() {
    let mut vertices = [MeshVertex::default(); CUBE_VERTEX_COUNT];
    let mut indices = [0u16; CUBE_INDEX_COUNT as usize];
    let counts = write_box(Vec3::new(1.0, 2.0, 3.0), &mut vertices, &mut indices);
    assert_eq!(
        counts,
        Some(MeshCounts {
            vertices: 24,
            indices: 36
        })
    );
    let max = |axis: usize| {
        vertices
            .iter()
            .map(|v| v.position[axis])
            .fold(f32::MIN, f32::max)
    };
    assert!((max(0) - 1.0).abs() < EPS && (max(1) - 2.0).abs() < EPS && (max(2) - 3.0).abs() < EPS);
    check_closed_surface(&Built {
        vertices: vertices.to_vec(),
        indices: indices.to_vec(),
    });
    assert_eq!(indices, crate::mesh::cube_indices());
}

#[test]
fn a_plane_lies_in_xz_facing_up() {
    let mut vertices = [MeshVertex::default(); 4];
    let mut indices = [0u16; 6];
    let counts = write_plane(2.0, 3.0, &mut vertices, &mut indices);
    assert_eq!(
        counts,
        Some(MeshCounts {
            vertices: 4,
            indices: 6
        })
    );
    for v in &vertices {
        assert!(
            v.normal[0].abs() < EPS && (v.normal[1] - 1.0).abs() < EPS && v.normal[2].abs() < EPS
        );
        assert!(v.position[1].abs() < EPS);
        assert!((v.position[0].abs() - 2.0).abs() < EPS && (v.position[2].abs() - 3.0).abs() < EPS);
    }
    check_closed_surface(&Built {
        vertices: vertices.to_vec(),
        indices: indices.to_vec(),
    });
}

#[test]
fn bad_parameters_are_refused_and_the_buffers_left_alone() {
    let mut vertices = vec![MeshVertex::default(); 1024];
    let mut indices = [7u16; 4096];
    let untouched = |v: &[MeshVertex], i: &[u16]| {
        v.iter().all(|x| *x == MeshVertex::default()) && i.iter().all(|x| *x == 7)
    };
    assert!(
        write_sphere(1.0, 2, 4, &mut vertices, &mut indices).is_none(),
        "two segments"
    );
    assert!(
        write_sphere(1.0, 8, 1, &mut vertices, &mut indices).is_none(),
        "one ring"
    );
    assert!(
        write_sphere(0.0, 8, 4, &mut vertices, &mut indices).is_none(),
        "no radius"
    );
    assert!(
        write_sphere(f32::NAN, 8, 4, &mut vertices, &mut indices).is_none(),
        "NaN radius"
    );
    assert!(
        write_capsule(
            Capsule {
                radius: 1.0,
                half_height: -1.0,
                segments: 8,
                cap_rings: 2
            },
            &mut vertices,
            &mut indices
        )
        .is_none(),
        "negative half"
    );
    assert!(
        write_capsule(
            Capsule {
                radius: 1.0,
                half_height: 1.0,
                segments: 8,
                cap_rings: 0
            },
            &mut vertices,
            &mut indices
        )
        .is_none(),
        "no cap rings"
    );
    assert!(
        write_box(Vec3::new(1.0, 0.0, 1.0), &mut vertices, &mut indices).is_none(),
        "flat box"
    );
    assert!(
        write_plane(-1.0, 1.0, &mut vertices, &mut indices).is_none(),
        "negative plane"
    );
    assert!(untouched(&vertices, &indices));
}

#[test]
fn buffers_that_are_too_small_are_refused_and_left_alone() {
    let mut vertices = [MeshVertex::default(); 44];
    let mut indices = [7u16; 4096];
    assert!(
        write_sphere(1.0, 8, 4, &mut vertices, &mut indices).is_none(),
        "one vertex short"
    );
    let mut vertices = vec![MeshVertex::default(); 1024];
    let mut indices = [7u16; 10];
    assert!(
        write_sphere(1.0, 8, 4, &mut vertices, &mut indices).is_none(),
        "indices short"
    );
    assert!(vertices.iter().all(|v| *v == MeshVertex::default()));
    assert!(indices.iter().all(|i| *i == 7));
}

#[test]
fn a_mesh_that_needs_more_than_16_bit_indices_is_refused() {
    assert!(sphere_counts(300, 300).is_none());
    assert!(sphere_counts(255, 255).is_some());
}

#[test]
fn the_segments_of_a_sphere_turn_a_full_circle_in_equal_steps() {
    let mesh = sphere(1.0, 4, 2);
    let equator = &mesh.vertices[5..10];
    let want = [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0], [1.0, 0.0]];
    for (v, [x, z]) in equator.iter().zip(want) {
        assert!((v.position[0] - x).abs() < 1e-5 && (v.position[2] - z).abs() < 1e-5);
    }
}

#[test]
fn the_smallest_sphere_is_accepted() {
    assert!(sphere_counts(3, 2).is_some());
    assert!(sphere_counts(2, 2).is_none() && sphere_counts(3, 1).is_none());
}

#[test]
fn a_plane_needs_both_half_extents() {
    let mut vertices = [MeshVertex::default(); 4];
    let mut indices = [0u16; 6];
    assert!(write_plane(1.0, 0.0, &mut vertices, &mut indices).is_none());
    assert!(write_plane(0.0, 1.0, &mut vertices, &mut indices).is_none());
    assert!(write_plane(1.0, 1.0, &mut vertices, &mut indices).is_some());
}
