use super::*;
use crate::desc::{
    AddressMode, BlendMode, BufferUsage, DepthState, Filter, IndexType, PassDesc, Rect,
    RenderTarget, ResourceState, ShaderStage, TextureFormat, VertexAttribute, VertexFormat,
};

fn device() -> NullDevice {
    NullDevice::with_capacity(4)
}

/// Unwraps a result the test expects to be Ok, failing with the error.
fn ok<T>(result: Result<T, RenderError>) -> T {
    assert_eq!(result.as_ref().err(), None);
    match result {
        Ok(value) => value,
        Err(_) => unreachable!(),
    }
}

fn buffer(d: &mut NullDevice, size: u32, usage: BufferUsage) -> BufferId {
    ok(d.create_buffer(BufferDesc { size, usage }))
}

fn texture(d: &mut NullDevice, width: u32, height: u32) -> TextureId {
    ok(d.create_texture(TextureDesc {
        width,
        height,
        mip_count: 1,
        format: TextureFormat::Rgba8,
    }))
}

fn surface(d: &mut NullDevice) -> SurfaceId {
    ok(d.create_surface(SurfaceDesc {
        native_token: 7,
        width: 64,
        height: 32,
    }))
}

fn shader(d: &mut NullDevice, stage: ShaderStage) -> ShaderId {
    ok(d.create_shader(ShaderDesc {
        stage,
        code: &[1, 2, 3],
        entry: "main",
    }))
}

fn sampler(d: &mut NullDevice) -> SamplerId {
    let desc = SamplerDesc {
        min_filter: Filter::Nearest,
        mag_filter: Filter::Linear,
        address: AddressMode::Clamp,
    };
    ok(d.create_sampler(desc))
}

const POSITION: VertexAttribute = VertexAttribute {
    location: 0,
    format: VertexFormat::Float2,
    offset: 0,
};

fn pipeline_desc(
    vs: ShaderId,
    fs: ShaderId,
    stride: u32,
    attrs: &[VertexAttribute],
) -> PipelineDesc<'_> {
    PipelineDesc {
        vertex_shader: vs,
        fragment_shader: fs,
        vertex_stride: stride,
        attributes: attrs,
        blend: BlendMode::Alpha,
        depth: DepthState {
            test: false,
            write: false,
        },
        color_format: TextureFormat::Rgba8,
    }
}

fn pipeline(d: &mut NullDevice) -> PipelineId {
    let vs = shader(d, ShaderStage::Vertex);
    let fs = shader(d, ShaderStage::Fragment);
    ok(d.create_pipeline(pipeline_desc(vs, fs, 8, &[POSITION])))
}

fn rect(x: i32, y: i32, width: u32, height: u32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn backbuffer_pass() -> PassDesc {
    PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: None,
        clear_depth: None,
    }
}

fn pass_to(texture: TextureId, color: Option<[f32; 4]>) -> PassDesc {
    PassDesc {
        target: RenderTarget::Texture(texture),
        clear_color: color,
        clear_depth: None,
    }
}

/// A stream with one backbuffer pass whose body `f` records.
fn in_pass(f: impl FnOnce(&mut CommandStream)) -> CommandStream {
    let mut s = CommandStream::new();
    s.begin_pass(backbuffer_pass());
    f(&mut s);
    s.end_pass();
    s
}

/// Submits `stream` in a fresh frame and ends the frame.
fn submit(d: &mut NullDevice, stream: &CommandStream) -> Result<SubmitStats, RenderError> {
    let s = surface(d);
    let frame = ok(d.begin_frame(s));
    let result = d.submit(frame, stream);
    ok(d.end_frame(frame));
    d.destroy_surface(s);
    result
}

fn rejected(d: &mut NullDevice, cases: Vec<(&str, CommandStream)>, error: RenderError) {
    for (name, stream) in cases {
        assert_eq!(submit(d, &stream), Err(error), "{name}");
    }
}

#[test]
fn caps_describe_a_zero_to_one_depth_device_named_null() {
    let caps = device().caps();
    assert_eq!(caps.device_name, "null");
    assert!(caps.depth_zero_to_one && caps.index_u16 && caps.index_u32);
    assert!(caps.max_texture_size >= 1024);
}

#[test]
fn a_destroyed_buffer_is_stale_and_its_slot_is_reused_with_a_new_generation() {
    let mut d = device();
    let old = buffer(&mut d, 16, BufferUsage::Vertex);
    d.destroy_buffer(old);
    assert_eq!(
        d.write_buffer(old, 0, &[1]),
        Err(RenderError::InvalidHandle)
    );
    d.destroy_buffer(old);
    let new = buffer(&mut d, 16, BufferUsage::Vertex);
    assert_eq!(new.index(), old.index());
    assert_ne!(new, old);
    assert_eq!(
        d.write_buffer(old, 0, &[1]),
        Err(RenderError::InvalidHandle)
    );
    assert_eq!(d.write_buffer(new, 0, &[1]), Ok(()));
}

#[test]
fn a_full_pool_reports_out_of_memory_and_destroying_frees_a_slot() {
    let mut d = device();
    let first = buffer(&mut d, 1, BufferUsage::Uniform);
    for _ in 0..3 {
        buffer(&mut d, 1, BufferUsage::Uniform);
    }
    let desc = BufferDesc {
        size: 1,
        usage: BufferUsage::Uniform,
    };
    assert_eq!(d.create_buffer(desc), Err(RenderError::OutOfMemory));
    d.destroy_buffer(first);
    assert!(d.create_buffer(desc).is_ok());
}

#[test]
fn buffer_writes_are_range_checked() {
    let mut d = device();
    let b = buffer(&mut d, 8, BufferUsage::Vertex);
    assert_eq!(d.write_buffer(b, 0, &[0; 8]), Ok(()));
    assert_eq!(
        d.write_buffer(b, 4, &[0; 5]),
        Err(RenderError::InvalidDescription)
    );
    assert_eq!(
        d.write_buffer(b, u32::MAX, &[0; 2]),
        Err(RenderError::InvalidDescription)
    );
    let empty = BufferDesc {
        size: 0,
        usage: BufferUsage::Index,
    };
    assert_eq!(d.create_buffer(empty), Err(RenderError::InvalidDescription));
}

#[test]
fn texture_descriptions_are_validated() {
    let mut d = device();
    let max = d.caps().max_texture_size;
    let desc = |w, h, mips| TextureDesc {
        width: w,
        height: h,
        mip_count: mips,
        format: TextureFormat::Rgba8,
    };
    assert_eq!(
        d.create_texture(desc(0, 4, 1)),
        Err(RenderError::InvalidDescription)
    );
    assert_eq!(
        d.create_texture(desc(4, 0, 1)),
        Err(RenderError::InvalidDescription)
    );
    assert_eq!(
        d.create_texture(desc(4, 4, 0)),
        Err(RenderError::InvalidDescription)
    );
    assert_eq!(
        d.create_texture(desc(4, 4, 4)),
        Err(RenderError::InvalidDescription)
    );
    assert_eq!(
        d.create_texture(desc(max + 1, 1, 1)),
        Err(RenderError::InvalidDescription)
    );
    assert!(d.create_texture(desc(4, 4, 3)).is_ok());
}

#[test]
fn texture_upload_reads_back_equal() {
    let mut d = device();
    let t = texture(&mut d, 4, 2);
    let pixels: Vec<u8> = (0..32).collect();
    let whole = rect(0, 0, 4, 2);
    assert_eq!(d.write_texture(t, whole, &pixels), Ok(()));
    let mut out = [0u8; 32];
    assert_eq!(d.read_back(t, &mut out), Ok(()));
    assert_eq!(out.to_vec(), pixels);
    // A 2x1 patch at (1,1) lands on bytes 20..28.
    let patch = rect(1, 1, 2, 1);
    assert_eq!(d.write_texture(t, patch, &[200; 8]), Ok(()));
    assert_eq!(d.read_back(t, &mut out), Ok(()));
    assert_eq!(&out[..20], &pixels[..20]);
    assert_eq!(&out[20..28], &[200; 8]);
    assert_eq!(&out[28..], &pixels[28..]);
}

#[test]
fn texture_regions_and_sizes_are_checked() {
    let mut d = device();
    let t = texture(&mut d, 4, 2);
    let whole = rect(0, 0, 4, 2);
    for bad in [
        rect(-1, 0, 1, 1),
        rect(0, -1, 1, 1),
        rect(3, 0, 2, 1),
        rect(0, 1, 1, 2),
        rect(0, 0, 0, 1),
        rect(0, 0, 1, 0),
    ] {
        assert_eq!(
            d.write_texture(t, bad, &[0; 8]),
            Err(RenderError::InvalidDescription),
            "{bad:?}"
        );
    }
    assert_eq!(
        d.write_texture(t, whole, &[0; 31]),
        Err(RenderError::InvalidDescription)
    );
    assert_eq!(
        d.write_texture(t, whole, &[0; 33]),
        Err(RenderError::InvalidDescription)
    );
    assert_eq!(
        d.read_back(t, &mut [0; 31]),
        Err(RenderError::InvalidDescription)
    );
    assert_eq!(
        d.read_back(t, &mut [0; 33]),
        Err(RenderError::InvalidDescription)
    );
    d.destroy_texture(t);
    assert_eq!(
        d.read_back(t, &mut [0; 32]),
        Err(RenderError::InvalidHandle)
    );
    assert_eq!(
        d.write_texture(t, whole, &[0; 32]),
        Err(RenderError::InvalidHandle)
    );
}

#[test]
fn shaders_are_validated() {
    let mut d = device();
    let empty = ShaderDesc {
        stage: ShaderStage::Vertex,
        code: &[],
        entry: "main",
    };
    assert_eq!(d.create_shader(empty), Err(RenderError::ShaderRejected));
    let unnamed = ShaderDesc {
        stage: ShaderStage::Vertex,
        code: &[1],
        entry: "",
    };
    assert_eq!(
        d.create_shader(unnamed),
        Err(RenderError::InvalidDescription)
    );
}

#[test]
fn pipelines_are_validated() {
    let mut d = device();
    let vs = shader(&mut d, ShaderStage::Vertex);
    let fs = shader(&mut d, ShaderStage::Fragment);
    assert!(
        d.create_pipeline(pipeline_desc(vs, fs, 8, &[POSITION]))
            .is_ok()
    );
    let invalid = RenderError::InvalidDescription;
    assert_eq!(
        d.create_pipeline(pipeline_desc(fs, vs, 8, &[POSITION])),
        Err(invalid),
        "stages swapped"
    );
    assert_eq!(
        d.create_pipeline(pipeline_desc(vs, fs, 4, &[POSITION])),
        Err(invalid),
        "attribute past stride"
    );
    assert_eq!(
        d.create_pipeline(pipeline_desc(vs, fs, 0, &[POSITION])),
        Err(invalid),
        "zero stride"
    );
    let depth_target = PipelineDesc {
        color_format: TextureFormat::Depth32F,
        ..pipeline_desc(vs, fs, 8, &[POSITION])
    };
    assert_eq!(d.create_pipeline(depth_target), Err(invalid));
    d.destroy_shader(vs);
    assert_eq!(
        d.create_pipeline(pipeline_desc(vs, fs, 8, &[POSITION])),
        Err(RenderError::InvalidHandle)
    );
}

#[test]
fn a_pipeline_without_attributes_needs_no_stride() {
    let mut d = device();
    let vs = shader(&mut d, ShaderStage::Vertex);
    let fs = shader(&mut d, ShaderStage::Fragment);
    assert!(d.create_pipeline(pipeline_desc(vs, fs, 0, &[])).is_ok());
}

#[test]
fn pipelines_outlive_their_shaders_and_destroy_calls_ignore_stale_handles() {
    let mut d = device();
    let vs = shader(&mut d, ShaderStage::Vertex);
    let fs = shader(&mut d, ShaderStage::Fragment);
    let p = ok(d.create_pipeline(pipeline_desc(vs, fs, 8, &[POSITION])));
    d.destroy_shader(vs);
    d.destroy_shader(fs);
    assert!(submit(&mut d, &in_pass(|s| s.set_pipeline(p))).is_ok());
    d.destroy_pipeline(p);
    d.destroy_pipeline(p);
    let smp = sampler(&mut d);
    d.destroy_sampler(smp);
    d.destroy_sampler(smp);
    assert_eq!(
        submit(&mut d, &in_pass(|s| s.set_pipeline(p))),
        Err(RenderError::InvalidHandle)
    );
}

#[test]
fn surfaces_frames_and_resize() {
    let mut d = device();
    let zero_width = SurfaceDesc {
        native_token: 1,
        width: 0,
        height: 4,
    };
    let zero_height = SurfaceDesc {
        native_token: 1,
        width: 4,
        height: 0,
    };
    let null_token = SurfaceDesc {
        native_token: 0,
        width: 4,
        height: 4,
    };
    for desc in [zero_width, zero_height, null_token] {
        assert_eq!(
            d.create_surface(desc),
            Err(RenderError::InvalidDescription),
            "{desc:?}"
        );
    }
    let s = surface(&mut d);
    assert_eq!(d.resize_surface(s, 128, 64), Ok(()));
    assert_eq!(
        d.resize_surface(s, 0, 64),
        Err(RenderError::InvalidDescription)
    );
    assert_eq!(
        d.resize_surface(s, 64, 0),
        Err(RenderError::InvalidDescription)
    );
    let frame = ok(d.begin_frame(s));
    assert_eq!(
        d.begin_frame(s),
        Err(RenderError::InvalidDescription),
        "one frame at a time"
    );
    assert_eq!(
        d.end_frame(FrameId(frame.0 + 1)),
        Err(RenderError::InvalidHandle)
    );
    assert_eq!(d.end_frame(frame), Ok(()));
    assert_eq!(
        d.end_frame(frame),
        Err(RenderError::InvalidHandle),
        "already ended"
    );
    let second = ok(d.begin_frame(s));
    assert_ne!(second, frame);
    d.destroy_surface(s);
    assert_eq!(d.end_frame(second), Err(RenderError::SurfaceLost));
    assert_eq!(d.begin_frame(s), Err(RenderError::InvalidHandle));
    assert_eq!(d.resize_surface(s, 4, 4), Err(RenderError::InvalidHandle));
}

#[test]
fn submit_needs_the_current_frame() {
    let mut d = device();
    let stream = CommandStream::new();
    assert_eq!(
        d.submit(FrameId(0), &stream),
        Err(RenderError::InvalidHandle)
    );
    let s = surface(&mut d);
    let frame = ok(d.begin_frame(s));
    assert_eq!(
        d.submit(FrameId(frame.0 + 1), &stream),
        Err(RenderError::InvalidHandle)
    );
    assert_eq!(d.submit(frame, &stream), Ok(SubmitStats::default()));
}

#[test]
fn a_valid_scene_is_counted() {
    let mut d = device();
    let p = pipeline(&mut d);
    let vb = buffer(&mut d, 24, BufferUsage::Vertex);
    let ib = buffer(&mut d, 12, BufferUsage::Index);
    let ub = buffer(&mut d, 64, BufferUsage::Uniform);
    let t = texture(&mut d, 2, 2);
    let smp = sampler(&mut d);
    let scene = in_pass(|s| {
        s.set_pipeline(p);
        s.set_viewport(rect(0, 0, 64, 32));
        s.bind_uniforms(0, ub);
        s.bind_texture(0, t, smp);
        s.bind_vertex_buffer(0, vb, 0);
        s.draw(3, 1);
        s.bind_index_buffer(ib, 0, IndexType::U16);
        s.draw_indexed(6, 1);
    });
    assert_eq!(
        submit(&mut d, &scene),
        Ok(SubmitStats {
            commands: 10,
            draws: 2
        })
    );
}

#[test]
fn pass_nesting_is_enforced() {
    let mut d = device();
    let mut begin_twice = CommandStream::new();
    begin_twice.begin_pass(backbuffer_pass());
    begin_twice.begin_pass(backbuffer_pass());
    let mut end_alone = CommandStream::new();
    end_alone.end_pass();
    let mut unclosed = CommandStream::new();
    unclosed.begin_pass(backbuffer_pass());
    let mut outside = CommandStream::new();
    outside.set_viewport(rect(0, 0, 1, 1));
    let cases = vec![
        ("nested", begin_twice),
        ("end alone", end_alone),
        ("unclosed", unclosed),
        ("outside", outside),
    ];
    rejected(&mut d, cases, RenderError::InvalidDescription);
}

/// A pass that binds `vertex` (buffer, offset) when given, then draws `count` vertices.
fn vertex_draw(p: PipelineId, vertex: Option<(BufferId, u32)>, count: u32) -> CommandStream {
    in_pass(|s| {
        s.set_pipeline(p);
        if let Some((buffer, offset)) = vertex {
            s.bind_vertex_buffer(0, buffer, offset);
        }
        s.draw(count, 1);
    })
}

/// A pass that binds the vertex buffer and `index` (buffer, offset, type), then draws indexed.
fn indexed_draw(
    p: PipelineId,
    vb: BufferId,
    index: Option<(BufferId, u32, IndexType)>,
    count: u32,
) -> CommandStream {
    in_pass(|s| {
        s.set_pipeline(p);
        s.bind_vertex_buffer(0, vb, 0);
        if let Some((buffer, offset, index_type)) = index {
            s.bind_index_buffer(buffer, offset, index_type);
        }
        s.draw_indexed(count, 1);
    })
}

#[test]
fn draws_need_a_pipeline_and_a_vertex_buffer_that_holds_them() {
    let mut d = device();
    let p = pipeline(&mut d);
    let vb = buffer(&mut d, 24, BufferUsage::Vertex);
    let cases = vec![
        ("no pipeline", in_pass(|s| s.draw(3, 1))),
        ("no vertex buffer", vertex_draw(p, None, 3)),
        ("vertex range", vertex_draw(p, Some((vb, 0)), 4)),
        ("vertex offset", vertex_draw(p, Some((vb, 16)), 2)),
    ];
    rejected(&mut d, cases, RenderError::InvalidDescription);
    let exact = vertex_draw(p, Some((vb, 8)), 2);
    assert_eq!(
        submit(&mut d, &exact),
        Ok(SubmitStats {
            commands: 5,
            draws: 1
        })
    );
}

#[test]
fn indexed_draws_need_an_index_buffer_that_holds_them() {
    let mut d = device();
    let p = pipeline(&mut d);
    let vb = buffer(&mut d, 24, BufferUsage::Vertex);
    let ib = buffer(&mut d, 12, BufferUsage::Index);
    let cases = vec![
        ("no index buffer", indexed_draw(p, vb, None, 3)),
        (
            "index range",
            indexed_draw(p, vb, Some((ib, 0, IndexType::U16)), 7),
        ),
        (
            "index u32 range",
            indexed_draw(p, vb, Some((ib, 0, IndexType::U32)), 4),
        ),
        (
            "index offset",
            indexed_draw(p, vb, Some((ib, 8, IndexType::U16)), 3),
        ),
    ];
    rejected(&mut d, cases, RenderError::InvalidDescription);
    let exact = indexed_draw(p, vb, Some((ib, 4, IndexType::U16)), 4);
    assert_eq!(
        submit(&mut d, &exact),
        Ok(SubmitStats {
            commands: 6,
            draws: 1
        })
    );
}

#[test]
fn bindings_do_not_leak_into_the_next_pass() {
    let mut d = device();
    let p = pipeline(&mut d);
    let vb = buffer(&mut d, 24, BufferUsage::Vertex);
    let mut s = CommandStream::new();
    s.begin_pass(backbuffer_pass());
    s.set_pipeline(p);
    s.bind_vertex_buffer(0, vb, 0);
    s.draw(3, 1);
    s.end_pass();
    s.begin_pass(backbuffer_pass());
    s.draw(3, 1);
    s.end_pass();
    assert_eq!(submit(&mut d, &s), Err(RenderError::InvalidDescription));
}

#[test]
fn bindings_are_validated() {
    let mut d = device();
    let vb = buffer(&mut d, 24, BufferUsage::Vertex);
    let ub = buffer(&mut d, 64, BufferUsage::Uniform);
    let t = texture(&mut d, 2, 2);
    let smp = sampler(&mut d);
    let slots = MAX_BINDING_SLOTS;
    let cases = vec![
        ("uniform slot", in_pass(|s| s.bind_uniforms(slots, ub))),
        ("uniform usage", in_pass(|s| s.bind_uniforms(0, vb))),
        ("texture slot", in_pass(|s| s.bind_texture(slots, t, smp))),
        ("vertex slot", in_pass(|s| s.bind_vertex_buffer(1, vb, 0))),
        ("vertex usage", in_pass(|s| s.bind_vertex_buffer(0, ub, 0))),
        (
            "index usage",
            in_pass(|s| s.bind_index_buffer(vb, 0, IndexType::U16)),
        ),
    ];
    rejected(&mut d, cases, RenderError::InvalidDescription);
    let last = slots - 1;
    let edge = in_pass(|s| {
        s.bind_uniforms(last, ub);
        s.bind_texture(last, t, smp);
    });
    assert_eq!(
        submit(&mut d, &edge),
        Ok(SubmitStats {
            commands: 4,
            draws: 0
        })
    );
}

#[test]
fn stale_handles_in_a_stream_are_refused() {
    let mut d = device();
    let ub = buffer(&mut d, 64, BufferUsage::Uniform);
    let t = texture(&mut d, 2, 2);
    let smp = sampler(&mut d);
    let p = pipeline(&mut d);
    d.destroy_buffer(ub);
    d.destroy_texture(t);
    d.destroy_sampler(smp);
    d.destroy_pipeline(p);
    let live_buffer = buffer(&mut d, 8, BufferUsage::Uniform);
    let live_texture = texture(&mut d, 2, 2);
    let live_sampler = sampler(&mut d);
    let mut stale_target = CommandStream::new();
    stale_target.begin_pass(pass_to(t, None));
    stale_target.end_pass();
    let cases = vec![
        ("buffer", in_pass(|s| s.bind_uniforms(0, ub))),
        ("texture", in_pass(|s| s.bind_texture(0, t, live_sampler))),
        ("sampler", in_pass(|s| s.bind_texture(0, live_texture, smp))),
        ("pipeline", in_pass(|s| s.set_pipeline(p))),
        ("target", stale_target),
    ];
    rejected(&mut d, cases, RenderError::InvalidHandle);
    let fine = in_pass(|s| s.bind_uniforms(0, live_buffer));
    assert_eq!(
        submit(&mut d, &fine),
        Ok(SubmitStats {
            commands: 3,
            draws: 0
        })
    );
}

#[test]
fn a_clear_lands_in_a_texture_only_when_the_whole_stream_is_valid() {
    let mut d = device();
    let t = texture(&mut d, 2, 1);
    let mut clear = CommandStream::new();
    clear.begin_pass(pass_to(t, Some([1.0, 0.5, 0.0, 1.0])));
    clear.end_pass();
    assert_eq!(
        submit(&mut d, &clear),
        Ok(SubmitStats {
            commands: 2,
            draws: 0
        })
    );
    let orange = [255, 128, 0, 255, 255, 128, 0, 255];
    let mut out = [0u8; 8];
    assert_eq!(d.read_back(t, &mut out), Ok(()));
    assert_eq!(out, orange);
    // Another clear followed by an invalid command: nothing is applied.
    let mut broken = CommandStream::new();
    broken.begin_pass(pass_to(t, Some([0.0, 0.0, 0.0, 0.0])));
    broken.draw(3, 1);
    broken.end_pass();
    assert_eq!(
        submit(&mut d, &broken),
        Err(RenderError::InvalidDescription)
    );
    assert_eq!(d.read_back(t, &mut out), Ok(()));
    assert_eq!(out, orange);
}

#[test]
fn clears_follow_the_texture_format_and_clamp() {
    let mut d = device();
    let desc = |format| TextureDesc {
        width: 1,
        height: 1,
        mip_count: 1,
        format,
    };
    let bgra = ok(d.create_texture(desc(TextureFormat::Bgra8)));
    let mut s = CommandStream::new();
    s.begin_pass(pass_to(bgra, Some([1.0, 0.0, 0.5, 2.0])));
    s.end_pass();
    assert!(submit(&mut d, &s).is_ok());
    let mut out = [0u8; 4];
    assert_eq!(d.read_back(bgra, &mut out), Ok(()));
    assert_eq!(out, [128, 0, 255, 255]);
    let mut negative = CommandStream::new();
    negative.begin_pass(pass_to(bgra, Some([-1.0, f32::NAN, 0.0, 1.0])));
    negative.end_pass();
    assert!(submit(&mut d, &negative).is_ok());
    assert_eq!(d.read_back(bgra, &mut out), Ok(()));
    assert_eq!(out, [0, 0, 0, 255]);
    let depth = ok(d.create_texture(desc(TextureFormat::Depth32F)));
    let mut bad = CommandStream::new();
    bad.begin_pass(pass_to(depth, Some([0.0; 4])));
    bad.end_pass();
    assert_eq!(submit(&mut d, &bad), Err(RenderError::InvalidDescription));
}

#[test]
fn a_malformed_stream_is_an_invalid_description() {
    let mut d = device();
    let s = surface(&mut d);
    let frame = ok(d.begin_frame(s));
    let mut bytes = CommandStream::new().as_bytes().to_vec();
    bytes.extend_from_slice(&[0xEE, 0, 0, 0]);
    let broken = CommandStream::from_bytes_unchecked(bytes);
    assert_eq!(
        d.submit(frame, &broken),
        Err(RenderError::InvalidDescription)
    );
}

#[test]
fn a_fragment_slot_needs_a_fragment_shader() {
    let mut d = device();
    let vs = shader(&mut d, ShaderStage::Vertex);
    let also_vertex = shader(&mut d, ShaderStage::Vertex);
    let result = d.create_pipeline(pipeline_desc(vs, also_vertex, 8, &[POSITION]));
    assert_eq!(result, Err(RenderError::InvalidDescription));
}

#[test]
fn an_empty_texture_region_is_refused_even_with_no_bytes() {
    let mut d = device();
    let t = texture(&mut d, 4, 4);
    assert_eq!(
        d.write_texture(t, rect(0, 0, 0, 2), &[]),
        Err(RenderError::InvalidDescription)
    );
    assert_eq!(
        d.write_texture(t, rect(0, 0, 2, 0), &[]),
        Err(RenderError::InvalidDescription)
    );
}

#[test]
fn a_region_that_ends_inside_a_wide_texture_is_accepted() {
    let mut d = device();
    let t = texture(&mut d, 8, 1);
    assert_eq!(d.write_texture(t, rect(3, 0, 4, 1), &[9; 16]), Ok(()));
    let mut out = [0u8; 32];
    assert_eq!(d.read_back(t, &mut out), Ok(()));
    assert_eq!(&out[12..28], &[9; 16]);
    assert_eq!(out[11], 0);
    assert_eq!(out[28], 0);
    assert_eq!(
        d.write_texture(t, rect(4, 0, 4, 1), &[1; 16]),
        Ok(()),
        "ends exactly at the edge"
    );
}

#[test]
fn a_buffer_can_be_bound_at_its_end_but_not_past_it() {
    let mut d = device();
    let p = pipeline(&mut d);
    let vb = buffer(&mut d, 24, BufferUsage::Vertex);
    assert_eq!(
        submit(&mut d, &vertex_draw(p, Some((vb, 24)), 0)),
        Ok(SubmitStats {
            commands: 5,
            draws: 1
        })
    );
    let past = vertex_draw(p, Some((vb, 25)), 0);
    assert_eq!(submit(&mut d, &past), Err(RenderError::InvalidDescription));
}

#[test]
fn a_pipeline_without_attributes_draws_without_a_vertex_buffer() {
    let mut d = device();
    let vs = shader(&mut d, ShaderStage::Vertex);
    let fs = shader(&mut d, ShaderStage::Fragment);
    let p = ok(d.create_pipeline(pipeline_desc(vs, fs, 0, &[])));
    assert_eq!(
        submit(&mut d, &vertex_draw(p, None, 3)),
        Ok(SubmitStats {
            commands: 4,
            draws: 1
        })
    );
}

#[test]
fn an_index_offset_must_be_a_multiple_of_the_index_size() {
    let mut d = device();
    let p = pipeline(&mut d);
    let vb = buffer(&mut d, 24, BufferUsage::Vertex);
    let ib = buffer(&mut d, 12, BufferUsage::Index);
    let odd16 = indexed_draw(p, vb, Some((ib, 1, IndexType::U16)), 1);
    let odd32 = indexed_draw(p, vb, Some((ib, 2, IndexType::U32)), 1);
    assert_eq!(submit(&mut d, &odd16), Err(RenderError::InvalidDescription));
    assert_eq!(submit(&mut d, &odd32), Err(RenderError::InvalidDescription));
    let aligned = indexed_draw(p, vb, Some((ib, 4, IndexType::U32)), 2);
    assert!(submit(&mut d, &aligned).is_ok());
}

#[test]
fn barriers_between_passes_are_accepted_and_counted() {
    let mut d = device();
    let t = texture(&mut d, 2, 2);
    let mut s = CommandStream::new();
    s.barrier(
        RenderTarget::Texture(t),
        ResourceState::Undefined,
        ResourceState::ColorTarget,
    );
    s.begin_pass(pass_to(t, None));
    s.end_pass();
    s.barrier(
        RenderTarget::Backbuffer,
        ResourceState::ColorTarget,
        ResourceState::Present,
    );
    assert_eq!(
        submit(&mut d, &s),
        Ok(SubmitStats {
            commands: 4,
            draws: 0
        })
    );
}

#[test]
fn a_barrier_inside_a_pass_or_on_a_stale_texture_is_refused() {
    let mut d = device();
    let t = texture(&mut d, 2, 2);
    let inside = in_pass(|s| {
        s.barrier(
            RenderTarget::Backbuffer,
            ResourceState::Undefined,
            ResourceState::ColorTarget,
        );
    });
    let mut stale = CommandStream::new();
    stale.barrier(
        RenderTarget::Texture(t),
        ResourceState::Undefined,
        ResourceState::ColorTarget,
    );
    assert_eq!(
        submit(&mut d, &inside),
        Err(RenderError::InvalidDescription)
    );
    d.destroy_texture(t);
    assert_eq!(submit(&mut d, &stale), Err(RenderError::InvalidHandle));
}

/// A device that keeps handles and rules but no bytes (the bookkeeping a GPU device lets the Null
/// device do for it).
fn bookkeeping() -> NullDevice {
    NullDevice::bookkeeping_only(4)
}

#[test]
fn a_bookkeeping_device_checks_buffer_ranges_without_keeping_bytes() {
    let mut d = bookkeeping();
    let b = buffer(&mut d, 8, BufferUsage::Vertex);
    assert_eq!(d.write_buffer(b, 0, &[1; 8]), Ok(()));
    assert_eq!(d.write_buffer(b, 4, &[0; 4]), Ok(()));
    assert_eq!(
        d.write_buffer(b, 4, &[0; 5]),
        Err(RenderError::InvalidDescription)
    );
    d.destroy_buffer(b);
    assert_eq!(
        d.write_buffer(b, 0, &[0; 1]),
        Err(RenderError::InvalidHandle)
    );
}

#[test]
fn a_bookkeeping_device_checks_texture_regions_and_cannot_read_back() {
    let mut d = bookkeeping();
    let t = texture(&mut d, 4, 2);
    assert_eq!(d.write_texture(t, rect(0, 0, 4, 2), &[7; 32]), Ok(()));
    assert_eq!(d.write_texture(t, rect(1, 1, 2, 1), &[7; 8]), Ok(()));
    assert_eq!(
        d.write_texture(t, rect(3, 0, 2, 1), &[0; 8]),
        Err(RenderError::InvalidDescription)
    );
    assert_eq!(
        d.write_texture(t, rect(0, 0, 4, 2), &[0; 31]),
        Err(RenderError::InvalidDescription)
    );
    let mut out = [0u8; 32];
    assert_eq!(d.read_back(t, &mut out), Err(RenderError::Unsupported));
    assert_eq!(out, [0u8; 32]);
}

#[test]
fn a_bookkeeping_device_validates_streams_like_the_full_one() {
    let mut d = bookkeeping();
    let t = texture(&mut d, 2, 2);
    let mut clear = CommandStream::new();
    clear.begin_pass(pass_to(t, Some([1.0, 0.0, 0.0, 1.0])));
    clear.end_pass();
    assert_eq!(
        submit(&mut d, &clear),
        Ok(SubmitStats {
            commands: 2,
            draws: 0
        })
    );
    let mut unclosed = CommandStream::new();
    unclosed.begin_pass(backbuffer_pass());
    assert_eq!(
        submit(&mut d, &unclosed),
        Err(RenderError::InvalidDescription)
    );
}
