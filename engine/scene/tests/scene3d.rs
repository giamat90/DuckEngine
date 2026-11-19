//! Card 8-10: the 3D transforms and their world update on the shared hierarchy.

use base::math::{Mat4, Quat, Vec3};
use ecs::{Entity, World};
use scene::{
    SceneError, Transform2D, Transform3D, TransformUpdater, TransformUpdater3D, WorldTransform2D,
    WorldTransform3D, children, despawn_subtree, parent_of, set_parent,
};

const EPS: f32 = 1e-5;

fn near(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < EPS
}

fn quarter_turn(axis: Vec3) -> Quat {
    Quat::from_axis_angle(axis, core::f32::consts::FRAC_PI_2).unwrap_or(Quat::IDENTITY)
}

fn spawn(world: &mut World, transform: Transform3D) -> Result<Entity, SceneError> {
    let entity = world.spawn()?;
    world.insert(entity, transform)?;
    Ok(entity)
}

fn world_point(world: &World, entity: Entity, point: Vec3) -> Vec3 {
    world
        .get::<WorldTransform3D>(entity)
        .map_or(Vec3::new(f32::NAN, 0.0, 0.0), |w| w.transform_point(point))
}

#[test]
fn the_identity_transform_is_the_identity_matrix() {
    assert_eq!(Transform3D::IDENTITY.to_mat4(), Mat4::IDENTITY);
    assert_eq!(Transform3D::default(), Transform3D::IDENTITY);
}

#[test]
fn a_transform_scales_then_rotates_then_translates() {
    let transform = Transform3D {
        position: Vec3::new(0.0, 0.0, 5.0),
        rotation: quarter_turn(Vec3::new(0.0, 1.0, 0.0)),
        scale: Vec3::new(2.0, 1.0, 1.0),
    };
    // (1,0,0) -> scaled (2,0,0) -> a quarter turn about +Y takes +X to -Z -> (0,0,-2) -> +5 in z.
    let at = WorldTransform3D {
        matrix: transform.to_mat4(),
    }
    .transform_point(Vec3::new(1.0, 0.0, 0.0));
    assert!(near(at, Vec3::new(0.0, 0.0, 3.0)), "{at:?}");
}

#[test]
fn from_position_only_translates() {
    let t = Transform3D::from_position(Vec3::new(1.0, 2.0, 3.0));
    assert!(near(
        WorldTransform3D {
            matrix: t.to_mat4()
        }
        .transform_point(Vec3::ZERO),
        Vec3::new(1.0, 2.0, 3.0)
    ));
}

#[test]
fn a_child_is_placed_in_its_parents_frame() -> Result<(), SceneError> {
    let mut world = World::new();
    let parent = spawn(
        &mut world,
        Transform3D {
            position: Vec3::new(1.0, 0.0, 0.0),
            rotation: quarter_turn(Vec3::new(0.0, 0.0, 1.0)),
            scale: Vec3::ONE,
        },
    )?;
    let child = spawn(
        &mut world,
        Transform3D::from_position(Vec3::new(1.0, 0.0, 0.0)),
    )?;
    set_parent(&mut world, child, parent)?;
    let report = TransformUpdater3D::new().update(&mut world)?;
    assert_eq!((report.updated, report.unreached), (2, 0));
    assert!(near(
        world_point(&world, child, Vec3::ZERO),
        Vec3::new(1.0, 1.0, 0.0)
    ));
    assert!(near(
        world_point(&world, parent, Vec3::ZERO),
        Vec3::new(1.0, 0.0, 0.0)
    ));
    assert_eq!(parent_of(&world, child), Some(parent));
    assert_eq!(children(&world, parent).count(), 1);
    Ok(())
}

#[test]
fn a_parent_is_updated_before_its_children_in_a_deep_chain() -> Result<(), SceneError> {
    let mut world = World::new();
    let mut last = spawn(
        &mut world,
        Transform3D::from_position(Vec3::new(0.0, 1.0, 0.0)),
    )?;
    let first = last;
    for _ in 0..200 {
        let next = spawn(
            &mut world,
            Transform3D::from_position(Vec3::new(0.0, 1.0, 0.0)),
        )?;
        set_parent(&mut world, next, last)?;
        last = next;
    }
    TransformUpdater3D::new().update(&mut world)?;
    assert!(near(
        world_point(&world, first, Vec3::ZERO),
        Vec3::new(0.0, 1.0, 0.0)
    ));
    assert!(near(
        world_point(&world, last, Vec3::ZERO),
        Vec3::new(0.0, 201.0, 0.0)
    ));
    Ok(())
}

#[test]
fn a_child_without_a_transform_passes_its_parents_matrix_on() -> Result<(), SceneError> {
    let mut world = World::new();
    let root = spawn(
        &mut world,
        Transform3D::from_position(Vec3::new(0.0, 0.0, 2.0)),
    )?;
    let middle = spawn(&mut world, Transform3D::IDENTITY)?;
    let leaf = spawn(
        &mut world,
        Transform3D::from_position(Vec3::new(1.0, 0.0, 0.0)),
    )?;
    set_parent(&mut world, middle, root)?;
    set_parent(&mut world, leaf, middle)?;
    world.remove::<Transform3D>(middle)?;
    let report = TransformUpdater3D::new().update(&mut world)?;
    assert_eq!(report.updated, 2, "the middle entity is not counted");
    assert!(near(
        world_point(&world, leaf, Vec3::ZERO),
        Vec3::new(1.0, 0.0, 2.0)
    ));
    Ok(())
}

#[test]
fn set_parent_refuses_an_entity_with_no_transform_and_a_cycle() -> Result<(), SceneError> {
    let mut world = World::new();
    let a = spawn(&mut world, Transform3D::IDENTITY)?;
    let b = spawn(&mut world, Transform3D::IDENTITY)?;
    let bare = world.spawn()?;
    assert_eq!(
        set_parent(&mut world, bare, a),
        Err(SceneError::MissingTransform)
    );
    assert_eq!(
        set_parent(&mut world, a, bare),
        Err(SceneError::MissingTransform)
    );
    set_parent(&mut world, b, a)?;
    assert_eq!(set_parent(&mut world, a, b), Err(SceneError::Cycle));
    Ok(())
}

#[test]
fn despawn_subtree_removes_a_3d_tree() -> Result<(), SceneError> {
    let mut world = World::new();
    let root = spawn(&mut world, Transform3D::IDENTITY)?;
    for _ in 0..3 {
        let child = spawn(&mut world, Transform3D::IDENTITY)?;
        set_parent(&mut world, child, root)?;
    }
    assert_eq!(despawn_subtree(&mut world, root)?, 4);
    assert_eq!(world.len(), 0);
    Ok(())
}

#[test]
fn a_second_update_after_a_change_follows_the_new_pose() -> Result<(), SceneError> {
    let mut world = World::new();
    let e = spawn(&mut world, Transform3D::IDENTITY)?;
    let mut updater = TransformUpdater3D::new();
    updater.update(&mut world)?;
    if let Some(t) = world.get_mut::<Transform3D>(e) {
        t.position = Vec3::new(4.0, 5.0, 6.0);
    }
    updater.update(&mut world)?;
    assert!(near(
        world_point(&world, e, Vec3::ZERO),
        Vec3::new(4.0, 5.0, 6.0)
    ));
    Ok(())
}

#[test]
fn the_2d_and_the_3d_update_leave_each_others_trees_alone() -> Result<(), SceneError> {
    let mut world = World::new();
    let flat = world.spawn()?;
    world.insert(flat, Transform2D::IDENTITY)?;
    let flat_child = world.spawn()?;
    world.insert(flat_child, Transform2D::IDENTITY)?;
    set_parent(&mut world, flat_child, flat)?;
    let solid = spawn(&mut world, Transform3D::IDENTITY)?;
    let solid_child = spawn(&mut world, Transform3D::IDENTITY)?;
    set_parent(&mut world, solid_child, solid)?;
    let r2 = TransformUpdater::new().update(&mut world)?;
    let r3 = TransformUpdater3D::new().update(&mut world)?;
    assert_eq!((r2.updated, r2.unreached), (2, 0));
    assert_eq!((r3.updated, r3.unreached), (2, 0));
    for e in [solid, solid_child] {
        assert!(
            !world.has::<WorldTransform2D>(e),
            "no 2D world transform on a 3D entity"
        );
    }
    for e in [flat, flat_child] {
        assert!(
            !world.has::<WorldTransform3D>(e),
            "no 3D world transform on a 2D entity"
        );
    }
    Ok(())
}

#[test]
fn every_axis_of_the_scale_stretches_its_own_axis() {
    let transform = Transform3D {
        scale: Vec3::new(2.0, 3.0, 4.0),
        ..Transform3D::IDENTITY
    };
    let world = WorldTransform3D {
        matrix: transform.to_mat4(),
    };
    assert!(near(
        world.transform_point(Vec3::new(1.0, 0.0, 0.0)),
        Vec3::new(2.0, 0.0, 0.0)
    ));
    assert!(near(
        world.transform_point(Vec3::new(0.0, 1.0, 0.0)),
        Vec3::new(0.0, 3.0, 0.0)
    ));
    assert!(near(
        world.transform_point(Vec3::new(0.0, 0.0, 1.0)),
        Vec3::new(0.0, 0.0, 4.0)
    ));
}

#[test]
fn set_parent_refuses_to_join_the_two_dimensions() -> Result<(), SceneError> {
    let mut world = World::new();
    let flat = world.spawn()?;
    world.insert(flat, Transform2D::IDENTITY)?;
    let solid = spawn(&mut world, Transform3D::IDENTITY)?;
    let both = spawn(&mut world, Transform3D::IDENTITY)?;
    world.insert(both, Transform2D::IDENTITY)?;
    assert_eq!(
        set_parent(&mut world, solid, flat),
        Err(SceneError::MixedDimensions)
    );
    assert_eq!(
        set_parent(&mut world, flat, solid),
        Err(SceneError::MixedDimensions)
    );
    assert_eq!(
        set_parent(&mut world, both, solid),
        Err(SceneError::MixedDimensions)
    );
    assert_eq!(
        set_parent(&mut world, solid, both),
        Err(SceneError::MixedDimensions)
    );
    assert!(parent_of(&world, solid).is_none() && parent_of(&world, flat).is_none());
    Ok(())
}
