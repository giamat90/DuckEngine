//! The application: opens the window with GLFW and the OpenGL 4.5 or (`--vulkan`) the Vulkan 1.3
//! device on it, and owns the loop (pump events, draw one frame with the physics steps the
//! elapsed time amounts to, present). Needs GLFW, a display and a driver for the API; without
//! them `run` returns
//! [`RunError::NoDevice`] and nothing is drawn. Everything that decides what is drawn is in
//! [`crate::session`], which the tests run on the Null device; `--null` runs the same loop with
//! no window.

use crate::cli::{Args, Report, RunError};
use crate::renderer::{LIT_GLSL, LIT_SPIRV};
use crate::sandbox::STEP_TICKS;
use crate::session::{DEFAULT_SEED, Session};
use adapter_render_gl::GlDevice;
use adapter_render_vk::VulkanDevice;
use app_sprite_sample::app::{EVENT_CAPACITY, FRAME, HEIGHT, Opened, WIDTH, failed, first_size};
use app_sprite_sample::backend::Backend;
use app_sprite_sample::looping::{Step, count_frames};
use assets::dshd::Program;
use platform::window::EventQueue;
use render::desc::SurfaceDesc;
use render::device::IRenderDevice;
use render::null::NullDevice;
use std::time::Instant;

const TITLE_GL: &str = "DuckEngine 8-11: 3D sandbox (OpenGL)";
const TITLE_VULKAN: &str = "DuckEngine 8-32: 3D sandbox (Vulkan)";

/// The lit shader in the form the graphics API takes: GLSL text for OpenGL, SPIR-V for Vulkan.
fn asset_for(vulkan: bool) -> &'static [u8] {
    if vulkan { LIT_SPIRV } else { LIT_GLSL }
}

/// Nanoseconds from `earlier` to `later`, saturating (the accumulator's tick is a nanosecond).
#[must_use]
pub fn ticks_between(earlier: Instant, later: Instant) -> u64 {
    u64::try_from(later.saturating_duration_since(earlier).as_nanos()).unwrap_or(u64::MAX)
}

/// The report of a finished run.
fn report(frames: u32, session: &Session) -> Report {
    Report {
        frames,
        state_hash: session.sandbox().state_hash(),
    }
}

/// The loop on the Null render device: `frames` frames of one physics step each, no window, no
/// sleeping, so a run is fast and the same everywhere.
///
/// # Errors
/// [`RunError::Failed`] when a step or the device fails.
pub fn run_null(frames: u32) -> Result<Report, RunError> {
    let mut device = NullDevice::new();
    let surface = device
        .create_surface(SurfaceDesc {
            native_token: 1,
            width: WIDTH,
            height: HEIGHT,
        })
        .map_err(failed)?;
    let program = Program::parse(LIT_GLSL).map_err(failed)?;
    let mut session = Session::new(
        &mut device,
        &program,
        surface,
        (WIDTH, HEIGHT),
        DEFAULT_SEED,
    )
    .map_err(failed)?;
    let drawn = count_frames(Some(frames), |_| {
        session.frame(&mut device, STEP_TICKS).map_err(failed)?;
        Ok(Step::Drawn)
    })?;
    Ok(report(drawn, &session))
}

/// What a frame does about the size of the framebuffer.
#[derive(Debug, PartialEq, Eq)]
enum SizePlan {
    /// A zero side (a minimised window): draw nothing.
    Skip,
    /// The size changed: resize the session first.
    Resize,
    /// The size is the one the session has.
    Keep,
}

/// Decides what to do with a framebuffer of `size` when the session has `current`.
fn plan_size(size: (u32, u32), current: (u32, u32)) -> SizePlan {
    if size.0 == 0 || size.1 == 0 {
        SizePlan::Skip
    } else if size != current {
        SizePlan::Resize
    } else {
        SizePlan::Keep
    }
}

/// Draws one frame, following a changed framebuffer size first, with the time since `last`. A
/// minimised window (a zero side) draws nothing and is [`Step::Skipped`]. An error the API kept
/// to itself (the GL error flag, the Vulkan validation report) after the frame is a failure.
fn draw_frame<D: Backend>(
    opened: &mut Opened<D>,
    session: &mut Session,
    last: &mut Instant,
) -> Result<Step, RunError> {
    let size = opened.framebuffer()?;
    match plan_size(size, session.size()) {
        SizePlan::Skip => {
            session.discard_input();
            return Ok(Step::Skipped);
        }
        SizePlan::Resize => session.resize(&mut opened.device, size).map_err(failed)?,
        SizePlan::Keep => {}
    }
    let now = Instant::now();
    let ticks = ticks_between(*last, now);
    *last = now;
    session.frame(&mut opened.device, ticks).map_err(failed)?;
    opened.device.check()?;
    Ok(Step::Drawn)
}

/// The loop: until `frames` frames are drawn (a minimised window draws none and does not count),
/// the window is closed or Escape is pressed.
fn run_window<D: Backend>(
    frames: Option<u32>,
    title: &'static str,
    asset: &[u8],
) -> Result<Report, RunError> {
    let mut opened = Opened::<D>::open_with(title)?;
    let size = first_size(&mut opened)?;
    let surface = opened
        .device
        .create_surface(SurfaceDesc {
            native_token: opened.token.0,
            width: size.0,
            height: size.1,
        })
        .map_err(failed)?;
    let program = Program::parse(asset).map_err(failed)?;
    let mut session =
        Session::new(&mut opened.device, &program, surface, size, DEFAULT_SEED).map_err(failed)?;
    let mut queue = EventQueue::with_capacity(EVENT_CAPACITY);
    let mut last = Instant::now();
    let drawn = count_frames(frames, |_| {
        let closing = opened.pump(&mut queue, |event| session.apply(event));
        if closing || session.quit() {
            return Ok(Step::Closing);
        }
        let step = draw_frame(&mut opened, &mut session, &mut last)?;
        std::thread::sleep(FRAME);
        Ok(step)
    })?;
    Ok(report(drawn, &session))
}

/// Runs what the command line asked for: the window on OpenGL, or with `--vulkan` on Vulkan, or
/// with `--null` the loop on the Null device.
///
/// # Errors
/// [`RunError::NoDevice`] when GLFW, the window or the graphics device is not available,
/// [`RunError::Failed`] when a later step fails.
pub fn run(args: &Args) -> Result<Report, RunError> {
    if args.vulkan {
        return run_window::<VulkanDevice>(args.frames, TITLE_VULKAN, asset_for(true));
    }
    if !args.null {
        return run_window::<GlDevice>(args.frames, TITLE_GL, asset_for(false));
    }
    let frames = args
        .frames
        .ok_or_else(|| RunError::Failed("`--null` needs `--frames N`".to_owned()))?;
    run_null(frames)
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
    fn a_frame_follows_a_new_size_and_skips_a_zero_side() {
        assert_eq!(plan_size((800, 450), (800, 450)), SizePlan::Keep);
        assert_eq!(plan_size((1024, 450), (800, 450)), SizePlan::Resize);
        assert_eq!(plan_size((800, 600), (800, 450)), SizePlan::Resize);
        assert_eq!(plan_size((0, 450), (800, 450)), SizePlan::Skip);
        assert_eq!(plan_size((800, 0), (800, 450)), SizePlan::Skip);
        assert_eq!(plan_size((0, 0), (0, 0)), SizePlan::Skip);
    }

    #[test]
    fn the_null_run_needs_a_frame_count_and_otherwise_runs() {
        let no_frames = Args {
            frames: None,
            null: true,
            vulkan: false,
        };
        assert!(matches!(run(&no_frames), Err(RunError::Failed(_))));
        let args = Args {
            frames: Some(5),
            null: true,
            vulkan: false,
        };
        assert_eq!(run(&args).map(|r| r.frames), Ok(5));
    }

    #[test]
    fn the_shader_asset_follows_the_graphics_api() {
        assert_eq!(asset_for(false), LIT_GLSL);
        assert_eq!(asset_for(true), LIT_SPIRV);
        assert_ne!(LIT_GLSL, LIT_SPIRV);
    }

    #[test]
    fn a_null_run_is_repeatable_and_follows_its_length() {
        let first = run_null(30);
        assert_eq!(first.as_ref().map(|r| r.frames), Ok(30));
        assert_eq!(first, run_null(30));
        assert_ne!(first, run_null(31));
    }
}
