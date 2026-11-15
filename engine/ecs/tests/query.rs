//! Typed queries: which entities match, what the closure sees, declared access and its conflicts.

use ecs::{Access, EcsError, Entity, Read, World, Write};
use std::any::TypeId;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Position(f32);
#[derive(Debug, Clone, Copy, PartialEq)]
struct Velocity(f32);
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mass(f32);
#[derive(Debug, Clone, Copy, PartialEq)]
struct Tag;

fn spawn(world: &mut World) -> Entity {
    world.spawn().unwrap_or_else(|e| panic!("spawn: {e}"))
}

fn put<T: ecs::Component>(world: &mut World, entity: Entity, value: T) {
    world
        .insert(entity, value)
        .unwrap_or_else(|e| panic!("insert: {e}"));
}

/// Entities 0..n, even ones with Position and Velocity, every third with Mass too, odd ones with
/// Position only.
fn sample(n: u8) -> (World, Vec<Entity>) {
    let mut world = World::new();
    let mut all = Vec::new();
    for i in 0..n {
        let entity = spawn(&mut world);
        put(&mut world, entity, Position(f32::from(i)));
        if i % 2 == 0 {
            put(&mut world, entity, Velocity(1.0));
        }
        if i % 3 == 0 {
            put(&mut world, entity, Mass(2.0));
        }
        all.push(entity);
    }
    (world, all)
}

#[test]
fn a_one_component_read_visits_every_entity_that_has_it() {
    let (mut world, _) = sample(6);
    let mut seen = Vec::new();
    let done = world.for_each::<Read<Position>, _>(|p| seen.push(p.0.to_bits()));
    assert_eq!(done, Ok(()));
    seen.sort_unstable();
    let expected: Vec<u32> = (0..6_u8).map(|i| f32::from(i).to_bits()).collect();
    assert_eq!(seen, expected);
}

#[test]
fn a_query_skips_entities_that_lack_a_component() {
    let (mut world, _) = sample(6);
    let mut count = 0;
    let done = world.for_each::<(Read<Position>, Read<Velocity>), _>(|_| count += 1);
    assert_eq!(done, Ok(()));
    assert_eq!(count, 3); // 0, 2, 4
}

#[test]
fn a_write_changes_the_stored_component() {
    let (mut world, all) = sample(6);
    let done = world.for_each::<(Write<Position>, Read<Velocity>), _>(|(p, v)| p.0 += v.0 * 10.0);
    assert_eq!(done, Ok(()));
    for (i, entity) in all.iter().enumerate() {
        let base = f32::from(u8::try_from(i).unwrap_or(0));
        let expected = if i % 2 == 0 { base + 10.0 } else { base };
        assert_eq!(world.get::<Position>(*entity), Some(&Position(expected)));
    }
}

#[test]
fn three_components_give_a_three_tuple() {
    let (mut world, _) = sample(7);
    let mut sum = 0.0_f32;
    let done = world.for_each::<(Read<Position>, Read<Velocity>, Read<Mass>), _>(|(p, v, m)| {
        sum += p.0 + v.0 + m.0;
    });
    assert_eq!(done, Ok(()));
    assert_eq!(
        sum.to_bits(),
        (0.0_f32 + 1.0 + 2.0 + 6.0 + 1.0 + 2.0).to_bits()
    );
}

#[test]
fn the_same_row_of_each_column_is_paired_after_structural_changes() {
    let (mut world, all) = sample(12);
    assert_eq!(world.despawn(all[2]), Ok(()));
    assert_eq!(world.remove::<Velocity>(all[4]), Ok(()));
    put(&mut world, all[5], Velocity(7.0));
    let mut bad = 0;
    let mut visited = 0;
    let done = world.for_each::<(Read<Position>, Read<Velocity>), _>(|(p, v)| {
        visited += 1;
        let odd = (p.0 % 2.0).to_bits() == 1.0_f32.to_bits();
        if (odd && v.0.to_bits() != 7.0_f32.to_bits())
            || (!odd && v.0.to_bits() != 1.0_f32.to_bits())
        {
            bad += 1;
        }
    });
    assert_eq!(done, Ok(()));
    assert_eq!(visited, 5); // 0, 6, 8, 10 and 5
    assert_eq!(bad, 0);
}

#[test]
fn a_component_nobody_has_matches_nothing() {
    let (mut world, _) = sample(4);
    let mut count = 0;
    assert_eq!(world.for_each::<Read<Tag>, _>(|_| count += 1), Ok(()));
    assert_eq!(
        world.for_each::<(Read<Position>, Read<Tag>), _>(|_| count += 1),
        Ok(())
    );
    assert_eq!(count, 0);
}

#[test]
fn an_empty_world_visits_nothing() {
    let mut world = World::new();
    let mut count = 0;
    assert_eq!(world.for_each::<Read<Position>, _>(|_| count += 1), Ok(()));
    assert_eq!(count, 0);
}

#[test]
fn naming_a_component_twice_is_refused_before_anything_runs() {
    let (mut world, _) = sample(4);
    let mut count = 0;
    let both = world.for_each::<(Read<Position>, Write<Position>), _>(|_| count += 1);
    assert_eq!(both, Err(EcsError::ConflictingAccess));
    let twice = world.for_each::<(Read<Position>, Read<Position>), _>(|_| count += 1);
    assert_eq!(twice, Err(EcsError::ConflictingAccess));
    let writes = world.for_each::<(Write<Mass>, Read<Velocity>, Write<Mass>), _>(|_| count += 1);
    assert_eq!(writes, Err(EcsError::ConflictingAccess));
    assert_eq!(count, 0);
}

#[test]
fn declared_access_lists_reads_and_writes() {
    let access = Access::of::<(Write<Position>, Read<Velocity>, Read<Mass>)>()
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(access.writes(), &[TypeId::of::<Position>()]);
    let mut reads = access.reads().to_vec();
    reads.sort();
    let mut expected = vec![TypeId::of::<Velocity>(), TypeId::of::<Mass>()];
    expected.sort();
    assert_eq!(reads, expected);
}

#[test]
fn declared_access_of_a_conflicting_query_is_refused() {
    assert_eq!(
        Access::of::<(Read<Position>, Write<Position>)>(),
        Err(EcsError::ConflictingAccess)
    );
}

#[test]
fn two_queries_conflict_when_one_writes_what_the_other_touches() {
    let write_pos = Access::of::<Write<Position>>().unwrap_or_else(|e| panic!("{e}"));
    let read_pos = Access::of::<Read<Position>>().unwrap_or_else(|e| panic!("{e}"));
    let read_vel = Access::of::<Read<Velocity>>().unwrap_or_else(|e| panic!("{e}"));
    let write_vel = Access::of::<Write<Velocity>>().unwrap_or_else(|e| panic!("{e}"));
    assert!(write_pos.conflicts_with(&read_pos));
    assert!(read_pos.conflicts_with(&write_pos));
    assert!(write_pos.conflicts_with(&write_pos));
    assert!(!read_pos.conflicts_with(&read_pos));
    assert!(!write_pos.conflicts_with(&read_vel));
    assert!(!write_pos.conflicts_with(&write_vel));
}

#[test]
fn iteration_order_is_table_then_row_order_and_repeatable() {
    fn visit() -> Vec<u32> {
        let (mut world, _) = sample(9);
        let mut order = Vec::new();
        let done = world.for_each::<Read<Position>, _>(|p| order.push(p.0.to_bits()));
        assert_eq!(done, Ok(()));
        order
    }
    let first = visit();
    assert_eq!(first.len(), 9);
    assert_eq!(first, visit());
}
