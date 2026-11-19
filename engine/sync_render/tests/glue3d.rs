//! Card 8-10: the mesh extraction. Entities with a `MeshRenderer` and a world transform become
//! the instances of a `MeshScene`.

use base::math::{Mat4, Quat, Vec3};
use base::pool::{Handle, Pool};
use ecs::{Entity, World};
use render::camera::PerspectiveCamera;
use render::ids::{Texture, TextureId};
use render::lit::{DirectionalLight, Material, transform_block};
use render::mesh_scene::{MeshRef, MeshScene};
use scene::{Transform3D, TransformUpdater3D, WorldTransform3D, set_parent};
use sync_render::ExtractReport;
use sync_render::SyncError;
use sync_render::d3::{MeshRenderer, extract_meshes};

fn texture() -> TextureId {
    let mut pool = Pool::<Texture>::with_capacity(1);
    pool.insert(Texture)
        .unwrap_or_else(|_| Handle::from_raw(0, 0))
}

fn camera() -> PerspectiveCamera {
    PerspectiveCamera {
        position: Vec3::new(0.0, 2.0, 8.0),
        target: Vec3::ZERO,
        up: Vec3::new(0.0, 1.0, 0.0),
        fov_y: 1.0,
        aspect: 1.5,
        near: 0.1,
        far: 100.0,
    }
}

fn light() -> DirectionalLight {
    DirectionalLight {
        direction: Vec3::new(-1.0, -1.0, 0.0),
        colour: [1.0; 3],
        ambient: [0.2; 3],
    }
}

fn mesh_scene(capacity: usize) -> MeshScene {
    MeshScene::new(capacity, camera(), light()).unwrap_or_else(|_| unreachable!())
}

fn renderer(mesh: u32) -> MeshRenderer {
    MeshRenderer::new(MeshRef(mesh), texture())
}

fn drawable(
    world: &mut World,
    local: Transform3D,
    renderer: MeshRenderer,
) -> Result<Entity, SyncError> {
    let entity = world.spawn()?;
    world.insert(entity, local)?;
    world.insert(entity, renderer)?;
    Ok(entity)
}

fn at(x: f32, y: f32, z: f32) -> Transform3D {
    Transform3D::from_position(Vec3::new(x, y, z))
}

fn refresh(world: &mut World) {
    assert!(TransformUpdater3D::new().update(world).is_ok());
}

#[test]
fn a_new_renderer_takes_the_defaults() {
    let t = texture();
    let r = MeshRenderer::new(MeshRef(7), t);
    assert_eq!((r.mesh, r.texture), (MeshRef(7), t));
    assert_eq!(r.material, Material { colour: [1.0; 4] });
}

#[test]
fn an_entity_with_a_world_transform_and_a_renderer_becomes_one_instance() -> Result<(), SyncError> {
    let mut world = World::new();
    let look = MeshRenderer {
        material: Material {
            colour: [0.1, 0.2, 0.3, 1.0],
        },
        ..renderer(5)
    };
    let local = Transform3D {
        position: Vec3::new(1.0, 2.0, 3.0),
        rotation: Quat::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), 0.7).unwrap_or(Quat::IDENTITY),
        scale: Vec3::new(1.0, 2.0, 3.0),
    };
    drawable(&mut world, local, look)?;
    refresh(&mut world);
    let mut scene = mesh_scene(4);
    let report = extract_meshes(&mut world, &mut scene)?;
    assert_eq!(
        report,
        ExtractReport {
            extracted: 1,
            skipped: 0,
            dropped: 0
        }
    );
    let got = scene.instances()[0];
    assert_eq!(got.mesh, MeshRef(5));
    assert_eq!(got.material, look.material);
    assert_eq!(got.texture, look.texture);
    assert_eq!(got.model, local.to_mat4());
    Ok(())
}

#[test]
fn a_child_is_drawn_where_its_parent_puts_it() -> Result<(), SyncError> {
    let mut world = World::new();
    let parent = drawable(&mut world, at(10.0, 0.0, 0.0), renderer(0))?;
    let child = drawable(&mut world, at(0.0, 1.0, 0.0), renderer(1))?;
    set_parent(&mut world, child, parent)
        .map_err(|_| SyncError::Ecs(ecs::EcsError::StaleEntity))?;
    refresh(&mut world);
    let mut scene = mesh_scene(4);
    extract_meshes(&mut world, &mut scene)?;
    let placed = scene
        .instances()
        .iter()
        .find(|i| i.mesh == MeshRef(1))
        .map(|i| i.model.transform_point3(Vec3::ZERO));
    assert_eq!(placed, Some(Vec3::new(10.0, 1.0, 0.0)));
    Ok(())
}

#[test]
fn an_entity_without_a_renderer_or_without_a_world_transform_is_not_drawn() -> Result<(), SyncError>
{
    let mut world = World::new();
    let bare = world.spawn()?;
    world.insert(bare, at(0.0, 0.0, 0.0))?;
    drawable(&mut world, at(1.0, 0.0, 0.0), renderer(0))?;
    refresh(&mut world);
    let stale = world.spawn()?;
    world.insert(stale, renderer(1))?;
    let mut scene = mesh_scene(4);
    let report = extract_meshes(&mut world, &mut scene)?;
    assert_eq!(report.extracted, 1);
    assert!(!world.has::<WorldTransform3D>(stale));
    Ok(())
}

#[test]
fn every_extraction_replaces_the_instances_and_leaves_the_camera_and_the_light()
-> Result<(), SyncError> {
    let mut world = World::new();
    drawable(&mut world, at(0.0, 0.0, 0.0), renderer(0))?;
    refresh(&mut world);
    let mut scene = mesh_scene(4);
    extract_meshes(&mut world, &mut scene)?;
    extract_meshes(&mut world, &mut scene)?;
    assert_eq!(scene.instances().len(), 1);
    assert_eq!(*scene.camera(), camera());
    assert_eq!(*scene.light(), light());
    Ok(())
}

#[test]
fn instances_come_out_in_the_storage_order_of_the_ecs() -> Result<(), SyncError> {
    let mut world = World::new();
    for mesh in [4, 2, 9] {
        drawable(&mut world, at(0.0, 0.0, 0.0), renderer(mesh))?;
    }
    refresh(&mut world);
    let mut scene = mesh_scene(4);
    extract_meshes(&mut world, &mut scene)?;
    let order: Vec<u32> = scene.instances().iter().map(|i| i.mesh.0).collect();
    assert_eq!(order, [4, 2, 9]);
    Ok(())
}

#[test]
fn values_that_cannot_be_drawn_are_skipped_and_a_full_scene_drops_the_rest() -> Result<(), SyncError>
{
    let mut world = World::new();
    drawable(&mut world, at(f32::NAN, 0.0, 0.0), renderer(0))?;
    drawable(
        &mut world,
        Transform3D {
            scale: Vec3::new(0.0, 1.0, 1.0),
            ..Transform3D::IDENTITY
        },
        renderer(1),
    )?;
    let dark = MeshRenderer {
        material: Material {
            colour: [f32::NAN, 0.0, 0.0, 1.0],
        },
        ..renderer(2)
    };
    drawable(&mut world, at(0.0, 0.0, 0.0), dark)?;
    for mesh in 3..6 {
        drawable(&mut world, at(0.0, 0.0, 0.0), renderer(mesh))?;
    }
    refresh(&mut world);
    let mut scene = mesh_scene(2);
    let report = extract_meshes(&mut world, &mut scene)?;
    assert_eq!(
        report,
        ExtractReport {
            extracted: 2,
            skipped: 3,
            dropped: 1
        }
    );
    Ok(())
}

#[test]
fn every_extracted_instance_has_a_transform_block() -> Result<(), SyncError> {
    let mut world = World::new();
    for n in 0..5_u8 {
        drawable(&mut world, at(f32::from(n), 0.0, 0.0), renderer(0))?;
    }
    refresh(&mut world);
    let mut scene = mesh_scene(8);
    extract_meshes(&mut world, &mut scene)?;
    let vp = scene.view_projection().unwrap_or(Mat4::IDENTITY);
    assert!(
        scene
            .instances()
            .iter()
            .all(|i| transform_block(&(vp * i.model), &i.model).is_some())
    );
    Ok(())
}

#[test]
fn a_model_that_overflows_once_the_camera_is_applied_is_skipped() -> Result<(), SyncError> {
    let mut world = World::new();
    drawable(&mut world, at(3.0e38, 3.0e38, 3.0e38), renderer(0))?;
    drawable(&mut world, at(1.0, 0.0, 0.0), renderer(1))?;
    refresh(&mut world);
    let mut scene = mesh_scene(4);
    let report = extract_meshes(&mut world, &mut scene)?;
    assert_eq!(
        report,
        ExtractReport {
            extracted: 1,
            skipped: 1,
            dropped: 0
        }
    );
    let vp = scene.view_projection().unwrap_or(Mat4::IDENTITY);
    assert!(
        scene
            .instances()
            .iter()
            .all(|i| transform_block(&(vp * i.model), &i.model).is_some())
    );
    Ok(())
}
