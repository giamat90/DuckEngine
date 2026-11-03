//! The two loops of the application as pure functions, so they are tested without a window:
//! counting the frames actually drawn, and polling for a window that has no size yet.

use crate::cli::RunError;

/// What one turn of the main loop did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The user asked to close the window.
    Closing,
    /// Nothing was drawn (a minimised window); the turn does not count as a frame.
    Skipped,
    /// One frame was drawn and presented.
    Drawn,
}

/// Runs `step` until `limit` frames are drawn (`None`: until it reports [`Step::Closing`]) and
/// returns the frames drawn. `step` is given the index of the frame it may draw. A skipped turn is
/// not a frame, so `--frames N` always means N drawn frames.
///
/// # Errors
/// The first error of `step`.
pub fn count_frames<F>(limit: Option<u32>, mut step: F) -> Result<u32, RunError>
where
    F: FnMut(u32) -> Result<Step, RunError>,
{
    let mut drawn = 0;
    while limit.is_none_or(|n| drawn < n) {
        match step(drawn)? {
            Step::Closing => break,
            Step::Skipped => {}
            Step::Drawn => drawn += 1,
        }
    }
    Ok(drawn)
}

/// Calls `probe` up to `attempts` times until it gives a value.
///
/// # Errors
/// The first error of `probe`; `Ok(None)` when every attempt came back empty.
pub fn poll<T, F>(attempts: u32, mut probe: F) -> Result<Option<T>, RunError>
where
    F: FnMut() -> Result<Option<T>, RunError>,
{
    for _ in 0..attempts {
        if let Some(value) = probe()? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drawn for the first `last` turns, then the window closes.
    fn draws_until(turn: u32, last: u32) -> Step {
        if turn > last {
            Step::Closing
        } else {
            Step::Drawn
        }
    }

    #[test]
    fn a_limit_counts_drawn_frames_and_stops_there() {
        let mut calls = 0;
        let drawn = count_frames(Some(3), |_| {
            calls += 1;
            Ok(Step::Drawn)
        });
        assert_eq!(drawn, Ok(3));
        assert_eq!(calls, 3);
    }

    #[test]
    fn skipped_turns_do_not_count_as_frames() {
        let steps = [
            Step::Skipped,
            Step::Drawn,
            Step::Skipped,
            Step::Skipped,
            Step::Drawn,
        ];
        let mut turn = 0;
        let mut indices = Vec::new();
        let drawn = count_frames(Some(2), |index| {
            indices.push(index);
            turn += 1;
            Ok(steps[turn - 1])
        });
        assert_eq!(drawn, Ok(2));
        assert_eq!(turn, 5);
        // The frame index the step sees is the number of frames drawn so far.
        assert_eq!(indices, [0, 0, 1, 1, 1]);
    }

    #[test]
    fn without_a_limit_the_loop_runs_until_the_window_closes() {
        let mut turn = 0;
        let drawn = count_frames(None, |_| {
            turn += 1;
            Ok(draws_until(turn, 4))
        });
        assert_eq!(drawn, Ok(4));
    }

    #[test]
    fn closing_ends_a_limited_run_early() {
        let mut turn = 0;
        let drawn = count_frames(Some(10), |_| {
            turn += 1;
            Ok(draws_until(turn, 2))
        });
        assert_eq!(drawn, Ok(2));
    }

    #[test]
    fn a_step_error_ends_the_loop() {
        let drawn = count_frames(Some(5), |_| Err(RunError::Failed("boom".to_owned())));
        assert_eq!(drawn, Err(RunError::Failed("boom".to_owned())));
    }

    #[test]
    fn polling_returns_the_first_value() {
        let mut calls = 0;
        let found = poll(10, || {
            calls += 1;
            Ok((calls == 3).then_some(calls))
        });
        assert_eq!(found, Ok(Some(3)));
        assert_eq!(calls, 3);
    }

    #[test]
    fn polling_gives_up_after_the_attempts() {
        let mut calls = 0;
        let found: Result<Option<u32>, _> = poll(4, || {
            calls += 1;
            Ok(None)
        });
        assert_eq!(found, Ok(None));
        assert_eq!(calls, 4);
    }

    #[test]
    fn polling_stops_at_an_error() {
        let found: Result<Option<u32>, _> = poll(4, || Err(RunError::Failed("x".to_owned())));
        assert_eq!(found, Err(RunError::Failed("x".to_owned())));
    }
}
