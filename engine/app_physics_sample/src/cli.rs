//! What the program says and which exit code it returns, decided apart from the window so it can be
//! tested without a device: `execute` parses the arguments, runs the sample through the function it
//! is given, and turns the result into an [`Outcome`]. The exit codes and the error type are those
//! of `app_sprite_sample`.

use crate::args::{Args, USAGE};
pub use app_sprite_sample::cli::{EXIT_FAILED, EXIT_NO_DEVICE, EXIT_OK, EXIT_USAGE, RunError};

/// What a finished run reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// Frames drawn and presented.
    pub frames: u32,
    /// The hash of the physics state at the end: equal for equal runs.
    pub state_hash: u64,
}

/// The exit code and the line to print: on standard output when the code is [`EXIT_OK`], else on
/// standard error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// The process exit code.
    pub code: u8,
    /// The message.
    pub message: String,
}

fn usage_error(error: &crate::args::ArgsError) -> Outcome {
    Outcome {
        code: EXIT_USAGE,
        message: format!("error: {error}\n{USAGE}"),
    }
}

/// Runs the program: parses `args`, calls `run` with them, and maps the result to an [`Outcome`].
/// `run` is not called when the arguments are refused.
pub fn execute<I, F>(args: I, run: F) -> Outcome
where
    I: IntoIterator<Item = String>,
    F: FnOnce(&Args) -> Result<Report, RunError>,
{
    let args = match Args::parse(args) {
        Ok(args) => args,
        Err(error) => return usage_error(&error),
    };
    match run(&args) {
        Ok(report) => Outcome {
            code: EXIT_OK,
            message: format!(
                "drew {} frames, state hash {:016x}",
                report.frames, report.state_hash
            ),
        },
        Err(error) => Outcome {
            code: match error {
                RunError::NoDevice(_) => EXIT_NO_DEVICE,
                RunError::Failed(_) => EXIT_FAILED,
            },
            message: error.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_owned()).collect()
    }

    #[test]
    fn a_finished_run_exits_zero_and_reports_frames_and_hash() {
        let outcome = execute(words(&["--frames", "3"]), |args| {
            assert_eq!(args.frames, Some(3));
            Ok(Report {
                frames: 3,
                state_hash: 0xabc,
            })
        });
        assert_eq!(
            outcome,
            Outcome {
                code: EXIT_OK,
                message: "drew 3 frames, state hash 0000000000000abc".to_owned()
            }
        );
    }

    #[test]
    fn a_missing_device_has_its_own_code_and_a_clear_message() {
        let outcome = execute(words(&[]), |_| {
            Err(RunError::NoDevice("GLFW: not found".to_owned()))
        });
        assert_eq!(outcome.code, EXIT_NO_DEVICE);
        assert_eq!(
            outcome.message,
            "not run: device not set up (GLFW: not found)"
        );
    }

    #[test]
    fn a_failure_exits_one_and_names_the_cause() {
        let outcome = execute(words(&[]), |_| {
            Err(RunError::Failed("GL error 0x502".to_owned()))
        });
        assert_eq!(outcome.code, EXIT_FAILED);
        assert_eq!(outcome.message, "error: GL error 0x502");
    }

    fn refused(args: &[&str]) -> Outcome {
        execute(words(args), |_| {
            Err(RunError::Failed("the sample must not run".to_owned()))
        })
    }

    #[test]
    fn bad_arguments_exit_two_with_the_usage_and_never_run() {
        for bad in [&["--frames", "0"][..], &["--null"][..]] {
            let outcome = refused(bad);
            assert_eq!(outcome.code, EXIT_USAGE);
            assert!(
                outcome.message.starts_with("error: "),
                "{}",
                outcome.message
            );
            assert!(outcome.message.ends_with(USAGE), "{}", outcome.message);
        }
    }
}
