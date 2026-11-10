use super::*;
use crate::desc::{
    AddressMode, BlendMode, BufferDesc, BufferUsage, DepthState, Filter, IndexType, PassDesc,
    PipelineDesc, RenderTarget, SamplerDesc, ShaderDesc, ShaderStage, SurfaceDesc, TextureDesc,
    TextureFormat,
};
use crate::device::IRenderDevice;
use crate::error::RenderError;
use crate::ids::{SurfaceId, Texture, TextureId};
use crate::null::NullDevice;
use crate::stream::Command;
use base::pool::{Handle, Pool};

const WHITE: [u8; 4] = [255, 255, 255, 255];

/// Distinct texture handles (never used with a device).
fn textures(count: u32) -> Vec<TextureId> {
    let mut pool = Pool::<Texture>::with_capacity(count);
    (0..count)
        .map(|_| {
            pool.insert(Texture)
                .unwrap_or_else(|_| Handle::from_raw(0, 0))
        })
        .collect()
}

fn sprite(texture: TextureId, x: f32, y: f32, layer: i16) -> Sprite {
    Sprite {
        texture,
        position: Vec2::new(x, y),
        size: Vec2::new(2.0, 4.0),
        angle: 0.0,
        uv: UvRect::FULL,
        tint: WHITE,
        layer,
    }
}

fn batcher(max: usize) -> SpriteBatcher {
    SpriteBatcher::new(max).unwrap_or_else(|_| unreachable!())
}

fn prepared(sprites: &[Sprite]) -> (Vec<SpriteVertex>, Vec<Batch>) {
    let mut sorter = batcher(64);
    let mut vertices = vec![SpriteVertex::default(); sprites.len() * 4];
    let result = sorter
        .prepare(sprites, &mut vertices)
        .map(<[Batch]>::to_vec);
    (vertices, result.unwrap_or_default())
}

fn corners(v: &[SpriteVertex]) -> Vec<[f32; 2]> {
    v.iter().map(|v| v.position).collect()
}

fn near(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5
}

#[test]
fn a_vertex_is_twenty_bytes_and_the_attributes_describe_it() {
    assert_eq!(VERTEX_STRIDE, 20);
    let layout = vertex_attributes().map(|a| (a.location, a.offset, a.format));
    assert_eq!(
        layout,
        [
            (0, 0, VertexFormat::Float2),
            (1, 8, VertexFormat::Float2),
            (2, 16, VertexFormat::Unorm8x4)
        ]
    );
}

#[test]
fn one_sprite_becomes_four_corners_around_its_centre_bottom_left_first() {
    let (v, batches) = prepared(&[sprite(textures(1)[0], 10.0, 20.0, 0)]);
    assert_eq!(
        corners(&v),
        vec![[9.0, 18.0], [11.0, 18.0], [9.0, 22.0], [11.0, 22.0]]
    );
    assert_eq!(batches.len(), 1);
}

#[test]
fn a_quarter_turn_rotates_counter_clockwise_about_the_centre() {
    let mut s = sprite(textures(1)[0], 0.0, 0.0, 0);
    s.angle = std::f32::consts::FRAC_PI_2;
    let (v, _) = prepared(&[s]);
    let expected = [[2.0, -1.0], [2.0, 1.0], [-2.0, -1.0], [-2.0, 1.0]];
    let got = corners(&v);
    assert!(
        got.iter().zip(expected).all(|(a, b)| near(*a, b)),
        "{got:?}"
    );
}

#[test]
fn uv_rectangle_maps_the_top_left_of_the_image_to_the_top_of_the_sprite() {
    let mut s = sprite(textures(1)[0], 0.0, 0.0, 0);
    s.uv = UvRect {
        min: Vec2::new(0.25, 0.5),
        max: Vec2::new(0.75, 1.0),
    };
    let (v, _) = prepared(&[s]);
    let uv: Vec<[f32; 2]> = v.iter().map(|v| v.uv).collect();
    assert_eq!(uv, vec![[0.25, 1.0], [0.75, 1.0], [0.25, 0.5], [0.75, 0.5]]);
}

#[test]
fn the_tint_is_copied_to_every_corner() {
    let mut s = sprite(textures(1)[0], 0.0, 0.0, 0);
    s.tint = [1, 2, 3, 4];
    let (v, _) = prepared(&[s]);
    assert!(v.iter().all(|v| v.color == [1, 2, 3, 4]));
}

#[test]
fn sprites_are_ordered_by_layer_then_texture_then_insertion() {
    let t = textures(2);
    let input = [
        sprite(t[1], 0.0, 0.0, 1),
        sprite(t[1], 1.0, 0.0, 0),
        sprite(t[0], 2.0, 0.0, 0),
        sprite(t[1], 3.0, 0.0, 0),
        sprite(t[0], 4.0, 0.0, 0),
    ];
    let (v, batches) = prepared(&input);
    let centres: Vec<f32> = v.chunks(4).map(|q| q[0].position[0] + 1.0).collect();
    assert_eq!(centres, vec![2.0, 4.0, 1.0, 3.0, 0.0]);
    let summary: Vec<(TextureId, u32, u32)> = batches
        .iter()
        .map(|b| (b.texture, b.first_sprite, b.sprite_count))
        .collect();
    assert_eq!(summary, vec![(t[0], 0, 2), (t[1], 2, 3)]);
}

#[test]
fn a_run_of_one_texture_across_layers_is_one_batch() {
    let t = textures(1);
    let input = [sprite(t[0], 0.0, 0.0, 0), sprite(t[0], 1.0, 0.0, 5)];
    let (_, batches) = prepared(&input);
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].sprite_count, 2);
}

#[test]
fn texture_changes_split_batches_in_layer_order() {
    let t = textures(2);
    let input = [
        sprite(t[0], 0.0, 0.0, 0),
        sprite(t[1], 1.0, 0.0, 1),
        sprite(t[0], 2.0, 0.0, 2),
    ];
    let (_, batches) = prepared(&input);
    let in_order: Vec<TextureId> = batches.iter().map(|b| b.texture).collect();
    assert_eq!(in_order, vec![t[0], t[1], t[0]]);
}

#[test]
fn negative_layers_come_before_positive_ones() {
    let t = textures(1);
    let input = [sprite(t[0], 1.0, 0.0, 3), sprite(t[0], 0.0, 0.0, -3)];
    let (v, _) = prepared(&input);
    assert!(v[0].position[0] < v[4].position[0]);
}

#[test]
fn preparing_nothing_gives_no_batches() {
    let (v, batches) = prepared(&[]);
    assert!(v.is_empty() && batches.is_empty());
}

#[test]
fn preparing_twice_gives_identical_output() {
    let t = textures(2);
    let input = [
        sprite(t[1], 0.0, 0.0, 1),
        sprite(t[0], 1.0, 0.0, 0),
        sprite(t[1], 2.0, 0.0, 0),
    ];
    assert_eq!(prepared(&input), prepared(&input));
}

#[test]
fn too_many_sprites_or_a_small_staging_buffer_is_an_error_and_writes_nothing() {
    let t = textures(1);
    let mut batcher = batcher(2);
    let three = [sprite(t[0], 0.0, 0.0, 0); 3];
    let mut big = vec![SpriteVertex::default(); 12];
    assert_eq!(
        batcher.prepare(&three, &mut big).err(),
        Some(BatchError::TooManySprites)
    );
    let two = [sprite(t[0], 0.0, 0.0, 0); 2];
    let mut small = vec![SpriteVertex::default(); 7];
    assert_eq!(
        batcher.prepare(&two, &mut small).err(),
        Some(BatchError::StagingTooSmall)
    );
    assert!(small.iter().all(|v| *v == SpriteVertex::default()));
    assert!(batcher.batches().is_empty());
}

#[test]
fn the_batcher_size_is_limited_by_sixteen_bit_indices() {
    assert!(SpriteBatcher::new(MAX_SPRITES).is_ok());
    assert_eq!(
        SpriteBatcher::new(MAX_SPRITES + 1).err(),
        Some(BatchError::TooManySprites)
    );
}

#[test]
fn the_shared_index_buffer_is_two_triangles_per_quad() {
    assert_eq!(quad_indices(2), vec![0, 1, 2, 2, 1, 3, 4, 5, 6, 6, 5, 7]);
    assert!(quad_indices(0).is_empty());
}

#[test]
fn vertices_encode_as_little_endian_bytes() {
    let v = SpriteVertex {
        position: [1.0, 2.0],
        uv: [0.5, 0.25],
        color: [9, 8, 7, 6],
    };
    let mut out = [0u8; 20];
    assert!(encode_vertices(&[v], &mut out));
    assert_eq!(out[0..4], 1.0f32.to_le_bytes());
    assert_eq!(out[4..8], 2.0f32.to_le_bytes());
    assert_eq!(out[8..12], 0.5f32.to_le_bytes());
    assert_eq!(out[12..16], 0.25f32.to_le_bytes());
    assert_eq!(out[16..20], [9, 8, 7, 6]);
    assert!(!encode_vertices(&[v], &mut [0u8; 19]));
}

/// Resources of a Null device for the recording tests.
struct Rig {
    device: NullDevice,
    bindings: SpriteBindings,
    surface: SurfaceId,
    texture: TextureId,
}

fn ok<T>(r: Result<T, RenderError>) -> T {
    assert!(r.is_ok(), "device refused: {:?}", r.as_ref().err());
    r.unwrap_or_else(|_| unreachable!())
}

fn buffer(d: &mut NullDevice, size: u32, usage: BufferUsage) -> crate::ids::BufferId {
    ok(d.create_buffer(BufferDesc { size, usage }))
}

fn shader(d: &mut NullDevice, stage: ShaderStage) -> crate::ids::ShaderId {
    ok(d.create_shader(ShaderDesc {
        stage,
        code: &[1],
        entry: "main",
    }))
}

fn rig(sprites: u32) -> Rig {
    let mut d = NullDevice::new();
    let vs = shader(&mut d, ShaderStage::Vertex);
    let fs = shader(&mut d, ShaderStage::Fragment);
    let attributes = vertex_attributes();
    let pipeline = ok(d.create_pipeline(PipelineDesc {
        vertex_shader: vs,
        fragment_shader: fs,
        vertex_stride: VERTEX_STRIDE,
        attributes: &attributes,
        blend: BlendMode::Alpha,
        depth: DepthState {
            test: false,
            write: false,
        },
        color_format: TextureFormat::Rgba8,
        depth_format: None,
    }));
    let sampler = ok(d.create_sampler(SamplerDesc {
        min_filter: Filter::Nearest,
        mag_filter: Filter::Nearest,
        address: AddressMode::Clamp,
    }));
    let texture = ok(d.create_texture(TextureDesc {
        width: 4,
        height: 4,
        mip_count: 1,
        format: TextureFormat::Rgba8,
    }));
    let surface = ok(d.create_surface(SurfaceDesc {
        native_token: 1,
        width: 8,
        height: 8,
    }));
    let bindings = SpriteBindings {
        pipeline,
        uniforms: buffer(&mut d, 64, BufferUsage::Uniform),
        vertices: buffer(&mut d, sprites * 80, BufferUsage::Vertex),
        indices: buffer(&mut d, sprites * 12, BufferUsage::Index),
        sampler,
    };
    Rig {
        device: d,
        bindings,
        surface,
        texture,
    }
}

fn decode(stream: &CommandStream) -> Vec<Command> {
    stream
        .commands()
        .collect::<Result<_, _>>()
        .unwrap_or_default()
}

#[test]
fn recording_binds_once_then_draws_each_batch_from_its_vertex_offset() {
    let b = rig(8).bindings;
    let t = textures(2);
    let mut batcher = batcher(8);
    let input = [
        sprite(t[0], 0.0, 0.0, 0),
        sprite(t[0], 1.0, 0.0, 0),
        sprite(t[1], 2.0, 0.0, 1),
    ];
    let mut vertices = vec![SpriteVertex::default(); 12];
    assert!(batcher.prepare(&input, &mut vertices).is_ok());
    let mut stream = CommandStream::new();
    batcher.record(&mut stream, &b);
    assert_eq!(
        decode(&stream),
        vec![
            Command::SetPipeline(b.pipeline),
            Command::BindUniforms {
                slot: 0,
                buffer: b.uniforms
            },
            Command::BindIndexBuffer {
                buffer: b.indices,
                offset: 0,
                index_type: IndexType::U16
            },
            Command::BindVertexBuffer {
                slot: 0,
                buffer: b.vertices,
                offset: 0
            },
            Command::BindTexture {
                slot: 0,
                texture: t[0],
                sampler: b.sampler
            },
            Command::DrawIndexed {
                indices: 12,
                instances: 1
            },
            Command::BindVertexBuffer {
                slot: 0,
                buffer: b.vertices,
                offset: 160
            },
            Command::BindTexture {
                slot: 0,
                texture: t[1],
                sampler: b.sampler
            },
            Command::DrawIndexed {
                indices: 6,
                instances: 1
            },
        ]
    );
}

#[test]
fn recording_nothing_to_draw_writes_nothing() {
    let r = rig(1);
    let batcher = batcher(4);
    let mut stream = CommandStream::new();
    batcher.record(&mut stream, &r.bindings);
    assert!(stream.is_empty());
}

#[test]
fn a_recorded_sprite_frame_is_accepted_by_the_null_device() {
    let mut r = rig(8);
    let mut batcher = batcher(8);
    let input = [
        sprite(r.texture, 0.0, 0.0, 0),
        sprite(r.texture, 1.0, 0.0, 0),
    ];
    let mut vertices = vec![SpriteVertex::default(); 8];
    assert!(batcher.prepare(&input, &mut vertices).is_ok());
    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: Some([0.0; 4]),
        clear_depth: None,
        depth: None,
    });
    batcher.record(&mut stream, &r.bindings);
    stream.end_pass();
    let frame = ok(r.device.begin_frame(r.surface));
    let stats = r.device.submit(frame, &stream);
    assert_eq!(r.device.end_frame(frame), Ok(()));
    assert_eq!(stats.map(|s| s.draws), Ok(1));
}

#[test]
fn batches_returns_the_batches_of_the_last_prepare_and_errors_read_clearly() {
    let t = textures(2);
    let mut sorter = batcher(4);
    let input = [sprite(t[0], 0.0, 0.0, 0), sprite(t[1], 1.0, 0.0, 0)];
    let mut staging = vec![SpriteVertex::default(); 8];
    let returned = sorter.prepare(&input, &mut staging).map(<[Batch]>::to_vec);
    assert_eq!(returned.as_deref(), Ok(sorter.batches()));
    assert_eq!(sorter.batches().len(), 2);
    let messages = [BatchError::TooManySprites, BatchError::StagingTooSmall].map(|e| e.to_string());
    assert!(messages[0].contains("sprites") && messages[1].contains("staging"));
}

#[test]
fn a_failed_prepare_leaves_nothing_to_record() {
    let t = textures(1);
    let mut sorter = batcher(2);
    let mut staging = vec![SpriteVertex::default(); 8];
    assert!(
        sorter
            .prepare(&[sprite(t[0], 0.0, 0.0, 0)], &mut staging)
            .is_ok()
    );
    let three = [sprite(t[0], 0.0, 0.0, 0); 3];
    assert!(sorter.prepare(&three, &mut staging).is_err());
    assert!(sorter.batches().is_empty());
}

#[test]
fn quad_indices_are_capped_at_the_sprite_limit() {
    assert_eq!(quad_indices(MAX_SPRITES + 5).len(), MAX_SPRITES * 6);
    assert_eq!(quad_indices(MAX_SPRITES).last().copied(), Some(65_535));
}
