//! Assertions that report a message and stop the program.
//!
//! Engine builds use `panic = "abort"`, so a failed assertion ends the process
//! at once; there is no unwinding across module boundaries.

/// Always-on assertion: checks `condition` in every build profile.
///
/// An optional message takes a literal format string and its arguments, as `format!` does.
#[macro_export]
macro_rules! duck_assert {
    ($condition:expr $(,)?) => {
        if !$condition {
            panic!("duck_assert failed: {}", stringify!($condition));
        }
    };
    ($condition:expr, $($message:tt)+) => {
        if !$condition {
            panic!("duck_assert failed: {}", format_args!($($message)+));
        }
    };
}

/// Debug-only assertion: compiled out of release builds, for checks too costly for hot paths.
#[macro_export]
macro_rules! duck_debug_assert {
    ($($arguments:tt)*) => {
        if cfg!(debug_assertions) {
            $crate::duck_assert!($($arguments)*);
        }
    };
}

#[cfg(test)]
mod tests {
    #[test]
    #[should_panic(expected = "duck_assert failed: 1 > 2")]
    fn failed_assertion_reports_the_condition() {
        duck_assert!(1 > 2);
    }

    #[test]
    #[should_panic(expected = "pool exhausted: capacity 4")]
    fn failed_assertion_reports_the_formatted_message() {
        duck_assert!(false, "pool exhausted: capacity {}", 4);
    }

    #[test]
    fn passing_assertion_does_nothing() {
        duck_assert!(2 > 1);
        duck_assert!(true, "never shown {}", 1);
    }

    #[test]
    fn assertion_condition_is_evaluated_exactly_once() {
        let mut calls = 0;
        duck_assert!({
            calls += 1;
            true
        });
        assert_eq!(calls, 1);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "duck_assert failed: false")]
    fn debug_assertion_fires_in_debug_builds() {
        duck_debug_assert!(false);
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn debug_assertion_is_compiled_out_of_release_builds() {
        duck_debug_assert!(false);
    }
}
