use super::*;
use crate::ids::{Buffer, BufferId, Pipeline, PipelineId, Sampler, SamplerId, Texture, TextureId};
use base::pool::{Handle, Pool};

/// Issues a handle from a pool, failing the test when the pool is full.
fn handle<T>(pool: &mut Pool<T>, value: T) -> Handle<T> {
    let issued = pool.insert(value).ok();
    assert!(issued.is_some(), "pool full");
    issued.unwrap_or_else(|| Handle::from_raw(0, 0))
}

fn decode_all(stream: &CommandStream) -> Result<Vec<Command>, StreamError> {
    stream.commands().collect()
}

#[test]
fn a_new_stream_has_the_version_header_and_no_commands() {
    let stream = CommandStream::new();
    assert_eq!(stream.as_bytes(), &STREAM_VERSION.to_le_bytes());
    assert!(stream.is_empty());
    assert_eq!(decode_all(&stream), Ok(Vec::new()));
}

type Ids = (BufferId, TextureId, SamplerId, PipelineId);

fn ids() -> Ids {
    (
        handle(&mut Pool::<Buffer>::with_capacity(1), Buffer),
        handle(&mut Pool::<Texture>::with_capacity(1), Texture),
        handle(&mut Pool::<Sampler>::with_capacity(1), Sampler),
        handle(&mut Pool::<Pipeline>::with_capacity(1), Pipeline),
    )
}

fn passes(texture: TextureId) -> (PassDesc, PassDesc) {
    let offscreen = PassDesc {
        target: RenderTarget::Texture(texture),
        clear_color: Some([0.25, 0.5, 0.75, 1.0]),
        clear_depth: Some(1.0),
    };
    let backbuffer = PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: None,
        clear_depth: None,
    };
    (offscreen, backbuffer)
}

const VIEW: Rect = Rect {
    x: -3,
    y: 4,
    width: 640,
    height: 480,
};

/// A stream with every command kind.
fn sample_stream((b, t, s, p): Ids) -> CommandStream {
    let (offscreen, backbuffer) = passes(t);
    let mut stream = CommandStream::new();
    stream.begin_pass(offscreen);
    stream.set_pipeline(p);
    stream.set_viewport(VIEW);
    stream.bind_uniforms(2, b);
    stream.bind_texture(1, t, s);
    stream.bind_vertex_buffer(0, b, 16);
    stream.bind_index_buffer(b, 8, IndexType::U16);
    stream.draw(3, 1);
    stream.draw_indexed(6, 2);
    stream.end_pass();
    stream.begin_pass(backbuffer);
    stream.bind_index_buffer(b, 0, IndexType::U32);
    stream.end_pass();
    stream
}

/// What `sample_stream` must decode to.
fn sample_commands((b, t, s, p): Ids) -> Vec<Command> {
    let (offscreen, backbuffer) = passes(t);
    vec![
        Command::BeginPass(offscreen),
        Command::SetPipeline(p),
        Command::SetViewport(VIEW),
        Command::BindUniforms { slot: 2, buffer: b },
        Command::BindTexture {
            slot: 1,
            texture: t,
            sampler: s,
        },
        Command::BindVertexBuffer {
            slot: 0,
            buffer: b,
            offset: 16,
        },
        Command::BindIndexBuffer {
            buffer: b,
            offset: 8,
            index_type: IndexType::U16,
        },
        Command::Draw {
            vertices: 3,
            instances: 1,
        },
        Command::DrawIndexed {
            indices: 6,
            instances: 2,
        },
        Command::EndPass,
        Command::BeginPass(backbuffer),
        Command::BindIndexBuffer {
            buffer: b,
            offset: 0,
            index_type: IndexType::U32,
        },
        Command::EndPass,
    ]
}

#[test]
fn every_command_round_trips() {
    let all = ids();
    assert_eq!(decode_all(&sample_stream(all)), Ok(sample_commands(all)));
}

#[test]
fn the_header_of_a_command_is_opcode_then_payload_length_little_endian() {
    let mut stream = CommandStream::new();
    stream.draw(3, 1);
    let bytes = stream.as_bytes();
    assert_eq!(&bytes[4..6], &9u16.to_le_bytes());
    assert_eq!(&bytes[6..8], &8u16.to_le_bytes());
    assert_eq!(&bytes[8..12], &3u32.to_le_bytes());
    assert_eq!(&bytes[12..16], &1u32.to_le_bytes());
    assert_eq!(bytes.len(), 16);
}

#[test]
fn clear_keeps_the_capacity_and_the_header() {
    let mut stream = CommandStream::with_capacity(1024);
    for _ in 0..50 {
        stream.draw(3, 1);
    }
    let capacity = stream.capacity();
    assert!(capacity >= 1024);
    assert!(!stream.is_empty());
    stream.clear();
    assert!(stream.is_empty());
    assert_eq!(stream.as_bytes(), &STREAM_VERSION.to_le_bytes());
    assert_eq!(stream.capacity(), capacity);
    for _ in 0..50 {
        stream.draw(3, 1);
    }
    assert_eq!(
        stream.capacity(),
        capacity,
        "refilling to the same size must not grow the buffer"
    );
}

#[test]
fn a_wrong_version_is_refused() {
    let mut bytes = 2u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[2, 0, 0, 0]);
    assert_eq!(
        CommandStream::from_bytes(&bytes).err(),
        Some(StreamError::BadVersion(2))
    );
}

#[test]
fn a_short_header_is_truncated() {
    assert_eq!(
        CommandStream::from_bytes(&[1, 0]).err(),
        Some(StreamError::Truncated)
    );
    assert_eq!(
        CommandStream::from_bytes(&[]).err(),
        Some(StreamError::Truncated)
    );
}

#[test]
fn a_cut_command_is_truncated() {
    let mut stream = CommandStream::new();
    stream.draw(3, 1);
    let bytes = stream.as_bytes();
    for cut in 5..bytes.len() {
        assert_eq!(
            CommandStream::from_bytes(&bytes[..cut]).err(),
            Some(StreamError::Truncated),
            "cut at {cut}"
        );
    }
}

#[test]
fn an_unknown_opcode_is_refused() {
    let mut bytes = STREAM_VERSION.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[0xFF, 0x7F, 0, 0]);
    assert_eq!(
        CommandStream::from_bytes(&bytes).err(),
        Some(StreamError::UnknownOpcode(0x7FFF))
    );
}

#[test]
fn a_payload_of_the_wrong_length_is_refused() {
    // Draw with 4 payload bytes instead of 8.
    let mut bytes = STREAM_VERSION.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[9, 0, 4, 0, 1, 0, 0, 0]);
    assert_eq!(
        CommandStream::from_bytes(&bytes).err(),
        Some(StreamError::BadLength { opcode: 9 })
    );
    // EndPass with a payload.
    let mut bytes = STREAM_VERSION.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[2, 0, 1, 0, 0]);
    assert_eq!(
        CommandStream::from_bytes(&bytes).err(),
        Some(StreamError::BadLength { opcode: 2 })
    );
}

#[test]
fn an_invalid_enum_value_is_refused() {
    let b = handle(&mut Pool::<Buffer>::with_capacity(1), Buffer);
    let mut stream = CommandStream::new();
    stream.bind_index_buffer(b, 0, IndexType::U16);
    let mut bytes = stream.as_bytes().to_vec();
    let tag = bytes.len() - 4;
    bytes[tag] = 9;
    assert_eq!(
        CommandStream::from_bytes(&bytes).err(),
        Some(StreamError::BadValue)
    );
}

#[test]
fn from_bytes_accepts_what_as_bytes_gave() {
    let mut stream = CommandStream::new();
    stream.draw(3, 1);
    stream.draw_indexed(6, 1);
    let copy = CommandStream::from_bytes(stream.as_bytes()).ok();
    assert_eq!(
        copy.map(|c| c.as_bytes().to_vec()),
        Some(stream.as_bytes().to_vec())
    );
}

#[test]
fn iteration_stops_after_the_first_error() {
    let mut bytes = STREAM_VERSION.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[0xEE, 0, 0, 0, 2, 0, 0, 0]);
    let stream = CommandStream { bytes };
    let mut commands = stream.commands();
    assert_eq!(commands.next(), Some(Err(StreamError::UnknownOpcode(0xEE))));
    assert_eq!(commands.next(), None);
}

#[test]
fn stream_errors_have_distinct_messages() {
    let all = [
        StreamError::BadVersion(3),
        StreamError::Truncated,
        StreamError::UnknownOpcode(1),
        StreamError::BadLength { opcode: 1 },
        StreamError::BadValue,
    ];
    let texts: Vec<String> = all.iter().map(ToString::to_string).collect();
    let pairs = texts
        .iter()
        .enumerate()
        .flat_map(|(i, a)| texts[i + 1..].iter().map(move |b| (a, b)));
    for (a, b) in pairs {
        assert_ne!(a, b);
    }
}

#[test]
fn a_pass_with_an_unknown_target_or_flag_is_refused() {
    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: None,
        clear_depth: None,
    });
    let target_at = HEADER_LEN + COMMAND_HEADER_LEN;
    for (offset, value) in [(target_at, 2u8), (target_at + 1, 0b100)] {
        let mut bytes = stream.as_bytes().to_vec();
        bytes[offset] = value;
        assert_eq!(
            CommandStream::from_bytes(&bytes).err(),
            Some(StreamError::BadValue),
            "byte {offset}"
        );
    }
}

#[test]
fn clear_values_survive_bit_for_bit() {
    let pass = PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: Some([f32::NAN, -0.0, f32::INFINITY, f32::MIN_POSITIVE]),
        clear_depth: Some(-0.0),
    };
    let mut stream = CommandStream::new();
    stream.begin_pass(pass);
    let decoded = decode_all(&stream).ok().and_then(|c| c.first().copied());
    let back = match decoded {
        Some(Command::BeginPass(back)) => back,
        other => PassDesc {
            target: RenderTarget::Backbuffer,
            clear_color: None,
            clear_depth: Some(f32::from(u8::from(other.is_some()))),
        },
    };
    let bits = |p: PassDesc| {
        (
            p.clear_color.map(|c| c.map(f32::to_bits)),
            p.clear_depth.map(f32::to_bits),
        )
    };
    assert_eq!(bits(back), bits(pass));
}

#[test]
fn viewport_extremes_round_trip() {
    for (x, y) in [(i32::MIN, i32::MAX), (-1, 0), (i32::MAX, i32::MIN)] {
        let view = Rect {
            x,
            y,
            width: u32::MAX,
            height: 0,
        };
        let mut stream = CommandStream::new();
        stream.set_viewport(view);
        assert_eq!(decode_all(&stream), Ok(vec![Command::SetViewport(view)]));
    }
}

#[test]
fn opcode_zero_is_refused() {
    let opcode = 0u16;
    let mut bytes = STREAM_VERSION.to_le_bytes().to_vec();
    bytes.extend_from_slice(&opcode.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    assert_eq!(
        CommandStream::from_bytes(&bytes).err(),
        Some(StreamError::UnknownOpcode(opcode))
    );
}

#[test]
fn a_declared_length_past_the_end_is_truncated_and_a_long_payload_is_a_bad_length() {
    let mut bytes = STREAM_VERSION.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[9, 0, 40, 0, 1, 2]);
    assert_eq!(
        CommandStream::from_bytes(&bytes).err(),
        Some(StreamError::Truncated)
    );
    let mut bytes = STREAM_VERSION.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[3, 0, 12, 0]);
    bytes.extend_from_slice(&[0; 12]);
    assert_eq!(
        CommandStream::from_bytes(&bytes).err(),
        Some(StreamError::BadLength { opcode: 3 })
    );
}

#[test]
fn commands_before_a_bad_one_are_still_delivered() {
    let mut stream = CommandStream::new();
    stream.draw(3, 1);
    let mut bytes = stream.as_bytes().to_vec();
    bytes.extend_from_slice(&[0xEE, 0, 0, 0]);
    let broken = CommandStream { bytes };
    let mut commands = broken.commands();
    assert_eq!(
        commands.next(),
        Some(Ok(Command::Draw {
            vertices: 3,
            instances: 1
        }))
    );
    assert_eq!(commands.next(), Some(Err(StreamError::UnknownOpcode(0xEE))));
    assert_eq!(commands.next(), None);
}

#[test]
fn non_canonical_pass_fields_are_refused() {
    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: None,
        clear_depth: None,
    });
    let first = HEADER_LEN + COMMAND_HEADER_LEN;
    // colour bytes, depth bytes, texture handle bytes: each non-zero while unused.
    for offset in [first + 2, first + 18, first + 22] {
        let mut bytes = stream.as_bytes().to_vec();
        bytes[offset] = 1;
        assert_eq!(
            CommandStream::from_bytes(&bytes).err(),
            Some(StreamError::BadValue),
            "byte {offset}"
        );
    }
}

#[test]
fn largest_payload_fits_the_length_field() {
    let (b, t, ..) = ids();
    let mut stream = CommandStream::new();
    stream.begin_pass(PassDesc {
        target: RenderTarget::Texture(t),
        clear_color: Some([1.0; 4]),
        clear_depth: Some(1.0),
    });
    stream.bind_texture(0, t, Handle::from_raw(1, 1));
    stream.bind_vertex_buffer(0, b, 0);
    let longest = stream.as_bytes()[HEADER_LEN + 2..HEADER_LEN + 4].to_vec();
    assert_eq!(u16::from_le_bytes([longest[0], longest[1]]), 30);
}

#[test]
fn barriers_round_trip() {
    let (_, t, ..) = ids();
    let mut stream = CommandStream::new();
    stream.barrier(
        RenderTarget::Texture(t),
        ResourceState::Undefined,
        ResourceState::ColorTarget,
    );
    stream.barrier(
        RenderTarget::Backbuffer,
        ResourceState::ColorTarget,
        ResourceState::Present,
    );
    stream.barrier(
        RenderTarget::Texture(t),
        ResourceState::ShaderRead,
        ResourceState::ColorTarget,
    );
    let expected = vec![
        Command::Barrier {
            target: RenderTarget::Texture(t),
            from: ResourceState::Undefined,
            to: ResourceState::ColorTarget,
        },
        Command::Barrier {
            target: RenderTarget::Backbuffer,
            from: ResourceState::ColorTarget,
            to: ResourceState::Present,
        },
        Command::Barrier {
            target: RenderTarget::Texture(t),
            from: ResourceState::ShaderRead,
            to: ResourceState::ColorTarget,
        },
    ];
    assert_eq!(decode_all(&stream), Ok(expected));
    assert_eq!(
        stream.as_bytes()[HEADER_LEN..HEADER_LEN + 2],
        11u16.to_le_bytes()
    );
}

#[test]
fn a_barrier_with_an_unknown_state_or_target_is_refused() {
    let mut stream = CommandStream::new();
    stream.barrier(
        RenderTarget::Backbuffer,
        ResourceState::Undefined,
        ResourceState::Present,
    );
    let first = HEADER_LEN + COMMAND_HEADER_LEN;
    // target tag, handle (must be zero for the backbuffer), from, to.
    for offset in [first, first + 4, first + 12, first + 16] {
        let mut bytes = stream.as_bytes().to_vec();
        bytes[offset] = 9;
        assert_eq!(
            CommandStream::from_bytes(&bytes).err(),
            Some(StreamError::BadValue),
            "byte {offset}"
        );
    }
}

#[test]
fn opcodes_after_the_barrier_are_still_reserved() {
    let mut bytes = STREAM_VERSION.to_le_bytes().to_vec();
    bytes.extend_from_slice(&[12, 0, 0, 0]);
    assert_eq!(
        CommandStream::from_bytes(&bytes).err(),
        Some(StreamError::UnknownOpcode(12))
    );
}
