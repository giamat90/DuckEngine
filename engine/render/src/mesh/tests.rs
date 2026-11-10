use super::*;
use crate::desc::{PassDesc, Rect, RenderTarget};
use crate::dump::dump;
use base::pool::Handle;

const GOLDEN: &str = include_str!("../../golden/mesh_scene.txt");

fn h<T>(index: u32, generation: u32) -> Handle<T> {
    Handle::from_raw(index, generation)
}

fn bindings() -> MeshBindings {
    MeshBindings {
        pipeline: h(0, 0),
        vertices: h(1, 0),
        uniforms: h(2, 0),
        indices: h(3, 0),
        texture: h(4, 0),
        sampler: h(5, 0),
    }
}

fn mesh_scene() -> CommandStream {
    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: Some([0.0, 0.0, 0.25, 1.0]),
        clear_depth: Some(1.0),
        depth: Some(h(6, 0)),
    });
    stream.set_viewport(Rect {
        x: 0,
        y: 0,
        width: 640,
        height: 480,
    });
    record_mesh(&mut stream, &bindings(), CUBE_INDEX_COUNT);
    stream.end_pass();
    stream
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Equal within the rounding of a few operations (the cube is made of halves, so this is slack).
fn is(value: f32, want: f32) -> bool {
    (value - want).abs() < 1e-6
}

fn same(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| is(*x, y))
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[test]
fn the_mesh_scene_matches_the_checked_in_golden_dump() {
    // A Windows checkout may convert the golden file to CRLF.
    let golden: String = GOLDEN.chars().filter(|c| *c != char::from(13u8)).collect();
    assert_eq!(dump(&mesh_scene()), golden);
}

#[test]
fn nothing_is_recorded_for_a_mesh_with_no_indices() {
    let mut stream = CommandStream::new();
    record_mesh(&mut stream, &bindings(), 0);
    assert_eq!(stream.commands().count(), 0);
}

#[test]
fn the_vertex_layout_is_position_normal_uv_in_32_bytes() {
    let attributes = mesh_vertex_attributes();
    let layout: Vec<_> = attributes
        .iter()
        .map(|a| (a.location, a.format, a.offset))
        .collect();
    assert_eq!(
        layout,
        [
            (0, VertexFormat::Float3, 0),
            (1, VertexFormat::Float3, 12),
            (2, VertexFormat::Float2, 24),
        ]
    );
    assert_eq!(MESH_VERTEX_STRIDE, 32);
}

#[test]
fn a_vertex_is_written_as_eight_little_endian_floats() {
    let vertex = MeshVertex {
        position: [1.0, 2.0, 3.0],
        normal: [0.0, 1.0, 0.0],
        uv: [0.25, 0.75],
    };
    let mut out = [0u8; 32];
    assert!(encode_mesh_vertices(&[vertex], &mut out));
    let floats: Vec<f32> = out
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    assert_eq!(floats, [1.0, 2.0, 3.0, 0.0, 1.0, 0.0, 0.25, 0.75]);
}

#[test]
fn a_slice_of_the_wrong_size_is_refused_and_left_alone() {
    let vertex = MeshVertex {
        position: [1.0; 3],
        normal: [1.0; 3],
        uv: [1.0; 2],
    };
    for len in [0usize, 31, 33, 64] {
        let mut out = vec![7u8; len];
        assert!(!encode_mesh_vertices(&[vertex], &mut out), "len {len}");
        assert!(out.iter().all(|b| *b == 7));
    }
}

#[test]
fn the_cube_has_24_vertices_and_36_indices_that_stay_in_range() {
    let (vertices, indices) = (cube_vertices(), cube_indices());
    assert_eq!(vertices.len(), 24);
    assert_eq!(indices.len(), 36);
    assert_eq!(CUBE_INDEX_COUNT, 36);
    assert!(indices.iter().all(|i| usize::from(*i) < vertices.len()));
}

#[test]
fn the_cube_is_one_unit_wide_around_the_origin() {
    for v in cube_vertices() {
        assert!(v.position.iter().all(|c| is(c.abs(), 0.5)), "{v:?}");
    }
}

#[test]
fn every_face_has_a_unit_axis_normal_shared_by_its_four_vertices() {
    let vertices = cube_vertices();
    for face in vertices.chunks_exact(4) {
        let n = face[0].normal;
        assert!(is(n.iter().map(|c| c.abs()).sum::<f32>(), 1.0), "{n:?}");
        assert!(face.iter().all(|v| same(v.normal, n)));
        // The four corners lie on the plane the normal names, on its outer side.
        assert!(face.iter().all(|v| is(dot(v.position, n), 0.5)));
    }
}

#[test]
fn every_triangle_is_counter_clockwise_seen_from_outside() {
    let (vertices, indices) = (cube_vertices(), cube_indices());
    for tri in indices.chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|k| vertices[usize::from(tri[k])]);
        let facing = cross(sub(b.position, a.position), sub(c.position, a.position));
        assert!(dot(facing, a.normal) > 0.0, "{tri:?} faces inwards");
    }
}

#[test]
fn every_face_uses_the_whole_texture_once() {
    for face in cube_vertices().chunks_exact(4) {
        assert!(
            face.iter()
                .all(|v| v.uv.iter().all(|c| is(*c, 0.0) || is(*c, 1.0)))
        );
        let mut corners: Vec<[bool; 2]> = face.iter().map(|v| v.uv.map(|c| is(c, 1.0))).collect();
        corners.sort_unstable();
        corners.dedup();
        assert_eq!(corners.len(), 4);
    }
}

#[test]
fn the_triangles_of_a_face_only_use_the_vertices_of_that_face() {
    for (face, quad) in cube_indices().chunks_exact(6).enumerate() {
        let low = u16::try_from(face * 4).unwrap_or(u16::MAX);
        assert!(quad.iter().all(|i| (low..low + 4).contains(i)), "{quad:?}");
    }
}
