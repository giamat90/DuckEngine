//! Tests of the floating-point control state helper.
//!
//! Every test that changes the register runs on its own spawned thread, so the harness thread and
//! the other tests are untouched, and puts the original state back on exit even when an assertion
//! fails. Arithmetic goes through `black_box` so that no result is folded at compile time.
//!
//! The file attribute below tells the architecture check that the whole file is test code (it spawns
//! threads, which only the job system, `platform` and tests may do).
#![cfg(test)]

use super::*;
use std::hint::black_box;

/// Puts the saved state back when the test body ends, normally or by a failed assertion.
struct Restore(FpEnv);

impl Drop for Restore {
    fn drop(&mut self) {
        let _restored = self.0.apply();
    }
}

/// Runs `body` on a new thread and fails the test if the body fails.
fn on_fresh_thread(body: impl FnOnce() + Send + 'static) {
    let handle = std::thread::spawn(move || {
        let _restore = current().map(Restore);
        body();
    });
    assert!(handle.join().is_ok(), "the test body failed on its thread");
}

/// The deterministic state with the given change, written to the register of this thread.
fn corrupt(change: impl FnOnce(FpEnv) -> FpEnv) {
    let state = change(FpEnv::DETERMINISTIC);
    assert!(state.apply().is_ok());
}

/// Bit-exact comparison, because the tests are about exactly which bits arithmetic produces.
fn is(value: f32, expected: f32) -> bool {
    value.to_bits() == expected.to_bits()
}

fn subnormal_from_halving() -> f32 {
    black_box(f32::MIN_POSITIVE) * black_box(0.5)
}

/// The state a mismatch reports.
fn reported_mismatch() -> (FpEnv, u32) {
    match assert_deterministic() {
        Err(FpEnvError::Mismatch {
            actual,
            differing_bits,
        }) => (actual, differing_bits),
        other => panic!("expected a mismatch, got {other:?}"),
    }
}

/// Makes a guard, checks the state inside it and returns the state it will restore.
fn previous_seen_inside_guard() -> Option<FpEnv> {
    let guard = FpGuard::deterministic().ok()?;
    assert_eq!(Ok(()), assert_deterministic());
    Some(guard.previous())
}

/// What a worker spawned from a thread in a bad state sees after it sets the state first.
fn worker_that_sets_first() -> (bool, Result<(), FpEnvError>) {
    let worker = std::thread::spawn(|| (set_deterministic().is_ok(), assert_deterministic()));
    worker
        .join()
        .unwrap_or((false, Err(FpEnvError::Unsupported)))
}

/// The bits of two results that depend on the rounding mode and on flush-to-zero. Not inlined, so
/// the calls cannot be merged with the callers' own arithmetic.
#[inline(never)]
fn arithmetic_bits() -> (u32, u32) {
    (
        one_plus_tiny().to_bits(),
        subnormal_from_halving().to_bits(),
    )
}

fn one_plus_tiny() -> f32 {
    black_box(1.0_f32) + black_box(2.0_f32.powi(-30))
}

fn minus_one_minus_tiny() -> f32 {
    black_box(-1.0_f32) - black_box(2.0_f32.powi(-30))
}

#[test]
fn the_deterministic_state_has_the_documented_register_value() {
    #[cfg(target_arch = "x86_64")]
    assert_eq!(0x1F80, FpEnv::DETERMINISTIC.raw());
    #[cfg(target_arch = "aarch64")]
    assert_eq!(0, FpEnv::DETERMINISTIC.raw());
}

#[test]
fn the_deterministic_state_reads_back_as_nearest_with_denormals_kept() {
    let state = FpEnv::DETERMINISTIC;
    assert_eq!(Rounding::Nearest, state.rounding());
    assert!(!state.flush_to_zero());
    assert!(!state.denormals_are_zero());
}

#[test]
fn after_set_deterministic_a_thread_asserts_ok_and_reads_the_deterministic_state() {
    on_fresh_thread(|| {
        assert!(set_deterministic().is_ok());
        assert_eq!(Ok(()), assert_deterministic());
        assert_eq!(Ok(FpEnv::DETERMINISTIC), current());
    });
}

#[test]
fn assert_reports_flush_to_zero_and_names_the_bit() {
    on_fresh_thread(|| {
        corrupt(|state| state.with_flush_to_zero(true));
        let expected = FpEnv::DETERMINISTIC.with_flush_to_zero(true);
        assert_eq!(
            Err(FpEnvError::Mismatch {
                actual: expected,
                differing_bits: target::FLUSH_TO_ZERO
            }),
            assert_deterministic()
        );
    });
}

#[test]
fn assert_reports_a_rounding_mode_and_only_the_rounding_bits() {
    on_fresh_thread(|| {
        corrupt(|state| state.with_rounding(Rounding::TowardZero));
        let (actual, differing_bits) = reported_mismatch();
        assert_eq!(Rounding::TowardZero, actual.rounding());
        assert_eq!(
            target::TOWARD_ZERO << target::ROUNDING_SHIFT,
            differing_bits
        );
    });
}

#[cfg(target_arch = "x86_64")]
#[test]
fn assert_reports_denormals_are_zero() {
    on_fresh_thread(|| {
        corrupt(|state| state.with_denormals_are_zero(true));
        let (actual, differing_bits) = reported_mismatch();
        assert!(actual.denormals_are_zero());
        assert_eq!(target::DENORMALS_ARE_ZERO, differing_bits);
    });
}

#[test]
fn set_deterministic_returns_the_previous_state_and_fixes_it() {
    on_fresh_thread(|| {
        corrupt(|state| state.with_flush_to_zero(true).with_rounding(Rounding::Up));
        let corrupted = current().unwrap_or(FpEnv::DETERMINISTIC);
        assert_eq!(Ok(corrupted), set_deterministic());
        assert_eq!(Ok(()), assert_deterministic());
    });
}

#[test]
fn the_guard_sets_the_deterministic_state_and_restores_the_old_one_on_drop() {
    on_fresh_thread(|| {
        corrupt(|state| state.with_flush_to_zero(true));
        let corrupted = current().unwrap_or(FpEnv::DETERMINISTIC);
        assert_eq!(Some(corrupted), previous_seen_inside_guard());
        assert_eq!(Ok(corrupted), current());
    });
}

#[test]
fn a_change_on_one_thread_does_not_reach_another() {
    let before = current();
    on_fresh_thread(|| {
        corrupt(|state| state.with_flush_to_zero(true).with_rounding(Rounding::Down));
    });
    assert_eq!(before, current());
}

#[test]
fn a_worker_that_sets_the_state_first_is_deterministic_even_if_it_inherited_a_bad_one() {
    on_fresh_thread(|| {
        corrupt(|state| state.with_rounding(Rounding::Up));
        assert_eq!((true, Ok(())), worker_that_sets_first());
    });
}

#[cfg(target_arch = "x86_64")]
#[test]
fn apply_keeps_the_bits_outside_the_control_state() {
    on_fresh_thread(|| {
        // 1/3 raises the sticky "inexact" flag, which is not part of the control state.
        let inexact = black_box(1.0_f32) / black_box(3.0_f32);
        assert!(inexact > 0.3);
        let outside = |register: u32| register & !target::CONTROL_MASK;
        let before = outside(target::read());
        assert_ne!(
            0, before,
            "the inexact flag must be set for this test to mean anything"
        );
        assert!(
            FpEnv::DETERMINISTIC
                .with_flush_to_zero(true)
                .apply()
                .is_ok()
        );
        assert_eq!(before, outside(target::read()));
    });
}

#[test]
fn the_four_rounding_modes_round_trip() {
    for mode in [
        Rounding::Nearest,
        Rounding::Down,
        Rounding::Up,
        Rounding::TowardZero,
    ] {
        assert_eq!(mode, FpEnv::DETERMINISTIC.with_rounding(mode).rounding());
    }
}

#[test]
fn changing_one_setting_keeps_the_others() {
    let state = FpEnv::DETERMINISTIC
        .with_rounding(Rounding::Up)
        .with_flush_to_zero(true);
    assert_eq!(Rounding::Up, state.rounding());
    assert!(state.flush_to_zero());
    let state = state.with_rounding(Rounding::Down);
    assert!(state.flush_to_zero());
    let state = state.with_flush_to_zero(false);
    assert_eq!(Rounding::Down, state.rounding());
    assert!(!state.flush_to_zero());
    assert_eq!(FpEnv::DETERMINISTIC, state.with_rounding(Rounding::Nearest));
}

#[test]
fn flush_to_zero_turns_a_denormal_result_into_zero() {
    on_fresh_thread(|| {
        assert!(set_deterministic().is_ok());
        let kept = subnormal_from_halving();
        assert!(
            kept > 0.0 && !kept.is_normal(),
            "denormals are kept by default"
        );
        corrupt(|state| state.with_flush_to_zero(true));
        assert!(is(subnormal_from_halving(), 0.0));
    });
}

#[cfg(target_arch = "x86_64")]
#[test]
fn denormals_are_zero_turns_a_denormal_input_into_zero() {
    on_fresh_thread(|| {
        let denormal = black_box(f32::from_bits(1));
        assert!(denormal * black_box(2.0) > 0.0);
        corrupt(|state| state.with_denormals_are_zero(true));
        assert!(is(denormal * black_box(2.0), 0.0));
    });
}

#[test]
fn the_rounding_mode_changes_the_result_of_an_inexact_sum() {
    on_fresh_thread(|| {
        assert!(set_deterministic().is_ok());
        assert!(is(one_plus_tiny(), 1.0) && is(minus_one_minus_tiny(), -1.0));
        corrupt(|state| state.with_rounding(Rounding::Up));
        assert!(one_plus_tiny() > 1.0);
        assert!(is(minus_one_minus_tiny(), -1.0));
        corrupt(|state| state.with_rounding(Rounding::Down));
        assert!(is(one_plus_tiny(), 1.0));
        assert!(minus_one_minus_tiny() < -1.0);
        corrupt(|state| state.with_rounding(Rounding::TowardZero));
        assert!(is(one_plus_tiny(), 1.0) && is(minus_one_minus_tiny(), -1.0));
    });
}

#[test]
fn the_deterministic_state_gives_the_same_bits_after_a_corrupted_run() {
    on_fresh_thread(|| {
        assert!(set_deterministic().is_ok());
        let reference = arithmetic_bits();
        corrupt(|state| state.with_rounding(Rounding::Up).with_flush_to_zero(true));
        assert_ne!(reference, arithmetic_bits());
        assert!(set_deterministic().is_ok());
        assert_eq!(reference, arithmetic_bits());
    });
}

/// Every exception unmasked (`MXCSR`: all six masks clear; `FPCR`: all trap enables set).
#[cfg(target_arch = "x86_64")]
const UNMASKED: u32 = 0;
#[cfg(target_arch = "aarch64")]
const UNMASKED: u32 = 0x9F00;

/// The bits `assert_deterministic` must report after `UNMASKED` was written and `seen` read back.
/// `MXCSR` has all six masks, so the answer is exact. The `FPCR` trap enables are optional (many
/// cores read them as zero), so there only a subset of what was written may be reported.
#[cfg(target_arch = "x86_64")]
fn bits_that_must_differ(_seen: FpEnv) -> u32 {
    UNMASKED ^ target::DETERMINISTIC
}

#[cfg(target_arch = "aarch64")]
fn bits_that_must_differ(seen: FpEnv) -> u32 {
    seen.raw() & UNMASKED
}

#[test]
fn assert_reports_unmasked_exceptions_and_names_their_bits() {
    on_fresh_thread(|| {
        // No floating-point instruction runs while the exceptions are unmasked: only the register
        // accesses and the plain-integer comparisons below sit between the write and the restore.
        assert!(FpEnv::from_raw(UNMASKED).apply().is_ok());
        let seen = current();
        let result = assert_deterministic();
        assert!(set_deterministic().is_ok());
        let seen = seen.unwrap_or(FpEnv::DETERMINISTIC);
        let expected = bits_that_must_differ(seen);
        match result {
            Ok(()) => assert_eq!(0, expected),
            Err(FpEnvError::Mismatch { differing_bits, .. }) => {
                assert_ne!(0, differing_bits);
                assert_eq!(expected, differing_bits);
            }
            Err(FpEnvError::Unsupported) => panic!("unsupported on a supported target"),
        }
    });
}

#[test]
fn setting_a_flag_that_is_already_set_keeps_it_set() {
    let on = FpEnv::DETERMINISTIC.with_flush_to_zero(true);
    assert_eq!(on, on.with_flush_to_zero(true));
    assert!(on.with_flush_to_zero(true).flush_to_zero());
    assert_eq!(
        FpEnv::DETERMINISTIC,
        FpEnv::DETERMINISTIC.with_flush_to_zero(false)
    );
}

#[test]
fn from_raw_keeps_only_the_control_bits() {
    assert_eq!(0, FpEnv::from_raw(!target::CONTROL_MASK).raw());
    assert_eq!(target::CONTROL_MASK, FpEnv::from_raw(u32::MAX).raw());
}
