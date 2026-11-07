//! The application: opens the window with GLFW, the OpenGL 4.5 device on it, and owns the loop
//! (pump events, run the physics steps the elapsed time amounts to, draw one frame, present).
//! Needs GLFW, a display and an OpenGL 4.5 driver; without them `run` returns
//! [`RunError::NoDevice`] and nothing is drawn. Everything that decides what is drawn is in
//! [`crate::session`], which the tests run on the Null device; `--null` runs the same loop with
//! no window ([`crate::headless`]). The physics step runs on this thread.

use crate::args::Args;
use crate::cli::{Report, RunError};
use crate::headless::run_null_with;
use crate::session::{Session, Start};
use crate::simulation::{DEFAULT_SEED, Setup};
use app_sprite_sample::GOLDEN_SHADER;
use app_sprite_sample::app::{EVENT_CAPACITY, FRAME, Opened, failed, first_size};
use app_sprite_sample::looping::{Step, count_frames};
use assets::dshd::Program;
use platform::window::EventQueue;
use render::desc::SurfaceDesc;
use render::device::IRenderDevice;
use std::time::Instant;

const TITLE: &str = "DuckEngine 4-24: falling boxes";

/// Nanoseconds from `earlier` to `later`, saturating (the accumulator's tick is a nanosecond).
#[must_use]
pub fn ticks_between(earlier: Instant, later: Instant) -> u64 {
    u64::try_from(later.saturating_duration_since(earlier).as_nanos()).unwrap_or(u64::MAX)
}

/// The window and device opened, the shader asset loaded and the session built.
fn start(setup: Setup) -> Result<(Opened, Session), RunError> {
    let mut opened = Opened::open(TITLE)?;
    let size = first_size(&mut opened)?;
    let surface = opened
        .device
        .create_surface(SurfaceDesc {
            native_token: opened.token.0,
            width: size.0,
            height: size.1,
        })
        .map_err(failed)?;
    let program = Program::parse(GOLDEN_SHADER).map_err(failed)?;
    let start = Start {
        surface,
        size,
        seed: DEFAULT_SEED,
        setup,
    };
    let session = Session::with_setup(&mut opened.device, &program, start).map_err(failed)?;
    Ok((opened, session))
}

/// Draws one frame, following a changed framebuffer size first, with the time since `last`. A
/// minimised window (a zero side) draws nothing and is [`Step::Skipped`]. A GL error flag after
/// the frame is a failure.
fn draw_frame(
    opened: &mut Opened,
    session: &mut Session,
    last: &mut Instant,
) -> Result<Step, RunError> {
    let size = opened.framebuffer()?;
    if size.0 == 0 || size.1 == 0 {
        return Ok(Step::Skipped);
    }
    if size != session.size() {
        session.resize(&mut opened.device, size).map_err(failed)?;
    }
    let now = Instant::now();
    let ticks = ticks_between(*last, now);
    *last = now;
    session.frame(&mut opened.device, ticks).map_err(failed)?;
    match opened.device.take_gl_error() {
        0 => Ok(Step::Drawn),
        code => Err(RunError::Failed(format!("OpenGL error {code:#06x}"))),
    }
}

/// The loop: until `frames` frames are drawn (a minimised window draws none and does not count),
/// the window is closed or Escape is pressed.
fn run_window(frames: Option<u32>, setup: Setup) -> Result<Report, RunError> {
    let (mut opened, mut session) = start(setup)?;
    let mut shown = session.describe();
    eprintln!("scene: {shown} (P switches scene, S switches sub-steps, R restarts)");
    let mut queue = EventQueue::with_capacity(EVENT_CAPACITY);
    let mut last = Instant::now();
    let drawn = count_frames(frames, |_| {
        let closing = opened.pump(&mut queue, |event| session.apply(event));
        if closing || session.quit() {
            return Ok(Step::Closing);
        }
        let step = draw_frame(&mut opened, &mut session, &mut last)?;
        if session.describe() != shown {
            shown = session.describe();
            eprintln!("scene: {shown}");
        }
        std::thread::sleep(FRAME);
        Ok(step)
    })?;
    Ok(Report {
        frames: drawn,
        state_hash: session.simulation().state_hash(),
    })
}

/// Runs what the command line asked for: the window, or with `--null` the loop on the Null device.
///
/// # Errors
/// [`RunError::NoDevice`] when GLFW, the window or the OpenGL 4.5 device is not available,
/// [`RunError::Failed`] when a later step fails.
pub fn run(args: &Args) -> Result<Report, RunError> {
    if !args.null {
        return run_window(args.frames, args.setup());
    }
    let frames = args
        .frames
        .ok_or_else(|| RunError::Failed("`--null` needs `--frames N`".to_owned()))?;
    run_null_with(frames, args.setup())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn elapsed_time_is_counted_in_nanoseconds_and_never_negative() {
        let start = Instant::now();
        let later = start + Duration::from_millis(16);
        assert_eq!(ticks_between(start, later), 16_000_000);
        assert_eq!(ticks_between(later, start), 0);
    }

    #[test]
    fn the_null_run_needs_a_frame_count_and_otherwise_runs() {
        let no_frames = Args {
            frames: None,
            null: true,
            ..Args::default()
        };
        assert!(matches!(run(&no_frames), Err(RunError::Failed(_))));
        let args = Args {
            frames: Some(5),
            null: true,
            ..Args::default()
        };
        assert_eq!(run(&args).map(|r| r.frames), Ok(5));
    }
}
