use super::*;
use render::mesh_scene::MeshRef;

fn extent(mesh: &MeshData, axis: usize) -> f32 {
    mesh.vertices
        .iter()
        .map(|v| v.position[axis].abs())
        .fold(0.0, f32::max)
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn there_is_one_mesh_per_piece_in_the_order_of_the_refs() {
    let meshes = unit_meshes();
    assert!(meshes.is_some());
    let meshes = meshes.unwrap_or_else(|| unreachable!());
    assert_eq!(meshes.len(), 3);
    assert_eq!(MeshRef(SPHERE), MeshRef(0));
    assert_eq!((SPHERE, BOX, CAPSULE), (0, 1, 2));
}

#[test]
fn every_mesh_is_whole_triangles_over_its_own_vertices() {
    for mesh in unit_meshes().unwrap_or_default() {
        assert!(!mesh.vertices.is_empty());
        assert_eq!(mesh.indices.len() % 3, 0);
        assert!(
            mesh.indices
                .iter()
                .all(|i| usize::from(*i) < mesh.vertices.len())
        );
    }
}

#[test]
fn the_sphere_has_radius_one() {
    let meshes = unit_meshes().unwrap_or_default();
    let sphere = &meshes[usize::try_from(SPHERE).unwrap_or(0)];
    for axis in 0..3 {
        assert!(near(extent(sphere, axis), 1.0), "axis {axis}");
    }
}

#[test]
fn the_box_has_half_extents_of_one() {
    let meshes = unit_meshes().unwrap_or_default();
    let cuboid = &meshes[usize::try_from(BOX).unwrap_or(0)];
    for axis in 0..3 {
        assert!(near(extent(cuboid, axis), 1.0), "axis {axis}");
    }
}

#[test]
fn the_capsule_matches_the_constants_the_bodies_scale_from() {
    let meshes = unit_meshes().unwrap_or_default();
    let capsule = &meshes[usize::try_from(CAPSULE).unwrap_or(0)];
    assert!(near(extent(capsule, 0), CAPSULE_RADIUS));
    assert!(near(extent(capsule, 2), CAPSULE_RADIUS));
    assert!(near(
        extent(capsule, 1),
        CAPSULE_RADIUS + CAPSULE_HALF_HEIGHT
    ));
}

#[test]
fn the_vertex_bytes_fill_the_stride() {
    for mesh in unit_meshes().unwrap_or_default() {
        let bytes = mesh.vertex_bytes();
        assert_eq!(bytes.map(|b| b.len()), Some(mesh.vertices.len() * 32));
    }
}

#[test]
fn the_index_bytes_are_two_per_index() {
    for mesh in unit_meshes().unwrap_or_default() {
        assert_eq!(mesh.index_bytes().len(), mesh.indices.len() * 2);
    }
}

/// The world-space direction (and length) one unit of `u` and one of `v` stand for on a triangle
/// of `mesh` drawn with half extents `half`, or `None` for a triangle without a texture area.
fn texture_axes(mesh: &MeshData, triangle: &[u16], half: Vec3) -> Option<([f32; 3], [f32; 3])> {
    let vertex = |i: u16| &mesh.vertices[usize::from(i)];
    let world = |i: u16| {
        let p = vertex(i).position;
        [p[0] * half.x, p[1] * half.y, p[2] * half.z]
    };
    let (a, b, c) = (world(triangle[0]), world(triangle[1]), world(triangle[2]));
    let (ua, ub, uc) = (
        vertex(triangle[0]).uv,
        vertex(triangle[1]).uv,
        vertex(triangle[2]).uv,
    );
    let (e1, e2) = (
        [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
        [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
    );
    let (d1, d2) = (
        [ub[0] - ua[0], ub[1] - ua[1]],
        [uc[0] - ua[0], uc[1] - ua[1]],
    );
    let det = d1[0] * d2[1] - d2[0] * d1[1];
    if det.abs() < 1e-9 {
        return None;
    }
    let along_u = std::array::from_fn(|k| (e1[k] * d2[1] - e2[k] * d1[1]) / det);
    let along_v = std::array::from_fn(|k| (e2[k] * d1[0] - e1[k] * d2[0]) / det);
    Some((along_u, along_v))
}

fn length(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

#[test]
fn every_scenery_box_has_its_own_mesh_after_the_three_unit_meshes() {
    let meshes = scenery_meshes();
    assert_eq!(meshes.len(), crate::spawn::scenery().len());
    // The three unit meshes come first: the scenery's mesh numbers start after them.
    assert_eq!(
        unit_meshes().map(|m| m.len()),
        Some(usize::try_from(SCENERY_FIRST).unwrap_or(0))
    );
    for mesh in meshes {
        assert_eq!(mesh.vertices.len(), render::mesh::CUBE_VERTEX_COUNT);
        assert_eq!(mesh.indices.len() % 3, 0);
        for axis in 0..3 {
            assert!(
                near(extent(&mesh, axis), 1.0),
                "a scenery mesh is a unit box: axis {axis}"
            );
        }
    }
}

#[test]
fn the_texture_covers_the_scenery_at_one_size_in_metres_in_both_directions() {
    // Card 8-31: the one unit box gave every face the whole texture however big the face was, so
    // a thin rim of a long wall got the same texels across a hand's width as along twelve metres.
    // Now a texture tile is TILE_METRES wide and high on every face of every block.
    let blocks = crate::spawn::scenery();
    let meshes = scenery_meshes();
    assert_eq!(meshes.len(), blocks.len());
    let axes: Vec<Option<([f32; 3], [f32; 3])>> = blocks
        .iter()
        .zip(&meshes)
        .flat_map(|(block, mesh)| {
            mesh.indices
                .chunks(3)
                .map(|triangle| texture_axes(mesh, triangle, block.half))
        })
        .collect();
    assert!(!axes.is_empty());
    for found in axes {
        assert!(
            found.is_some(),
            "a triangle without texture area in a scenery mesh"
        );
        let (along_u, along_v) = found.unwrap_or_default();
        assert!(near(length(along_u), TILE_METRES), "u: {}", length(along_u));
        assert!(near(length(along_v), TILE_METRES), "v: {}", length(along_v));
        let dot = along_u[0] * along_v[0] + along_u[1] * along_v[1] + along_u[2] * along_v[2];
        assert!(dot.abs() < 1e-2, "the directions are not square: {dot}");
    }
}

#[test]
fn the_unit_box_stretches_the_texture_on_a_thin_wall_which_is_why_the_scenery_has_its_own() {
    let [_, cuboid, _] = unit_meshes().unwrap_or_default();
    let wall = Vec3::new(0.5, 1.5, 12.0);
    let lengths: Vec<f32> = cuboid
        .indices
        .chunks(3)
        .filter_map(|t| texture_axes(&cuboid, t, wall))
        .flat_map(|(u, v)| [length(u), length(v)])
        .collect();
    let (low, high) = lengths
        .iter()
        .fold((f32::MAX, 0.0_f32), |(l, h), x| (l.min(*x), h.max(*x)));
    assert!(
        high > 10.0 * low,
        "the unit box does not stretch: {low} to {high}"
    );
}
