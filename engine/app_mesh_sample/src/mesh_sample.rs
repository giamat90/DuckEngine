//! The mesh sample's frame, over any render device: the resources built from the mesh shader
//! asset (pipeline with a depth format, the cube's buffers, a texture, a depth texture) and one
//! `draw` per frame that spins the cube, records one pass with depth into the backbuffer and
//! presents. The device is passed in (`&mut dyn IRenderDevice`), so the same code runs on the Null
//! device in tests and on the OpenGL and Vulkan devices in the window.

use app_sprite_sample::sample::{create_buffer, create_texture};
use app_sprite_sample::scene::{CHECKER_SIDE, checker_pixels};
use assets::dshd::{Module, Program, Stage};
use base::math::{Mat4, Vec3};
use render::camera::{PerspectiveCamera, matrix_bytes};
use render::desc::{
    AddressMode, BlendMode, BufferUsage, DepthState, Filter, PassDesc, PipelineDesc, Rect,
    RenderTarget, SamplerDesc, ShaderDesc, ShaderStage, SubmitStats, TextureDesc, TextureFormat,
};
use render::device::IRenderDevice;
use render::error::RenderError;
use render::ids::{ShaderId, SurfaceId, TextureId};
use render::mesh::{
    CUBE_INDEX_COUNT, MESH_VERTEX_STRIDE, MeshBindings, cube_indices, cube_vertices,
    encode_mesh_vertices, mesh_vertex_attributes, record_mesh,
};
use render::stream::CommandStream;
use std::fmt;

/// The mesh shader asset: the shaders of `shaders/` made by `shaderc` (GLSL text for OpenGL).
pub const MESH_GLSL: &[u8] = include_bytes!("../assets/mesh.dshd");

/// The same shaders as SPIR-V (dshd backend 1), for the Vulkan device.
pub const MESH_SPIRV: &[u8] = include_bytes!("../assets/mesh.spv.dshd");

/// The slot the cube's transform and texture are bound to.
const SLOT: u8 = 0;
/// Bytes of the model-view-projection matrix (a std140 `mat4`).
const MATRIX_BYTES: u32 = 64;
const CLEAR: [f32; 4] = [0.05, 0.07, 0.12, 1.0];
/// The far plane value a cleared depth buffer holds.
const CLEAR_DEPTH: f32 = 1.0;
/// Radians the cube turns about Y per frame.
const SPIN_Y: f32 = 0.031;
/// Radians the cube turns about X per frame.
const SPIN_X: f32 = 0.017;
/// Distance of the camera from the cube, in cube edges.
/// Frames after which the angle counter starts again, which keeps the angles small (and exact);
/// the cube jumps to its start pose once then (about 28 minutes at 60 frames per second).
const TURN_FRAMES: u32 = 100_000;
/// Capacity of the command stream: one pass and one draw need far less.
const STREAM_CAPACITY: usize = 1 << 8;
const DISTANCE: f32 = 3.0;
/// Vertical field of view in radians (about 57 degrees).
const FOV_Y: f32 = 1.0;
const NEAR: f32 = 0.1;
const FAR: f32 = 20.0;

/// Why the sample could not build or draw a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeshError {
    /// The device refused something.
    Render(RenderError),
    /// The shader asset does not fit the mesh vertex layout the sample draws.
    Asset(&'static str),
    /// The view size gives no camera (a zero side).
    NoCamera,
    /// The cube's vertices do not fit the byte buffer.
    Staging,
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Render(error) => write!(f, "render device: {error}"),
            Self::Asset(reason) => write!(f, "shader asset: {reason}"),
            Self::NoCamera => write!(f, "no camera for this view size"),
            Self::Staging => write!(f, "vertex bytes do not fit"),
        }
    }
}

impl std::error::Error for MeshError {}

impl From<RenderError> for MeshError {
    fn from(error: RenderError) -> Self {
        Self::Render(error)
    }
}

/// The vertex and fragment modules, after checking that the asset's reflection matches what the
/// sample feeds it: position (vec3), normal (vec3) and uv (vec2), the transform block and the
/// texture on the slot the sample binds.
///
/// # Errors
/// [`MeshError::Asset`] when a module or a binding does not fit.
pub fn check_asset<'a>(program: &Program<'a>) -> Result<(Module<'a>, Module<'a>), MeshError> {
    let (Some(vertex), Some(fragment)) = (
        program.module_for(Stage::Vertex),
        program.module_for(Stage::Fragment),
    ) else {
        return Err(MeshError::Asset(
            "a vertex and a fragment module are needed",
        ));
    };
    let layout: Vec<(u8, u8)> = vertex
        .inputs()
        .map(|i| (i.location, i.components))
        .collect();
    if layout != [(0, 3), (1, 3), (2, 2)] {
        return Err(MeshError::Asset(
            "vertex inputs must be position, normal (vec3) and uv (vec2)",
        ));
    }
    let transform_ok = vertex
        .uniform()
        .is_some_and(|block| block.binding == SLOT && block.size >= MATRIX_BYTES);
    if !transform_ok {
        return Err(MeshError::Asset("the transform block must be at binding 0"));
    }
    if fragment.texture_binding() != Some(SLOT) {
        return Err(MeshError::Asset("the texture must be at binding 0"));
    }
    Ok((vertex, fragment))
}

fn create_shader(
    device: &mut dyn IRenderDevice,
    stage: ShaderStage,
    module: &Module<'_>,
) -> Result<ShaderId, RenderError> {
    device.create_shader(ShaderDesc {
        stage,
        // A SPIR-V module carries a binary and no text; a GLSL module the other way round.
        code: module.spirv().unwrap_or_else(|| module.code().as_bytes()),
        entry: "main",
    })
}

/// The mesh pipeline: opaque, depth test and write, `Depth32F`.
fn create_pipeline(
    device: &mut dyn IRenderDevice,
    vertex: &Module<'_>,
    fragment: &Module<'_>,
) -> Result<render::ids::PipelineId, RenderError> {
    let vertex_shader = create_shader(device, ShaderStage::Vertex, vertex)?;
    let fragment_shader = create_shader(device, ShaderStage::Fragment, fragment)?;
    device.create_pipeline(PipelineDesc {
        vertex_shader,
        fragment_shader,
        vertex_stride: MESH_VERTEX_STRIDE,
        attributes: &mesh_vertex_attributes(),
        blend: BlendMode::Opaque,
        depth: DepthState {
            test: true,
            write: true,
        },
        color_format: TextureFormat::Rgba8,
        depth_format: Some(TextureFormat::Depth32F),
    })
}

/// A depth texture of the size of the view.
fn depth_texture(
    device: &mut dyn IRenderDevice,
    size: (u32, u32),
) -> Result<TextureId, RenderError> {
    device.create_texture(TextureDesc {
        width: size.0,
        height: size.1,
        mip_count: 1,
        format: TextureFormat::Depth32F,
    })
}

/// The camera of a view of `size` pixels: it looks at the cube from [`DISTANCE`] on the Z axis.
///
/// # Errors
/// [`MeshError::NoCamera`] for a zero side.
#[expect(
    clippy::cast_precision_loss,
    reason = "window sizes are far below 2^24, so the f32 ratio is exact enough for a lens"
)]
pub fn camera_for(size: (u32, u32)) -> Result<PerspectiveCamera, MeshError> {
    if size.0 == 0 || size.1 == 0 {
        return Err(MeshError::NoCamera);
    }
    Ok(PerspectiveCamera {
        position: Vec3::new(0.0, 0.0, DISTANCE),
        target: Vec3::ZERO,
        up: Vec3::new(0.0, 1.0, 0.0),
        fov_y: FOV_Y,
        aspect: size.0 as f32 / size.1 as f32,
        near: NEAR,
        far: FAR,
    })
}

/// The model matrix of `frame`: the cube turned about Y and X by a fixed step per frame.
#[expect(
    clippy::cast_precision_loss,
    reason = "a frame counter far below 2^24 is exact in f32; the angle only has to look smooth"
)]
#[must_use]
pub fn model_for(frame: u32) -> Mat4 {
    let turns = (frame % TURN_FRAMES) as f32;
    Mat4::from_rotation_y(turns * SPIN_Y) * Mat4::from_rotation_x(turns * SPIN_X)
}

/// The sample's state between frames. After `new`, `draw` allocates nothing on its own.
pub struct MeshSample {
    surface: SurfaceId,
    size: (u32, u32),
    camera: PerspectiveCamera,
    bindings: MeshBindings,
    depth: TextureId,
    stream: CommandStream,
}

impl MeshSample {
    /// Builds the pipeline, buffers, textures and sampler on `device`, for a view of `size`
    /// pixels on `surface`.
    ///
    /// # Errors
    /// [`MeshError::Asset`] when the asset does not fit the mesh vertex layout,
    /// [`MeshError::NoCamera`] for a zero side, or the device's error.
    pub fn new(
        device: &mut dyn IRenderDevice,
        program: &Program<'_>,
        surface: SurfaceId,
        size: (u32, u32),
    ) -> Result<Self, MeshError> {
        let camera = camera_for(size)?;
        let (vertex, fragment) = check_asset(program)?;
        let pipeline = create_pipeline(device, &vertex, &fragment)?;
        let mut vertex_bytes = vec![0u8; cube_vertices().len() * MESH_VERTEX_STRIDE as usize];
        if !encode_mesh_vertices(&cube_vertices(), &mut vertex_bytes) {
            return Err(MeshError::Staging);
        }
        let index_bytes: Vec<u8> = cube_indices()
            .iter()
            .flat_map(|index| index.to_le_bytes())
            .collect();
        let bindings = MeshBindings {
            pipeline,
            vertices: create_buffer(device, BufferUsage::Vertex, &vertex_bytes)?,
            uniforms: create_buffer(device, BufferUsage::Uniform, &[0; MATRIX_BYTES as usize])?,
            indices: create_buffer(device, BufferUsage::Index, &index_bytes)?,
            texture: create_texture(device, CHECKER_SIDE, &checker_pixels())?,
            sampler: device.create_sampler(SamplerDesc {
                min_filter: Filter::Nearest,
                mag_filter: Filter::Nearest,
                address: AddressMode::Clamp,
            })?,
        };
        Ok(Self {
            surface,
            size,
            camera,
            bindings,
            depth: depth_texture(device, size)?,
            stream: CommandStream::with_capacity(STREAM_CAPACITY),
        })
    }

    /// The view size in pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Follows a window resize: the surface, the depth texture and the lens get the new size.
    ///
    /// # Errors
    /// [`MeshError::NoCamera`] for a zero side (nothing changes), or the device's error.
    pub fn resize(
        &mut self,
        device: &mut dyn IRenderDevice,
        size: (u32, u32),
    ) -> Result<(), MeshError> {
        let camera = camera_for(size)?;
        device.resize_surface(self.surface, size.0, size.1)?;
        let depth = depth_texture(device, size)?;
        device.destroy_texture(self.depth);
        self.depth = depth;
        self.camera = camera;
        self.size = size;
        Ok(())
    }

    /// Records the frame: one cleared pass with depth into the backbuffer, the cube turned to
    /// `frame`.
    fn record(&mut self, device: &mut dyn IRenderDevice, frame: u32) -> Result<(), MeshError> {
        let mvp = self
            .camera
            .mvp(&model_for(frame))
            .ok_or(MeshError::NoCamera)?;
        device.write_buffer(self.bindings.uniforms, 0, &matrix_bytes(&mvp))?;
        self.stream.clear();
        self.stream.begin_pass(PassDesc {
            target: RenderTarget::Backbuffer,
            clear_color: Some(CLEAR),
            clear_depth: Some(CLEAR_DEPTH),
            depth: Some(self.depth),
        });
        self.stream.set_viewport(Rect {
            x: 0,
            y: 0,
            width: self.size.0,
            height: self.size.1,
        });
        record_mesh(&mut self.stream, &self.bindings, CUBE_INDEX_COUNT);
        self.stream.end_pass();
        Ok(())
    }

    /// Draws one frame (`frame` turns the cube) and presents it.
    ///
    /// # Errors
    /// The first [`MeshError`] of recording or the device (a lost surface included).
    pub fn draw(
        &mut self,
        device: &mut dyn IRenderDevice,
        frame: u32,
    ) -> Result<SubmitStats, MeshError> {
        self.record(device, frame)?;
        let id = device.begin_frame(self.surface)?;
        let submitted = device.submit(id, &self.stream);
        let ended = device.end_frame(id);
        let stats = submitted?;
        ended?;
        Ok(stats)
    }
}
