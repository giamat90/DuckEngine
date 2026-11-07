//! One running sample: the simulation, the view and the controls, and the one `frame` call that
//! turns the requests of the controls and a frame time into a drawn frame. The window loop and the
//! headless loop both call it, so what runs without a window is what runs in one.

use crate::controls::Controls;
use crate::error::Error;
use crate::simulation::Simulation;
use crate::view::View;
use assets::dshd::Program;
use platform::window::Event;
use render::desc::SubmitStats;
use render::device::IRenderDevice;
use render::ids::SurfaceId;
use render::sprite::Sprite;

/// The simulation, the view and the controls of the sample.
pub struct Session {
    simulation: Simulation,
    view: View,
    controls: Controls,
}

impl Session {
    /// Builds the simulation (seeded with `seed`) and the view on `device`, for a view of `size`
    /// pixels on `surface`.
    ///
    /// # Errors
    /// The physics world's error, or the render side's (a zero side, a shader asset that does not
    /// fit, the device).
    pub fn new(
        device: &mut dyn IRenderDevice,
        program: &Program<'_>,
        surface: SurfaceId,
        size: (u32, u32),
        seed: u64,
    ) -> Result<Self, Error> {
        Ok(Self {
            simulation: Simulation::new(seed)?,
            view: View::new(device, program, surface, size)?,
            controls: Controls::new(size),
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
        self.view.size()
    }

    /// Follows a window resize.
    ///
    /// # Errors
    /// [`Error::Render`] for a zero side (nothing changes) or the device's error.
    pub fn resize(
        &mut self,
        device: &mut dyn IRenderDevice,
        size: (u32, u32),
    ) -> Result<(), Error> {
        self.view.resize(device, size)?;
        self.controls.resize(size);
        Ok(())
    }

    /// The simulation.
    #[must_use]
    pub fn simulation(&self) -> &Simulation {
        &self.simulation
    }

    /// The sprites of the last frame.
    #[must_use]
    pub fn sprites(&self) -> &[Sprite] {
        self.view.sprites()
    }

    /// Debug sprites dropped because a frame was full.
    #[must_use]
    pub fn dropped(&self) -> usize {
        self.view.dropped()
    }

    /// One frame: carries out the requests of the controls (a reset, a click), runs the physics
    /// steps `frame_ticks` nanoseconds amount to, then draws and presents.
    ///
    /// # Errors
    /// The physics world's error, or the first render error of the frame.
    pub fn frame(
        &mut self,
        device: &mut dyn IRenderDevice,
        frame_ticks: u64,
    ) -> Result<SubmitStats, Error> {
        if self.controls.take_reset() {
            self.simulation.reset()?;
        }
        if let Some(at) = self.controls.take_spawn() {
            self.simulation.spawn_at(at)?;
        }
        self.simulation.advance(frame_ticks)?;
        let stats = self
            .view
            .draw(device, self.simulation.world(), self.controls.debug())?;
        Ok(stats)
    }
}
