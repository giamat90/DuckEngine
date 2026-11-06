//! Work counters: how much work a scene made the engine do, summed over its steps. They are
//! counts, not times, so they do not vary with the machine (the deterministic proxy of
//! `wiki/architecture/physics-and-jobs-v0.md` section 7) and tests can pin them.

use physics::d2::World;

/// Work done over a run, read from the world after each step.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkCounters {
    /// Steps taken.
    pub steps: u64,
    /// Solver sub-steps (steps times the configured sub-steps per step).
    pub sub_steps: u64,
    /// Overlapping pairs the broadphase produced, summed over the steps (each is tested by the
    /// narrowphase).
    pub pairs: u64,
    /// Manifolds the narrowphase built, summed over the steps.
    pub manifolds: u64,
    /// Constraints the solver solved, summed over the steps (those with an awake dynamic body).
    pub constraints_solved: u64,
    /// Constraints left alone because every body of them slept or was static.
    pub constraints_frozen: u64,
    /// The most colours any step used.
    pub colours_max: u64,
    /// Colours used, summed over the steps (divide by `steps` for the mean).
    pub colours_sum: u64,
    /// Constraints that found no colour and were solved serially, summed over the steps.
    pub overflow: u64,
    /// Awake dynamic bodies, summed over the steps (divide by `steps` for the mean).
    pub bodies_awake: u64,
}

/// A count as a `u64`; a count that does not fit saturates (it cannot, the capacities are far
/// below it).
fn wide(count: usize) -> u64 {
    u64::try_from(count).unwrap_or(u64::MAX)
}

/// A count as an `f64`, exact up to `u32::MAX` (the sums of a run are far below it; a larger one
/// saturates, which only affects a printed mean).
fn as_f64(count: u64) -> f64 {
    f64::from(u32::try_from(count).unwrap_or(u32::MAX))
}

impl WorkCounters {
    /// Adds the work of the step the world has just finished; `sub_steps` is the solver's
    /// sub-steps per step.
    pub fn record(&mut self, world: &World, sub_steps: u32) {
        let stats = world.solver_stats();
        self.steps += 1;
        self.sub_steps += u64::from(sub_steps);
        self.pairs += wide(world.pairs().len());
        self.manifolds += wide(world.manifolds().len());
        self.constraints_solved += wide(stats.solved);
        self.constraints_frozen += wide(stats.frozen);
        self.colours_max = self.colours_max.max(wide(stats.colours));
        self.colours_sum += wide(stats.colours);
        self.overflow += wide(stats.overflow);
        self.bodies_awake += wide(awake_dynamic(world));
    }

    /// The mean per step of a sum, 0 for a run without steps.
    #[must_use]
    pub fn per_step(&self, sum: u64) -> f64 {
        if self.steps == 0 {
            return 0.0;
        }
        as_f64(sum) / as_f64(self.steps)
    }
}

/// Dynamic bodies that are not asleep.
fn awake_dynamic(world: &World) -> usize {
    world
        .iter()
        .filter(|(_, body)| body.inverse_mass() > 0.0 && !body.is_asleep())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mean_of_a_run_without_steps_is_zero() {
        assert!((WorkCounters::default().per_step(10) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_mean_divides_the_sum_by_the_steps() {
        let counters = WorkCounters {
            steps: 4,
            ..WorkCounters::default()
        };
        assert!((counters.per_step(10) - 2.5).abs() < f64::EPSILON);
    }
}
