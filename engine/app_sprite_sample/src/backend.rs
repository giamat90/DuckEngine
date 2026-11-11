//! The two graphics APIs the sample can draw with, behind one small trait. This is the
//! composition root: the only place of the sample that names an adapter device (card 6-10). The
//! sample code itself (`sample`, `scene`) sees `&mut dyn IRenderDevice` only.

use crate::app::no_device;
use crate::cli::RunError;
use crate::sample::{GOLDEN_SHADER, GOLDEN_SPIRV};
use adapter_render_gl::GlDevice;
use adapter_render_vk::{VulkanDevice, VulkanOptions};
use platform::window::NativeToken;
use render::device::IRenderDevice;

/// A device the sample can open on its window and check after each frame.
pub trait Backend: IRenderDevice + Sized {
    /// The shader asset in the form this API takes: GLSL text or SPIR-V (both made by `shaderc`).
    const SHADER: &'static [u8];

    /// Opens the device on the window named by `token`.
    ///
    /// # Errors
    /// [`RunError::NoDevice`] when the API or its driver is not available.
    fn open(token: NativeToken) -> Result<Self, RunError>;

    /// Looks for an error the API kept to itself while the frame was drawn.
    ///
    /// # Errors
    /// [`RunError::Failed`] with the API's own words.
    fn check(&mut self) -> Result<(), RunError>;
}

impl Backend for GlDevice {
    const SHADER: &'static [u8] = GOLDEN_SHADER;

    fn open(token: NativeToken) -> Result<Self, RunError> {
        Self::create(token).map_err(|error| no_device("OpenGL 4.5 device", error))
    }

    fn check(&mut self) -> Result<(), RunError> {
        match self.take_gl_error() {
            0 => Ok(()),
            code => Err(RunError::Failed(format!("OpenGL error {code:#06x}"))),
        }
    }
}

impl Backend for VulkanDevice {
    const SHADER: &'static [u8] = GOLDEN_SPIRV;

    fn open(token: NativeToken) -> Result<Self, RunError> {
        // The Khronos validation layer is switched on when it is installed.
        Self::create(token, VulkanOptions { validation: true })
            .map_err(|error| no_device("Vulkan 1.3 device", error))
    }

    fn check(&mut self) -> Result<(), RunError> {
        let report = self.validation_report();
        if report.is_clean() {
            return Ok(());
        }
        Err(RunError::Failed(format!(
            "Vulkan validation: {} errors, {} warnings, first: {:?}",
            report.errors,
            report.warnings,
            report.messages.first()
        )))
    }
}
