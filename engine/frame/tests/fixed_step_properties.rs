//! Property and loop tests of the fixed-step accumulator, driven by the seeded `base` RNG and a
//! manual tick counter (no clock).

use base::rng::Pcg32;
use frame::FixedStep;

fn fixed(step: u64, max: u32) -> FixedStep {
    FixedStep::new(step, max).unwrap_or_else(|_| panic!("valid"))
}

fn random_ticks(dice: &mut Pcg32, step: u64) -> u64 {
    let spread = u32::try_from(step * 12);
    assert!(spread.is_ok(), "the step is small enough for a u32 spread");
    let drawn = dice.below(spread.unwrap_or(1));
    assert!(drawn.is_ok(), "the spread is not zero");
    u64::from(drawn.unwrap_or(0))
}

#[test]
fn time_is_conserved_over_a_random_run() {
    for seed in 0..20_u64 {
        let mut dice = Pcg32::new(seed, 3);
        let mut f = fixed(1 + seed * 5, 4);
        let (mut fed, mut used, mut dropped) = (0_u128, 0_u128, 0_u128);
        for _ in 0..2_000 {
            let frame = random_ticks(&mut dice, f.step_ticks());
            let s = f.advance(frame);
            assert!(s.count <= f.max_steps());
            assert!(f.remainder() < f.step_ticks());
            fed += u128::from(frame);
            used += u128::from(s.count) * u128::from(f.step_ticks());
            dropped += s.dropped;
        }
        assert_eq!(fed, used + dropped + u128::from(f.remainder()));
    }
}

#[test]
fn the_same_seed_gives_the_same_steps() {
    let run = || {
        let mut dice = Pcg32::new(42, 1);
        let mut f = fixed(16, 3);
        (0..500)
            .map(|_| f.advance(random_ticks(&mut dice, 16)))
            .collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}

#[test]
fn a_headless_loop_on_a_manual_counter_runs_the_expected_steps() {
    const STEP: u64 = 16_666_667;
    let mut f = fixed(STEP, 5);
    let (mut now, mut last, mut sim_ticks) = (0_u64, 0_u64, 0_u64);
    let mut dice = Pcg32::new(7, 2);
    for _ in 0..600 {
        now += STEP + u64::from(dice.below(1_000).unwrap_or(0));
        let s = f.advance(now - last);
        last = now;
        sim_ticks += u64::from(s.count) * STEP;
        assert_eq!(s.dropped, 0);
    }
    assert_eq!(sim_ticks + f.remainder(), now);
}

#[test]
fn a_stall_in_the_loop_is_capped_and_then_the_loop_recovers() {
    let mut f = fixed(10, 4);
    let stalled = f.advance(10_000);
    assert_eq!(stalled.count, 4);
    assert_eq!(stalled.dropped, 10_000 - 40);
    let after = f.advance(10);
    assert_eq!((after.count, after.dropped), (1, 0));
}
