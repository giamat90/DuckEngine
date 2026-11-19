use super::*;
use platform::window::{Action, Event, WindowId};

fn window() -> WindowId {
    WindowId::from_raw(0, 0)
}

fn press(key: KeyCode) -> Event {
    Event::Key {
        window: window(),
        scancode: scancode(key),
        action: Action::Press,
        mods: 0,
    }
}

fn release(key: KeyCode) -> Event {
    Event::Key {
        window: window(),
        scancode: scancode(key),
        action: Action::Release,
        mods: 0,
    }
}

fn scancode(key: KeyCode) -> u32 {
    (0..=0x1FF)
        .find(|code| KeyMap::host().key(*code) == Some(key))
        .unwrap_or(u32::MAX)
}

fn button(button: u8, action: Action) -> Event {
    Event::MouseButton {
        window: window(),
        button,
        action,
    }
}

fn motion(x: f32, y: f32) -> Event {
    Event::MouseMove {
        window: window(),
        x,
        y,
    }
}

fn frame(controls: &mut Controls, events: &[Event]) -> Requests {
    controls.begin_frame();
    for event in events {
        controls.apply(event);
    }
    controls.requests()
}

fn controls() -> Controls {
    Controls::new().unwrap_or_else(|e| unreachable!("{e:?}"))
}

#[test]
fn a_quiet_frame_asks_for_nothing() {
    assert_eq!(frame(&mut controls(), &[]), Requests::default());
}

#[test]
fn space_and_the_primary_button_drop_a_sphere_at_first() {
    let mut controls = controls();
    let by_key = frame(&mut controls, &[press(KeyCode::Space)]);
    assert_eq!(by_key.drop, Some(Piece::Sphere));
    let by_click = frame(&mut controls, &[button(0, Action::Press)]);
    assert_eq!(by_click.drop, Some(Piece::Sphere));
    let held = frame(&mut controls, &[]);
    assert_eq!(held.drop, None, "a drop is asked once per press");
}

#[test]
fn the_number_keys_choose_the_piece_the_next_drop_makes() {
    let mut controls = controls();
    for (key, piece) in [
        (KeyCode::Digit2, Piece::Box),
        (KeyCode::Digit3, Piece::Capsule),
        (KeyCode::Digit1, Piece::Sphere),
    ] {
        let chosen = frame(&mut controls, &[press(key)]);
        assert_eq!(chosen.drop, None, "choosing drops nothing");
        let dropped = frame(&mut controls, &[press(KeyCode::Space)]);
        assert_eq!(dropped.drop, Some(piece));
        frame(&mut controls, &[release(KeyCode::Space)]);
    }
}

#[test]
fn r_asks_for_a_reset_once() {
    let mut controls = controls();
    assert!(frame(&mut controls, &[press(KeyCode::R)]).reset);
    assert!(!frame(&mut controls, &[]).reset);
}

#[test]
fn dragging_with_the_secondary_button_orbits_and_moving_alone_does_not() {
    let mut controls = controls();
    frame(&mut controls, &[motion(100.0, 100.0)]);
    let moved = frame(&mut controls, &[motion(110.0, 100.0)]);
    assert_eq!(moved.orbit, Vec2::ZERO);
    frame(&mut controls, &[button(1, Action::Press)]);
    let dragged = frame(&mut controls, &[motion(130.0, 90.0)]);
    assert_eq!(dragged.orbit, Vec2::new(20.0, -10.0));
    frame(&mut controls, &[button(1, Action::Release)]);
    let free = frame(&mut controls, &[motion(150.0, 90.0)]);
    assert_eq!(free.orbit, Vec2::ZERO);
}

#[test]
fn the_wheel_zooms() {
    let mut controls = controls();
    let scrolled = frame(
        &mut controls,
        &[Event::Scroll {
            window: window(),
            dx: 0.0,
            dy: 2.0,
        }],
    );
    assert!((scrolled.zoom - 2.0).abs() < 1e-6);
    assert!(frame(&mut controls, &[]).zoom.abs() < 1e-6);
}

#[test]
fn escape_and_closing_the_window_ask_to_quit() {
    let mut by_key = controls();
    assert!(!by_key.quit());
    frame(&mut by_key, &[press(KeyCode::Escape)]);
    assert!(by_key.quit());
    let mut by_window = controls();
    frame(
        &mut by_window,
        &[Event::CloseRequested { window: window() }],
    );
    assert!(by_window.quit());
}

#[test]
fn losing_focus_ends_a_drag() {
    let mut controls = controls();
    frame(&mut controls, &[motion(0.0, 0.0), button(1, Action::Press)]);
    frame(
        &mut controls,
        &[Event::Focus {
            window: window(),
            gained: false,
        }],
    );
    let after = frame(&mut controls, &[motion(50.0, 50.0)]);
    assert_eq!(after.orbit, Vec2::ZERO);
}

#[test]
fn quit_is_known_as_soon_as_the_event_is_applied() {
    let mut controls = controls();
    controls.begin_frame();
    controls.apply(&press(KeyCode::Escape));
    assert!(controls.quit(), "before the requests are taken");
    controls.requests();
    controls.begin_frame();
    assert!(controls.quit(), "and after the frame, as a latch");
}
