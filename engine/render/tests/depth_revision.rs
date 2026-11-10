//! Card 6-2: the port revision that adds depth to a pass, a pipeline and a barrier.
//! The first group pins the bytes of streams that do not use depth: they must not change.

use base::pool::Handle;
use render::desc::{
    BlendMode, BufferDesc, BufferUsage, DepthState, PassDesc, PipelineDesc, Rect, RenderTarget,
    ResourceState, ShaderDesc, ShaderStage, SurfaceDesc, TextureDesc, TextureFormat,
};
use render::device::IRenderDevice;
use render::error::RenderError;
use render::ids::TextureId;
use render::null::NullDevice;
use render::stream::{Command, CommandStream, StreamError};

const HEADER: [u8; 4] = [1, 0, 0, 0];

fn texture(index: u32) -> TextureId {
    Handle::from_raw(index, 0)
}

fn le(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Bytes of a `begin_pass` command as the port wrote them before the revision (independent oracle).
fn old_begin_pass(
    target_tag: u8,
    flags: u8,
    color: [f32; 4],
    depth: f32,
    texture: (u32, u32),
) -> Vec<u8> {
    let mut payload = vec![target_tag, flags];
    payload.extend(le(&color.map(f32::to_bits)));
    payload.extend(le(&[depth.to_bits(), texture.0, texture.1]));
    let mut out = vec![1, 0, u8::try_from(payload.len()).unwrap_or(0), 0];
    out.extend(payload);
    out
}

#[test]
fn a_pass_without_depth_keeps_its_bytes() {
    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target: RenderTarget::Texture(texture(2)),
        clear_color: Some([0.25, 0.5, 0.75, 1.0]),
        clear_depth: Some(1.0),
        depth: None,
    });
    stream.begin_pass(PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: None,
        clear_depth: None,
        depth: None,
    });
    let mut expected = HEADER.to_vec();
    expected.extend(old_begin_pass(1, 3, [0.25, 0.5, 0.75, 1.0], 1.0, (2, 0)));
    expected.extend(old_begin_pass(0, 0, [0.0; 4], 0.0, (0, 0)));
    assert_eq!(stream.as_bytes(), expected.as_slice());
}

#[test]
fn a_barrier_between_old_states_keeps_its_bytes() {
    let mut stream = CommandStream::new();
    stream.barrier(
        RenderTarget::Texture(texture(5)),
        ResourceState::ColorTarget,
        ResourceState::ShaderRead,
    );
    let mut expected = HEADER.to_vec();
    expected.extend([11, 0, 20, 0]);
    expected.extend(le(&[1, 5, 0, 1, 2]));
    assert_eq!(stream.as_bytes(), expected.as_slice());
}

#[test]
fn a_pass_with_depth_adds_the_depth_handle_and_round_trips() {
    let pass = PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: Some([0.0, 0.0, 0.0, 1.0]),
        clear_depth: Some(1.0),
        depth: Some(texture(7)),
    };
    let mut stream = CommandStream::new();
    stream.begin_pass(pass);
    let mut expected = HEADER.to_vec();
    let old = old_begin_pass(0, 3, [0.0, 0.0, 0.0, 1.0], 1.0, (0, 0));
    expected.extend([1, 0, 38, 0]);
    expected.extend(&old[4..]);
    expected.extend(le(&[7, 0]));
    assert_eq!(stream.as_bytes(), expected.as_slice());
    let decoded: Result<Vec<Command>, StreamError> = stream.commands().collect();
    assert_eq!(decoded, Ok(vec![Command::BeginPass(pass)]));
    assert_eq!(CommandStream::from_bytes(stream.as_bytes()), Ok(stream));
}

#[test]
fn a_depth_target_state_round_trips_and_unknown_states_are_refused() {
    let mut stream = CommandStream::new();
    stream.barrier(
        RenderTarget::Texture(texture(1)),
        ResourceState::Undefined,
        ResourceState::DepthTarget,
    );
    let decoded: Vec<_> = stream.commands().flatten().collect();
    assert_eq!(
        decoded,
        vec![Command::Barrier {
            target: RenderTarget::Texture(texture(1)),
            from: ResourceState::Undefined,
            to: ResourceState::DepthTarget,
        }]
    );
    let mut bad = HEADER.to_vec();
    bad.extend([11, 0, 20, 0]);
    bad.extend(le(&[1, 1, 0, 0, 5]));
    assert_eq!(CommandStream::from_bytes(&bad), Err(StreamError::BadValue));
}

#[test]
fn a_pass_payload_of_another_length_is_refused() {
    let mut bytes = HEADER.to_vec();
    bytes.extend(old_begin_pass(0, 0, [0.0; 4], 0.0, (0, 0)));
    bytes[6] = 34;
    bytes.extend([0, 0, 0, 0]);
    assert!(matches!(
        CommandStream::from_bytes(&bytes),
        Err(StreamError::BadLength { opcode: 1 })
    ));
}

// ---- Null device validation ----

struct Rig {
    device: NullDevice,
    frame: render::ids::FrameId,
}

fn rig() -> Rig {
    let mut device = NullDevice::new();
    let surface = device
        .create_surface(SurfaceDesc {
            native_token: 1,
            width: 4,
            height: 4,
        })
        .unwrap_or_else(|_| unreachable!());
    let frame = device
        .begin_frame(surface)
        .unwrap_or_else(|_| unreachable!());
    Rig { device, frame }
}

fn make_texture(rig: &mut Rig, size: u32, format: TextureFormat) -> TextureId {
    rig.device
        .create_texture(TextureDesc {
            width: size,
            height: size,
            mip_count: 1,
            format,
        })
        .unwrap_or_else(|_| unreachable!())
}

fn pipeline(
    rig: &mut Rig,
    depth: DepthState,
    depth_format: Option<TextureFormat>,
) -> Result<render::ids::PipelineId, RenderError> {
    let shader = |rig: &mut Rig, stage| {
        rig.device
            .create_shader(ShaderDesc {
                stage,
                code: b"x",
                entry: "main",
            })
            .unwrap_or_else(|_| unreachable!())
    };
    let vertex_shader = shader(rig, ShaderStage::Vertex);
    let fragment_shader = shader(rig, ShaderStage::Fragment);
    rig.device.create_pipeline(PipelineDesc {
        vertex_shader,
        fragment_shader,
        vertex_stride: 0,
        attributes: &[],
        blend: BlendMode::Opaque,
        depth,
        color_format: TextureFormat::Rgba8,
        depth_format,
    })
}

fn pass_with(colour: RenderTarget, depth: Option<TextureId>) -> PassDesc {
    PassDesc {
        target: colour,
        clear_color: Some([0.0; 4]),
        clear_depth: depth.map(|_| 1.0),
        depth,
    }
}

fn submit(rig: &mut Rig, stream: &CommandStream) -> Result<(), RenderError> {
    rig.device.submit(rig.frame, stream).map(|_| ())
}

const NO_DEPTH: DepthState = DepthState {
    test: false,
    write: false,
};
const DEPTH_ON: DepthState = DepthState {
    test: true,
    write: true,
};

#[test]
fn a_pass_may_use_a_depth_texture_of_the_colour_size() {
    let mut rig = rig();
    let colour = make_texture(&mut rig, 4, TextureFormat::Rgba8);
    let depth = make_texture(&mut rig, 4, TextureFormat::Depth32F);
    let mut stream = CommandStream::new();
    stream.begin_pass(pass_with(RenderTarget::Texture(colour), Some(depth)));
    stream.end_pass();
    assert_eq!(submit(&mut rig, &stream), Ok(()));
}

#[test]
fn a_depth_attachment_must_be_a_live_depth_texture_of_the_colour_size() {
    let mut rig = rig();
    let colour = make_texture(&mut rig, 4, TextureFormat::Rgba8);
    let wrong_format = make_texture(&mut rig, 4, TextureFormat::Rgba8);
    let wrong_size = make_texture(&mut rig, 2, TextureFormat::Depth32F);
    let stale = make_texture(&mut rig, 4, TextureFormat::Depth32F);
    rig.device.destroy_texture(stale);
    for (name, depth, expected) in [
        (
            "not a depth format",
            wrong_format,
            RenderError::InvalidDescription,
        ),
        ("size differs", wrong_size, RenderError::InvalidDescription),
        ("stale handle", stale, RenderError::InvalidHandle),
    ] {
        let mut stream = CommandStream::new();
        stream.begin_pass(pass_with(RenderTarget::Texture(colour), Some(depth)));
        stream.end_pass();
        assert_eq!(submit(&mut rig, &stream), Err(expected), "{name}");
    }
}

#[test]
fn the_backbuffer_takes_a_depth_texture_of_its_size() {
    let mut rig = rig();
    let ok = make_texture(&mut rig, 4, TextureFormat::Depth32F);
    let small = make_texture(&mut rig, 2, TextureFormat::Depth32F);
    for (depth, expected) in [(ok, Ok(())), (small, Err(RenderError::InvalidDescription))] {
        let mut stream = CommandStream::new();
        stream.begin_pass(pass_with(RenderTarget::Backbuffer, Some(depth)));
        stream.end_pass();
        assert_eq!(submit(&mut rig, &stream), expected);
    }
}

#[test]
fn a_pipeline_that_tests_depth_needs_a_depth_format() {
    let mut rig = rig();
    assert!(pipeline(&mut rig, DEPTH_ON, Some(TextureFormat::Depth32F)).is_ok());
    assert!(pipeline(&mut rig, NO_DEPTH, None).is_ok());
    assert_eq!(
        pipeline(&mut rig, DEPTH_ON, None),
        Err(RenderError::InvalidDescription)
    );
    assert_eq!(
        pipeline(&mut rig, NO_DEPTH, Some(TextureFormat::Rgba8)),
        Err(RenderError::InvalidDescription)
    );
}

#[test]
fn a_pipeline_must_match_the_depth_of_the_pass() {
    let mut rig = rig();
    let colour = make_texture(&mut rig, 4, TextureFormat::Rgba8);
    let depth = make_texture(&mut rig, 4, TextureFormat::Depth32F);
    let with = pipeline(&mut rig, DEPTH_ON, Some(TextureFormat::Depth32F))
        .unwrap_or_else(|_| unreachable!());
    let without = pipeline(&mut rig, NO_DEPTH, None).unwrap_or_else(|_| unreachable!());
    let vertices = rig
        .device
        .create_buffer(BufferDesc {
            size: 64,
            usage: BufferUsage::Vertex,
        })
        .unwrap_or_else(|_| unreachable!());
    let _ = vertices;
    for (name, attachment, chosen, expected) in [
        ("depth pipeline, depth pass", Some(depth), with, Ok(())),
        ("plain pipeline, plain pass", None, without, Ok(())),
        (
            "depth pipeline, plain pass",
            None,
            with,
            Err(RenderError::InvalidDescription),
        ),
        (
            "plain pipeline, depth pass",
            Some(depth),
            without,
            Err(RenderError::InvalidDescription),
        ),
    ] {
        let mut stream = CommandStream::new();
        stream.begin_pass(pass_with(RenderTarget::Texture(colour), attachment));
        stream.set_pipeline(chosen);
        stream.draw(3, 1);
        stream.end_pass();
        assert_eq!(submit(&mut rig, &stream), expected, "{name}");
    }
}

#[test]
fn barriers_keep_depth_states_on_depth_textures() {
    let mut rig = rig();
    let colour = make_texture(&mut rig, 4, TextureFormat::Rgba8);
    let depth = make_texture(&mut rig, 4, TextureFormat::Depth32F);
    for (name, target, to, expected) in [
        (
            "depth target on a depth texture",
            RenderTarget::Texture(depth),
            ResourceState::DepthTarget,
            Ok(()),
        ),
        (
            "shader read of a depth texture",
            RenderTarget::Texture(depth),
            ResourceState::ShaderRead,
            Ok(()),
        ),
        (
            "depth target on a colour texture",
            RenderTarget::Texture(colour),
            ResourceState::DepthTarget,
            Err(RenderError::InvalidDescription),
        ),
        (
            "colour target on a depth texture",
            RenderTarget::Texture(depth),
            ResourceState::ColorTarget,
            Err(RenderError::InvalidDescription),
        ),
        (
            "depth target on the backbuffer",
            RenderTarget::Backbuffer,
            ResourceState::DepthTarget,
            Err(RenderError::InvalidDescription),
        ),
    ] {
        let mut stream = CommandStream::new();
        stream.barrier(target, ResourceState::Undefined, to);
        assert_eq!(submit(&mut rig, &stream), expected, "{name}");
    }
}

#[test]
fn the_null_device_clears_a_depth_texture() {
    let mut rig = rig();
    let colour = make_texture(&mut rig, 2, TextureFormat::Rgba8);
    let depth = make_texture(&mut rig, 2, TextureFormat::Depth32F);
    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target: RenderTarget::Texture(colour),
        clear_color: None,
        clear_depth: Some(0.5),
        depth: Some(depth),
    });
    stream.end_pass();
    assert_eq!(submit(&mut rig, &stream), Ok(()));
    let mut out = vec![0; 16];
    assert_eq!(rig.device.read_back(depth, &mut out), Ok(()));
    assert!(
        out.chunks_exact(4)
            .all(|texel| texel == 0.5f32.to_le_bytes())
    );
    let _ = Rect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };
}

#[test]
fn a_depth_texture_cannot_be_its_own_colour_target() {
    let mut rig = rig();
    let depth = make_texture(&mut rig, 4, TextureFormat::Depth32F);
    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target: RenderTarget::Texture(depth),
        clear_color: None,
        clear_depth: Some(1.0),
        depth: Some(depth),
    });
    stream.end_pass();
    assert_eq!(
        submit(&mut rig, &stream),
        Err(RenderError::InvalidDescription)
    );
}
