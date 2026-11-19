//! The command line and what the program says: `execute` parses the arguments, runs the sandbox
//! through the function it is given, and turns the result into an [`Outcome`] (the exit code and
//! the line to print). Decided apart from the window, so it is tested without a device. The exit
//! codes and the error type are those of `app_sprite_sample`.

use core::fmt;

pub use app_sprite_sample::cli::{EXIT_FAILED, EXIT_NO_DEVICE, EXIT_OK, EXIT_USAGE, RunError};

/// The most frames `--frames` accepts.
pub const MAX_FRAMES: u32 = 1_000_000;
/// The usage line, printed after an argument error.
pub const USAGE: &str = "usage: app_sandbox3d [--frames N] [--null] [--vulkan]  (--null: no window, needs --frames; --vulkan: draw with Vulkan 1.3 instead of OpenGL 4.5)";

/// What the command line asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Args {
    /// Frames to draw; `None` runs until the window is closed.
    pub frames: Option<u32>,
    /// Run on the Null render device without a window.
    pub null: bool,
    /// Draw in the window with the Vulkan 1.3 device instead of the OpenGL 4.5 one.
    pub vulkan: bool,
}

/// A command line the sandbox refuses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArgsError {
    /// An argument the sandbox does not know.
    Unknown(String),
    /// `--frames` without a value.
    MissingValue,
    /// A `--frames` value that is not a whole number from 1 to [`MAX_FRAMES`].
    BadValue(String),
    /// `--null` without `--frames`.
    NullNeedsFrames,
    /// `--vulkan` together with `--null` (the Null device has no graphics API to choose).
    VulkanWithNull,
}

impl fmt::Display for ArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(flag) => write!(f, "unknown argument `{flag}`"),
            Self::MissingValue => f.write_str("`--frames` needs a value"),
            Self::BadValue(value) => {
                write!(
                    f,
                    "`--frames {value}`: a whole number from 1 to {MAX_FRAMES}"
                )
            }
            Self::NullNeedsFrames => f.write_str("`--null` needs `--frames N`"),
            Self::VulkanWithNull => f.write_str("`--vulkan` cannot be combined with `--null`"),
        }
    }
}

impl std::error::Error for ArgsError {}

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

fn frames_value(value: Option<String>) -> Result<u32, ArgsError> {
    let value = value.ok_or(ArgsError::MissingValue)?;
    match value.parse::<u32>() {
        Ok(n) if (1..=MAX_FRAMES).contains(&n) => Ok(n),
        _ => Err(ArgsError::BadValue(value)),
    }
}

impl Args {
    /// Parses the arguments after the program name.
    ///
    /// # Errors
    /// [`ArgsError`] for an unknown argument, a missing or bad `--frames` value, `--null`
    /// without `--frames`, or `--null` together with `--vulkan`.
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Self, ArgsError> {
        let mut parsed = Self::default();
        let mut args = args.into_iter();
        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--frames" => parsed.frames = Some(frames_value(args.next())?),
                "--null" => parsed.null = true,
                "--vulkan" => parsed.vulkan = true,
                _ => return Err(ArgsError::Unknown(flag)),
            }
        }
        if parsed.null && parsed.frames.is_none() {
            return Err(ArgsError::NullNeedsFrames);
        }
        if parsed.null && parsed.vulkan {
            return Err(ArgsError::VulkanWithNull);
        }
        Ok(parsed)
    }
}

/// Parses `args`, calls `run` with them, and maps the result to an [`Outcome`]. `run` is not
/// called when the arguments are refused.
pub fn execute<I, F>(args: I, run: F) -> Outcome
where
    I: IntoIterator<Item = String>,
    F: FnOnce(&Args) -> Result<Report, RunError>,
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
    fn the_arguments_are_read() {
        assert_eq!(Args::parse(words(&[])), Ok(Args::default()));
        let both = Args::parse(words(&["--null", "--frames", "5"]));
        assert_eq!(
            both,
            Ok(Args {
                frames: Some(5),
                null: true,
                vulkan: false
            })
        );
        let top = Args::parse(words(&["--frames", "1000000"]));
        assert_eq!(top.map(|a| a.frames), Ok(Some(MAX_FRAMES)));
    }

    #[test]
    fn vulkan_is_chosen_by_its_flag_and_opengl_is_the_default() {
        assert!(!Args::default().vulkan);
        let vulkan = Args::parse(words(&["--vulkan", "--frames", "7"]));
        assert_eq!(
            vulkan,
            Ok(Args {
                frames: Some(7),
                null: false,
                vulkan: true
            })
        );
        assert_eq!(
            Args::parse(words(&["--vulkan"])).map(|a| a.vulkan),
            Ok(true)
        );
    }

    #[test]
    fn vulkan_cannot_be_asked_for_on_the_null_device() {
        let bad = Args::parse(words(&["--vulkan", "--null", "--frames", "3"]));
        assert_eq!(bad, Err(ArgsError::VulkanWithNull));
        let other_order = Args::parse(words(&["--null", "--frames", "3", "--vulkan"]));
        assert_eq!(other_order, Err(ArgsError::VulkanWithNull));
        assert!(ArgsError::VulkanWithNull.to_string().contains("--vulkan"));
    }

    #[test]
    fn the_usage_line_names_the_vulkan_flag() {
        assert!(USAGE.contains("[--vulkan]"), "{USAGE}");
    }

    #[test]
    fn bad_arguments_are_named() {
        let bad = |args: &[&str]| Args::parse(words(args));
        assert_eq!(
            bad(&["--nope"]),
            Err(ArgsError::Unknown("--nope".to_owned()))
        );
        assert_eq!(bad(&["--frames"]), Err(ArgsError::MissingValue));
        for value in ["0", "x", "-1", "1000001"] {
            assert_eq!(
                bad(&["--frames", value]),
                Err(ArgsError::BadValue(value.to_owned()))
            );
        }
        assert_eq!(bad(&["--null"]), Err(ArgsError::NullNeedsFrames));
        for error in [
            ArgsError::Unknown("a".to_owned()),
            ArgsError::MissingValue,
            ArgsError::BadValue("b".to_owned()),
            ArgsError::NullNeedsFrames,
        ] {
            assert!(!error.to_string().is_empty());
        }
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
    fn a_missing_device_and_a_failure_have_their_own_codes() {
        let none = execute(words(&[]), |_| {
            Err(RunError::NoDevice("GLFW: not found".to_owned()))
        });
        assert_eq!(none.code, EXIT_NO_DEVICE);
        assert_eq!(none.message, "not run: device not set up (GLFW: not found)");
        let failed = execute(words(&[]), |_| {
            Err(RunError::Failed("GL error 0x502".to_owned()))
        });
        assert_eq!(failed.code, EXIT_FAILED);
        assert_eq!(failed.message, "error: GL error 0x502");
    }

    fn refused(args: &[&str]) -> Outcome {
        execute(words(args), |_| {
            Err(RunError::Failed("must not run".to_owned()))
        })
    }

    #[test]
    fn refused_arguments_exit_two_with_the_usage_and_never_run() {
        for bad in [
            &["--frames", "0"][..],
            &["--null"][..],
            &["--scene", "x"][..],
        ] {
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
