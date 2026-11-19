//! The render side: the device resources built from the lit shader asset, and one `draw` per
//! frame that writes the object blocks of a [`MeshScene`], orders the draws, records the frame
//! graph's one pass (cleared, with depth) and submits it. The device is passed in
//! (`&mut dyn IRenderDevice`), so the same code runs on the Null device in tests and on the
//! OpenGL device in the window. After `new`, `draw` allocates nothing on its own.

use crate::error::Error;
use crate::mesh::{MeshData, unit_meshes};
use crate::sandbox::Draw;
use app_sprite_sample::sample::{create_buffer, create_texture};
use app_sprite_sample::scene::{CHECKER_SIDE, checker_pixels};
use assets::dshd::{Module, Program, Stage};
use base::math::Mat4;
use render::desc::{
    AddressMode, BlendMode, BufferDesc, BufferUsage, DepthState, Filter, PassDesc, PipelineDesc,
    Rect, RenderTarget, SamplerDesc, ShaderDesc, ShaderStage, SubmitStats, TextureDesc,
    TextureFormat,
};
use render::device::IRenderDevice;
use render::ids::{BufferId, PipelineId, SamplerId, ShaderId, SurfaceId, TextureId};
use render::lit::{
    LIGHT_BLOCK_BYTES, LitBindings, LitDraw, LitQueue, OBJECT_BLOCK_BYTES, SLOT_LIGHT, SLOT_OBJECT,
    object_block,
};
use render::mesh::{MESH_VERTEX_STRIDE, mesh_vertex_attributes};
use render::mesh_scene::{MeshInstance, MeshScene};
use render::stream::CommandStream;

/// The lit shader asset as GLSL text, for the OpenGL device.
pub const LIT_GLSL: &[u8] = include_bytes!("../assets/lit.dshd");
/// The lit shader asset as SPIR-V, for the Vulkan device.
pub const LIT_SPIRV: &[u8] = include_bytes!("../assets/lit.spv.dshd");

/// Objects one frame can draw.
pub const MAX_OBJECTS: usize = 160;
/// The colour the frame is cleared to.
const CLEAR: [f32; 4] = [0.05, 0.07, 0.12, 1.0];
/// The depth the frame is cleared to (the far plane).
const CLEAR_DEPTH: f32 = 1.0;
/// Bytes the command stream starts with (it does not grow in a frame of this size).
const STREAM_BYTES: usize = 32 * 1024;

/// A mesh on the device.
#[derive(Clone, Copy, Debug)]
struct GpuMesh {
    vertices: BufferId,
    indices: BufferId,
    index_count: u32,
}

/// The vertex and fragment modules, after checking that the asset's reflection matches what the
/// renderer feeds it: position (vec3), normal (vec3) and uv (vec2), the object block in the
/// vertex stage and the light block and the texture in the fragment stage, on the slots the
/// stream binds.
fn check_asset<'a>(program: &Program<'a>) -> Result<(Module<'a>, Module<'a>), Error> {
    let bad = |text: &str| Error::Render(format!("the lit shader asset: {text}"));
    let (Some(vertex), Some(fragment)) = (
        program.module_for(Stage::Vertex),
        program.module_for(Stage::Fragment),
    ) else {
        return Err(bad("a vertex and a fragment module are needed"));
    };
    let layout: Vec<(u8, u8)> = vertex
        .inputs()
        .map(|i| (i.location, i.components))
        .collect();
    if layout != [(0, 3), (1, 3), (2, 2)] {
        return Err(bad(
            "vertex inputs must be position, normal (vec3) and uv (vec2)",
        ));
    }
    let object_ok = vertex.uniform().is_some_and(|block| {
        u32::from(block.binding) == SLOT_OBJECT && block.size as usize >= OBJECT_BLOCK_BYTES
    });
    if !object_ok {
        return Err(bad(
            "the object block must be in the vertex stage at binding 0",
        ));
    }
    let light_ok = fragment.uniform().is_some_and(|block| {
        u32::from(block.binding) == SLOT_LIGHT && block.size as usize >= LIGHT_BLOCK_BYTES
    });
    if !light_ok {
        return Err(bad(
            "the light block must be in the fragment stage at binding 1",
        ));
    }
    if fragment.texture_binding() != Some(0) {
        return Err(bad("the texture must be at binding 0"));
    }
    Ok((vertex, fragment))
}

fn create_shader(
    device: &mut dyn IRenderDevice,
    stage: ShaderStage,
    module: &Module<'_>,
) -> Result<ShaderId, Error> {
    Ok(device.create_shader(ShaderDesc {
        stage,
        // A SPIR-V module carries a binary and no text; a GLSL module the other way round.
        code: module.spirv().unwrap_or_else(|| module.code().as_bytes()),
        entry: "main",
    })?)
}

/// The lit pipeline: opaque, depth test and write, `Depth32F`.
fn create_pipeline(
    device: &mut dyn IRenderDevice,
    vertex: &Module<'_>,
    fragment: &Module<'_>,
) -> Result<PipelineId, Error> {
    let vertex_shader = create_shader(device, ShaderStage::Vertex, vertex)?;
    let fragment_shader = create_shader(device, ShaderStage::Fragment, fragment)?;
    Ok(device.create_pipeline(PipelineDesc {
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
    })?)
}

/// A depth texture of the size of the view.
fn depth_texture(device: &mut dyn IRenderDevice, size: (u32, u32)) -> Result<TextureId, Error> {
    Ok(device.create_texture(TextureDesc {
        width: size.0,
        height: size.1,
        mip_count: 1,
        format: TextureFormat::Depth32F,
    })?)
}

/// The vertex and index buffers of `mesh`.
fn upload_mesh(device: &mut dyn IRenderDevice, mesh: &MeshData) -> Result<GpuMesh, Error> {
    let vertex_bytes = mesh
        .vertex_bytes()
        .ok_or_else(|| Error::Render("a mesh cannot be encoded".to_owned()))?;
    Ok(GpuMesh {
        vertices: create_buffer(device, BufferUsage::Vertex, &vertex_bytes)?,
        indices: create_buffer(device, BufferUsage::Index, &mesh.index_bytes())?,
        index_count: u32::try_from(mesh.indices.len()).unwrap_or(u32::MAX),
    })
}

/// A uniform buffer of `size` bytes.
fn uniform_buffer(device: &mut dyn IRenderDevice, size: usize) -> Result<BufferId, Error> {
    Ok(device.create_buffer(BufferDesc {
        size: u32::try_from(size).unwrap_or(u32::MAX),
        usage: BufferUsage::Uniform,
    })?)
}

/// The device resources and the buffers of a frame.
pub struct Renderer {
    surface: SurfaceId,
    size: (u32, u32),
    depth: TextureId,
    target: RenderTarget,
    pipeline: PipelineId,
    texture: TextureId,
    sampler: SamplerId,
    light: BufferId,
    meshes: [GpuMesh; 3],
    objects: Vec<BufferId>,
    queue: LitQueue,
    stream: CommandStream,
    stats: SubmitStats,
}

impl Renderer {
    /// Builds the pipeline, meshes, buffers, texture and sampler on `device`, for a view of
    /// `size` pixels on `surface`.
    ///
    /// # Errors
    /// [`Error::Render`] when the shader asset does not fit the lit mesh layout or the device
    /// refuses something, [`Error::NoView`] for a zero side.
    pub fn new(
        device: &mut dyn IRenderDevice,
        program: &Program<'_>,
        surface: SurfaceId,
        size: (u32, u32),
    ) -> Result<Self, Error> {
        if size.0 == 0 || size.1 == 0 {
            return Err(Error::NoView);
        }
        let (vertex, fragment) = check_asset(program)?;
        let pipeline = create_pipeline(device, &vertex, &fragment)?;
        let [sphere, cuboid, capsule] =
            unit_meshes().ok_or_else(|| Error::Render("a mesh cannot be made".to_owned()))?;
        let mut objects = Vec::with_capacity(MAX_OBJECTS);
        for _ in 0..MAX_OBJECTS {
            objects.push(uniform_buffer(device, OBJECT_BLOCK_BYTES)?);
        }
        Ok(Self {
            surface,
            size,
            depth: depth_texture(device, size)?,
            target: RenderTarget::Backbuffer,
            pipeline,
            texture: create_texture(device, CHECKER_SIDE, &checker_pixels())?,
            sampler: device.create_sampler(SamplerDesc {
                min_filter: Filter::Nearest,
                mag_filter: Filter::Nearest,
                address: AddressMode::Clamp,
            })?,
            light: uniform_buffer(device, LIGHT_BLOCK_BYTES)?,
            meshes: [
                upload_mesh(device, &sphere)?,
                upload_mesh(device, &cuboid)?,
                upload_mesh(device, &capsule)?,
            ],
            objects,
            queue: LitQueue::new(MAX_OBJECTS),
            stream: CommandStream::with_capacity(STREAM_BYTES),
            stats: SubmitStats::default(),
        })
    }

    /// The texture the objects are drawn with.
    #[must_use]
    pub fn texture(&self) -> TextureId {
        self.texture
    }

    /// Draws into `texture` (an `Rgba8` texture of the view's size) from now on, instead of the
    /// back buffer; the image test reads it back.
    pub fn set_target(&mut self, texture: TextureId) {
        self.target = RenderTarget::Texture(texture);
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

    /// Follows a window resize: the surface and the depth texture get the new size.
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
        let depth = depth_texture(device, size)?;
        device.resize_surface(self.surface, size.0, size.1)?;
        device.destroy_texture(self.depth);
        self.depth = depth;
        self.size = size;
        Ok(())
    }

    /// The draw of `instance` through the buffer `object`, and the block to write into it; `None`
    /// when the mesh is unknown or the instance has no block.
    fn object_draw(
        &self,
        instance: &MeshInstance,
        object: BufferId,
        view_projection: &Mat4,
    ) -> Option<(LitDraw, [u8; OBJECT_BLOCK_BYTES])> {
        let mesh = self.meshes.get(instance.mesh.0 as usize)?;
        let mvp = *view_projection * instance.model;
        let block = object_block(&mvp, &instance.model, &instance.material)?;
        let draw = LitDraw {
            bindings: LitBindings {
                pipeline: self.pipeline,
                vertices: mesh.vertices,
                indices: mesh.indices,
                object,
                light: self.light,
                texture: instance.texture,
                sampler: self.sampler,
            },
            index_count: mesh.index_count,
        };
        Some((draw, block))
    }

    /// Writes the light and the object blocks of `scene` and queues one draw per object that has
    /// a block.
    fn upload(&mut self, device: &mut dyn IRenderDevice, scene: &MeshScene) -> Result<(), Error> {
        let light = scene
            .light()
            .block_bytes()
            .ok_or_else(|| Error::Render("the light has no direction".to_owned()))?;
        device.write_buffer(self.light, 0, &light)?;
        let view_projection = scene
            .view_projection()
            .ok_or_else(|| Error::Render("the camera cannot look".to_owned()))?;
        self.queue.clear();
        for (index, instance) in scene.instances().iter().take(MAX_OBJECTS).enumerate() {
            self.queue_one(device, instance, index, &view_projection)?;
        }
        self.queue.sort();
        Ok(())
    }

    /// Writes the block of `instance` into the buffer of object `index` and queues its draw; an
    /// instance that has no block is left out.
    fn queue_one(
        &mut self,
        device: &mut dyn IRenderDevice,
        instance: &MeshInstance,
        index: usize,
        view_projection: &Mat4,
    ) -> Result<(), Error> {
        let Some(object) = self.objects.get(index).copied() else {
            return Ok(());
        };
        let Some((draw, block)) = self.object_draw(instance, object, view_projection) else {
            return Ok(());
        };
        device.write_buffer(object, 0, &block)?;
        // The queue and the object buffers are as large as each other, so this cannot be full.
        self.queue
            .push(draw)
            .map_err(|_| Error::Render("more draws than objects".to_owned()))
    }

    /// Records the frame: one cleared pass with depth into the backbuffer, the queued draws.
    fn record(&mut self) {
        self.stream.clear();
        self.stream.begin_pass(PassDesc {
            target: self.target,
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
        self.queue.record(&mut self.stream);
        self.stream.end_pass();
    }

    /// Draws one frame of `scene` on `device` and presents it.
    ///
    /// # Errors
    /// The first error of staging, recording or the device (a lost surface included).
    pub fn draw(&mut self, device: &mut dyn IRenderDevice, scene: &MeshScene) -> Result<(), Error> {
        self.upload(device, scene)?;
        self.record();
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
    fn draw(&mut self, scene: &MeshScene) -> Result<(), Error> {
        self.renderer.draw(self.device, scene)
    }
}
