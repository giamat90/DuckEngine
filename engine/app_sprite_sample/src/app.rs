//! The application: opens the window with GLFW, the OpenGL 4.5 device on it, and owns the loop
//! (pump events, draw one frame, present). Needs GLFW, a display and an OpenGL 4.5 driver; without
//! them `run` returns [`RunError::NoDevice`] and nothing is drawn. Everything that decides what is
//! drawn is in [`crate::sample`] and [`crate::scene`], which the tests run on the Null device.

use crate::args::Args;
use crate::cli::{RunError, RunReport};
use crate::input::Controls;
use crate::looping::{Step, count_frames, poll};
use crate::sample::{GOLDEN_SHADER, Sample};
use crate::scene::scene_for;
use adapter_render_gl::GlDevice;
use adapter_window_glfw::GlfwWindows;
use assets::dshd::Program;
use platform::window::{Event, EventQueue, IWindowSystem, NativeToken, WindowDesc, WindowId};
use render::desc::SurfaceDesc;
use render::device::IRenderDevice;
use std::fmt::Display;
use std::time::Duration;

const TITLE: &str = "DuckEngine 3-15: triangle and sprites";
/// Window size at the start, in screen units.
pub const WIDTH: u32 = 800;
/// See [`WIDTH`].
pub const HEIGHT: u32 = 450;
/// Events the queue of the loop holds.
pub const EVENT_CAPACITY: u32 = 64;
/// About 60 frames a second; the sample does not wait for the display.
pub const FRAME: Duration = Duration::from_millis(16);
/// How long `run` waits for a window that reports no size.
const SIZE_WAIT: Duration = Duration::from_secs(2);
/// Polls in that wait, one per [`FRAME`] (125 of 16 ms is 2 s).
const SIZE_ATTEMPTS: u32 = 125;

/// A missing part of the stack as [`RunError::NoDevice`].
pub fn no_device(what: &str, error: impl Display) -> RunError {
    RunError::NoDevice(format!("{what}: {error}"))
}

/// Any other error as [`RunError::Failed`].
pub fn failed(error: impl Display) -> RunError {
    RunError::Failed(error.to_string())
}

/// The window system, its one window and the device on it. The device is the first field so it
/// is dropped before the window system that owns its window.
///
/// Shared with `app_physics_sample` (card 4-24).
pub struct Opened {
    /// The OpenGL device.
    pub device: GlDevice,
    /// The native token of the window, for `create_surface`.
    pub token: NativeToken,
    window: WindowId,
    windows: GlfwWindows,
}

impl Opened {
    /// Loads GLFW, opens a window titled `title` and the OpenGL 4.5 device on it.
    ///
    /// # Errors
    /// [`RunError::NoDevice`] when GLFW, the window or the device is not available.
    pub fn open(title: &'static str) -> Result<Self, RunError> {
        let mut windows = GlfwWindows::load_default(1).map_err(|error| no_device("GLFW", error))?;
        let desc = WindowDesc {
            title,
            width: WIDTH,
            height: HEIGHT,
        };
        let window = windows
            .create_window(&desc)
            .map_err(|error| no_device("window", error))?;
        let token = windows
            .native_token(window)
            .map_err(|error| no_device("native token", error))?;
        let device =
            GlDevice::create(token).map_err(|error| no_device("OpenGL 4.5 device", error))?;
        Ok(Self {
            device,
            token,
            window,
            windows,
        })
    }

    /// The framebuffer size in pixels.
    ///
    /// # Errors
    /// [`RunError::Failed`] when the window is gone.
    pub fn framebuffer(&self) -> Result<(u32, u32), RunError> {
        let info = self.windows.window_info(self.window).map_err(failed)?;
        Ok((info.framebuffer_width, info.framebuffer_height))
    }

    /// Pumps the window events, hands each to `on_event`; true when the user asked to close.
    pub fn pump(&mut self, queue: &mut EventQueue, mut on_event: impl FnMut(&Event)) -> bool {
        self.windows.pump(queue);
        let mut closing = false;
        while let Some(event) = queue.pop() {
            closing |= matches!(event, Event::CloseRequested { .. } | Event::Quit);
            on_event(&event);
        }
        closing
    }

    /// Pumps the window events and drops them; true when the user asked to close.
    fn closing(&mut self, queue: &mut EventQueue) -> bool {
        self.pump(queue, |_| {})
    }
}

/// Draws one frame, following a changed framebuffer size first. A minimised window (a zero side)
/// draws nothing and is [`Step::Skipped`]. A GL error flag after the frame is a failure.
fn draw_frame(
    opened: &mut Opened,
    sample: &mut Sample,
    frame: u32,
    scene: crate::Scene,
) -> Result<Step, RunError> {
    let size = opened.framebuffer()?;
    if size.0 == 0 || size.1 == 0 {
        return Ok(Step::Skipped);
    }
    if size != sample.size() {
        sample.resize(&mut opened.device, size).map_err(failed)?;
    }
    sample
        .draw(&mut opened.device, frame, scene)
        .map_err(failed)?;
    match opened.device.take_gl_error() {
        0 => Ok(Step::Drawn),
        code => Err(RunError::Failed(format!("OpenGL error {code:#06x}"))),
    }
}

/// The loop: until `args.frames` frames are drawn (a minimised window draws none and does not
/// count), or the window is closed.
fn run_loop(args: Args, opened: &mut Opened, sample: &mut Sample) -> Result<u32, RunError> {
    let mut queue = EventQueue::with_capacity(EVENT_CAPACITY);
    let triangle_frames = args.triangle_frames();
    count_frames(args.frames, |frame| {
        if opened.closing(&mut queue) {
            return Ok(Step::Closing);
        }
        let step = draw_frame(opened, sample, frame, scene_for(frame, triangle_frames))?;
        std::thread::sleep(FRAME);
        Ok(step)
    })
}

/// The framebuffer size once it is not zero. A window can report zero for its first moments (or
/// when it starts minimised): events are pumped for up to [`SIZE_WAIT`] before giving up.
///
/// # Errors
/// [`RunError::Failed`] when the window is closed or still has no size after the wait.
pub fn first_size(opened: &mut Opened) -> Result<(u32, u32), RunError> {
    let mut queue = EventQueue::with_capacity(EVENT_CAPACITY);
    let size = poll(SIZE_ATTEMPTS, || {
        if opened.closing(&mut queue) {
            return Err(RunError::Failed(
                "the window was closed before it had a size".to_owned(),
            ));
        }
        let size = opened.framebuffer()?;
        if size.0 == 0 || size.1 == 0 {
            std::thread::sleep(FRAME);
            return Ok(None);
        }
        Ok(Some(size))
    })?;
    size.ok_or_else(|| {
        RunError::Failed(format!("the window still has no size after {SIZE_WAIT:?}"))
    })
}

/// The window and device opened, the shader asset loaded and the sample built.
fn start() -> Result<(Opened, Sample), RunError> {
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
    let sample = Sample::new(&mut opened.device, &program, surface, size).map_err(failed)?;
    Ok((opened, sample))
}

/// Opens the window and the device, loads the shader asset and runs the loop.
///
/// # Errors
/// [`RunError::NoDevice`] when GLFW, the window or the OpenGL 4.5 device is not available,
/// [`RunError::Failed`] when a later step fails.
pub fn run(args: &Args) -> Result<RunReport, RunError> {
    let (mut opened, mut sample) = start()?;
    let frames = run_loop(*args, &mut opened, &mut sample)?;
    Ok(RunReport { frames })
}

/// One interactive frame: follows a changed framebuffer size, then draws the controls' sprites.
fn draw_controls_frame(
    opened: &mut Opened,
    sample: &mut Sample,
    controls: &Controls,
) -> Result<Step, RunError> {
    let size = opened.framebuffer()?;
    if size.0 == 0 || size.1 == 0 {
        return Ok(Step::Skipped);
    }
    if size != sample.size() {
        sample.resize(&mut opened.device, size).map_err(failed)?;
    }
    sample
        .draw_controls(&mut opened.device, controls)
        .map_err(failed)?;
    match opened.device.take_gl_error() {
        0 => Ok(Step::Drawn),
        code => Err(RunError::Failed(format!("OpenGL error {code:#06x}"))),
    }
}

/// Like [`run`], for the interactive example: the window events steer the sprites (arrows or WASD
/// move one, the mouse places and clicks tint the other); Escape or closing the window ends it.
///
/// # Errors
/// As [`run`].
pub fn run_interactive(args: &Args) -> Result<RunReport, RunError> {
    let (mut opened, mut sample) = start()?;
    let mut controls = Controls::new(sample.size());
    let mut queue = EventQueue::with_capacity(EVENT_CAPACITY);
    let frames = count_frames(args.frames, |_| {
        let closing = opened.pump(&mut queue, |event| controls.apply(event));
        if closing || controls.quit() {
            return Ok(Step::Closing);
        }
        controls.step();
        let step = draw_controls_frame(&mut opened, &mut sample, &controls)?;
        std::thread::sleep(FRAME);
        Ok(step)
    })?;
    Ok(RunReport { frames })
}
