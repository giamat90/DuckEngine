//! Fixed-step accumulator: turns variable frame times into a whole number of fixed simulation steps.
//!
//! All arithmetic is on integer ticks (`u64`, for example nanoseconds), so there is no float drift.
//! [`FixedStep::advance`] is a pure function of the accumulator state and its argument: it reads no
//! clock, makes no allocation and cannot panic, so a replay gives the same steps on every run and at
//! every thread count.
//!
//! **Alpha** is the leftover fraction of a step for render interpolation, as a fixed-point
//! number in `0..2^32`: `alpha = remainder * 2^32 / step_ticks`, rounded down. `alpha / 2^32` is
//! the fraction in `[0, 1)`.
//!
//! **Stall cap.** After a long stall at most `max_steps` steps run. The time of the steps that
//! were not run is dropped and reported in [`Steps::dropped`]; it is never run later and never lost
//! without a report.
//!
//! **Conservation.** For every call, `old_remainder + frame_ticks == count * step_ticks +
//! dropped + new_remainder`, and the remainder is always below `step_ticks`.

/// Why a [`FixedStep`] could not be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixedStepError {
    /// `step_ticks` was zero: a step must last at least one tick.
    ZeroStep,
    /// `max_steps` was zero: at least one step per frame must be allowed.
    ZeroMaxSteps,
}

/// What one call of [`FixedStep::advance`] decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Steps {
    /// Number of fixed steps to run now, at most `max_steps`.
    pub count: u32,
    /// Leftover fraction of a step in `0..2^32` (see the module documentation).
    pub alpha: u32,
    /// Ticks of whole steps that were cut by the cap and will not be run.
    pub dropped: u128,
}

/// Accumulator that hands out fixed steps; see the module documentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedStep {
    step_ticks: u64,
    max_steps: u32,
    remainder: u64,
}

impl FixedStep {
    /// An accumulator with steps of `step_ticks` ticks and at most `max_steps` steps per call.
    ///
    /// # Errors
    /// [`FixedStepError::ZeroStep`] when `step_ticks` is zero, [`FixedStepError::ZeroMaxSteps`]
    /// when `max_steps` is zero.
    pub fn new(step_ticks: u64, max_steps: u32) -> Result<Self, FixedStepError> {
        if step_ticks == 0 {
            return Err(FixedStepError::ZeroStep);
        }
        if max_steps == 0 {
            return Err(FixedStepError::ZeroMaxSteps);
        }
        Ok(Self {
            step_ticks,
            max_steps,
            remainder: 0,
        })
    }

    /// Length of one step in ticks.
    #[must_use]
    pub fn step_ticks(&self) -> u64 {
        self.step_ticks
    }

    /// Most steps one call may return.
    #[must_use]
    pub fn max_steps(&self) -> u32 {
        self.max_steps
    }

    /// Ticks carried over to the next call, always below `step_ticks`.
    #[must_use]
    pub fn remainder(&self) -> u64 {
        self.remainder
    }

    /// Adds `frame_ticks` and returns the steps to run now.
    pub fn advance(&mut self, frame_ticks: u64) -> Steps {
        let step = u128::from(self.step_ticks);
        let total = u128::from(self.remainder) + u128::from(frame_ticks);
        let whole = total / step;
        let kept = whole.min(u128::from(self.max_steps));
        // Both narrowings cannot fail: the remainder is below `step_ticks` (a `u64`) and `kept`
        // is at most `max_steps` (a `u32`); the fallbacks only keep this function total.
        self.remainder = u64::try_from(total % step).unwrap_or(0);
        Steps {
            count: u32::try_from(kept).unwrap_or(self.max_steps),
            alpha: self.alpha(),
            dropped: (whole - kept) * step,
        }
    }

    /// The remainder as a fraction of a step, in `0..2^32`.
    fn alpha(&self) -> u32 {
        let scaled = (u128::from(self.remainder) << u32::BITS) / u128::from(self.step_ticks);
        u32::try_from(scaled).unwrap_or(u32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed(step: u64, max: u32) -> FixedStep {
        FixedStep::new(step, max).unwrap_or_else(|_| panic!("valid"))
    }

    #[test]
    fn a_zero_step_is_refused() {
        assert_eq!(FixedStep::new(0, 5), Err(FixedStepError::ZeroStep));
    }

    #[test]
    fn zero_max_steps_is_refused() {
        assert_eq!(FixedStep::new(10, 0), Err(FixedStepError::ZeroMaxSteps));
    }

    #[test]
    fn a_new_accumulator_reports_its_settings_and_is_empty() {
        let f = fixed(16, 4);
        assert_eq!((f.step_ticks(), f.max_steps(), f.remainder()), (16, 4, 0));
    }

    #[test]
    fn less_than_a_step_gives_no_step_and_is_carried() {
        let mut f = fixed(10, 4);
        let s = f.advance(7);
        assert_eq!((s.count, s.dropped), (0, 0));
        assert_eq!(f.remainder(), 7);
    }

    #[test]
    fn whole_steps_are_counted_and_the_rest_is_carried() {
        let mut f = fixed(10, 4);
        let s = f.advance(35);
        assert_eq!((s.count, s.dropped, f.remainder()), (3, 0, 5));
    }

    #[test]
    fn carried_time_adds_to_the_next_frame() {
        let mut f = fixed(10, 4);
        f.advance(7);
        assert_eq!(f.advance(7).count, 1);
        assert_eq!(f.remainder(), 4);
    }

    #[test]
    fn alpha_is_the_leftover_fraction_in_32_bit_fixed_point() {
        let mut f = fixed(10, 4);
        assert_eq!(f.advance(5).alpha, 1 << 31);
        assert_eq!(f.advance(0).alpha, 1 << 31);
        let mut g = fixed(10, 4);
        assert_eq!(g.advance(10).alpha, 0);
        assert_eq!(g.advance(3).alpha, 1_288_490_188);
    }

    #[test]
    fn alpha_stays_below_one_for_a_huge_step_and_after_a_cap() {
        let mut f = fixed(u64::MAX, 1);
        let s = f.advance(u64::MAX - 1);
        assert_eq!((s.count, f.remainder()), (0, u64::MAX - 1));
        assert_eq!(s.alpha, u32::MAX);
        let mut g = fixed(10, 2);
        let s = g.advance(399);
        assert_eq!((s.count, g.remainder()), (2, 9));
        assert_eq!(s.alpha, 3_865_470_566);
    }

    #[test]
    fn a_long_stall_is_capped_and_the_cut_time_is_reported() {
        let mut f = fixed(10, 3);
        let s = f.advance(1_000 + 4);
        assert_eq!(s.count, 3);
        assert_eq!(s.dropped, 970);
        assert_eq!(f.remainder(), 4);
    }

    #[test]
    fn exactly_max_steps_drops_nothing() {
        let mut f = fixed(10, 3);
        let s = f.advance(30);
        assert_eq!((s.count, s.dropped, f.remainder()), (3, 0, 0));
    }

    #[test]
    fn one_step_over_the_cap_drops_exactly_that_step() {
        let mut f = fixed(10, 3);
        let s = f.advance(40);
        assert_eq!((s.count, s.dropped), (3, 10));
    }

    #[test]
    fn the_largest_input_does_not_overflow() {
        let mut f = fixed(7, 2);
        f.advance(6);
        let s = f.advance(u64::MAX);
        assert_eq!(s.count, 2);
        let total = u128::from(u64::MAX) + 6;
        assert_eq!(total, 2 * 7 + s.dropped + u128::from(f.remainder()));
    }

    #[test]
    fn a_one_tick_step_has_no_remainder_and_zero_alpha() {
        let mut f = fixed(1, 8);
        let s = f.advance(5);
        assert_eq!((s.count, s.alpha, f.remainder()), (5, 0, 0));
    }

    #[test]
    fn the_same_inputs_give_the_same_steps() {
        let mut a = fixed(7, 3);
        let mut b = fixed(7, 3);
        for t in [3, 9, 0, 50, 7, 1] {
            assert_eq!(a.advance(t), b.advance(t));
        }
        assert_eq!(a, b);
    }
}
