//! Synthetic workloads standing in for the physics solve loop, ECS iteration and the mixer, which
//! do not exist yet. Every body is a pure function of its index (and of the previous phase's
//! buffer), so by the port's contract the output hash must be the same under every scheduler and
//! worker count (D1).

use jobs::error::JobError;
use jobs::scheduler::IScheduler;
use std::sync::atomic::{AtomicU64, Ordering};

const MUL: u64 = 6_364_136_223_846_793_005;
const INC: u64 = 1_442_695_040_888_963_407;
const FNV_PRIME: u64 = 0x0100_0000_01b3;
const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2645;
/// Items of one call of the dispatch workload (at least the largest worker count measured).
const DISPATCH_ITEMS: u32 = 24;
/// Burn iterations per index of the phases workload.
const PHASE_ITERS: u32 = 24;
/// Chunk size of the phases workload.
const PHASE_GRAIN: u32 = 8;
/// Chunk size of the coarsened tiny-jobs workload (the usual remedy for tiny jobs).
const COARSE_GRAIN: u32 = 64;

/// A dependent chain of `iters` multiply-xorshift steps: about a nanosecond per iteration, cannot
/// be folded away, pure.
#[must_use]
pub fn burn(seed: u64, iters: u32) -> u64 {
    let mut x = seed;
    for _ in 0..iters {
        x = x.wrapping_mul(MUL).wrapping_add(INC);
        x ^= x >> 29;
    }
    x
}

/// How big each workload is.
#[derive(Clone, Copy, Debug)]
pub struct Sizes {
    /// Tiny jobs: indices per call, iterations per index, calls per run.
    pub tiny: (u32, u32, u32),
    /// Balanced jobs: indices, iterations per index.
    pub balanced: (u32, u32),
    /// Imbalanced: indices, heavy indices (the first ones), heavy and light iterations.
    pub imbalanced: (u32, u32, u32, u32),
    /// Dispatch: calls per run.
    pub dispatch_calls: u32,
    /// Phases: phases per run, indices per phase.
    pub phases: (u32, u32),
}

impl Sizes {
    /// The sizes of the report.
    pub const FULL: Self = Self {
        tiny: (65_536, 4, 8),
        balanced: (2048, 6000),
        imbalanced: (256, 8, 400_000, 8000),
        dispatch_calls: 2000,
        phases: (100, 512),
    };

    /// Small sizes for tests.
    pub const SMALL: Self = Self {
        tiny: (500, 4, 2),
        balanced: (60, 50),
        imbalanced: (40, 3, 400, 20),
        dispatch_calls: 30,
        phases: (8, 128),
    };
}

/// The workloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workload {
    /// Many tiny jobs, one index each: scheduling overhead per job.
    Tiny,
    /// The same jobs as `Tiny` in chunks of 64 indices: the usual remedy for tiny jobs.
    TinyCoarse,
    /// Balanced medium jobs.
    Balanced,
    /// A few heavy jobs at the start, many light ones: needs rebalancing.
    Imbalanced,
    /// Many fork-joins with almost no work: dispatch and join latency.
    Dispatch,
    /// Dependent short phases (a physics step runs several): join latency under load.
    Phases,
}

impl Workload {
    /// All workloads in report order.
    pub const ALL: [Self; 6] = [
        Self::Tiny,
        Self::TinyCoarse,
        Self::Balanced,
        Self::Imbalanced,
        Self::Dispatch,
        Self::Phases,
    ];

    /// Short name used in the report and on the command line.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Tiny => "w1-tiny",
            Self::TinyCoarse => "w1b-tiny-g64",
            Self::Balanced => "w2-balanced",
            Self::Imbalanced => "w3-imbalanced",
            Self::Dispatch => "w4a-dispatch",
            Self::Phases => "w4b-phases",
        }
    }

    /// The workload called `name`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|w| w.name() == name)
    }

    /// What one unit is: a job, a call or a phase.
    #[must_use]
    pub fn units_per_run(self, sizes: &Sizes) -> u64 {
        match self {
            Self::Tiny | Self::TinyCoarse => u64::from(sizes.tiny.0) * u64::from(sizes.tiny.2),
            Self::Balanced => u64::from(sizes.balanced.0),
            Self::Imbalanced => u64::from(sizes.imbalanced.0),
            Self::Dispatch => u64::from(sizes.dispatch_calls),
            Self::Phases => u64::from(sizes.phases.0),
        }
    }
}

fn slots(len: u32) -> Vec<AtomicU64> {
    (0..len).map(|_| AtomicU64::new(0)).collect()
}

fn digest(values: &[AtomicU64]) -> u64 {
    values.iter().fold(FNV_BASIS, |hash, value| {
        (hash ^ value.load(Ordering::Relaxed)).wrapping_mul(FNV_PRIME)
    })
}

/// The buffers of one workload, allocated once so a run allocates nothing itself.
pub struct Bench {
    workload: Workload,
    sizes: Sizes,
    first: Vec<AtomicU64>,
    second: Vec<AtomicU64>,
}

impl Bench {
    /// Buffers for `workload`.
    #[must_use]
    pub fn new(workload: Workload, sizes: Sizes) -> Self {
        let len = match workload {
            Workload::Tiny | Workload::TinyCoarse => sizes.tiny.0,
            Workload::Balanced => sizes.balanced.0,
            Workload::Imbalanced => sizes.imbalanced.0,
            Workload::Dispatch => DISPATCH_ITEMS,
            Workload::Phases => sizes.phases.1,
        };
        Self {
            workload,
            sizes,
            first: slots(len),
            second: slots(if workload == Workload::Phases { len } else { 0 }),
        }
    }

    /// Runs the workload once and returns the hash of its output.
    ///
    /// # Errors
    /// A refusal of the scheduler.
    pub fn run(&self, scheduler: &dyn IScheduler) -> Result<u64, JobError> {
        match self.workload {
            Workload::Tiny => self.tiny(scheduler, 1),
            Workload::TinyCoarse => self.tiny(scheduler, COARSE_GRAIN),
            Workload::Balanced => self.balanced(scheduler),
            Workload::Imbalanced => self.imbalanced(scheduler),
            Workload::Dispatch => self.dispatch(scheduler),
            Workload::Phases => self.phases(scheduler),
        }
        .map(|()| digest(&self.first))
    }

    fn tiny(&self, scheduler: &dyn IScheduler, grain: u32) -> Result<(), JobError> {
        (0..self.sizes.tiny.2).try_for_each(|_| self.tiny_call(scheduler, grain))
    }

    fn tiny_call(&self, scheduler: &dyn IScheduler, grain: u32) -> Result<(), JobError> {
        let (count, iters, _) = self.sizes.tiny;
        scheduler.for_each(count, grain, &|index| {
            store_burn(&self.first, index, iters);
        })
    }

    fn balanced(&self, scheduler: &dyn IScheduler) -> Result<(), JobError> {
        let (count, iters) = self.sizes.balanced;
        scheduler.for_each(count, 1, &|index| store_burn(&self.first, index, iters))
    }

    fn imbalanced(&self, scheduler: &dyn IScheduler) -> Result<(), JobError> {
        let (count, heavy, heavy_iters, light_iters) = self.sizes.imbalanced;
        scheduler.for_each(count, 1, &|index| {
            let iters = pick(index < heavy, heavy_iters, light_iters);
            store_burn(&self.first, index, iters);
        })
    }

    fn dispatch(&self, scheduler: &dyn IScheduler) -> Result<(), JobError> {
        self.first
            .iter()
            .for_each(|slot| slot.store(0, Ordering::Relaxed));
        (0..self.sizes.dispatch_calls).try_for_each(|_| self.dispatch_call(scheduler))
    }

    fn dispatch_call(&self, scheduler: &dyn IScheduler) -> Result<(), JobError> {
        scheduler.for_each(DISPATCH_ITEMS, 1, &|index| {
            bump(&self.first[index as usize], index);
        })
    }

    fn phases(&self, scheduler: &dyn IScheduler) -> Result<(), JobError> {
        for (position, slot) in (0_u64..).zip(&self.first) {
            slot.store(position, Ordering::Relaxed);
        }
        (0..self.sizes.phases.0).try_for_each(|phase| self.phase(scheduler, phase))?;
        if self.sizes.phases.0 % 2 == 1 {
            // The last phase wrote `second`; leave the result in `first` for the digest.
            copy(&self.second, &self.first);
        }
        Ok(())
    }

    /// Even phases read `first` and write `second`, odd phases the other way round.
    fn phase(&self, scheduler: &dyn IScheduler, phase: u32) -> Result<(), JobError> {
        let (source, target) = if phase.is_multiple_of(2) {
            (&self.first, &self.second)
        } else {
            (&self.second, &self.first)
        };
        scheduler.parallel_for(self.sizes.phases.1, PHASE_GRAIN, &|range| {
            phase_chunk(source, target, range);
        })
    }
}

fn pick(first: bool, a: u32, b: u32) -> u32 {
    if first { a } else { b }
}

fn store_burn(slots: &[AtomicU64], index: u32, iters: u32) {
    slots[index as usize].store(burn(u64::from(index), iters), Ordering::Relaxed);
}

fn bump(slot: &AtomicU64, index: u32) {
    let next = burn(slot.load(Ordering::Relaxed) + u64::from(index), 1);
    slot.store(next, Ordering::Relaxed);
}

fn copy(from: &[AtomicU64], to: &[AtomicU64]) {
    for (from, to) in from.iter().zip(to) {
        to.store(from.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}

/// One chunk of a phase: each slot becomes a function of itself and its right neighbour in the
/// previous phase's buffer.
fn phase_chunk(source: &[AtomicU64], target: &[AtomicU64], range: std::ops::Range<u32>) {
    for index in range.map(|index| index as usize) {
        let here = source[index].load(Ordering::Relaxed);
        let next = source[(index + 1) % source.len()].load(Ordering::Relaxed);
        target[index].store(burn(here ^ next, PHASE_ITERS), Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jobs::inline::InlineScheduler;

    #[test]
    fn every_workload_gives_the_same_hash_twice_with_the_reference() {
        for workload in Workload::ALL {
            let bench = Bench::new(workload, Sizes::SMALL);
            let (first, second) = (bench.run(&InlineScheduler), bench.run(&InlineScheduler));
            assert!(first.is_ok());
            assert_eq!(first, second, "{}", workload.name());
        }
    }

    fn hash_of(workload: Workload) -> Option<u64> {
        Bench::new(workload, Sizes::SMALL)
            .run(&InlineScheduler)
            .ok()
    }

    #[test]
    fn different_workloads_give_different_hashes() {
        let hashes: Vec<_> = Workload::ALL
            .into_iter()
            .filter(|workload| *workload != Workload::TinyCoarse)
            .map(hash_of)
            .collect();
        let mut unique = hashes.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), hashes.len());
    }

    #[test]
    fn the_chunk_size_does_not_change_the_result() {
        assert_eq!(hash_of(Workload::Tiny), hash_of(Workload::TinyCoarse));
    }

    #[test]
    fn names_parse_back() {
        for workload in Workload::ALL {
            assert_eq!(Workload::parse(workload.name()), Some(workload));
        }
        assert_eq!(Workload::parse("nope"), None);
    }
}
