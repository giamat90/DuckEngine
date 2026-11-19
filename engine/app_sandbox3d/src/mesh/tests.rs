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
