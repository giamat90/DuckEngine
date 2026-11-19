use super::*;
use base::math::{Mat4, Vec3};
use base::pool::Handle;

fn camera() -> PerspectiveCamera {
    PerspectiveCamera {
        position: Vec3::new(0.0, 0.0, 5.0),
        target: Vec3::ZERO,
        up: Vec3::new(0.0, 1.0, 0.0),
        fov_y: 1.0,
        aspect: 1.0,
        near: 0.1,
        far: 50.0,
    }
}

fn light() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::new(0.0, -1.0, 0.0),
        colour: [1.0; 3],
        ambient: [0.1; 3],
    }
}

fn instance(mesh: u32) -> MeshInstance {
    MeshInstance {
        mesh: MeshRef(mesh),
        model: Mat4::IDENTITY,
        material: Material { colour: [1.0; 4] },
        texture: Handle::from_raw(0, 0),
    }
}

fn scene(capacity: usize) -> MeshScene {
    MeshScene::new(capacity, camera(), light()).unwrap_or_else(|_| unreachable!())
}

#[test]
fn a_new_scene_is_empty_with_the_camera_and_the_light_it_was_given() {
    let scene = scene(4);
    assert!(scene.instances().is_empty());
    assert_eq!(scene.capacity(), 4);
    assert_eq!(*scene.camera(), camera());
    assert_eq!(*scene.light(), light());
}

#[test]
fn a_scene_cannot_be_larger_than_the_limit() {
    assert!(MeshScene::new(MAX_MESH_INSTANCES, camera(), light()).is_ok());
    assert_eq!(
        MeshScene::new(MAX_MESH_INSTANCES + 1, camera(), light()).err(),
        Some(MeshSceneError::TooManyInstances)
    );
}

#[test]
fn instances_keep_the_order_they_were_pushed_in() {
    let mut scene = scene(4);
    for mesh in [3, 1, 2] {
        assert!(scene.push(instance(mesh)).is_ok());
    }
    let order: Vec<u32> = scene.instances().iter().map(|i| i.mesh.0).collect();
    assert_eq!(order, [3, 1, 2]);
}

#[test]
fn a_full_scene_refuses_an_instance_and_is_unchanged() {
    let mut scene = scene(1);
    assert!(scene.push(instance(0)).is_ok());
    assert_eq!(
        scene.push(instance(1)),
        Err(MeshSceneError::TooManyInstances)
    );
    assert_eq!(scene.instances().len(), 1);
}

#[test]
fn clearing_removes_the_instances_and_keeps_the_capacity_the_camera_and_the_light() {
    let mut scene = scene(2);
    assert!(scene.push(instance(0)).is_ok());
    scene.clear();
    assert!(scene.instances().is_empty());
    assert_eq!(scene.capacity(), 2);
    assert_eq!(*scene.camera(), camera());
    assert_eq!(*scene.light(), light());
}

#[test]
fn the_camera_and_the_light_can_be_replaced() {
    let mut scene = scene(1);
    let moved = PerspectiveCamera {
        position: Vec3::new(1.0, 2.0, 3.0),
        ..camera()
    };
    let dimmer = DirectionalLight {
        colour: [0.5; 3],
        ..light()
    };
    scene.set_camera(moved);
    scene.set_light(dimmer);
    assert_eq!(*scene.camera(), moved);
    assert_eq!(*scene.light(), dimmer);
}

#[test]
fn the_view_projection_is_the_cameras() {
    assert_eq!(scene(1).view_projection(), camera().view_projection());
}
