//! Behaviour of the world: spawn, despawn, insert, remove, lookup, storage order, stale handles.

use ecs::{EcsError, Entity, World};

#[derive(Debug, Clone, Copy, PartialEq)]
struct Position {
    x: f32,
    y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Velocity(f32);

#[derive(Debug, Clone, PartialEq)]
struct Name(String);

fn spawn(world: &mut World) -> Entity {
    world.spawn().unwrap_or_else(|e| panic!("spawn: {e}"))
}

fn put<T: ecs::Component>(world: &mut World, entity: Entity, value: T) {
    world
        .insert(entity, value)
        .unwrap_or_else(|e| panic!("insert: {e}"));
}

#[test]
fn a_new_world_is_empty_with_one_empty_table() {
    let world = World::new();
    assert_eq!((world.len(), world.is_empty()), (0, true));
    assert_eq!(world.archetype_count(), 1);
}

#[test]
fn spawn_makes_a_live_entity_without_components() {
    let mut world = World::new();
    let entity = spawn(&mut world);
    assert!(world.is_alive(entity));
    assert_eq!((world.len(), world.is_empty()), (1, false));
    assert!(!world.has::<Position>(entity));
    assert_eq!(world.get::<Position>(entity), None);
}

#[test]
fn a_new_table_is_counted_when_a_component_set_first_appears() {
    let mut world = World::new();
    let entity = spawn(&mut world);
    put(&mut world, entity, Velocity(1.0));
    assert_eq!(world.archetype_count(), 2);
    put(&mut world, entity, Position { x: 0.0, y: 0.0 });
    assert_eq!(world.archetype_count(), 3);
}

#[test]
fn insert_then_get_returns_the_value() {
    let mut world = World::new();
    let entity = spawn(&mut world);
    put(&mut world, entity, Position { x: 1.0, y: 2.0 });
    assert_eq!(
        world.get::<Position>(entity),
        Some(&Position { x: 1.0, y: 2.0 })
    );
    assert!(world.has::<Position>(entity));
}

#[test]
fn insert_of_an_existing_component_replaces_it_in_place() {
    let mut world = World::new();
    let entity = spawn(&mut world);
    put(&mut world, entity, Velocity(1.0));
    let tables = world.archetype_count();
    put(&mut world, entity, Velocity(5.0));
    assert_eq!(world.get::<Velocity>(entity), Some(&Velocity(5.0)));
    assert_eq!(world.archetype_count(), tables);
}

#[test]
fn get_mut_changes_the_stored_value() {
    let mut world = World::new();
    let entity = spawn(&mut world);
    put(&mut world, entity, Velocity(1.0));
    if let Some(v) = world.get_mut::<Velocity>(entity) {
        v.0 = 9.0;
    }
    assert_eq!(world.get::<Velocity>(entity), Some(&Velocity(9.0)));
}

#[test]
fn components_of_one_entity_survive_another_entitys_changes() {
    let mut world = World::new();
    let a = spawn(&mut world);
    let b = spawn(&mut world);
    put(&mut world, a, Position { x: 1.0, y: 1.0 });
    put(&mut world, b, Position { x: 2.0, y: 2.0 });
    put(&mut world, a, Velocity(3.0));
    put(&mut world, b, Name("b".to_string()));
    assert_eq!(world.get::<Position>(a), Some(&Position { x: 1.0, y: 1.0 }));
    assert_eq!(world.get::<Velocity>(a), Some(&Velocity(3.0)));
    assert_eq!(world.get::<Position>(b), Some(&Position { x: 2.0, y: 2.0 }));
    assert_eq!(world.get::<Name>(b), Some(&Name("b".to_string())));
    assert!(!world.has::<Name>(a));
}

#[test]
fn remove_drops_the_component_and_keeps_the_others() {
    let mut world = World::new();
    let entity = spawn(&mut world);
    put(&mut world, entity, Position { x: 1.0, y: 2.0 });
    put(&mut world, entity, Velocity(3.0));
    assert_eq!(world.remove::<Velocity>(entity), Ok(()));
    assert!(!world.has::<Velocity>(entity));
    assert_eq!(
        world.get::<Position>(entity),
        Some(&Position { x: 1.0, y: 2.0 })
    );
}

#[test]
fn removing_a_component_the_entity_lacks_is_refused() {
    let mut world = World::new();
    let entity = spawn(&mut world);
    assert_eq!(
        world.remove::<Velocity>(entity),
        Err(EcsError::MissingComponent)
    );
    put(&mut world, entity, Position { x: 0.0, y: 0.0 });
    assert_eq!(
        world.remove::<Velocity>(entity),
        Err(EcsError::MissingComponent)
    );
    assert!(world.has::<Position>(entity));
}

#[test]
fn despawn_makes_the_handle_stale_everywhere() {
    let mut world = World::new();
    let entity = spawn(&mut world);
    put(&mut world, entity, Velocity(1.0));
    assert_eq!(world.despawn(entity), Ok(()));
    assert!(!world.is_alive(entity));
    assert_eq!(world.len(), 0);
    assert_eq!(world.get::<Velocity>(entity), None);
    assert_eq!(world.get_mut::<Velocity>(entity), None);
    assert!(!world.has::<Velocity>(entity));
    assert_eq!(world.despawn(entity), Err(EcsError::StaleEntity));
    assert_eq!(
        world.insert(entity, Velocity(2.0)),
        Err(EcsError::StaleEntity)
    );
    assert_eq!(world.remove::<Velocity>(entity), Err(EcsError::StaleEntity));
}

#[test]
fn a_stale_handle_does_not_reach_the_entity_that_reuses_its_slot() {
    let mut world = World::new();
    let old = spawn(&mut world);
    put(&mut world, old, Velocity(1.0));
    assert_eq!(world.despawn(old), Ok(()));
    let new = spawn(&mut world);
    put(&mut world, new, Velocity(2.0));
    assert_eq!(new.index(), old.index());
    assert_ne!(new, old);
    assert_eq!(world.get::<Velocity>(old), None);
    assert_eq!(world.get::<Velocity>(new), Some(&Velocity(2.0)));
    assert_eq!(world.despawn(old), Err(EcsError::StaleEntity));
    assert!(world.is_alive(new));
}

#[test]
fn despawn_keeps_the_entity_moved_into_its_row_reachable() {
    let mut world = World::new();
    let values = [0.0_f32, 1.0, 2.0, 3.0];
    let entities: Vec<Entity> = values.iter().map(|_| spawn(&mut world)).collect();
    for (entity, value) in entities.iter().zip(values) {
        put(&mut world, *entity, Velocity(value));
    }
    assert_eq!(world.despawn(entities[0]), Ok(()));
    for (entity, value) in entities.iter().zip(values).skip(1) {
        assert_eq!(world.get::<Velocity>(*entity), Some(&Velocity(value)));
    }
}

#[test]
fn a_structural_change_keeps_the_entity_moved_into_the_hole_reachable() {
    let mut world = World::new();
    let values = [0.0_f32, 1.0, 2.0, 3.0];
    let entities: Vec<Entity> = values.iter().map(|_| spawn(&mut world)).collect();
    for (entity, value) in entities.iter().zip(values) {
        put(&mut world, *entity, Velocity(value));
    }
    put(&mut world, entities[0], Position { x: 0.0, y: 0.0 });
    assert_eq!(world.remove::<Velocity>(entities[1]), Ok(()));
    assert!(!world.has::<Velocity>(entities[1]));
    for (entity, value) in entities.iter().zip(values).skip(2) {
        assert_eq!(world.get::<Velocity>(*entity), Some(&Velocity(value)));
    }
    assert_eq!(world.get::<Velocity>(entities[0]), Some(&Velocity(0.0)));
}

#[test]
fn entities_with_the_same_components_share_one_table() {
    let mut world = World::new();
    let a = spawn(&mut world);
    put(&mut world, a, Position { x: 0.0, y: 0.0 });
    let tables = world.archetype_count();
    let b = spawn(&mut world);
    put(&mut world, b, Position { x: 1.0, y: 1.0 });
    assert_eq!(world.archetype_count(), tables);
}

#[test]
fn the_order_components_are_added_does_not_change_the_table() {
    let mut world = World::new();
    let a = spawn(&mut world);
    put(&mut world, a, Position { x: 0.0, y: 0.0 });
    put(&mut world, a, Velocity(1.0));
    let b = spawn(&mut world);
    put(&mut world, b, Velocity(1.0));
    put(&mut world, b, Position { x: 0.0, y: 0.0 });
    let tables = world.archetype_count();
    let c = spawn(&mut world);
    put(&mut world, c, Velocity(1.0));
    put(&mut world, c, Position { x: 0.0, y: 0.0 });
    assert_eq!(world.archetype_count(), tables);
    // a, b and c are in the table with both components, in spawn order, after the earlier tables.
    let order: Vec<Entity> = world.entities().collect();
    assert_eq!(order, vec![a, b, c]);
}

#[test]
fn entities_iterate_in_table_creation_order_then_row_order() {
    let mut world = World::new();
    let plain = spawn(&mut world);
    let moving = spawn(&mut world);
    let placed = spawn(&mut world);
    let moving2 = spawn(&mut world);
    put(&mut world, moving, Velocity(1.0));
    put(&mut world, placed, Position { x: 0.0, y: 0.0 });
    put(&mut world, moving2, Velocity(2.0));
    let order: Vec<Entity> = world.entities().collect();
    assert_eq!(order, vec![plain, moving, moving2, placed]);
}

#[test]
fn registering_twice_gives_the_same_id_and_ids_follow_first_use() {
    let mut world = World::new();
    let position = world.register::<Position>();
    let velocity = world.register::<Velocity>();
    assert_eq!(world.register::<Position>(), position);
    assert_ne!(position, velocity);
    assert_eq!(position.map(ecs::ComponentId::index), Ok(0));
    assert_eq!(velocity.map(ecs::ComponentId::index), Ok(1));
}

#[test]
fn insert_registers_a_type_on_first_use() {
    let mut world = World::new();
    let entity = spawn(&mut world);
    put(&mut world, entity, Velocity(1.0));
    let id = world.register::<Velocity>();
    assert_eq!(id.map(ecs::ComponentId::index), Ok(0));
}

#[test]
fn the_world_can_cross_threads() {
    fn needs_send_sync<T: Send + Sync>() {}
    needs_send_sync::<World>();
    needs_send_sync::<Entity>();
}

#[test]
fn values_with_drop_glue_are_dropped_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    struct Counted;
    impl Drop for Counted {
        fn drop(&mut self) {
            DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }
    let mut world = World::new();
    let a = spawn(&mut world);
    let b = spawn(&mut world);
    let c = spawn(&mut world);
    put(&mut world, a, Counted);
    put(&mut world, b, Counted);
    put(&mut world, c, Counted);
    put(&mut world, c, Counted); // replaced: the old value drops
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    assert_eq!(world.remove::<Counted>(a), Ok(()));
    assert_eq!(DROPS.load(Ordering::SeqCst), 2);
    assert_eq!(world.despawn(b), Ok(()));
    assert_eq!(DROPS.load(Ordering::SeqCst), 3);
    drop(world);
    assert_eq!(DROPS.load(Ordering::SeqCst), 4);
}
