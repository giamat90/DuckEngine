//! The sample's frame over any render device: the resources built from the golden shader asset,
//! and one `draw` per frame that turns the world into sprites (bodies, then the debug drawing),
//! batches them, records the frame graph and submits it. The device is passed in
//! (`&mut dyn IRenderDevice`), so the same code runs on the Null device in tests and on the OpenGL
//! device in the window. After `new`, `draw` allocates nothing on its own.

use crate::mapping::{DISC_SIDE, SpriteDebugDraw, Textures, body_sprites, disc_pixels};
use app_sprite_sample::SampleError;
use app_sprite_sample::sample::{
    SPRITE_BYTES, VERTICES_PER_SPRITE, camera_bytes, check_asset, create_buffer, create_pipeline,
    create_texture, encode_staged,
};
use app_sprite_sample::scene::{CHECKER_SIDE, WHITE_TEXEL, checker_pixels};
use assets::dshd::Program;
use physics::d2::{World, draw_debug};
use render::desc::{AddressMode, BufferUsage, Filter, Rect, SamplerDesc, SubmitStats};
use render::device::IRenderDevice;
use render::graph::RenderGraph;
use render::ids::SurfaceId;
use render::sprite::{Sprite, SpriteBatcher, SpriteBindings, SpriteVertex, quad_indices};
use render::stream::CommandStream;

/// Sprites one frame can hold: the bodies and the debug drawing. A debug sprite past it is dropped
/// and counted ([`View::dropped`]).
pub const SPRITE_CAPACITY: usize = 16_384;
const CLEAR: [f32; 4] = [0.05, 0.07, 0.12, 1.0];
/// Bytes the command stream starts with (it does not grow in a frame of this size).
const STREAM_BYTES: usize = 1 << 12;

/// The size of the part of the world, in sprite pixels, that a window of `size` shows.
fn zoomed(size: (u32, u32), zoom_out: u32) -> (u32, u32) {
    (
        size.0.saturating_mul(zoom_out.max(1)),
        size.1.saturating_mul(zoom_out.max(1)),
    )
}

/// The device resources and the buffers of a frame.
pub struct View {
    surface: SurfaceId,
    size: (u32, u32),
    bindings: SpriteBindings,
    textures: Textures,
    batcher: SpriteBatcher,
    sprites: Vec<Sprite>,
    staging: Vec<SpriteVertex>,
    vertex_bytes: Vec<u8>,
    stream: CommandStream,
    dropped: usize,
    zoom_out: u32,
}

impl View {
    /// Builds the pipeline, buffers, textures and sampler on `device`, for a view of `size`
    /// pixels on `surface`.
    ///
    /// # Errors
    /// [`SampleError::Asset`] when the shader asset does not fit the sprite vertex layout,
    /// [`SampleError::NoCamera`] for a zero side, or the device's error.
    pub fn new(
        device: &mut dyn IRenderDevice,
        program: &Program<'_>,
        surface: SurfaceId,
        size: (u32, u32),
    ) -> Result<Self, SampleError> {
        let camera = camera_bytes(size)?;
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
        let vertex_bytes = vec![0u8; SPRITE_CAPACITY * SPRITE_BYTES];
        Ok(Self {
            surface,
            size,
            bindings: SpriteBindings {
                pipeline,
                uniforms: create_buffer(device, BufferUsage::Uniform, &camera)?,
                vertices: create_buffer(device, BufferUsage::Vertex, &vertex_bytes)?,
                indices: create_buffer(device, BufferUsage::Index, &indices)?,
                sampler,
            },
            textures: Textures {
                white: create_texture(device, 1, &WHITE_TEXEL)?,
                checker: create_texture(device, CHECKER_SIDE, &checker_pixels())?,
                disc: create_texture(device, u32::from(DISC_SIDE), &disc_pixels())?,
            },
            batcher: SpriteBatcher::new(SPRITE_CAPACITY)?,
            sprites: Vec::with_capacity(SPRITE_CAPACITY),
            staging: vec![SpriteVertex::default(); SPRITE_CAPACITY * VERTICES_PER_SPRITE],
            vertex_bytes,
            stream: CommandStream::with_capacity(STREAM_BYTES),
            dropped: 0,
            zoom_out: 1,
        })
    }

    /// The view size in pixels.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The sprites of the last frame, in the order they were made (bodies, then the debug drawing).
    #[must_use]
    pub fn sprites(&self) -> &[Sprite] {
        &self.sprites
    }

    /// Debug sprites dropped because the frame was full, since the start.
    #[must_use]
    pub fn dropped(&self) -> usize {
        self.dropped
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
        let camera = camera_bytes(zoomed(size, self.zoom_out))?;
        device.resize_surface(self.surface, size.0, size.1)?;
        device.write_buffer(self.bindings.uniforms, 0, &camera)?;
        self.size = size;
        Ok(())
    }

    /// Shows `zoom_out` times more of the world in the same window (1 is 20 pixels to the metre).
    ///
    /// # Errors
    /// The render device's error.
    pub fn set_zoom_out(
        &mut self,
        device: &mut dyn IRenderDevice,
        zoom_out: u32,
    ) -> Result<(), SampleError> {
        self.zoom_out = zoom_out.max(1);
        let camera = camera_bytes(zoomed(self.size, self.zoom_out))?;
        device.write_buffer(self.bindings.uniforms, 0, &camera)?;
        Ok(())
    }

    /// How many times more of the world a pixel covers.
    #[must_use]
    pub fn zoom_out(&self) -> u32 {
        self.zoom_out
    }

    /// Makes the sprites of `world`: the bodies, then the debug drawing when `debug`. The bodies
    /// (at most `MAX_BODIES`, 128) are far below [`SPRITE_CAPACITY`]; only debug sprites can overflow.
    fn fill(&mut self, world: &World, debug: bool) {
        body_sprites(world, &self.textures, &mut self.sprites);
        if debug {
            let mut sink = SpriteDebugDraw::new(&mut self.sprites, self.textures.white);
            draw_debug(world, &mut sink);
            self.dropped += sink.dropped();
        }
    }

    /// Sorts, batches and uploads the sprites now in `self.sprites`.
    fn upload(&mut self, device: &mut dyn IRenderDevice) -> Result<(), SampleError> {
        self.batcher.prepare(&self.sprites, &mut self.staging)?;
        let written = encode_staged(&self.staging, self.sprites.len(), &mut self.vertex_bytes)
            .ok_or(SampleError::Staging)?;
        let bytes = self
            .vertex_bytes
            .get(..written)
            .ok_or(SampleError::Staging)?;
        device.write_buffer(self.bindings.vertices, 0, bytes)?;
        Ok(())
    }

    /// Records the frame graph (one cleared pass to the back buffer) with the batches.
    fn record(&mut self) -> Result<(), SampleError> {
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
        let (batcher, bindings) = (&self.batcher, &self.bindings);
        graph.compile().record(&[], &mut self.stream, |_, stream| {
            stream.set_viewport(view);
            batcher.record(stream, bindings);
        })?;
        Ok(())
    }

    /// Draws one frame of `world` (the debug drawing included when `debug`) and presents it.
    ///
    /// # Errors
    /// The first [`SampleError`] of staging, recording or the device (a lost surface included).
    pub fn draw(
        &mut self,
        device: &mut dyn IRenderDevice,
        world: &World,
        debug: bool,
    ) -> Result<SubmitStats, SampleError> {
        self.fill(world, debug);
        self.upload(device)?;
        self.record()?;
        let id = device.begin_frame(self.surface)?;
        let submitted = device.submit(id, &self.stream);
        let ended = device.end_frame(id);
        let stats = submitted?;
        ended?;
        Ok(stats)
    }
}
