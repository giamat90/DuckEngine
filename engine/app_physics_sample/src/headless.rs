//! The loop without a window: the whole sample on the Null render device, a fixed frame time and
//! no sleeping, so a run is fast and the same everywhere. `app_physics_sample --null --frames N`
//! runs it, and CI can run it without a GPU.

use crate::cli::{Report, RunError};
use crate::session::Session;
use crate::simulation::{DEFAULT_SEED, STEP_TICKS};
use app_sprite_sample::GOLDEN_SHADER;
use app_sprite_sample::app::{HEIGHT, WIDTH, failed};
use app_sprite_sample::looping::{Step, count_frames};
use assets::dshd::Program;
use render::desc::SurfaceDesc;
use render::device::IRenderDevice;
use render::null::NullDevice;

/// Draws `frames` frames on the Null device, one physics step each, and reports the end state.
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
    let program = Program::parse(GOLDEN_SHADER).map_err(failed)?;
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
    Ok(Report {
        frames: drawn,
        state_hash: session.simulation().state_hash(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_draws_the_frames_asked_for_and_reports_the_state() {
        let first = run_null(30);
        let second = run_null(30);
        assert_eq!(first.as_ref().map(|r| r.frames), Ok(30));
        assert_eq!(first, second);
        assert_ne!(first, run_null(31));
    }
}
