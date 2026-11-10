use super::*;
use crate::desc::{IndexType, PassDesc, Rect, RenderTarget, ResourceState};
use base::pool::Handle;

const GOLDEN: &str = include_str!("../../golden/basic_scene.txt");

fn h<T>(index: u32, generation: u32) -> Handle<T> {
    Handle::from_raw(index, generation)
}

fn basic_scene() -> CommandStream {
    let mut s = CommandStream::new();
    s.begin_pass(PassDesc {
        target: RenderTarget::Backbuffer,
        clear_color: Some([0.25, 0.5, 0.75, 1.0]),
        clear_depth: None,
        depth: None,
    });
    s.set_pipeline(h(0, 0));
    s.set_viewport(Rect {
        x: 0,
        y: 0,
        width: 640,
        height: 480,
    });
    s.bind_uniforms(0, h(2, 0));
    s.bind_texture(1, h(3, 1), h(4, 0));
    s.bind_vertex_buffer(0, h(1, 0), 0);
    s.bind_index_buffer(h(5, 2), 8, IndexType::U16);
    s.draw(3, 1);
    s.draw_indexed(6, 2);
    s.end_pass();
    s.begin_pass(PassDesc {
        target: RenderTarget::Texture(h(3, 1)),
        clear_color: None,
        clear_depth: Some(1.0),
        depth: None,
    });
    s.end_pass();
    s
}

#[test]
fn the_basic_scene_matches_the_checked_in_golden_dump() {
    // A Windows checkout may convert the golden file to CRLF.
    let golden: String = GOLDEN.chars().filter(|c| *c != char::from(13u8)).collect();
    assert_eq!(dump(&basic_scene()), golden);
}

#[test]
fn an_empty_stream_is_only_the_header_line() {
    assert_eq!(dump(&CommandStream::new()), "render-stream v1\n");
}

#[test]
fn a_u32_index_type_and_negative_viewport_are_spelled_out() {
    let mut s = CommandStream::new();
    s.bind_index_buffer(h(0, 0), 0, IndexType::U32);
    s.set_viewport(Rect {
        x: -1,
        y: -2,
        width: 3,
        height: 4,
    });
    let text = dump(&s);
    assert!(text.contains("type=u32"), "{text}");
    assert!(text.contains("x=-1 y=-2 w=3 h=4"), "{text}");
}

#[test]
fn a_broken_stream_ends_with_an_error_line() {
    let mut bytes = CommandStream::new().as_bytes().to_vec();
    bytes.extend_from_slice(&[0xEE, 0, 0, 0]);
    let text = dump_bytes(&bytes);
    assert_eq!(text, "render-stream v1\nerror: unknown opcode 238\n");
    assert_eq!(dump_bytes(&[1]), "error: stream is truncated\n");
}

#[test]
fn barriers_are_dumped_with_their_states() {
    let mut s = CommandStream::new();
    s.barrier(
        RenderTarget::Texture(h(3, 1)),
        ResourceState::Undefined,
        ResourceState::ColorTarget,
    );
    s.barrier(
        RenderTarget::Backbuffer,
        ResourceState::ColorTarget,
        ResourceState::Present,
    );
    assert_eq!(
        dump(&s),
        "render-stream v1
barrier target=texture(#3.1) from=undefined to=color_target
barrier target=backbuffer from=color_target to=present
"
    );
    let mut s = CommandStream::new();
    s.barrier(
        RenderTarget::Backbuffer,
        ResourceState::ShaderRead,
        ResourceState::ShaderRead,
    );
    assert!(dump(&s).contains("from=shader_read to=shader_read"));
}
