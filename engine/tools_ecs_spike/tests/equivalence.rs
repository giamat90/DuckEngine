//! The four storages answer the same operations with the same results, and each is
//! deterministic: the spike's measurements compare like with like.

use tools_ecs_spike::workloads::{churn, mark_half, populate, toggle};
use tools_ecs_spike::{ArchetypeRaw, ArchetypeSafe, Entity, Hybrid, Kind, SparseWorld, Store};

/// A fixed script of operations, returning the query results after each step.
fn script<S: Store>() -> Vec<(u64, u64, u64)> {
    let (mut world, ids) = populate::<S>(2_000);
    let mut results = Vec::new();
    let mut record = |world: &S| {
        results.push((world.sum_x().to_bits(), world.extract(), world.digest()));
    };
    record(&world);
    world.integrate();
    world.integrate();
    record(&world);
    mark_half(&mut world, &ids);
    world.bump_hit();
    record(&world);
    let mut scratch = Vec::new();
    churn(&mut world, 200, 1_000_000, &mut scratch);
    toggle(&mut world, &ids, 300);
    world.integrate();
    record(&world);
    for &entity in ids.iter().step_by(7) {
        world.despawn(entity);
    }
    world.integrate();
    record(&world);
    results.push((u64::try_from(world.len()).unwrap_or(0), 0, 0));
    results
}

#[test]
fn all_storages_give_the_same_results() {
    let reference = script::<ArchetypeSafe>();
    assert_eq!(script::<ArchetypeRaw>(), reference, "archetype-raw");
    assert_eq!(script::<SparseWorld>(), reference, "sparse-set");
    assert_eq!(script::<Hybrid>(), reference, "hybrid");
}

#[test]
fn the_script_changes_the_results() {
    let results = script::<ArchetypeSafe>();
    assert!(results.windows(2).any(|pair| pair[0] != pair[1]));
}

fn order_hash_of_run<S: Store>() -> u64 {
    let (mut world, ids) = populate::<S>(1_500);
    mark_half(&mut world, &ids);
    let mut scratch = Vec::new();
    churn(&mut world, 150, 5_000_000, &mut scratch);
    toggle(&mut world, &ids, 200);
    world.integrate();
    for &entity in ids.iter().step_by(5) {
        world.despawn(entity);
    }
    world.order_hash()
}

fn is_deterministic<S: Store>() -> bool {
    order_hash_of_run::<S>() == order_hash_of_run::<S>()
}

#[test]
fn iteration_order_is_the_same_in_two_runs() {
    assert!(is_deterministic::<ArchetypeSafe>());
    assert!(is_deterministic::<ArchetypeRaw>());
    assert!(is_deterministic::<SparseWorld>());
    assert!(is_deterministic::<Hybrid>());
}

fn stale_handles_are_refused<S: Store>() {
    let mut world = S::with_capacity(8);
    let first = world.spawn(Kind::Box, 1);
    let other = world.spawn(Kind::Disc, 2);
    assert!(world.despawn(first));
    assert!(!world.is_alive(first));
    assert!(!world.despawn(first), "a second despawn is refused");
    assert!(
        !world.set_hit(first, true),
        "a stale handle takes no marker"
    );
    let reused = world.spawn(Kind::Wall, 3);
    assert_eq!(reused.index, first.index, "the slot is reused");
    assert_ne!(reused.generation, first.generation);
    assert!(
        !world.is_alive(first),
        "the old handle stays dead after reuse"
    );
    assert!(world.is_alive(reused));
    assert!(world.is_alive(other));
    assert_eq!(world.len(), 2);
}

#[test]
fn a_stale_handle_is_refused_in_every_storage() {
    stale_handles_are_refused::<ArchetypeSafe>();
    stale_handles_are_refused::<ArchetypeRaw>();
    stale_handles_are_refused::<SparseWorld>();
    stale_handles_are_refused::<Hybrid>();
}

fn marker_toggles_are_idempotent<S: Store>() {
    let mut world = S::with_capacity(8);
    let e: Entity = world.spawn(Kind::Box, 4);
    let before = (world.sum_x().to_bits(), world.extract());
    assert!(world.set_hit(e, true));
    assert!(world.set_hit(e, true), "adding twice is harmless");
    world.bump_hit();
    assert!(world.set_hit(e, false));
    assert!(world.set_hit(e, false), "removing twice is harmless");
    world.bump_hit();
    assert_eq!((world.sum_x().to_bits(), world.extract()), before);
}

#[test]
fn the_marker_toggles_are_idempotent_in_every_storage() {
    marker_toggles_are_idempotent::<ArchetypeSafe>();
    marker_toggles_are_idempotent::<ArchetypeRaw>();
    marker_toggles_are_idempotent::<SparseWorld>();
    marker_toggles_are_idempotent::<Hybrid>();
}

fn despawning_everything_empties_the_world<S: Store>() {
    let (mut world, ids) = populate::<S>(500);
    for &entity in ids.iter().rev() {
        assert!(world.despawn(entity));
    }
    assert!(world.is_empty());
    assert_eq!(world.sum_x().abs().to_bits(), 0, "an empty sum is zero");
    assert_eq!(world.extract(), 0);
}

#[test]
fn despawning_everything_empties_every_storage() {
    despawning_everything_empties_the_world::<ArchetypeSafe>();
    despawning_everything_empties_the_world::<ArchetypeRaw>();
    despawning_everything_empties_the_world::<SparseWorld>();
    despawning_everything_empties_the_world::<Hybrid>();
}

/// A deterministic mix of spawn, despawn and marker changes, compared across storages after
/// every batch.
fn random_session<S: Store>() -> Vec<(u64, u64, u64, u64)> {
    let mut world = S::with_capacity(64);
    let mut live: Vec<Entity> = Vec::new();
    let mut state = 12_345_u32;
    let mut next = || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        state >> 8
    };
    let mut out = Vec::new();
    for step in 0..3_000_u32 {
        let roll = next() % 10;
        match roll {
            0..=4 => {
                let serial = next();
                live.push(world.spawn(Kind::of_serial(serial), serial));
            }
            5..=6 if !live.is_empty() => {
                let pick = usize::try_from(next()).unwrap_or(0) % live.len();
                world.despawn(live.swap_remove(pick));
            }
            _ if !live.is_empty() => {
                let pick = usize::try_from(next()).unwrap_or(0) % live.len();
                world.set_hit(live[pick], roll % 2 == 0);
            }
            _ => {}
        }
        if step % 250 == 0 {
            world.integrate();
            world.bump_hit();
            out.push((
                world.sum_x().to_bits(),
                world.extract(),
                u64::try_from(world.len()).unwrap_or(0),
                world.digest(),
            ));
        }
    }
    out
}

#[test]
fn a_random_session_gives_the_same_results_in_every_storage() {
    let reference = random_session::<ArchetypeSafe>();
    assert_eq!(random_session::<ArchetypeRaw>(), reference, "archetype-raw");
    assert_eq!(random_session::<SparseWorld>(), reference, "sparse-set");
    assert_eq!(random_session::<Hybrid>(), reference, "hybrid");
}
