//! The application: opens the window with GLFW, the OpenGL 4.5 or the Vulkan 1.3 device on it
//! (`--backend`), and owns the loop (pump events, draw one frame, present). Needs GLFW, a display
//! and a driver for the API; without them `run` returns [`RunError::NoDevice`] and nothing is drawn.
//! This is the composition root of the sample: it names the two adapter devices.

use crate::mesh_sample::{MESH_GLSL, MESH_SPIRV, MeshSample};
use adapter_render_gl::GlDevice;
use adapter_render_vk::VulkanDevice;
use app_sprite_sample::app::{EVENT_CAPACITY, FRAME, Opened, failed, first_size};
use app_sprite_sample::args::{Args, Backend as Api};
use app_sprite_sample::backend::Backend;
use app_sprite_sample::cli::{RunError, RunReport};
use app_sprite_sample::looping::{Step, count_frames};
use assets::dshd::Program;
use platform::window::EventQueue;
use render::desc::SurfaceDesc;

const TITLE: &str = "DuckEngine 6-11: spinning cube";

/// The mesh shader asset in the form a device takes.
trait MeshShader: Backend {
    /// The `dshd` bytes: GLSL text or SPIR-V.
    const MESH: &'static [u8];
}

impl MeshShader for GlDevice {
    const MESH: &'static [u8] = MESH_GLSL;
}

impl MeshShader for VulkanDevice {
    const MESH: &'static [u8] = MESH_SPIRV;
}

/// The window and device opened, the shader asset loaded and the sample built.
fn start<D: MeshShader>() -> Result<(Opened<D>, MeshSample), RunError> {
    let mut opened = Opened::<D>::open_with(TITLE)?;
    let size = first_size(&mut opened)?;
    let surface = opened
        .device
        .create_surface(SurfaceDesc {
            native_token: opened.token.0,
            width: size.0,
            height: size.1,
        })
        .map_err(failed)?;
    let program = Program::parse(D::MESH).map_err(failed)?;
    let sample = MeshSample::new(&mut opened.device, &program, surface, size).map_err(failed)?;
    Ok((opened, sample))
}

/// One frame, following a changed framebuffer size first. A minimised window (a zero side) draws
/// nothing and is [`Step::Skipped`]. The backend's `check` after the frame can fail it.
fn draw_frame<D: MeshShader>(
    opened: &mut Opened<D>,
    sample: &mut MeshSample,
    frame: u32,
) -> Result<Step, RunError> {
    let size = opened.framebuffer()?;
    if size.0 == 0 || size.1 == 0 {
        return Ok(Step::Skipped);
    }
    if size != sample.size() {
        sample.resize(&mut opened.device, size).map_err(failed)?;
    }
    sample.draw(&mut opened.device, frame).map_err(failed)?;
    opened.device.check()?;
    Ok(Step::Drawn)
}

fn run_on<D: MeshShader>(args: &Args) -> Result<RunReport, RunError> {
    let (mut opened, mut sample) = start::<D>()?;
    let mut queue = EventQueue::with_capacity(EVENT_CAPACITY);
    let frames = count_frames(args.frames, |frame| {
        if opened.pump(&mut queue, |_| {}) {
            return Ok(Step::Closing);
        }
        let step = draw_frame(&mut opened, &mut sample, frame)?;
        std::thread::sleep(FRAME);
        Ok(step)
    })?;
    Ok(RunReport { frames })
}

/// Opens the window and the device, loads the shader asset and runs the loop.
///
/// # Errors
/// [`RunError::NoDevice`] when GLFW, the window or the device of the chosen API is not available,
/// [`RunError::Failed`] when a later step fails.
pub fn run(args: &Args) -> Result<RunReport, RunError> {
    match args.backend {
        Api::Gl => run_on::<GlDevice>(args),
        Api::Vulkan => run_on::<VulkanDevice>(args),
    }
}
