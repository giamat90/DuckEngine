//! The whole sandbox on the Null device: the golden command stream, the controls and a resize.

use crate::common::{Failure, fingerprint, null_session};
use app_physics_sample::simulation::STEP_TICKS;
use platform::window::{Action, Event, WindowId};

/// The fingerprint of the command stream after the given frame of the session with seed 24 (one
/// physics step per frame, no input). A change here means the picture changed: look at it before
/// you update the number.
const GOLDEN: [(u32, u64); 3] = [
    (1, 8_348_549_300_978_947_645),
    (60, 17_657_774_038_950_972_366),
    (200, 9_033_642_974_862_857_580),
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
fn two_frames_with_the_same_sprites_share_a_stream() -> Result<(), Failure> {
    let (mut device, mut session) = null_session(24)?;
    session.frame(&mut device, STEP_TICKS)?;
    let first = fingerprint(session.renderer().stream().as_bytes());
    session.frame(&mut device, STEP_TICKS)?;
    // The vertex data differs, the commands do not: the stream holds handles and counts only.
    assert_eq!(first, fingerprint(session.renderer().stream().as_bytes()));
    Ok(())
}

#[test]
fn every_sprite_is_drawn_in_one_frame() -> Result<(), Failure> {
    let (mut device, mut session) = null_session(24)?;
    for _ in 0..30 {
        session.frame(&mut device, STEP_TICKS)?;
    }
    let stats = session.renderer().stats();
    assert!(stats.draws >= 1, "{stats:?}");
    assert!(stats.commands > stats.draws);
    Ok(())
}

#[test]
fn the_controls_drop_a_body_and_reset_the_arena() -> Result<(), Failure> {
    let (mut device, mut session) = null_session(24)?;
    let window = WindowId::from_raw(0, 0);
    session.frame(&mut device, STEP_TICKS)?;
    let before = session.sandbox().bodies();
    session.apply(&Event::MouseButton {
        window,
        button: 0,
        action: Action::Press,
    });
    let report = session.frame(&mut device, STEP_TICKS)?;
    assert_eq!(report.spawned, 1);
    assert_eq!(session.sandbox().bodies(), before + 1);
    session.apply(&Event::Key {
        window,
        scancode: app_physics_sample::controls::keys::RESET,
        action: Action::Press,
        mods: 0,
    });
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
    session.apply(&Event::Key {
        window: WindowId::from_raw(0, 0),
        scancode: app_physics_sample::controls::keys::ESCAPE,
        action: Action::Press,
        mods: 0,
    });
    assert!(session.quit());
    Ok(())
}
