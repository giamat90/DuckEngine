//! The command line of the examples: `app_sprite_sample [--frames N] [--backend gl|vulkan]` and
//! `app_input_sample` (same options).

use std::fmt;

/// Largest `--frames` accepted: a sample run stays short and the count stays well inside `u32`.
pub const MAX_FRAMES: u32 = 1_000_000;
/// Frames of the triangle before the sprites start, when the run has no fixed length.
pub const TRIANGLE_FRAMES: u32 = 60;

/// The graphics API the sample draws with; chosen here and in `app.rs` only (the composition root).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Backend {
    /// OpenGL 4.5 (the default).
    #[default]
    Gl,
    /// Vulkan 1.3.
    Vulkan,
}

/// What the command line asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Args {
    /// Frames to draw; `None` runs until the window is closed.
    pub frames: Option<u32>,
    /// The graphics API.
    pub backend: Backend,
}

/// A command line the sample refuses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArgsError {
    /// An argument the sample does not know.
    Unknown(String),
    /// `--frames` without a value.
    MissingValue,
    /// `--frames` with something other than a whole number from 1 to [`MAX_FRAMES`].
    BadValue(String),
    /// `--backend` with a name other than `gl` or `vulkan`.
    BadBackend(String),
}

impl fmt::Display for ArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(arg) => write!(f, "unknown argument `{arg}`"),
            Self::MissingValue => write!(f, "`--frames` needs a number, `--backend` a name"),
            Self::BadValue(value) => {
                write!(
                    f,
                    "`--frames {value}`: use a whole number from 1 to {MAX_FRAMES}"
                )
            }
            Self::BadBackend(name) => {
                write!(f, "`--backend {name}`: use `gl` or `vulkan`")
            }
        }
    }
}

impl std::error::Error for ArgsError {}

/// The usage line, printed after an argument error.
pub const USAGE: &str = "usage: app_sprite_sample [--frames N] [--backend gl|vulkan]  (app_input_sample takes the same)";

/// The value of `--frames`: a whole number in range.
fn frames_value(value: String) -> Result<u32, ArgsError> {
    match value.parse::<u32>() {
        Ok(n) if (1..=MAX_FRAMES).contains(&n) => Ok(n),
        _ => Err(ArgsError::BadValue(value)),
    }
}

/// The value of `--backend`: `gl` or `vulkan`.
fn backend_value(value: String) -> Result<Backend, ArgsError> {
    match value.as_str() {
        "gl" => Ok(Backend::Gl),
        "vulkan" => Ok(Backend::Vulkan),
        _ => Err(ArgsError::BadBackend(value)),
    }
}

impl Args {
    /// Parses the arguments after the program name.
    ///
    /// # Errors
    /// [`ArgsError`] for an unknown argument, a missing value, a bad `--frames` value or an unknown `--backend`.
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Self, ArgsError> {
        let mut parsed = Self {
            frames: None,
            backend: Backend::default(),
        };
        let mut args = args.into_iter();
        while let Some(flag) = args.next() {
            let value = |value: Option<String>| value.ok_or(ArgsError::MissingValue);
            match flag.as_str() {
                "--frames" => parsed.frames = Some(frames_value(value(args.next())?)?),
                "--backend" => parsed.backend = backend_value(value(args.next())?)?,
                _ => return Err(ArgsError::Unknown(flag)),
            }
        }
        Ok(parsed)
    }

    /// Frames that show the triangle before the sprites: [`TRIANGLE_FRAMES`] in an open-ended
    /// run, else a third of the run (so a short run still shows both).
    #[must_use]
    pub fn triangle_frames(&self) -> u32 {
        self.frames
            .map_or(TRIANGLE_FRAMES, |n| (n / 3).clamp(1, TRIANGLE_FRAMES))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, ArgsError> {
        Args::parse(args.iter().map(|a| (*a).to_owned()))
    }

    #[test]
    fn no_arguments_run_until_the_window_closes() {
        assert_eq!(
            parse(&[]),
            Ok(Args {
                frames: None,
                backend: Backend::Gl
            })
        );
    }

    #[test]
    fn frames_sets_the_length() {
        assert_eq!(
            parse(&["--frames", "5"]),
            Ok(Args {
                frames: Some(5),
                backend: Backend::Gl
            })
        );
    }

    #[test]
    fn the_last_frames_wins() {
        assert_eq!(
            parse(&["--frames", "5", "--frames", "7"]),
            Ok(Args {
                frames: Some(7),
                backend: Backend::Gl
            })
        );
    }

    #[test]
    fn the_limits_are_one_and_the_maximum() {
        assert_eq!(
            parse(&["--frames", "1"]),
            Ok(Args {
                frames: Some(1),
                backend: Backend::Gl
            })
        );
        assert_eq!(
            parse(&["--frames", "1000000"]),
            Ok(Args {
                frames: Some(MAX_FRAMES),
                backend: Backend::Gl
            })
        );
        assert_eq!(
            parse(&["--frames", "0"]),
            Err(ArgsError::BadValue("0".to_owned()))
        );
        assert_eq!(
            parse(&["--frames", "1000001"]),
            Err(ArgsError::BadValue("1000001".to_owned()))
        );
    }

    #[test]
    fn a_value_that_is_not_a_number_is_refused() {
        assert_eq!(
            parse(&["--frames", "ten"]),
            Err(ArgsError::BadValue("ten".to_owned()))
        );
        assert_eq!(
            parse(&["--frames", "-3"]),
            Err(ArgsError::BadValue("-3".to_owned()))
        );
    }

    #[test]
    fn a_missing_value_is_refused() {
        assert_eq!(parse(&["--frames"]), Err(ArgsError::MissingValue));
    }

    #[test]
    fn an_unknown_argument_is_refused() {
        assert_eq!(
            parse(&["--fast"]),
            Err(ArgsError::Unknown("--fast".to_owned()))
        );
        assert_eq!(parse(&["5"]), Err(ArgsError::Unknown("5".to_owned())));
    }

    #[test]
    fn the_errors_say_what_is_wrong() {
        assert!(
            ArgsError::Unknown("x".to_owned())
                .to_string()
                .contains("`x`")
        );
        assert!(ArgsError::MissingValue.to_string().contains("number"));
        assert!(
            ArgsError::BadValue("0".to_owned())
                .to_string()
                .contains("1000000")
        );
    }

    #[test]
    fn the_backend_is_opengl_unless_vulkan_is_asked_for() {
        assert_eq!(parse(&[]).map(|a| a.backend), Ok(Backend::Gl));
        assert_eq!(
            parse(&["--backend", "gl"]).map(|a| a.backend),
            Ok(Backend::Gl)
        );
        assert_eq!(
            parse(&["--backend", "vulkan"]),
            Ok(Args {
                frames: None,
                backend: Backend::Vulkan
            })
        );
    }

    #[test]
    fn the_backend_and_the_frames_go_in_any_order() {
        let expected = Ok(Args {
            frames: Some(4),
            backend: Backend::Vulkan,
        });
        assert_eq!(parse(&["--backend", "vulkan", "--frames", "4"]), expected);
        assert_eq!(parse(&["--frames", "4", "--backend", "vulkan"]), expected);
    }

    #[test]
    fn an_unknown_backend_or_a_missing_name_is_refused() {
        assert_eq!(
            parse(&["--backend", "metal"]),
            Err(ArgsError::BadBackend("metal".to_owned()))
        );
        assert_eq!(parse(&["--backend"]), Err(ArgsError::MissingValue));
        assert!(
            ArgsError::BadBackend("x".to_owned())
                .to_string()
                .contains("vulkan")
        );
    }

    #[test]
    fn the_triangle_gets_a_third_of_a_short_run_and_at_most_sixty_frames() {
        let frames = |n| {
            Args {
                frames: Some(n),
                backend: Backend::Gl,
            }
            .triangle_frames()
        };
        assert_eq!(frames(1), 1);
        assert_eq!(frames(3), 1);
        assert_eq!(frames(9), 3);
        assert_eq!(frames(180), 60);
        assert_eq!(frames(1_000_000), 60);
        assert_eq!(
            Args {
                frames: None,
                backend: Backend::Gl
            }
            .triangle_frames(),
            60
        );
    }
}
