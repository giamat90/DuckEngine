//! The hierarchy: linking, order, cycles, re-parenting, detaching and subtree despawn.

use ecs::{EcsError, Entity, World};
use scene::{SceneError, Transform2D, children, despawn_subtree, detach, parent_of, set_parent};

fn node(world: &mut World) -> Result<Entity, SceneError> {
    let entity = world.spawn()?;
    world.insert(entity, Transform2D::IDENTITY)?;
    Ok(entity)
}

fn kids(world: &World, parent: Entity) -> Vec<Entity> {
    children(world, parent).collect()
}

#[test]
fn a_new_entity_has_no_parent_and_no_children() -> Result<(), SceneError> {
    let mut world = World::new();
    let a = node(&mut world)?;
    assert_eq!(parent_of(&world, a), None);
    assert!(kids(&world, a).is_empty());
    Ok(())
}

#[test]
fn set_parent_links_both_directions() -> Result<(), SceneError> {
    let mut world = World::new();
    let (parent, child) = (node(&mut world)?, node(&mut world)?);
    set_parent(&mut world, child, parent)?;
    assert_eq!(parent_of(&world, child), Some(parent));
    assert_eq!(kids(&world, parent), vec![child]);
    Ok(())
}

#[test]
fn children_keep_the_order_they_were_added_in() -> Result<(), SceneError> {
    let mut world = World::new();
    let parent = node(&mut world)?;
    let added = [node(&mut world)?, node(&mut world)?, node(&mut world)?];
    for child in added {
        set_parent(&mut world, child, parent)?;
    }
    assert_eq!(kids(&world, parent), added.to_vec());
    Ok(())
}

#[test]
fn an_entity_cannot_be_its_own_parent() -> Result<(), SceneError> {
    let mut world = World::new();
    let a = node(&mut world)?;
    assert_eq!(set_parent(&mut world, a, a), Err(SceneError::Cycle));
    assert_eq!(parent_of(&world, a), None);
    Ok(())
}

#[test]
fn a_descendant_cannot_become_the_parent_of_its_ancestor() -> Result<(), SceneError> {
    let mut world = World::new();
    let (a, b, c) = (node(&mut world)?, node(&mut world)?, node(&mut world)?);
    set_parent(&mut world, b, a)?;
    set_parent(&mut world, c, b)?;
    assert_eq!(set_parent(&mut world, a, c), Err(SceneError::Cycle));
    assert_eq!(parent_of(&world, a), None);
    assert_eq!(kids(&world, c), Vec::<Entity>::new());
    Ok(())
}

#[test]
fn a_despawned_entity_is_refused_and_nothing_changes() -> Result<(), SceneError> {
    let mut world = World::new();
    let (parent, child) = (node(&mut world)?, node(&mut world)?);
    world.despawn(child)?;
    assert_eq!(
        set_parent(&mut world, child, parent),
        Err(SceneError::Ecs(EcsError::StaleEntity))
    );
    assert_eq!(
        set_parent(&mut world, parent, child),
        Err(SceneError::Ecs(EcsError::StaleEntity))
    );
    assert!(kids(&world, parent).is_empty());
    Ok(())
}

#[test]
fn an_entity_without_a_transform_is_refused() -> Result<(), SceneError> {
    let mut world = World::new();
    let parent = node(&mut world)?;
    let bare = world.spawn()?;
    assert_eq!(
        set_parent(&mut world, bare, parent),
        Err(SceneError::MissingTransform)
    );
    assert_eq!(
        set_parent(&mut world, parent, bare),
        Err(SceneError::MissingTransform)
    );
    Ok(())
}

#[test]
fn re_parenting_moves_the_child_and_keeps_the_other_siblings_linked() -> Result<(), SceneError> {
    let mut world = World::new();
    let (old, new) = (node(&mut world)?, node(&mut world)?);
    let (a, b, c) = (node(&mut world)?, node(&mut world)?, node(&mut world)?);
    for child in [a, b, c] {
        set_parent(&mut world, child, old)?;
    }
    set_parent(&mut world, b, new)?;
    assert_eq!(kids(&world, old), vec![a, c]);
    assert_eq!(kids(&world, new), vec![b]);
    assert_eq!(parent_of(&world, b), Some(new));
    Ok(())
}

#[test]
fn re_parenting_the_first_and_the_last_child_works() -> Result<(), SceneError> {
    let mut world = World::new();
    let (old, new) = (node(&mut world)?, node(&mut world)?);
    let (a, b, c) = (node(&mut world)?, node(&mut world)?, node(&mut world)?);
    for child in [a, b, c] {
        set_parent(&mut world, child, old)?;
    }
    set_parent(&mut world, a, new)?;
    set_parent(&mut world, c, new)?;
    assert_eq!(kids(&world, old), vec![b]);
    assert_eq!(kids(&world, new), vec![a, c]);
    Ok(())
}

#[test]
fn setting_the_same_parent_again_moves_the_child_to_the_end() -> Result<(), SceneError> {
    let mut world = World::new();
    let p = node(&mut world)?;
    let (a, b) = (node(&mut world)?, node(&mut world)?);
    set_parent(&mut world, a, p)?;
    set_parent(&mut world, b, p)?;
    set_parent(&mut world, a, p)?;
    assert_eq!(kids(&world, p), vec![b, a]);
    Ok(())
}

#[test]
fn detach_makes_a_root_and_keeps_the_siblings() -> Result<(), SceneError> {
    let mut world = World::new();
    let p = node(&mut world)?;
    let (a, b, c) = (node(&mut world)?, node(&mut world)?, node(&mut world)?);
    for child in [a, b, c] {
        set_parent(&mut world, child, p)?;
    }
    detach(&mut world, b)?;
    assert_eq!(parent_of(&world, b), None);
    assert_eq!(kids(&world, p), vec![a, c]);
    detach(&mut world, b)?;
    let gone = node(&mut world)?;
    world.despawn(gone)?;
    assert_eq!(
        detach(&mut world, gone),
        Err(SceneError::Ecs(EcsError::StaleEntity))
    );
    Ok(())
}

#[test]
fn despawn_subtree_removes_the_node_its_descendants_and_the_link() -> Result<(), SceneError> {
    let mut world = World::new();
    let root = node(&mut world)?;
    let (a, b) = (node(&mut world)?, node(&mut world)?);
    let (a1, a2, b1) = (node(&mut world)?, node(&mut world)?, node(&mut world)?);
    set_parent(&mut world, a, root)?;
    set_parent(&mut world, b, root)?;
    set_parent(&mut world, a1, a)?;
    set_parent(&mut world, a2, a)?;
    set_parent(&mut world, b1, b)?;
    assert_eq!(despawn_subtree(&mut world, a)?, 3);
    for gone in [a, a1, a2] {
        assert!(!world.is_alive(gone));
    }
    for kept in [root, b, b1] {
        assert!(world.is_alive(kept));
    }
    assert_eq!(kids(&world, root), vec![b]);
    Ok(())
}

#[test]
fn despawn_subtree_of_a_stale_entity_is_refused() -> Result<(), SceneError> {
    let mut world = World::new();
    let a = node(&mut world)?;
    world.despawn(a)?;
    assert_eq!(
        despawn_subtree(&mut world, a),
        Err(SceneError::Ecs(EcsError::StaleEntity))
    );
    Ok(())
}

#[test]
fn a_child_despawned_outside_the_scene_cuts_the_list_without_a_panic() -> Result<(), SceneError> {
    let mut world = World::new();
    let p = node(&mut world)?;
    let (a, b) = (node(&mut world)?, node(&mut world)?);
    set_parent(&mut world, a, p)?;
    set_parent(&mut world, b, p)?;
    world.despawn(a)?;
    assert!(kids(&world, p).is_empty());
    assert_eq!(parent_of(&world, b), Some(p));
    Ok(())
}

#[test]
fn a_child_whose_parent_was_despawned_outside_the_scene_has_no_parent() -> Result<(), SceneError> {
    let mut world = World::new();
    let (p, a) = (node(&mut world)?, node(&mut world)?);
    set_parent(&mut world, a, p)?;
    world.despawn(p)?;
    assert_eq!(parent_of(&world, a), None);
    Ok(())
}

#[test]
fn a_loop_made_by_removing_a_link_by_hand_is_reported_not_followed() -> Result<(), SceneError> {
    let mut world = World::new();
    let (parent, child) = (node(&mut world)?, node(&mut world)?);
    set_parent(&mut world, child, parent)?;
    world.remove::<scene::Parent>(child)?;
    set_parent(&mut world, parent, child)?;
    assert_eq!(
        despawn_subtree(&mut world, child),
        Err(SceneError::BrokenHierarchy)
    );
    assert!(world.is_alive(parent) && world.is_alive(child));
    assert!(kids(&world, child).len() <= world.len());
    Ok(())
}

#[test]
fn moving_the_only_child_leaves_the_old_parent_empty() -> Result<(), SceneError> {
    let mut world = World::new();
    let (old, new, only) = (node(&mut world)?, node(&mut world)?, node(&mut world)?);
    set_parent(&mut world, only, old)?;
    set_parent(&mut world, only, new)?;
    assert!(kids(&world, old).is_empty());
    assert_eq!(kids(&world, new), vec![only]);
    assert_eq!(parent_of(&world, only), Some(new));
    Ok(())
}

#[test]
fn setting_the_same_parent_for_an_only_child_keeps_it() -> Result<(), SceneError> {
    let mut world = World::new();
    let (parent, only) = (node(&mut world)?, node(&mut world)?);
    set_parent(&mut world, only, parent)?;
    set_parent(&mut world, only, parent)?;
    assert_eq!(kids(&world, parent), vec![only]);
    assert_eq!(parent_of(&world, only), Some(parent));
    Ok(())
}

#[test]
fn a_refused_call_leaves_the_child_where_it_was() -> Result<(), SceneError> {
    let mut world = World::new();
    let (old, child, sibling) = (node(&mut world)?, node(&mut world)?, node(&mut world)?);
    set_parent(&mut world, child, old)?;
    set_parent(&mut world, sibling, old)?;
    let bare = world.spawn()?;
    assert_eq!(
        set_parent(&mut world, child, bare),
        Err(SceneError::MissingTransform)
    );
    assert_eq!(set_parent(&mut world, old, child), Err(SceneError::Cycle));
    assert_eq!(parent_of(&world, child), Some(old));
    assert_eq!(kids(&world, old), vec![child, sibling]);
    Ok(())
}

#[test]
fn despawn_subtree_of_a_leaf_a_middle_child_and_a_deep_first_path() -> Result<(), SceneError> {
    let mut world = World::new();
    let root = node(&mut world)?;
    let (a, b, c) = (node(&mut world)?, node(&mut world)?, node(&mut world)?);
    for child in [a, b, c] {
        set_parent(&mut world, child, root)?;
    }
    let before = world.len();
    assert_eq!(despawn_subtree(&mut world, b)?, 1);
    assert_eq!(world.len(), before - 1);
    assert_eq!(kids(&world, root), vec![a, c]);
    let (d1, d2, d3) = (node(&mut world)?, node(&mut world)?, node(&mut world)?);
    set_parent(&mut world, d1, a)?;
    set_parent(&mut world, d2, d1)?;
    set_parent(&mut world, d3, d2)?;
    let before = world.len();
    assert_eq!(despawn_subtree(&mut world, a)?, 4);
    assert_eq!(world.len(), before - 4);
    assert_eq!(kids(&world, root), vec![c]);
    assert_eq!(despawn_subtree(&mut world, root)?, 2);
    assert!(world.is_empty());
    Ok(())
}

#[test]
fn a_sibling_that_points_at_itself_ends_the_walks_without_hanging() -> Result<(), SceneError> {
    let mut world = World::new();
    let (parent, first) = (node(&mut world)?, node(&mut world)?);
    set_parent(&mut world, first, parent)?;
    world.remove::<scene::Parent>(first)?;
    set_parent(&mut world, first, parent)?;
    assert!(kids(&world, parent).len() <= world.len());
    assert!(despawn_subtree(&mut world, parent).is_err());
    Ok(())
}
