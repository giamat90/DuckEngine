//! The command line of the examples: `app_sprite_sample [--frames N]` and `app_input_sample [--frames N]`.

use std::fmt;

/// Largest `--frames` accepted: a sample run stays short and the count stays well inside `u32`.
pub const MAX_FRAMES: u32 = 1_000_000;
/// Frames of the triangle before the sprites start, when the run has no fixed length.
pub const TRIANGLE_FRAMES: u32 = 60;

/// What the command line asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Args {
    /// Frames to draw; `None` runs until the window is closed.
    pub frames: Option<u32>,
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
}

impl fmt::Display for ArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(arg) => write!(f, "unknown argument `{arg}`"),
            Self::MissingValue => write!(f, "`--frames` needs a number"),
            Self::BadValue(value) => {
                write!(
                    f,
                    "`--frames {value}`: use a whole number from 1 to {MAX_FRAMES}"
                )
            }
        }
    }
}

impl std::error::Error for ArgsError {}

/// The usage line, printed after an argument error.
pub const USAGE: &str = "usage: app_sprite_sample [--frames N]  (app_input_sample takes the same)";

/// One `--frames N` pair: `flag` must be `--frames` and `value` a whole number in range.
fn frames_option(flag: &str, value: Option<String>) -> Result<u32, ArgsError> {
    if flag != "--frames" {
        return Err(ArgsError::Unknown(flag.to_owned()));
    }
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
    /// [`ArgsError`] for an unknown argument or a missing or bad `--frames` value.
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Self, ArgsError> {
        let mut frames = None;
        let mut args = args.into_iter();
        while let Some(flag) = args.next() {
            frames = Some(frames_option(&flag, args.next())?);
        }
        Ok(Self { frames })
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
        assert_eq!(parse(&[]), Ok(Args { frames: None }));
    }

    #[test]
    fn frames_sets_the_length() {
        assert_eq!(parse(&["--frames", "5"]), Ok(Args { frames: Some(5) }));
    }

    #[test]
    fn the_last_frames_wins() {
        assert_eq!(
            parse(&["--frames", "5", "--frames", "7"]),
            Ok(Args { frames: Some(7) })
        );
    }

    #[test]
    fn the_limits_are_one_and_the_maximum() {
        assert_eq!(parse(&["--frames", "1"]), Ok(Args { frames: Some(1) }));
        assert_eq!(
            parse(&["--frames", "1000000"]),
            Ok(Args {
                frames: Some(MAX_FRAMES)
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
    fn the_triangle_gets_a_third_of_a_short_run_and_at_most_sixty_frames() {
        let frames = |n| Args { frames: Some(n) }.triangle_frames();
        assert_eq!(frames(1), 1);
        assert_eq!(frames(3), 1);
        assert_eq!(frames(9), 3);
        assert_eq!(frames(180), 60);
        assert_eq!(frames(1_000_000), 60);
        assert_eq!(Args { frames: None }.triangle_frames(), 60);
    }
}
