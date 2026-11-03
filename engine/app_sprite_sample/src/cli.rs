//! What the program says and which exit code it returns, decided apart from the window so it can
//! be tested without a device: `execute` parses the arguments, runs the sample through the
//! function it is given, and turns the result into an [`Outcome`].

use crate::args::{Args, USAGE};
use std::fmt;

/// The run drew every frame it was asked for, or the window was closed.
pub const EXIT_OK: u8 = 0;
/// The run started and then failed.
pub const EXIT_FAILED: u8 = 1;
/// The command line was refused.
pub const EXIT_USAGE: u8 = 2;
/// There is no window system or no OpenGL 4.5 device here, so nothing was run.
pub const EXIT_NO_DEVICE: u8 = 3;

/// What a finished run reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunReport {
    /// Frames drawn and presented.
    pub frames: u32,
}

/// Why a run did not finish.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunError {
    /// GLFW, the window or the OpenGL 4.5 device is not available; the text says which.
    NoDevice(String),
    /// The run started and a step failed.
    Failed(String),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice(reason) => write!(f, "not run: device not set up ({reason})"),
            Self::Failed(reason) => write!(f, "error: {reason}"),
        }
    }
}

impl std::error::Error for RunError {}

/// The exit code and the line to print: on standard output when the code is [`EXIT_OK`], else on
/// standard error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// The process exit code.
    pub code: u8,
    /// The message.
    pub message: String,
}

/// Runs the program: parses `args`, calls `run` with them, and maps the result to an [`Outcome`].
/// `run` is not called when the arguments are refused.
pub fn execute<I, F>(args: I, run: F) -> Outcome
where
    I: IntoIterator<Item = String>,
    F: FnOnce(&Args) -> Result<RunReport, RunError>,
{
    let args = match Args::parse(args) {
        Ok(args) => args,
        Err(error) => {
            return Outcome {
                code: EXIT_USAGE,
                message: format!("error: {error}\n{USAGE}"),
            };
        }
    };
    match run(&args) {
        Ok(report) => Outcome {
            code: EXIT_OK,
            message: format!("drew {} frames", report.frames),
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
    fn a_finished_run_exits_zero_and_says_how_many_frames() {
        let outcome = execute(words(&["--frames", "3"]), |args| {
            assert_eq!(args.frames, Some(3));
            Ok(RunReport { frames: 3 })
        });
        assert_eq!(
            outcome,
            Outcome {
                code: EXIT_OK,
                message: "drew 3 frames".to_owned()
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

    #[test]
    fn bad_arguments_exit_two_with_the_usage_and_never_run() {
        let outcome = execute(words(&["--frames", "0"]), |_| {
            panic!("the sample must not run with a refused command line")
        });
        assert_eq!(outcome.code, EXIT_USAGE);
        assert!(
            outcome.message.starts_with("error: "),
            "{}",
            outcome.message
        );
        assert!(outcome.message.ends_with(USAGE), "{}", outcome.message);
    }

    #[test]
    fn the_exit_codes_differ() {
        let codes = [EXIT_OK, EXIT_FAILED, EXIT_USAGE, EXIT_NO_DEVICE];
        for (i, a) in codes.iter().enumerate() {
            assert!(codes[i + 1..].iter().all(|b| a != b));
        }
    }
}
