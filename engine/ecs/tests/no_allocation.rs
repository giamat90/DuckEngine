//! After a warm-up that has created the tables and grown the columns, structural changes and the
//! command buffer make no allocator call on the calling thread (digest section 4: zero heap
//! allocations per frame on hot paths; counting allocator with per-thread counters).

use base::alloc::{CountingAllocator, count_allocations};
use ecs::{Commands, Entity, World};
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[derive(Clone, Copy)]
struct Position(f32);

#[derive(Clone, Copy)]
struct Velocity(f32);

#[derive(Clone, Copy)]
struct Marker(u32);

const ENTITIES: usize = 64;

/// Reads every component an entity has, so the fields are used.
fn read_sum(world: &World, entity: Entity) -> f32 {
    let position = world.get::<Position>(entity).map_or(0.0, |p| p.0);
    let velocity = world.get::<Velocity>(entity).map_or(0.0, |v| v.0);
    let marker = world.get::<Marker>(entity).map_or(0, |m| m.0);
    position + velocity + f32::from(u16::try_from(marker).unwrap_or(0))
}

/// One frame of structural churn through the world's own calls.
fn direct_frame(world: &mut World, scratch: &mut Vec<Entity>) -> usize {
    scratch.clear();
    for i in 0..ENTITIES {
        let Ok(entity) = world.spawn() else { return 0 };
        scratch.push(entity);
        let _ = world.insert(entity, Position(1.0));
        let _ = world.insert(entity, Velocity(2.0));
        if i % 2 == 0 {
            let _ = world.insert(entity, Marker(3));
        }
    }
    for entity in scratch.iter().step_by(2) {
        let _ = world.remove::<Marker>(*entity);
    }
    for entity in scratch.iter().step_by(3) {
        let _ = world.remove::<Velocity>(*entity);
    }
    let alive = world.len();
    for entity in scratch.iter() {
        assert!(read_sum(world, *entity) >= 0.0);
        let _ = world.despawn(*entity);
    }
    alive
}

#[test]
fn the_counter_sees_an_allocation() {
    let ((), counts) = count_allocations(|| drop(std::hint::black_box(vec![1_u8; 16])));
    assert!(
        counts.calls() >= 1,
        "the counting allocator is not installed"
    );
}

#[test]
fn the_warm_up_does_allocate_so_the_zero_below_means_something() {
    let mut world = World::new();
    let mut scratch = Vec::with_capacity(ENTITIES);
    let (alive, counts) = count_allocations(|| direct_frame(&mut world, &mut scratch));
    assert_eq!(alive, ENTITIES);
    assert!(counts.calls() > 0);
}

#[test]
fn direct_structural_changes_do_not_allocate_after_warm_up() {
    let mut world = World::new();
    let mut scratch = Vec::with_capacity(ENTITIES);
    assert_eq!(direct_frame(&mut world, &mut scratch), ENTITIES);
    let (alive, counts) = count_allocations(|| {
        let mut alive = 0;
        for _ in 0..20 {
            alive += direct_frame(&mut world, &mut scratch);
        }
        alive
    });
    assert_eq!(alive, 20 * ENTITIES);
    assert_eq!(counts.calls(), 0);
}

#[test]
fn the_command_buffer_does_not_allocate_after_warm_up() {
    let mut world = World::new();
    let mut commands = Commands::new();
    // One warm-up frame creates the tables, the queues and the capacities.
    buffered_frame_no_vec(&mut world, &mut commands);
    let ((), counts) = count_allocations(|| {
        for _ in 0..20 {
            buffered_frame_no_vec(&mut world, &mut commands);
        }
    });
    assert_eq!(counts.calls(), 0);
}

/// One frame of churn through the command buffer: spawn and fill, then remove and despawn.
fn buffered_frame_no_vec(world: &mut World, commands: &mut Commands) {
    for i in 0..ENTITIES {
        let pending = commands.spawn();
        commands.insert(pending, Position(1.0));
        commands.insert(pending, Velocity(2.0));
        if i % 2 == 0 {
            commands.insert(pending, Marker(3));
        }
    }
    commands.apply(world);
    for i in 0..commands.spawned().len() {
        let Some(Some(entity)) = commands.spawned().get(i).copied() else {
            continue;
        };
        commands.remove::<Marker>(entity);
        commands.despawn(entity);
    }
    commands.apply(world);
}
