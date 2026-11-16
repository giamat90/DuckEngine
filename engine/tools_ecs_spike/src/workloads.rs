//! The workloads of the spike, generic over the storage, and the timing around them.

use crate::components::Kind;
use crate::entity::Entity;
use crate::store::Store;
use std::time::Instant;

/// A workload of the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workload {
    /// Query of one component (read).
    Iter1,
    /// Query of two components (one written).
    Iter2,
    /// Query of three components (read), the render extract.
    Iter3,
    /// Query over the marker and a component, half the entities marked.
    IterHit,
    /// Spawn a tenth of the entities, then despawn them.
    Churn,
    /// Add the marker to a tenth of the entities, then remove it.
    Toggle,
}

impl Workload {
    /// All workloads, in report order.
    pub const ALL: [Self; 6] = [
        Self::Iter1,
        Self::Iter2,
        Self::Iter3,
        Self::IterHit,
        Self::Churn,
        Self::Toggle,
    ];

    /// Name in the report.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Iter1 => "iterate 1 comp",
            Self::Iter2 => "iterate 2 comp (write)",
            Self::Iter3 => "iterate 3 comp (extract)",
            Self::IterHit => "iterate marker + 1",
            Self::Churn => "spawn + despawn 10%",
            Self::Toggle => "add + remove marker 10%",
        }
    }

    /// Entities the workload touches in one run, for the per-entity figure.
    #[must_use]
    pub const fn touched(self, n: usize) -> usize {
        match self {
            Self::Iter1 | Self::Iter2 | Self::Iter3 => n,
            Self::IterHit => n / 2,
            Self::Churn | Self::Toggle => n / 10 * 2,
        }
    }
}

/// A world of `n` entities in the mix of the sandbox, and their ids in spawn order.
#[must_use]
pub fn populate<S: Store>(n: usize) -> (S, Vec<Entity>) {
    let mut world = S::with_capacity(n + n / 10);
    let mut ids = Vec::with_capacity(n + n / 10);
    for serial in 0..n {
        let serial = u32::try_from(serial).unwrap_or(u32::MAX);
        ids.push(world.spawn(Kind::of_serial(serial), serial));
    }
    (world, ids)
}

/// Marks every second entity (the state the marker queries run on).
pub fn mark_half<S: Store>(world: &mut S, ids: &[Entity]) {
    for &entity in ids.iter().step_by(2) {
        world.set_hit(entity, true);
    }
}

/// Spawns `count` entities, then despawns the ones just spawned; returns a value the optimiser
/// cannot drop.
pub fn churn<S: Store>(
    world: &mut S,
    count: usize,
    first_serial: u32,
    scratch: &mut Vec<Entity>,
) -> u64 {
    scratch.clear();
    for i in 0..count {
        let serial = first_serial.wrapping_add(u32::try_from(i).unwrap_or(0));
        scratch.push(world.spawn(Kind::of_serial(serial), serial));
    }
    let mut despawned = 0_u64;
    for &entity in scratch.iter() {
        despawned += u64::from(world.despawn(entity));
    }
    despawned
}

/// Adds the marker to the first `count` entities, then removes it.
pub fn toggle<S: Store>(world: &mut S, ids: &[Entity], count: usize) -> u64 {
    let mut changed = 0_u64;
    for &entity in ids.iter().take(count) {
        changed += u64::from(world.set_hit(entity, true));
    }
    for &entity in ids.iter().take(count) {
        changed += u64::from(world.set_hit(entity, false));
    }
    changed
}

/// Runs one workload once; the value is a checksum that keeps the work alive.
pub fn run_once<S: Store>(
    world: &mut S,
    ids: &[Entity],
    workload: Workload,
    scratch: &mut Vec<Entity>,
) -> u64 {
    let tenth = ids.len() / 10;
    match workload {
        Workload::Iter1 => world.sum_x().to_bits(),
        Workload::Iter2 => {
            world.integrate();
            u64::from(world.len() > 0)
        }
        Workload::Iter3 => world.extract(),
        Workload::IterHit => {
            world.bump_hit();
            u64::from(world.len() > 0)
        }
        Workload::Churn => churn(world, tenth, 1_000_000, scratch),
        Workload::Toggle => toggle(world, ids, tenth),
    }
}

/// Runs timed together in one sample, so a small world is not lost in the clock's resolution
/// (every workload leaves the world in the state it found it).
fn inner_runs(n: usize) -> u32 {
    u32::try_from(400_000 / n.max(1)).unwrap_or(1).clamp(1, 400)
}

/// Median nanoseconds per run of `workload` on a fresh world of `n` entities, over `reps` timed
/// runs after one warm-up run; also the minimum.
#[must_use]
pub fn time_workload<S: Store>(n: usize, workload: Workload, reps: usize) -> (f64, f64) {
    let (mut world, ids) = populate::<S>(n);
    if workload == Workload::IterHit {
        mark_half(&mut world, &ids);
    }
    let mut scratch = Vec::with_capacity(n / 10 + 1);
    let mut sink = run_once(&mut world, &ids, workload, &mut scratch);
    let inner = inner_runs(n);
    let mut samples = Vec::with_capacity(reps);
    for _ in 0..reps {
        let start = Instant::now();
        for _ in 0..inner {
            sink = sink.wrapping_add(run_once(&mut world, &ids, workload, &mut scratch));
        }
        samples.push(start.elapsed().as_secs_f64() * 1e9 / f64::from(inner));
    }
    std::hint::black_box(sink);
    samples.sort_by(f64::total_cmp);
    let min = samples.first().copied().unwrap_or(0.0);
    let median = samples.get(samples.len() / 2).copied().unwrap_or(0.0);
    (median, min)
}
