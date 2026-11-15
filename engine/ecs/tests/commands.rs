//! Behaviour of the command buffer: structural changes recorded while a system iterates and
//! applied later, in order, at a fixed point.

use ecs::{ApplyReport, Commands, Entity, Target, World};

#[derive(Debug, Clone, Copy, PartialEq)]
struct Position(f32);

#[derive(Debug, Clone, Copy, PartialEq)]
struct Velocity(f32);

fn spawn(world: &mut World) -> Entity {
    world.spawn().unwrap_or_else(|e| panic!("spawn: {e}"))
}

fn report(applied: usize, refused: usize) -> ApplyReport {
    ApplyReport { applied, refused }
}

#[test]
fn an_empty_buffer_applies_nothing() {
    let mut world = World::new();
    let mut commands = Commands::new();
    assert!(commands.is_empty());
    assert_eq!(commands.apply(&mut world), report(0, 0));
    assert!(commands.spawned().is_empty());
}

#[test]
fn recording_changes_nothing_until_apply() {
    let mut world = World::new();
    let mut commands = Commands::new();
    assert_eq!(commands.len(), 0);
    commands.spawn();
    assert_eq!((commands.len(), commands.is_empty()), (1, false));
    commands.spawn();
    commands.despawn(Target::Pending(0));
    assert_eq!(commands.len(), 3);
    let mut commands = Commands::new();
    commands.spawn();
    assert_eq!(world.len(), 0);
    commands.apply(&mut world);
    assert_eq!(world.len(), 1);
}

#[test]
fn a_pending_spawn_takes_inserts_and_reports_its_entity() {
    let mut world = World::new();
    let mut commands = Commands::new();
    let pending = commands.spawn();
    commands.insert(pending, Position(1.0));
    commands.insert(pending, Velocity(2.0));
    assert_eq!(commands.apply(&mut world), report(3, 0));
    let spawned = commands.spawned().to_vec();
    assert_eq!(spawned.len(), 1);
    let Some(Some(entity)) = spawned.first().copied() else {
        panic!("no entity was spawned");
    };
    assert_eq!(world.get::<Position>(entity), Some(&Position(1.0)));
    assert_eq!(world.get::<Velocity>(entity), Some(&Velocity(2.0)));
}

#[test]
fn pending_numbers_count_the_spawns_of_this_buffer() {
    let mut commands = Commands::new();
    assert_eq!(commands.spawn(), Target::Pending(0));
    assert_eq!(commands.spawn(), Target::Pending(1));
}

#[test]
fn commands_on_existing_entities_apply() {
    let mut world = World::new();
    let a = spawn(&mut world);
    let b = spawn(&mut world);
    world
        .insert(b, Velocity(1.0))
        .unwrap_or_else(|e| panic!("{e}"));
    let mut commands = Commands::new();
    commands.insert(a, Position(5.0));
    commands.remove::<Velocity>(b);
    commands.despawn(b);
    assert_eq!(commands.apply(&mut world), report(3, 0));
    assert_eq!(world.get::<Position>(a), Some(&Position(5.0)));
    assert!(!world.is_alive(b));
}

#[test]
fn commands_apply_in_the_order_they_were_recorded() {
    let mut world = World::new();
    let entity = spawn(&mut world);
    let mut commands = Commands::new();
    commands.insert(entity, Position(1.0));
    commands.despawn(entity);
    commands.insert(entity, Position(2.0)); // after the despawn: refused
    assert_eq!(commands.apply(&mut world), report(2, 1));
    assert!(!world.is_alive(entity));
}

#[test]
fn a_refused_insert_still_consumes_its_value() {
    let mut world = World::new();
    let gone = spawn(&mut world);
    let live = spawn(&mut world);
    world.despawn(gone).unwrap_or_else(|e| panic!("{e}"));
    let mut commands = Commands::new();
    commands.insert(gone, Position(1.0));
    commands.insert(live, Position(2.0));
    assert_eq!(commands.apply(&mut world), report(1, 1));
    assert_eq!(world.get::<Position>(live), Some(&Position(2.0)));
}

#[test]
fn a_refused_insert_drops_its_value() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    struct Counted;
    impl Drop for Counted {
        fn drop(&mut self) {
            DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }
    let mut world = World::new();
    let mut commands = Commands::new();
    commands.insert(Target::Pending(3), Counted);
    assert_eq!(DROPS.load(Ordering::SeqCst), 0);
    assert_eq!(commands.apply(&mut world), report(0, 1));
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
}

#[test]
fn values_of_one_type_keep_their_order_across_interleaved_types() {
    let mut world = World::new();
    let a = spawn(&mut world);
    let b = spawn(&mut world);
    let c = spawn(&mut world);
    let mut commands = Commands::new();
    commands.insert(a, Position(1.0));
    commands.insert(b, Velocity(10.0));
    commands.insert(b, Position(2.0));
    commands.insert(c, Position(3.0));
    commands.insert(c, Velocity(20.0));
    assert_eq!(commands.apply(&mut world), report(5, 0));
    assert_eq!(world.get::<Position>(a), Some(&Position(1.0)));
    assert_eq!(world.get::<Position>(b), Some(&Position(2.0)));
    assert_eq!(world.get::<Position>(c), Some(&Position(3.0)));
    assert_eq!(world.get::<Velocity>(b), Some(&Velocity(10.0)));
    assert_eq!(world.get::<Velocity>(c), Some(&Velocity(20.0)));
}

#[test]
fn removing_a_missing_component_is_counted_as_refused() {
    let mut world = World::new();
    let entity = spawn(&mut world);
    let mut commands = Commands::new();
    commands.remove::<Position>(entity);
    assert_eq!(commands.apply(&mut world), report(0, 1));
}

#[test]
fn an_unknown_pending_target_is_refused() {
    let mut world = World::new();
    let mut commands = Commands::new();
    commands.insert(Target::Pending(7), Position(1.0));
    commands.despawn(Target::Pending(7));
    commands.remove::<Position>(Target::Pending(7));
    assert_eq!(commands.apply(&mut world), report(0, 3));
}

#[test]
fn a_pending_spawn_can_be_despawned_in_the_same_buffer() {
    let mut world = World::new();
    let mut commands = Commands::new();
    let pending = commands.spawn();
    commands.insert(pending, Position(1.0));
    commands.despawn(pending);
    assert_eq!(commands.apply(&mut world), report(3, 0));
    assert_eq!(world.len(), 0);
}

#[test]
fn apply_empties_the_buffer_and_the_next_batch_starts_at_pending_zero() {
    let mut world = World::new();
    let mut commands = Commands::new();
    commands.spawn();
    commands.apply(&mut world);
    assert!(commands.is_empty());
    assert_eq!(commands.spawn(), Target::Pending(0));
    commands.apply(&mut world);
    assert_eq!(world.len(), 2);
    assert_eq!(commands.spawned().len(), 1);
}

#[test]
fn unapplied_values_are_dropped_with_the_buffer() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    struct Counted;
    impl Drop for Counted {
        fn drop(&mut self) {
            DROPS.fetch_add(1, Ordering::SeqCst);
        }
    }
    let mut world = World::new();
    let entity = spawn(&mut world);
    let mut commands = Commands::new();
    commands.insert(entity, Counted);
    commands.insert(entity, Counted);
    drop(commands);
    assert_eq!(DROPS.load(Ordering::SeqCst), 2);
}

fn run_script() -> Vec<(u32, u32)> {
    let mut world = World::new();
    let mut commands = Commands::new();
    for i in 0..6_u8 {
        let pending = commands.spawn();
        commands.insert(pending, Position(f32::from(i)));
        if i % 2 == 0 {
            commands.insert(pending, Velocity(1.0));
        }
    }
    commands.apply(&mut world);
    let victims: Vec<Entity> = world.entities().take(2).collect();
    for victim in victims {
        commands.despawn(victim);
    }
    commands.apply(&mut world);
    world
        .entities()
        .map(|e| (e.index(), e.generation()))
        .collect()
}

#[test]
fn the_same_script_gives_the_same_storage_order() {
    let first = run_script();
    assert_eq!(first.len(), 4);
    assert_eq!(first, run_script());
}

#[test]
fn the_buffer_can_cross_threads() {
    fn needs_send_sync<T: Send + Sync>() {}
    needs_send_sync::<Commands>();
}

#[test]
fn a_pending_number_used_before_its_spawn_is_refused() {
    let mut world = World::new();
    let mut commands = Commands::new();
    commands.insert(Target::Pending(0), Position(1.0));
    commands.spawn();
    assert_eq!(commands.apply(&mut world), report(1, 1));
    assert_eq!(world.len(), 1);
}

#[test]
fn a_pending_number_kept_from_an_earlier_apply_names_the_new_spawn_at_that_position() {
    let mut world = World::new();
    let mut commands = Commands::new();
    let old = commands.spawn();
    commands.apply(&mut world);
    let fresh = commands.spawn();
    assert_eq!(old, fresh);
    commands.insert(old, Position(4.0));
    commands.apply(&mut world);
    let entities: Vec<Entity> = world.entities().collect();
    assert_eq!(entities.len(), 2);
    assert_eq!(world.get::<Position>(entities[1]), Some(&Position(4.0)));
    assert_eq!(world.get::<Position>(entities[0]), None);
}
