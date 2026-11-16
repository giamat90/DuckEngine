//! The render side: the device resources built from the golden shader asset, and one `draw` per
//! frame that batches the sprites of a [`RenderScene`], uploads them, records the frame graph and
//! submits it. The device is passed in (`&mut dyn IRenderDevice`), so the same code runs on the
//! Null device in tests and on the OpenGL device in the window. After `new`, `draw` allocates
//! nothing on its own.

use crate::error::Error;
use crate::sandbox::Draw;
use app_physics_sample::mapping::{DISC_SIDE, Textures, disc_pixels};
use app_sprite_sample::SampleError;
use app_sprite_sample::sample::{
    SPRITE_BYTES, VERTICES_PER_SPRITE, check_asset, create_buffer, create_pipeline, create_texture,
    encode_staged,
};
use app_sprite_sample::scene::{CHECKER_SIDE, WHITE_TEXEL, checker_pixels};
use assets::dshd::Program;
use render::camera::matrix_bytes;
use render::desc::{AddressMode, BufferUsage, Filter, Rect, SamplerDesc, SubmitStats};
use render::device::IRenderDevice;
use render::graph::RenderGraph;
use render::ids::SurfaceId;
use render::scene::RenderScene;
use render::sprite::{SpriteBatcher, SpriteBindings, SpriteVertex, quad_indices};
use render::stream::CommandStream;

/// Sprites one frame can hold.
pub const SPRITE_CAPACITY: usize = 256;
const CLEAR: [f32; 4] = [0.05, 0.07, 0.12, 1.0];
/// Bytes the command stream starts with (it does not grow in a frame of this size).
const STREAM_BYTES: usize = 4096;

/// The device resources and the buffers of a frame.
pub struct Renderer {
    surface: SurfaceId,
    size: (u32, u32),
    bindings: SpriteBindings,
    textures: Textures,
    batcher: SpriteBatcher,
    staging: Vec<SpriteVertex>,
    vertex_bytes: Vec<u8>,
    stream: CommandStream,
    stats: SubmitStats,
}

impl Renderer {
    /// Builds the pipeline, buffers, textures and sampler on `device`, for a view of `size`
    /// pixels on `surface`.
    ///
    /// # Errors
    /// [`Error::Render`] when the shader asset does not fit the sprite vertex layout or the
    /// device refuses something.
    pub fn new(
        device: &mut dyn IRenderDevice,
        program: &Program<'_>,
        surface: SurfaceId,
        size: (u32, u32),
    ) -> Result<Self, Error> {
        let (vertex, fragment) = check_asset(program)?;
        let pipeline = create_pipeline(device, &vertex, &fragment)?;
        let sampler = device
            .create_sampler(SamplerDesc {
                min_filter: Filter::Nearest,
                mag_filter: Filter::Nearest,
                address: AddressMode::Clamp,
            })
            .map_err(SampleError::Render)?;
        let indices: Vec<u8> = quad_indices(SPRITE_CAPACITY)
            .iter()
            .flat_map(|index| index.to_le_bytes())
            .collect();
        let vertex_bytes = vec![0u8; SPRITE_CAPACITY * SPRITE_BYTES];
        Ok(Self {
            surface,
            size,
            bindings: SpriteBindings {
                pipeline,
                uniforms: create_buffer(device, BufferUsage::Uniform, &[0u8; 64])?,
                vertices: create_buffer(device, BufferUsage::Vertex, &vertex_bytes)?,
                indices: create_buffer(device, BufferUsage::Index, &indices)?,
                sampler,
            },
            textures: Textures {
                white: create_texture(device, 1, &WHITE_TEXEL)?,
                checker: create_texture(device, CHECKER_SIDE, &checker_pixels())?,
                disc: create_texture(device, u32::from(DISC_SIDE), &disc_pixels())?,
            },
            batcher: SpriteBatcher::new(SPRITE_CAPACITY).map_err(SampleError::Batch)?,
            staging: vec![SpriteVertex::default(); SPRITE_CAPACITY * VERTICES_PER_SPRITE],
            vertex_bytes,
            stream: CommandStream::with_capacity(STREAM_BYTES),
            stats: SubmitStats::default(),
        })
    }

    /// The textures the sandbox gives its sprites.
    #[must_use]
    pub fn textures(&self) -> Textures {
        self.textures
    }

    /// The view size in pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The command stream of the last frame.
    #[must_use]
    pub fn stream(&self) -> &CommandStream {
        &self.stream
    }

    /// What the device reported for the last frame.
    #[must_use]
    pub fn stats(&self) -> SubmitStats {
        self.stats
    }

    /// Follows a window resize: the surface gets the new size.
    ///
    /// # Errors
    /// [`Error::NoView`] for a zero side (nothing changes), or the device's error.
    pub fn resize(
        &mut self,
        device: &mut dyn IRenderDevice,
        size: (u32, u32),
    ) -> Result<(), Error> {
        if size.0 == 0 || size.1 == 0 {
            return Err(Error::NoView);
        }
        device.resize_surface(self.surface, size.0, size.1)?;
        self.size = size;
        Ok(())
    }

    /// Writes the camera, sorts, batches and uploads the sprites of `scene`.
    fn upload(&mut self, device: &mut dyn IRenderDevice, scene: &RenderScene) -> Result<(), Error> {
        let matrix = scene.view_projection().ok_or(SampleError::NoCamera)?;
        device.write_buffer(self.bindings.uniforms, 0, &matrix_bytes(&matrix))?;
        self.batcher
            .prepare_scene(scene, &mut self.staging)
            .map_err(SampleError::Batch)?;
        let written = encode_staged(&self.staging, scene.sprites().len(), &mut self.vertex_bytes)
            .ok_or(SampleError::Staging)?;
        let bytes = self
            .vertex_bytes
            .get(..written)
            .ok_or(SampleError::Staging)?;
        device.write_buffer(self.bindings.vertices, 0, bytes)?;
        Ok(())
    }

    /// Records the frame graph (one cleared pass to the back buffer) with the batches.
    fn record(&mut self) -> Result<(), Error> {
        self.stream.clear();
        let mut graph = RenderGraph::new();
        let back = graph.import_backbuffer().map_err(SampleError::Graph)?;
        graph
            .add_pass("scene", back, Some(CLEAR), &[])
            .map_err(SampleError::Graph)?;
        let view = Rect {
            x: 0,
            y: 0,
            width: self.size.0,
            height: self.size.1,
        };
        let (batcher, bindings) = (&self.batcher, &self.bindings);
        graph
            .compile()
            .record(&[], &mut self.stream, |_, stream| {
                stream.set_viewport(view);
                batcher.record(stream, bindings);
            })
            .map_err(SampleError::Graph)?;
        Ok(())
    }

    /// Draws one frame of `scene` on `device` and presents it.
    ///
    /// # Errors
    /// The first error of staging, recording or the device (a lost surface included).
    pub fn draw(
        &mut self,
        device: &mut dyn IRenderDevice,
        scene: &RenderScene,
    ) -> Result<(), Error> {
        self.upload(device, scene)?;
        self.record()?;
        let id = device.begin_frame(self.surface)?;
        let submitted = device.submit(id, &self.stream);
        let ended = device.end_frame(id);
        self.stats = submitted?;
        ended?;
        Ok(())
    }
}

/// A renderer and its device, as the target of the render phase.
pub struct Target<'a> {
    /// The device.
    pub device: &'a mut dyn IRenderDevice,
    /// The renderer.
    pub renderer: &'a mut Renderer,
}

impl Draw for Target<'_> {
    fn draw(&mut self, scene: &RenderScene) -> Result<(), Error> {
        self.renderer.draw(self.device, scene)
    }
}
