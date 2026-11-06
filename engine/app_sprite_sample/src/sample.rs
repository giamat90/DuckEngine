//! The sample's frame, over any render device: the resources built from the golden shader asset,
//! and one `draw` per frame that records the triangle or the sprites and submits them. The
//! device is passed in (`&mut dyn IRenderDevice`), so the same code runs on the Null device in
//! tests and on the OpenGL device in the window.

use crate::input::Controls;
use crate::scene::{
    CHECKER_SIDE, Scene, SceneTextures, WHITE_TEXEL, checker_pixels, fill_sprites,
    triangle_vertices,
};
use assets::dshd::{Module, Program, Stage};
use base::math::Vec2;
use render::camera::{OrthographicCamera, matrix_bytes};
use render::desc::{
    AddressMode, BlendMode, BufferDesc, BufferUsage, DepthState, Filter, PipelineDesc, Rect,
    SamplerDesc, ShaderDesc, ShaderStage, SubmitStats, TextureDesc, TextureFormat,
};
use render::device::IRenderDevice;
use render::error::RenderError;
use render::graph::{GraphError, RenderGraph};
use render::ids::{BufferId, PipelineId, ShaderId, SurfaceId, TextureId};
use render::sprite::{
    BatchError, Sprite, SpriteBatcher, SpriteBindings, SpriteVertex, VERTEX_STRIDE,
    encode_vertices, quad_indices, vertex_attributes,
};
use render::stream::CommandStream;
use std::fmt;

/// The shader asset the sample draws with: the golden `sprite.dshd` made by `tools_shaderc`, read
/// as data (the sample links no shader compiler).
pub const GOLDEN_SHADER: &[u8] = include_bytes!("../../tools_shaderc/golden/sprite.dshd");

/// Sprites one frame can hold.
const SPRITE_CAPACITY: usize = 64;
const VERTICES_PER_SPRITE: usize = 4;
/// Bytes of one sprite's four vertices.
const SPRITE_BYTES: usize = VERTICES_PER_SPRITE * VERTEX_STRIDE as usize;
/// Bytes of the camera matrix (a std140 `mat4`).
const CAMERA_BYTES: u32 = 64;
/// Bytes of the three triangle vertices.
const TRIANGLE_BYTES: usize = 3 * VERTEX_STRIDE as usize;
const CLEAR: [f32; 4] = [0.05, 0.07, 0.12, 1.0];
/// The slot the sprite batcher binds the camera and the texture to.
const SLOT: u8 = 0;

/// Why the sample could not build or draw a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SampleError {
    /// The device refused something.
    Render(RenderError),
    /// The sprite batcher refused the sprites.
    Batch(BatchError),
    /// The frame graph could not be declared or recorded.
    Graph(GraphError),
    /// The shader asset does not fit the sprite vertex layout the sample draws.
    Asset(&'static str),
    /// The view size gives no camera (a zero side).
    NoCamera,
    /// The staged vertices do not fit the byte buffer.
    Staging,
}

impl fmt::Display for SampleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Render(error) => write!(f, "render device: {error}"),
            Self::Batch(error) => write!(f, "sprite batcher: {error}"),
            Self::Graph(error) => write!(f, "frame graph: {error}"),
            Self::Asset(reason) => write!(f, "shader asset: {reason}"),
            Self::NoCamera => write!(f, "no camera for this view size"),
            Self::Staging => write!(f, "vertex bytes do not fit"),
        }
    }
}

impl std::error::Error for SampleError {}

impl From<RenderError> for SampleError {
    fn from(error: RenderError) -> Self {
        Self::Render(error)
    }
}

impl From<BatchError> for SampleError {
    fn from(error: BatchError) -> Self {
        Self::Batch(error)
    }
}

impl From<GraphError> for SampleError {
    fn from(error: GraphError) -> Self {
        Self::Graph(error)
    }
}

/// The vertex and fragment modules, after checking that the asset's reflection matches what the
/// sample feeds it: the sprite vertex (position, uv, colour), the camera block and the texture on
/// the slot the batcher binds.
fn check_asset<'a>(program: &Program<'a>) -> Result<(Module<'a>, Module<'a>), SampleError> {
    let (Some(vertex), Some(fragment)) = (
        program.module_for(Stage::Vertex),
        program.module_for(Stage::Fragment),
    ) else {
        return Err(SampleError::Asset(
            "a vertex and a fragment module are needed",
        ));
    };
    let layout: Vec<(u8, u8)> = vertex
        .inputs()
        .map(|i| (i.location, i.components))
        .collect();
    if layout != [(0, 2), (1, 2), (2, 4)] {
        return Err(SampleError::Asset(
            "vertex inputs must be position, uv (vec2) and colour (vec4)",
        ));
    }
    let camera_ok = vertex
        .uniform()
        .is_some_and(|block| block.binding == SLOT && block.size >= CAMERA_BYTES);
    if !camera_ok {
        return Err(SampleError::Asset("the camera block must be at binding 0"));
    }
    if fragment.texture_binding() != Some(SLOT) {
        return Err(SampleError::Asset("the texture must be at binding 0"));
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
        code: module.code().as_bytes(),
        entry: "main",
    })
}

fn create_pipeline(
    device: &mut dyn IRenderDevice,
    vertex: &Module<'_>,
    fragment: &Module<'_>,
) -> Result<PipelineId, RenderError> {
    let vertex_shader = create_shader(device, ShaderStage::Vertex, vertex)?;
    let fragment_shader = create_shader(device, ShaderStage::Fragment, fragment)?;
    device.create_pipeline(PipelineDesc {
        vertex_shader,
        fragment_shader,
        vertex_stride: VERTEX_STRIDE,
        attributes: &vertex_attributes(),
        blend: BlendMode::Alpha,
        depth: DepthState {
            test: false,
            write: false,
        },
        color_format: TextureFormat::Rgba8,
    })
}

fn create_buffer(
    device: &mut dyn IRenderDevice,
    usage: BufferUsage,
    bytes: &[u8],
) -> Result<BufferId, RenderError> {
    let size = u32::try_from(bytes.len()).map_err(|_| RenderError::InvalidDescription)?;
    let buffer = device.create_buffer(BufferDesc { size, usage })?;
    device.write_buffer(buffer, 0, bytes)?;
    Ok(buffer)
}

fn create_texture(
    device: &mut dyn IRenderDevice,
    side: u32,
    pixels: &[u8],
) -> Result<TextureId, RenderError> {
    let texture = device.create_texture(TextureDesc {
        width: side,
        height: side,
        mip_count: 1,
        format: TextureFormat::Rgba8,
    })?;
    let all = Rect {
        x: 0,
        y: 0,
        width: side,
        height: side,
    };
    device.write_texture(texture, all, pixels)?;
    Ok(texture)
}

/// The device resources of the sample.
struct Gpu {
    bindings: SpriteBindings,
    triangle: BufferId,
    textures: SceneTextures,
}

impl Gpu {
    fn create(
        device: &mut dyn IRenderDevice,
        program: &Program<'_>,
        camera: &[u8; 64],
    ) -> Result<Self, SampleError> {
        let (vertex, fragment) = check_asset(program)?;
        let pipeline = create_pipeline(device, &vertex, &fragment)?;
        let sampler = device.create_sampler(SamplerDesc {
            min_filter: Filter::Nearest,
            mag_filter: Filter::Nearest,
            address: AddressMode::Clamp,
        })?;
        let indices: Vec<u8> = quad_indices(SPRITE_CAPACITY)
            .iter()
            .flat_map(|index| index.to_le_bytes())
            .collect();
        let mut triangle = vec![0u8; TRIANGLE_BYTES];
        if !encode_vertices(&triangle_vertices(), &mut triangle) {
            return Err(SampleError::Staging);
        }
        let capacity = vec![0u8; SPRITE_CAPACITY * SPRITE_BYTES];
        Ok(Self {
            bindings: SpriteBindings {
                pipeline,
                uniforms: create_buffer(device, BufferUsage::Uniform, camera)?,
                vertices: create_buffer(device, BufferUsage::Vertex, &capacity)?,
                indices: create_buffer(device, BufferUsage::Index, &indices)?,
                sampler,
            },
            triangle: create_buffer(device, BufferUsage::Vertex, &triangle)?,
            textures: SceneTextures {
                white: create_texture(device, 1, &WHITE_TEXEL)?,
                checker: create_texture(device, CHECKER_SIDE, &checker_pixels())?,
            },
        })
    }
}

/// The camera of a view of `size` pixels: one unit per pixel, the origin in the middle, Y up.
fn camera_bytes(size: (u32, u32)) -> Result<[u8; 64], SampleError> {
    OrthographicCamera::pixel_perfect(size.0, size.1, Vec2::new(0.0, 0.0), 1.0)
        .and_then(|camera| camera.view_projection())
        .map(|matrix| matrix_bytes(&matrix))
        .ok_or(SampleError::NoCamera)
}

/// Encodes the vertices of the first `sprites` staged sprites into `out`; the bytes written, or
/// `None` when the staging or the byte buffer is too small.
fn encode_staged(staging: &[SpriteVertex], sprites: usize, out: &mut [u8]) -> Option<usize> {
    let vertices = sprites * VERTICES_PER_SPRITE;
    let bytes = vertices * VERTEX_STRIDE as usize;
    let staged = staging.get(..vertices)?;
    encode_vertices(staged, out.get_mut(..bytes)?).then_some(bytes)
}

/// Records the triangle: the sprite pipeline with the white texture and the triangle's buffer.
fn record_triangle(stream: &mut CommandStream, gpu: &Gpu) {
    let bindings = &gpu.bindings;
    stream.set_pipeline(bindings.pipeline);
    stream.bind_uniforms(u32::from(SLOT), bindings.uniforms);
    stream.bind_texture(u32::from(SLOT), gpu.textures.white, bindings.sampler);
    stream.bind_vertex_buffer(0, gpu.triangle, 0);
    stream.draw(3, 1);
}

/// The sample's state between frames. After `new`, `draw` allocates nothing on its own.
pub struct Sample {
    surface: SurfaceId,
    size: (u32, u32),
    gpu: Gpu,
    batcher: SpriteBatcher,
    sprites: Vec<Sprite>,
    staging: Vec<SpriteVertex>,
    vertex_bytes: Vec<u8>,
    stream: CommandStream,
}

impl Sample {
    /// Builds the pipeline, buffers, textures and sampler on `device`, for a view of `size` pixels
    /// on `surface`.
    ///
    /// # Errors
    /// [`SampleError::Asset`] when the asset does not fit the sprite vertex layout,
    /// [`SampleError::NoCamera`] for a zero side, or the device's error.
    pub fn new(
        device: &mut dyn IRenderDevice,
        program: &Program<'_>,
        surface: SurfaceId,
        size: (u32, u32),
    ) -> Result<Self, SampleError> {
        let gpu = Gpu::create(device, program, &camera_bytes(size)?)?;
        Ok(Self {
            surface,
            size,
            gpu,
            batcher: SpriteBatcher::new(SPRITE_CAPACITY)?,
            sprites: Vec::with_capacity(SPRITE_CAPACITY),
            staging: vec![SpriteVertex::default(); SPRITE_CAPACITY * VERTICES_PER_SPRITE],
            vertex_bytes: vec![0; SPRITE_CAPACITY * SPRITE_BYTES],
            stream: CommandStream::with_capacity(1 << 12),
        })
    }

    /// The view size in pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Follows a window resize: the surface and the camera get the new size.
    ///
    /// # Errors
    /// [`SampleError::NoCamera`] for a zero side (nothing changes), or the device's error.
    pub fn resize(
        &mut self,
        device: &mut dyn IRenderDevice,
        size: (u32, u32),
    ) -> Result<(), SampleError> {
        let camera = camera_bytes(size)?;
        device.resize_surface(self.surface, size.0, size.1)?;
        device.write_buffer(self.gpu.bindings.uniforms, 0, &camera)?;
        self.size = size;
        Ok(())
    }

    /// Sorts, batches and uploads the sprites of `frame`.
    fn stage_sprites(
        &mut self,
        device: &mut dyn IRenderDevice,
        frame: u32,
    ) -> Result<(), SampleError> {
        fill_sprites(frame, &self.gpu.textures, &mut self.sprites);
        self.upload_sprites(device)
    }

    /// Sorts, batches and uploads the sprites now in `self.sprites`.
    fn upload_sprites(&mut self, device: &mut dyn IRenderDevice) -> Result<(), SampleError> {
        self.batcher.prepare(&self.sprites, &mut self.staging)?;
        let written = encode_staged(&self.staging, self.sprites.len(), &mut self.vertex_bytes)
            .ok_or(SampleError::Staging)?;
        let bytes = self
            .vertex_bytes
            .get(..written)
            .ok_or(SampleError::Staging)?;
        device.write_buffer(self.gpu.bindings.vertices, 0, bytes)?;
        Ok(())
    }

    /// Records the frame graph (one cleared pass to the back buffer) with the scene's draws.
    fn record(&mut self, scene: Scene) -> Result<(), SampleError> {
        self.stream.clear();
        let mut graph = RenderGraph::new();
        let back = graph.import_backbuffer()?;
        graph.add_pass("scene", back, Some(CLEAR), &[])?;
        let view = Rect {
            x: 0,
            y: 0,
            width: self.size.0,
            height: self.size.1,
        };
        let (batcher, gpu) = (&self.batcher, &self.gpu);
        graph.compile().record(&[], &mut self.stream, |_, stream| {
            stream.set_viewport(view);
            match scene {
                Scene::Triangle => record_triangle(stream, gpu),
                Scene::Sprites => batcher.record(stream, &gpu.bindings),
            }
        })?;
        Ok(())
    }

    /// Draws one frame of `scene` (`frame` animates the sprites) and presents it.
    ///
    /// # Errors
    /// The first [`SampleError`] of staging, recording or the device (a lost surface included).
    pub fn draw(
        &mut self,
        device: &mut dyn IRenderDevice,
        frame: u32,
        scene: Scene,
    ) -> Result<SubmitStats, SampleError> {
        if scene == Scene::Sprites {
            self.stage_sprites(device, frame)?;
        }
        self.record(scene)?;
        self.present(device)
    }

    /// Draws one frame of the interactive example: the sprites of `controls`, and presents it.
    ///
    /// # Errors
    /// The first [`SampleError`] of staging, recording or the device (a lost surface included).
    pub fn draw_controls(
        &mut self,
        device: &mut dyn IRenderDevice,
        controls: &Controls,
    ) -> Result<SubmitStats, SampleError> {
        controls.fill_sprites(&self.gpu.textures, &mut self.sprites);
        self.upload_sprites(device)?;
        self.record(Scene::Sprites)?;
        self.present(device)
    }

    /// Submits the recorded stream and presents the frame.
    fn present(&mut self, device: &mut dyn IRenderDevice) -> Result<SubmitStats, SampleError> {
        let id = device.begin_frame(self.surface)?;
        let submitted = device.submit(id, &self.stream);
        let ended = device.end_frame(id);
        let stats = submitted?;
        ended?;
        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertex(x: f32) -> SpriteVertex {
        SpriteVertex {
            position: [x, 0.0],
            uv: [0.0, 0.0],
            color: [1, 2, 3, 4],
        }
    }

    #[test]
    fn a_sprite_is_four_vertices_of_twenty_bytes() {
        assert_eq!(SPRITE_BYTES, 80);
        assert_eq!(TRIANGLE_BYTES, 60);
    }

    #[test]
    fn staged_sprites_are_encoded_up_to_the_count_asked() {
        let staging: Vec<_> = (0u8..12).map(|i| vertex(f32::from(i))).collect();
        let mut out = vec![0u8; 12 * 20];
        assert_eq!(encode_staged(&staging, 2, &mut out), Some(160));
        // The fifth vertex (first of the second sprite) is at byte 80; the ninth is not written.
        assert_eq!(out[80..84], 4.0f32.to_le_bytes());
        assert_eq!(out[140..144], 7.0f32.to_le_bytes());
        assert!(out[160..].iter().all(|b| *b == 0));
    }

    #[test]
    fn too_little_staging_or_too_small_a_buffer_is_refused() {
        let staging = vec![vertex(0.0); 8];
        assert_eq!(encode_staged(&staging, 3, &mut [0u8; 240]), None);
        assert_eq!(encode_staged(&staging, 2, &mut [0u8; 159]), None);
        assert_eq!(encode_staged(&staging, 2, &mut [0u8; 160]), Some(160));
    }
}
