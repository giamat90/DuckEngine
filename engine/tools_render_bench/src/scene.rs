//! The scene: sprites, textures and the Null device, with one method per stage of a frame so the
//! report can time each stage on its own.

use base::math::Vec2;
use base::rng::Pcg32;
use render::camera::{OrthographicCamera, matrix_bytes};
use render::desc::{
    AddressMode, BlendMode, BufferDesc, BufferUsage, DepthState, Filter, PipelineDesc, SamplerDesc,
    ShaderDesc, ShaderStage, SubmitStats, SurfaceDesc, TextureDesc, TextureFormat,
};
use render::device::IRenderDevice;
use render::graph::RenderGraph;
use render::ids::{SurfaceId, TextureId};
use render::null::NullDevice;
use render::sprite::{
    Sprite, SpriteBatcher, SpriteBindings, SpriteVertex, UvRect, VERTEX_STRIDE, encode_vertices,
    quad_indices, vertex_attributes,
};
use render::stream::CommandStream;
use std::fmt::Debug;

/// Sprites per frame.
pub const SPRITES: usize = 4096;
/// Textures the sprites share.
pub const TEXTURES: u32 = 8;
/// Drawing layers.
pub const LAYERS: u32 = 8;
/// Every layer uses every texture, so a frame has at least this many batches.
pub const BATCH_FLOOR: usize = 64;

/// Seed and stream of the generator that places the sprites.
pub const SEED: u64 = 0x3_13;
const STREAM: u64 = 1;
const CLEAR: [f32; 4] = [0.1, 0.1, 0.1, 1.0];
/// World units visible from bottom to top; the sprites are spread a little wider than the view.
const VIEW_HEIGHT: f32 = 100.0;
const ASPECT: f32 = 16.0 / 9.0;
const SPREAD_X: f32 = 180.0;
const SPREAD_Y: f32 = 100.0;
const SIZE_MIN: f32 = 1.0;
const SIZE_RANGE: f32 = 2.0;
const TEXTURE_SIDE: u32 = 64;
const SURFACE: (u32, u32) = (1280, 720);
/// Bytes of one staged sprite: four vertices.
const SPRITE_BYTES: usize = 4 * VERTEX_STRIDE as usize;

/// A device error or a bad scene, as text for the report.
fn text(error: impl Debug) -> String {
    format!("{error:?}")
}

/// A draw below `bound` from the generator, or an error text when `bound` is zero.
fn below(rng: &mut Pcg32, bound: u32) -> Result<u32, String> {
    rng.below(bound).map_err(text)
}

/// One seeded sprite over `textures` (at least one) and the layers.
fn make_sprite(rng: &mut Pcg32, textures: &[TextureId], rotated: bool) -> Result<Sprite, String> {
    let texture_count = u32::try_from(textures.len()).map_err(text)?;
    let index = usize::try_from(below(rng, texture_count)?).map_err(text)?;
    let texture = *textures.get(index).ok_or("no textures")?;
    // The angle is drawn either way so the other values do not depend on `rotated`.
    let angle = rng.next_f32() * std::f32::consts::TAU;
    let size = SIZE_MIN + rng.next_f32() * SIZE_RANGE;
    Ok(Sprite {
        texture,
        position: Vec2::new(
            (rng.next_f32() - 0.5) * SPREAD_X,
            (rng.next_f32() - 0.5) * SPREAD_Y,
        ),
        size: Vec2::new(size, size),
        angle: if rotated { angle } else { 0.0 },
        uv: UvRect::FULL,
        tint: rng.next_u32().to_le_bytes(),
        layer: i16::try_from(below(rng, LAYERS)?).map_err(text)?,
    })
}

/// `count` sprites at seeded positions, sizes and angles, spread over `textures` and the layers.
///
/// # Errors
/// Text when `textures` is empty.
fn make_sprites(
    seed: u64,
    textures: &[TextureId],
    count: usize,
    rotated: bool,
) -> Result<Vec<Sprite>, String> {
    let mut rng = Pcg32::new(seed, STREAM);
    (0..count)
        .map(|_| make_sprite(&mut rng, textures, rotated))
        .collect()
}

/// One benchmark frame and everything it needs, allocated once.
pub struct FrameScene {
    device: NullDevice,
    surface: SurfaceId,
    sprites: Vec<Sprite>,
    bindings: SpriteBindings,
    batcher: SpriteBatcher,
    staging: Vec<SpriteVertex>,
    vertex_bytes: Vec<u8>,
    stream: CommandStream,
}

impl FrameScene {
    /// Creates the device, its resources and the sprites.
    ///
    /// # Errors
    /// The device error as text, when the Null device refuses a resource.
    pub fn new() -> Result<Self, String> {
        Self::with_seed(SEED)
    }

    /// Creates the scene with the sprites placed from `seed`.
    ///
    /// # Errors
    /// The device error as text, when the Null device refuses a resource.
    pub fn with_seed(seed: u64) -> Result<Self, String> {
        Self::with_options(seed, true)
    }

    /// Creates the scene from `seed`; with `rotated` false every sprite has angle 0, so staging
    /// calls no `sin` or `cos` and the vertex bytes are the same on every platform.
    ///
    /// # Errors
    /// The device error as text, when the Null device refuses a resource.
    pub fn with_options(seed: u64, rotated: bool) -> Result<Self, String> {
        let mut device = NullDevice::new();
        let surface = device
            .create_surface(SurfaceDesc {
                native_token: 1,
                width: SURFACE.0,
                height: SURFACE.1,
            })
            .map_err(text)?;
        let textures = (0..TEXTURES)
            .map(|_| {
                device.create_texture(TextureDesc {
                    width: TEXTURE_SIDE,
                    height: TEXTURE_SIDE,
                    mip_count: 1,
                    format: TextureFormat::Rgba8,
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(text)?;
        let bindings = create_bindings(&mut device)?;
        let sprites = make_sprites(seed, &textures, SPRITES, rotated)?;
        Ok(Self {
            device,
            surface,
            sprites,
            bindings,
            batcher: SpriteBatcher::new(SPRITES).map_err(text)?,
            staging: vec![SpriteVertex::default(); SPRITES * 4],
            vertex_bytes: vec![0; SPRITES * SPRITE_BYTES],
            stream: CommandStream::with_capacity(1 << 16),
        })
    }

    /// Sorts the sprites, stages their vertices and groups them into batches.
    ///
    /// # Errors
    /// The batcher error as text.
    pub fn prepare(&mut self) -> Result<(), String> {
        self.batcher
            .prepare(&self.sprites, &mut self.staging)
            .map(|_| ())
            .map_err(text)
    }

    /// Encodes the staged vertices as the bytes a device upload takes.
    ///
    /// # Errors
    /// Text when the byte buffer does not fit the staged vertices.
    pub fn encode(&mut self) -> Result<(), String> {
        let used = self.sprites.len() * 4;
        let staged = self.staging.get(..used).ok_or("staging too small")?;
        if encode_vertices(staged, &mut self.vertex_bytes) {
            Ok(())
        } else {
            Err("vertex bytes do not fit".to_owned())
        }
    }

    /// Declares the frame graph (one pass to the back buffer, cleared) and records it with the
    /// sprite draws into the command stream, which is cleared first.
    ///
    /// # Errors
    /// The graph error as text.
    pub fn record(&mut self) -> Result<(), String> {
        self.stream.clear();
        let mut graph = RenderGraph::new();
        let back = graph.import_backbuffer().map_err(text)?;
        graph
            .add_pass("sprites", back, Some(CLEAR), &[])
            .map_err(text)?;
        let compiled = graph.compile();
        let (batcher, bindings) = (&self.batcher, &self.bindings);
        compiled
            .record(&[], &mut self.stream, |_, stream| {
                batcher.record(stream, bindings);
            })
            .map_err(text)
    }

    /// The CPU side of a frame: prepare, encode, record.
    ///
    /// # Errors
    /// The first stage error as text.
    pub fn cpu_frame(&mut self) -> Result<(), String> {
        self.prepare()?;
        self.encode()?;
        self.record()
    }

    /// Uploads the vertices and submits the stream to the Null device, which validates it.
    ///
    /// # Errors
    /// The device error as text.
    pub fn submit(&mut self) -> Result<SubmitStats, String> {
        self.device
            .write_buffer(self.bindings.vertices, 0, &self.vertex_bytes)
            .map_err(text)?;
        let frame = self.device.begin_frame(self.surface).map_err(text)?;
        let stats = self.device.submit(frame, &self.stream).map_err(text)?;
        self.device.end_frame(frame).map_err(text)?;
        Ok(stats)
    }

    /// Sprites in the frame.
    #[must_use]
    pub fn sprite_count(&self) -> usize {
        self.sprites.len()
    }

    /// Batches (draw calls) of the last prepared frame.
    #[must_use]
    pub fn batch_count(&self) -> usize {
        self.batcher.batches().len()
    }

    /// The encoded vertex bytes of the last `encode`.
    #[must_use]
    pub fn vertex_data(&self) -> &[u8] {
        &self.vertex_bytes
    }

    /// The recorded command stream.
    #[must_use]
    pub fn stream_data(&self) -> &[u8] {
        self.stream.as_bytes()
    }

    /// Bytes in the recorded command stream.
    #[must_use]
    pub fn stream_bytes(&self) -> usize {
        self.stream.as_bytes().len()
    }

    /// Bytes of vertex data uploaded per frame.
    #[must_use]
    pub fn vertex_upload_bytes(&self) -> usize {
        self.vertex_bytes.len()
    }
}

/// The shaders, pipeline, buffers (camera, vertices, indices) and sampler a sprite frame binds.
fn create_bindings(device: &mut NullDevice) -> Result<SpriteBindings, String> {
    let shader = |device: &mut NullDevice, stage| {
        device.create_shader(ShaderDesc {
            stage,
            code: &[0],
            entry: "main",
        })
    };
    let vertex_shader = shader(device, ShaderStage::Vertex).map_err(text)?;
    let fragment_shader = shader(device, ShaderStage::Fragment).map_err(text)?;
    let pipeline = device
        .create_pipeline(PipelineDesc {
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
        .map_err(text)?;
    let sampler = device
        .create_sampler(SamplerDesc {
            min_filter: Filter::Nearest,
            mag_filter: Filter::Nearest,
            address: AddressMode::Clamp,
        })
        .map_err(text)?;
    let buffer = |device: &mut NullDevice, size: usize, usage| {
        let size = u32::try_from(size).unwrap_or(u32::MAX);
        device.create_buffer(BufferDesc { size, usage })
    };
    let uniforms = buffer(device, 64, BufferUsage::Uniform).map_err(text)?;
    let vertices = buffer(device, SPRITES * SPRITE_BYTES, BufferUsage::Vertex).map_err(text)?;
    let index_bytes: Vec<u8> = quad_indices(SPRITES)
        .iter()
        .flat_map(|index| index.to_le_bytes())
        .collect();
    let indices = buffer(device, index_bytes.len(), BufferUsage::Index).map_err(text)?;
    device
        .write_buffer(indices, 0, &index_bytes)
        .map_err(text)?;
    let camera = OrthographicCamera {
        position: Vec2::new(0.0, 0.0),
        height: VIEW_HEIGHT,
        aspect: ASPECT,
    };
    let matrix = camera.view_projection().ok_or("camera has no extent")?;
    device
        .write_buffer(uniforms, 0, &matrix_bytes(&matrix))
        .map_err(text)?;
    Ok(SpriteBindings {
        pipeline,
        uniforms,
        vertices,
        indices,
        sampler,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_texture_set_is_an_error_not_a_panic() {
        assert!(make_sprites(SEED, &[], 3, true).is_err());
    }

    #[test]
    fn no_sprites_need_no_textures() {
        assert_eq!(make_sprites(SEED, &[], 0, true).map(|s| s.len()), Ok(0));
    }

    #[test]
    fn error_text_names_the_error() {
        assert_eq!(text("boom"), "\"boom\"");
    }

    #[test]
    fn a_zero_bound_is_an_error() {
        assert!(below(&mut Pcg32::new(1, 1), 0).is_err());
        assert!(below(&mut Pcg32::new(1, 1), 5).is_ok_and(|n| n < 5));
    }
}
