//! The whole sample on the Null device: the golden command stream, the controls, a resize, and
//! the renderer's refusals.

use crate::common::{Failure, STEP_TICKS, VIEW, fingerprint, null_session};
use app_sandbox3d::renderer::{LIT_GLSL, LIT_SPIRV, Renderer};
use assets::dshd::Program;
use platform::window::{Action, Event, WindowId};
use render::desc::SurfaceDesc;
use render::device::IRenderDevice;
use render::null::NullDevice;

fn window() -> WindowId {
    WindowId::from_raw(0, 0)
}

fn key(code: u32) -> Event {
    Event::Key {
        window: window(),
        scancode: code,
        action: Action::Press,
        mods: 0,
    }
}

/// The scancode of `wanted` on this machine.
fn scancode(wanted: input::KeyCode) -> u32 {
    (0..=0x1FF)
        .find(|code| input::KeyMap::host().key(*code) == Some(wanted))
        .unwrap_or(u32::MAX)
}

/// The fingerprint of the command stream after the given frame of the session with seed 24 (one
/// physics step per frame, no input). A change here means the picture changed: look at it before
/// you update the number.
const GOLDEN: [(u32, u64); 3] = [
    (1, 10_615_325_251_511_061_453),
    (60, 2_537_191_119_467_477_856),
    (200, 14_345_566_213_154_742_765),
];

#[test]
fn the_command_stream_matches_the_golden_fingerprints() -> Result<(), Failure> {
    let (mut device, mut session) = null_session(24)?;
    let mut seen = Vec::new();
    for frame in 1..=200 {
        session.frame(&mut device, STEP_TICKS)?;
        if GOLDEN.iter().any(|(at, _)| *at == frame) {
            seen.push((frame, fingerprint(session.renderer().stream().as_bytes())));
        }
    }
    assert_eq!(seen, GOLDEN.to_vec());
    Ok(())
}

#[test]
fn the_first_frame_draws_the_box_and_one_piece_in_one_depth_pass() -> Result<(), Failure> {
    let (mut device, mut session) = null_session(24)?;
    session.frame(&mut device, STEP_TICKS)?;
    let stats = session.renderer().stats();
    assert_eq!(stats.draws, 6, "{stats:?}");
    let text = render::dump::dump(session.renderer().stream());
    assert_eq!(text.matches("begin_pass").count(), 1, "{text}");
    assert!(text.contains("depth="), "{text}");
    assert_eq!(text.matches("draw_indexed").count(), 6, "{text}");
    Ok(())
}

#[test]
fn two_frames_with_the_same_objects_share_a_stream() -> Result<(), Failure> {
    let (mut device, mut session) = null_session(24)?;
    session.frame(&mut device, STEP_TICKS)?;
    let first = fingerprint(session.renderer().stream().as_bytes());
    session.frame(&mut device, STEP_TICKS)?;
    // The block data differs, the commands do not: the stream holds handles and counts only.
    assert_eq!(first, fingerprint(session.renderer().stream().as_bytes()));
    Ok(())
}

#[test]
fn the_space_key_drops_a_piece_and_r_resets_the_box() -> Result<(), Failure> {
    let (mut device, mut session) = null_session(24)?;
    session.frame(&mut device, STEP_TICKS)?;
    let before = session.sandbox().pieces();
    session.apply(&key(scancode(input::KeyCode::Space)));
    let report = session.frame(&mut device, STEP_TICKS)?;
    assert_eq!(report.spawned, 1);
    assert_eq!(session.sandbox().pieces(), before + 1);
    session.apply(&key(scancode(input::KeyCode::R)));
    session.frame(&mut device, STEP_TICKS)?;
    assert_eq!(session.sandbox().steps(), 1, "a reset starts at step zero");
    Ok(())
}

#[test]
fn a_resize_follows_the_window_and_a_zero_side_is_refused() -> Result<(), Failure> {
    let (mut device, mut session) = null_session(24)?;
    session.resize(&mut device, (640, 360))?;
    assert_eq!(session.size(), (640, 360));
    assert!(session.resize(&mut device, (0, 360)).is_err());
    assert!(session.resize(&mut device, (640, 0)).is_err());
    assert_eq!(session.size(), (640, 360));
    session.frame(&mut device, STEP_TICKS)?;
    Ok(())
}

#[test]
fn escape_asks_to_quit_and_nothing_else_does() -> Result<(), Failure> {
    let (mut device, mut session) = null_session(24)?;
    assert!(!session.quit());
    session.frame(&mut device, STEP_TICKS)?;
    assert!(!session.quit());
    session.apply(&key(scancode(input::KeyCode::Escape)));
    assert!(session.quit());
    Ok(())
}

fn surface(device: &mut NullDevice) -> Result<render::ids::SurfaceId, Failure> {
    Ok(device.create_surface(SurfaceDesc {
        native_token: 1,
        width: VIEW.0,
        height: VIEW.1,
    })?)
}

#[test]
fn both_checked_in_assets_build_a_renderer() -> Result<(), Failure> {
    for bytes in [LIT_GLSL, LIT_SPIRV] {
        let mut device = NullDevice::new();
        let surface = surface(&mut device)?;
        let program = Program::parse(bytes)?;
        assert!(Renderer::new(&mut device, &program, surface, VIEW).is_ok());
    }
    Ok(())
}

#[test]
fn a_shader_that_is_not_the_lit_one_is_refused() -> Result<(), Failure> {
    let mut device = NullDevice::new();
    let surface = surface(&mut device)?;
    let program = Program::parse(app_sprite_sample::GOLDEN_SHADER)?;
    let made = Renderer::new(&mut device, &program, surface, VIEW);
    assert!(matches!(made, Err(app_sandbox3d::Error::Render(_))));
    Ok(())
}

#[test]
fn a_renderer_for_a_zero_view_is_refused() -> Result<(), Failure> {
    let mut device = NullDevice::new();
    let surface = surface(&mut device)?;
    let program = Program::parse(LIT_GLSL)?;
    let made = Renderer::new(&mut device, &program, surface, (0, 10));
    assert!(matches!(made, Err(app_sandbox3d::Error::NoView)));
    Ok(())
}

#[test]
fn the_renderer_has_a_block_for_every_instance_the_scene_can_hold() -> Result<(), Failure> {
    let (_, session) = null_session(24)?;
    assert!(session.sandbox().scene().capacity() <= app_sandbox3d::renderer::MAX_OBJECTS);
    Ok(())
}

#[test]
fn a_zero_side_is_named_when_the_renderer_resizes() -> Result<(), Failure> {
    let (mut device, mut session) = null_session(24)?;
    for size in [(0, 360), (640, 0), (0, 0)] {
        let refused = session.resize(&mut device, size);
        assert!(
            matches!(refused, Err(app_sandbox3d::Error::NoView)),
            "{size:?}"
        );
    }
    Ok(())
}

#[test]
fn drawing_into_a_texture_changes_the_pass_target() -> Result<(), Failure> {
    let mut device = NullDevice::new();
    let surface = surface(&mut device)?;
    let program = Program::parse(LIT_GLSL)?;
    let mut renderer = Renderer::new(&mut device, &program, surface, VIEW)?;
    let scene = {
        let mut sandbox = app_sandbox3d::sandbox::Sandbox::new(renderer.texture(), 24, VIEW)?;
        sandbox.frame(
            app_sandbox3d::controls::Requests::default(),
            0,
            &mut Nothing,
        )?;
        sandbox
    };
    renderer.draw(&mut device, scene.scene())?;
    let before = render::dump::dump(renderer.stream());
    let target = device.create_texture(render::desc::TextureDesc {
        width: VIEW.0,
        height: VIEW.1,
        mip_count: 1,
        format: render::desc::TextureFormat::Rgba8,
    })?;
    renderer.set_target(target);
    renderer.draw(&mut device, scene.scene())?;
    assert_ne!(before, render::dump::dump(renderer.stream()));
    Ok(())
}

struct Nothing;

impl app_sandbox3d::sandbox::Draw for Nothing {
    fn draw(&mut self, _: &render::mesh_scene::MeshScene) -> Result<(), app_sandbox3d::Error> {
        Ok(())
    }
}

#[test]
fn input_made_while_no_frame_is_drawn_is_dropped() -> Result<(), Failure> {
    let (mut device, mut session) = null_session(24)?;
    session.frame(&mut device, STEP_TICKS)?;
    session.apply(&key(scancode(input::KeyCode::Space)));
    session.discard_input();
    let report = session.frame(&mut device, 0)?;
    assert_eq!(
        report.spawned, 0,
        "a press from a minimised window is not carried out later"
    );
    Ok(())
}
