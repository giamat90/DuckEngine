//! One running sample: the sandbox, the renderer and the controls, and the one `frame` call that
//! turns the requests of the controls and a frame time into a drawn frame. The window loop and
//! the Null loop both call it, so what runs without a window is what runs in one.

use crate::controls::Controls;
use crate::error::Error;
use crate::renderer::{Renderer, Target};
use crate::sandbox::{FrameReport, Sandbox};
use assets::dshd::Program;
use platform::window::Event;
use render::device::IRenderDevice;
use render::ids::SurfaceId;

/// The seed of the stream of pieces.
pub const DEFAULT_SEED: u64 = 24;

/// The sandbox, its renderer and its controls.
pub struct Session {
    sandbox: Sandbox,
    renderer: Renderer,
    controls: Controls,
}

impl Session {
    /// Builds the renderer on `device` for a view of `size` pixels on `surface`, and the sandbox
    /// whose stream is seeded by `seed`.
    ///
    /// # Errors
    /// The error of the renderer or of the sandbox.
    pub fn new(
        device: &mut dyn IRenderDevice,
        program: &Program<'_>,
        surface: SurfaceId,
        size: (u32, u32),
        seed: u64,
    ) -> Result<Self, Error> {
        let renderer = Renderer::new(device, program, surface, size)?;
        Ok(Self {
            sandbox: Sandbox::new(renderer.texture(), seed, size)?,
            renderer,
            controls: Controls::new()?,
        })
    }

    /// Follows a window event.
    pub fn apply(&mut self, event: &Event) {
        self.controls.apply(event);
    }

    /// True once the user asked to leave.
    #[must_use]
    pub fn quit(&self) -> bool {
        self.controls.quit()
    }

    /// The view size in pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        self.renderer.size()
    }

    /// The sandbox.
    #[must_use]
    pub fn sandbox(&self) -> &Sandbox {
        &self.sandbox
    }

    /// The renderer.
    #[must_use]
    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    /// Follows a window resize.
    ///
    /// # Errors
    /// [`Error::NoView`] for a zero side (nothing changes), or the device's error.
    pub fn resize(
        &mut self,
        device: &mut dyn IRenderDevice,
        size: (u32, u32),
    ) -> Result<(), Error> {
        self.renderer.resize(device, size)?;
        self.sandbox.resize(size)
    }

    /// Forgets the events since the last frame; for a frame that is not drawn (a minimised
    /// window), so that its presses and drags are not carried out when drawing resumes.
    pub fn discard_input(&mut self) {
        self.controls.begin_frame();
    }

    /// One frame: carries out the requests of the events since the last frame, runs the physics
    /// steps `frame_ticks` nanoseconds amount to and draws.
    ///
    /// # Errors
    /// The error of the first phase that fails.
    pub fn frame(
        &mut self,
        device: &mut dyn IRenderDevice,
        frame_ticks: u64,
    ) -> Result<FrameReport, Error> {
        let requests = self.controls.requests();
        self.controls.begin_frame();
        let mut target = Target {
            device,
            renderer: &mut self.renderer,
        };
        self.sandbox.frame(requests, frame_ticks, &mut target)
    }
}
