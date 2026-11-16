//! The world transform update: parents before children, matrices as products along the chain.

use base::math::{Mat3, Vec2};
use ecs::{Entity, World};
use scene::{
    SceneError, Transform2D, TransformUpdater, UpdateReport, WorldTransform2D, set_parent,
};
use std::f32::consts::FRAC_PI_2;

const EPSILON: f32 = 1e-4;

fn node(world: &mut World, local: Transform2D) -> Result<Entity, SceneError> {
    let entity = world.spawn()?;
    world.insert(entity, local)?;
    Ok(entity)
}

fn at(x: f32, y: f32) -> Transform2D {
    Transform2D::from_position(Vec2::new(x, y))
}

fn world_matrix(world: &World, entity: Entity) -> Option<Mat3> {
    world.get::<WorldTransform2D>(entity).map(|w| w.matrix)
}

fn world_point(world: &World, entity: Entity) -> Option<Vec2> {
    world
        .get::<WorldTransform2D>(entity)
        .map(|w| w.transform_point(Vec2::ZERO))
}

fn close(a: Vec2, b: Vec2) -> bool {
    (a.x - b.x).abs() < EPSILON && (a.y - b.y).abs() < EPSILON
}

#[test]
fn a_root_gets_its_local_transform_as_world_transform() -> Result<(), SceneError> {
    let mut world = World::new();
    let local = Transform2D {
        position: Vec2::new(1.0, 2.0),
        rotation: 0.5,
        scale: Vec2::new(2.0, 3.0),
    };
    let root = node(&mut world, local)?;
    let report = TransformUpdater::new().update(&mut world)?;
    assert_eq!(
        report,
        UpdateReport {
            updated: 1,
            unreached: 0
        }
    );
    assert_eq!(world_matrix(&world, root), Some(local.to_mat3()));
    Ok(())
}

#[test]
fn a_child_is_placed_relative_to_its_parent() -> Result<(), SceneError> {
    let mut world = World::new();
    let (parent, child) = (
        node(&mut world, at(10.0, 0.0))?,
        node(&mut world, at(0.0, 5.0))?,
    );
    set_parent(&mut world, child, parent)?;
    TransformUpdater::new().update(&mut world)?;
    assert_eq!(world_point(&world, child), Some(Vec2::new(10.0, 5.0)));
    Ok(())
}

#[test]
fn a_rotated_parent_turns_the_offset_of_its_child() -> Result<(), SceneError> {
    let mut world = World::new();
    let turned = Transform2D {
        rotation: FRAC_PI_2,
        ..Transform2D::IDENTITY
    };
    let (parent, child) = (node(&mut world, turned)?, node(&mut world, at(1.0, 0.0))?);
    set_parent(&mut world, child, parent)?;
    TransformUpdater::new().update(&mut world)?;
    let placed = world_point(&world, child).unwrap_or(Vec2::splat(f32::NAN));
    assert!(close(placed, Vec2::new(0.0, 1.0)));
    Ok(())
}

#[test]
fn a_chain_of_three_is_the_product_of_the_local_matrices() -> Result<(), SceneError> {
    let mut world = World::new();
    let locals = [
        Transform2D {
            position: Vec2::new(1.0, 2.0),
            rotation: 0.3,
            scale: Vec2::new(2.0, 1.0),
        },
        Transform2D {
            position: Vec2::new(-3.0, 0.5),
            rotation: -0.7,
            scale: Vec2::new(1.0, 0.5),
        },
        Transform2D {
            position: Vec2::new(0.25, 4.0),
            rotation: 1.1,
            scale: Vec2::new(3.0, 3.0),
        },
    ];
    let a = node(&mut world, locals[0])?;
    let b = node(&mut world, locals[1])?;
    let c = node(&mut world, locals[2])?;
    set_parent(&mut world, b, a)?;
    set_parent(&mut world, c, b)?;
    TransformUpdater::new().update(&mut world)?;
    let expected = locals[0].to_mat3() * locals[1].to_mat3() * locals[2].to_mat3();
    assert_eq!(world_matrix(&world, c), Some(expected));
    assert_eq!(
        world_matrix(&world, b),
        Some(locals[0].to_mat3() * locals[1].to_mat3())
    );
    Ok(())
}

#[test]
fn moving_the_parent_moves_the_children_on_the_next_update() -> Result<(), SceneError> {
    let mut world = World::new();
    let (parent, child) = (
        node(&mut world, at(1.0, 1.0))?,
        node(&mut world, at(2.0, 0.0))?,
    );
    set_parent(&mut world, child, parent)?;
    let mut updater = TransformUpdater::new();
    updater.update(&mut world)?;
    assert_eq!(world_point(&world, child), Some(Vec2::new(3.0, 1.0)));
    if let Some(local) = world.get_mut::<Transform2D>(parent) {
        local.position = Vec2::new(5.0, 5.0);
    }
    updater.update(&mut world)?;
    assert_eq!(world_point(&world, child), Some(Vec2::new(7.0, 5.0)));
    Ok(())
}

#[test]
fn every_branch_of_a_tree_is_updated_once() -> Result<(), SceneError> {
    let mut world = World::new();
    let root = node(&mut world, at(1.0, 0.0))?;
    let (a, b) = (
        node(&mut world, at(0.0, 1.0))?,
        node(&mut world, at(0.0, 2.0))?,
    );
    let (a1, b1, b2) = (
        node(&mut world, at(1.0, 0.0))?,
        node(&mut world, at(2.0, 0.0))?,
        node(&mut world, at(3.0, 0.0))?,
    );
    set_parent(&mut world, a, root)?;
    set_parent(&mut world, b, root)?;
    set_parent(&mut world, a1, a)?;
    set_parent(&mut world, b1, b)?;
    set_parent(&mut world, b2, b)?;
    let report = TransformUpdater::new().update(&mut world)?;
    assert_eq!(report.updated, 6);
    let expected = [
        (root, Vec2::new(1.0, 0.0)),
        (a, Vec2::new(1.0, 1.0)),
        (b, Vec2::new(1.0, 2.0)),
        (a1, Vec2::new(2.0, 1.0)),
        (b1, Vec2::new(3.0, 2.0)),
        (b2, Vec2::new(4.0, 2.0)),
    ];
    for (entity, point) in expected {
        assert_eq!(world_point(&world, entity), Some(point));
    }
    Ok(())
}

#[test]
fn a_detached_child_stops_following_its_old_parent() -> Result<(), SceneError> {
    let mut world = World::new();
    let (parent, child) = (
        node(&mut world, at(10.0, 0.0))?,
        node(&mut world, at(1.0, 0.0))?,
    );
    set_parent(&mut world, child, parent)?;
    scene::detach(&mut world, child)?;
    TransformUpdater::new().update(&mut world)?;
    assert_eq!(world_point(&world, child), Some(Vec2::new(1.0, 0.0)));
    Ok(())
}

#[test]
fn entities_without_a_transform_are_left_alone() -> Result<(), SceneError> {
    let mut world = World::new();
    let bare = world.spawn()?;
    let report = TransformUpdater::new().update(&mut world)?;
    assert_eq!(report.updated, 0);
    assert!(!world.has::<WorldTransform2D>(bare));
    Ok(())
}

#[test]
fn a_child_whose_parent_was_despawned_outside_the_scene_is_a_root() -> Result<(), SceneError> {
    let mut world = World::new();
    let (parent, child) = (
        node(&mut world, at(10.0, 0.0))?,
        node(&mut world, at(1.0, 0.0))?,
    );
    set_parent(&mut world, child, parent)?;
    world.despawn(parent)?;
    TransformUpdater::new().update(&mut world)?;
    assert_eq!(world_point(&world, child), Some(Vec2::new(1.0, 0.0)));
    Ok(())
}

#[test]
fn a_chain_of_two_thousand_needs_no_recursion() -> Result<(), SceneError> {
    const DEPTH: u16 = 2000;
    let mut world = World::new();
    let mut last = node(&mut world, at(1.0, 0.0))?;
    for _ in 1..DEPTH {
        let next = node(&mut world, at(1.0, 0.0))?;
        set_parent(&mut world, next, last)?;
        last = next;
    }
    let report = TransformUpdater::new().update(&mut world)?;
    assert_eq!(report.updated, usize::from(DEPTH));
    assert_eq!(
        world_point(&world, last),
        Some(Vec2::new(f32::from(DEPTH), 0.0))
    );
    Ok(())
}

#[test]
fn the_same_scene_built_twice_gives_identical_matrices() -> Result<(), SceneError> {
    fn build() -> Result<Vec<[u32; 9]>, SceneError> {
        let mut world = World::new();
        let root = node(&mut world, at(1.5, -2.0))?;
        let mut all = vec![root];
        for i in 0..20_u8 {
            let local = Transform2D {
                position: Vec2::new(f32::from(i), 0.5),
                rotation: f32::from(i) * 0.1,
                scale: Vec2::new(1.0, 1.0 + f32::from(i) * 0.05),
            };
            let child = node(&mut world, local)?;
            let parent = all[usize::from(i) / 2];
            set_parent(&mut world, child, parent)?;
            all.push(child);
        }
        TransformUpdater::new().update(&mut world)?;
        Ok(all
            .iter()
            .filter_map(|e| world_matrix(&world, *e))
            .map(|m| m.to_cols_array().map(f32::to_bits))
            .collect())
    }
    assert_eq!(build()?, build()?);
    Ok(())
}

#[test]
fn a_parent_spawned_after_its_child_is_still_updated_first() -> Result<(), SceneError> {
    let mut world = World::new();
    let child = node(&mut world, at(0.0, 1.0))?;
    let parent = node(&mut world, at(5.0, 0.0))?;
    set_parent(&mut world, child, parent)?;
    TransformUpdater::new().update(&mut world)?;
    assert_eq!(world_point(&world, child), Some(Vec2::new(5.0, 1.0)));
    Ok(())
}

#[test]
fn re_parenting_overwrites_the_old_world_transform() -> Result<(), SceneError> {
    let mut world = World::new();
    let (near, far) = (
        node(&mut world, at(1.0, 0.0))?,
        node(&mut world, at(100.0, 0.0))?,
    );
    let child = node(&mut world, at(0.0, 2.0))?;
    set_parent(&mut world, child, near)?;
    let mut updater = TransformUpdater::new();
    updater.update(&mut world)?;
    assert_eq!(world_point(&world, child), Some(Vec2::new(1.0, 2.0)));
    set_parent(&mut world, child, far)?;
    updater.update(&mut world)?;
    assert_eq!(world_point(&world, child), Some(Vec2::new(100.0, 2.0)));
    Ok(())
}

#[test]
fn a_loop_made_by_removing_a_link_by_hand_ends_the_update_with_an_error() -> Result<(), SceneError>
{
    let mut world = World::new();
    let (parent, child) = (
        node(&mut world, at(1.0, 0.0))?,
        node(&mut world, at(1.0, 0.0))?,
    );
    set_parent(&mut world, child, parent)?;
    world.remove::<scene::Parent>(child)?;
    set_parent(&mut world, parent, child)?;
    assert_eq!(
        TransformUpdater::new().update(&mut world),
        Err(SceneError::BrokenHierarchy)
    );
    Ok(())
}

#[test]
fn a_sibling_cut_off_by_an_outside_despawn_is_reported_as_unreached() -> Result<(), SceneError> {
    let mut world = World::new();
    let parent = node(&mut world, at(10.0, 0.0))?;
    let (first, second) = (
        node(&mut world, at(1.0, 0.0))?,
        node(&mut world, at(2.0, 0.0))?,
    );
    set_parent(&mut world, first, parent)?;
    set_parent(&mut world, second, parent)?;
    world.despawn(first)?;
    let report = TransformUpdater::new().update(&mut world)?;
    assert_eq!(
        report,
        UpdateReport {
            updated: 1,
            unreached: 1
        }
    );
    assert!(!world.has::<WorldTransform2D>(second));
    Ok(())
}

#[test]
fn a_child_without_a_transform_passes_its_parent_matrix_on_and_is_not_counted()
-> Result<(), SceneError> {
    let mut world = World::new();
    let (parent, middle, leaf) = (
        node(&mut world, at(10.0, 0.0))?,
        node(&mut world, at(0.0, 3.0))?,
        node(&mut world, at(1.0, 0.0))?,
    );
    set_parent(&mut world, middle, parent)?;
    set_parent(&mut world, leaf, middle)?;
    world.remove::<Transform2D>(middle)?;
    let report = TransformUpdater::new().update(&mut world)?;
    assert_eq!(
        report,
        UpdateReport {
            updated: 2,
            unreached: 0
        }
    );
    assert_eq!(world_point(&world, middle), Some(Vec2::new(10.0, 0.0)));
    assert_eq!(world_point(&world, leaf), Some(Vec2::new(11.0, 0.0)));
    Ok(())
}

#[test]
fn a_root_without_a_transform_acts_as_the_identity_for_its_children() -> Result<(), SceneError> {
    let mut world = World::new();
    let (root, child) = (
        node(&mut world, at(10.0, 0.0))?,
        node(&mut world, at(1.0, 2.0))?,
    );
    set_parent(&mut world, child, root)?;
    world.remove::<Transform2D>(root)?;
    let report = TransformUpdater::new().update(&mut world)?;
    assert_eq!(
        report,
        UpdateReport {
            updated: 1,
            unreached: 0
        }
    );
    assert_eq!(world_point(&world, child), Some(Vec2::new(1.0, 2.0)));
    Ok(())
}

#[test]
fn a_sibling_that_points_at_itself_ends_the_update_with_an_error() -> Result<(), SceneError> {
    let mut world = World::new();
    let (parent, first) = (
        node(&mut world, at(0.0, 0.0))?,
        node(&mut world, at(1.0, 0.0))?,
    );
    set_parent(&mut world, first, parent)?;
    world.remove::<scene::Parent>(first)?;
    set_parent(&mut world, first, parent)?;
    assert_eq!(
        TransformUpdater::new().update(&mut world),
        Err(SceneError::BrokenHierarchy)
    );
    Ok(())
}
