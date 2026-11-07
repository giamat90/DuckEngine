//! The command line: `app_physics_sample [--frames N] [--null] [--scene stream|pyramid] [--sub-steps N]`.

use crate::simulation::{SceneKind, Setup};
use app_sprite_sample::args::MAX_FRAMES;
use physics::d2::MAX_SUB_STEPS;
use std::fmt;

/// What the command line asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// Frames to draw; `None` runs until the window is closed.
    pub frames: Option<u32>,
    /// Run on the Null render device without a window (needs `frames`): the whole loop, no GPU.
    pub null: bool,
    /// The scene to start with.
    pub scene: SceneKind,
    /// Sub-steps of the solver; `None` is the solver's default.
    pub sub_steps: Option<u32>,
}

/// A command line the sample refuses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArgsError {
    /// An argument the sample does not know.
    Unknown(String),
    /// `--frames` without a value.
    MissingValue,
    /// `--frames` with something other than a whole number from 1 to the maximum.
    BadValue(String),
    /// `--null` without `--frames`: a run without a window has nothing to close it.
    NullNeedsFrames,
    /// `--scene` without a name.
    MissingScene,
    /// `--scene` with a name that is not a scene.
    BadScene(String),
    /// `--sub-steps` without a number.
    MissingSubSteps,
    /// `--sub-steps` with a value outside 1 to the solver's maximum.
    BadSubSteps(String),
}

impl fmt::Display for ArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(arg) => write!(f, "unknown argument `{arg}`"),
            Self::MissingValue => write!(f, "`--frames` needs a number"),
            Self::BadValue(value) => write!(
                f,
                "`--frames {value}`: use a whole number from 1 to {MAX_FRAMES}"
            ),
            Self::NullNeedsFrames => write!(f, "`--null` needs `--frames N`"),
            Self::MissingScene => write!(f, "`--scene` needs `stream` or `pyramid`"),
            Self::BadScene(name) => write!(f, "`--scene {name}`: use `stream` or `pyramid`"),
            Self::MissingSubSteps => write!(f, "`--sub-steps` needs a number"),
            Self::BadSubSteps(value) => write!(
                f,
                "`--sub-steps {value}`: use a whole number from 1 to {MAX_SUB_STEPS}"
            ),
        }
    }
}

impl std::error::Error for ArgsError {}

/// The usage line, printed after an argument error.
pub const USAGE: &str = "usage: app_physics_sample [--frames N] [--null] [--scene stream|pyramid] [--sub-steps N]  (--null: no window, needs --frames)";

fn frames_value(value: Option<String>) -> Result<u32, ArgsError> {
    let value = value.ok_or(ArgsError::MissingValue)?;
    match value.parse::<u32>() {
        Ok(n) if (1..=MAX_FRAMES).contains(&n) => Ok(n),
        _ => Err(ArgsError::BadValue(value)),
    }
}

fn scene_value(value: Option<String>) -> Result<SceneKind, ArgsError> {
    match value.ok_or(ArgsError::MissingScene)?.as_str() {
        "stream" => Ok(SceneKind::Stream),
        "pyramid" => Ok(SceneKind::Pyramid),
        other => Err(ArgsError::BadScene(other.to_owned())),
    }
}

fn sub_steps_value(value: Option<String>) -> Result<u32, ArgsError> {
    let value = value.ok_or(ArgsError::MissingSubSteps)?;
    match value.parse::<u32>() {
        Ok(n) if (1..=MAX_SUB_STEPS).contains(&n) => Ok(n),
        _ => Err(ArgsError::BadSubSteps(value)),
    }
}

impl Args {
    /// The scene and solver setting the arguments ask for.
    #[must_use]
    pub fn setup(&self) -> Setup {
        let default = Setup::default();
        Setup {
            scene: self.scene,
            sub_steps: self.sub_steps.unwrap_or(default.sub_steps),
        }
    }

    /// Parses the arguments after the program name.
    ///
    /// # Errors
    /// [`ArgsError`] for an unknown argument, a missing or bad `--frames` value, or `--null`
    /// without `--frames`.
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Self, ArgsError> {
        let mut parsed = Self::default();
        let mut args = args.into_iter();
        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--frames" => parsed.frames = Some(frames_value(args.next())?),
                "--null" => parsed.null = true,
                "--scene" => parsed.scene = scene_value(args.next())?,
                "--sub-steps" => parsed.sub_steps = Some(sub_steps_value(args.next())?),
                _ => return Err(ArgsError::Unknown(flag)),
            }
        }
        if parsed.null && parsed.frames.is_none() {
            return Err(ArgsError::NullNeedsFrames);
        }
        Ok(parsed)
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
                null: false,
                scene: SceneKind::Stream,
                sub_steps: None,
            })
        );
    }

    #[test]
    fn frames_sets_the_length_and_null_picks_the_headless_run() {
        assert_eq!(
            parse(&["--frames", "5"]).map(|a| (a.frames, a.null)),
            Ok((Some(5), false))
        );
        assert_eq!(
            parse(&["--null", "--frames", "5"]).map(|a| (a.frames, a.null)),
            Ok((Some(5), true))
        );
        assert_eq!(
            parse(&["--frames", "5", "--null"]).map(|a| (a.frames, a.null)),
            Ok((Some(5), true))
        );
    }

    #[test]
    fn the_frame_count_is_checked_at_both_ends() {
        let max = MAX_FRAMES.to_string();
        assert_eq!(parse(&["--frames", "1"]).map(|a| a.frames), Ok(Some(1)));
        assert_eq!(
            parse(&["--frames", &max]).map(|a| a.frames),
            Ok(Some(MAX_FRAMES))
        );
        for bad in ["0", "-1", "x", "", "1000001", "4294967296"] {
            assert_eq!(
                parse(&["--frames", bad]),
                Err(ArgsError::BadValue(bad.to_owned())),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_missing_value_an_unknown_flag_and_a_null_without_frames_are_refused() {
        assert_eq!(parse(&["--frames"]), Err(ArgsError::MissingValue));
        assert_eq!(
            parse(&["--nope"]),
            Err(ArgsError::Unknown("--nope".to_owned()))
        );
        assert_eq!(parse(&["--null"]), Err(ArgsError::NullNeedsFrames));
    }

    #[test]
    fn every_error_has_a_message_naming_the_cause() {
        let text = |e: ArgsError| e.to_string();
        assert!(text(ArgsError::Unknown("--x".to_owned())).contains("--x"));
        assert!(text(ArgsError::MissingValue).contains("--frames"));
        assert!(text(ArgsError::BadValue("0".to_owned())).contains("1000000"));
        assert!(text(ArgsError::NullNeedsFrames).contains("--null"));
    }

    #[test]
    fn the_scene_and_the_sub_steps_are_chosen_by_name_and_number() {
        let args = parse(&["--scene", "pyramid", "--sub-steps", "8"]);
        assert_eq!(args.clone().map(|a| a.scene), Ok(SceneKind::Pyramid));
        assert_eq!(args.clone().map(|a| a.sub_steps), Ok(Some(8)));
        assert_eq!(
            args.map(|a| a.setup()),
            Ok(Setup {
                scene: SceneKind::Pyramid,
                sub_steps: 8
            })
        );
        assert_eq!(parse(&[]).map(|a| a.setup()), Ok(Setup::default()));
        assert_eq!(
            parse(&["--scene", "stream"]).map(|a| a.scene),
            Ok(SceneKind::Stream)
        );
    }

    #[test]
    fn a_bad_scene_or_sub_step_count_is_refused() {
        assert_eq!(parse(&["--scene"]), Err(ArgsError::MissingScene));
        assert_eq!(
            parse(&["--scene", "castle"]),
            Err(ArgsError::BadScene("castle".to_owned()))
        );
        assert_eq!(parse(&["--sub-steps"]), Err(ArgsError::MissingSubSteps));
        let max = MAX_SUB_STEPS.to_string();
        assert_eq!(
            parse(&["--sub-steps", &max]).map(|a| a.sub_steps),
            Ok(Some(MAX_SUB_STEPS))
        );
        assert_eq!(
            parse(&["--sub-steps", "1"]).map(|a| a.sub_steps),
            Ok(Some(1))
        );
        for bad in ["0", "17", "-1", "x", ""] {
            assert_eq!(
                parse(&["--sub-steps", bad]),
                Err(ArgsError::BadSubSteps(bad.to_owned())),
                "{bad}"
            );
        }
        assert!(
            ArgsError::BadScene("castle".to_owned())
                .to_string()
                .contains("castle")
        );
        assert!(ArgsError::MissingScene.to_string().contains("--scene"));
        assert!(
            ArgsError::MissingSubSteps
                .to_string()
                .contains("--sub-steps")
        );
        assert!(
            ArgsError::BadSubSteps("0".to_owned())
                .to_string()
                .contains("16")
        );
    }
}
