//! The per-frame state: events in, action states out.

use base::math::Vec2;
use input::{
    ActionId, ActionKind, Binding, ContextId, Input, InputError, InputMap, KeyCode, KeyMap,
};
use platform::headless::HeadlessWindows;
use platform::window::{Action, Event, IWindowSystem, WindowDesc, WindowId, mods};

const SPACE: u32 = 0x39;
const KEY_A: u32 = 0x1E;
const KEY_D: u32 = 0x20;
const ESCAPE: u32 = 0x01;
const UNMAPPED: u32 = 0x7F7;

struct Rig {
    input: Input,
    window: WindowId,
    fire: ActionId,
    walk: ActionId,
    zoom: ActionId,
    gameplay: ContextId,
    menu: ContextId,
}

fn rig() -> Result<Rig, InputError> {
    let mut map = InputMap::new();
    let fire = map.add_action("fire", ActionKind::Button)?;
    let walk = map.add_action("walk", ActionKind::Axis)?;
    let zoom = map.add_action("zoom", ActionKind::Axis)?;
    let gameplay = map.add_context("gameplay")?;
    let menu = map.add_context("menu")?;
    map.bind(gameplay, fire, Binding::Key(KeyCode::Space))?;
    map.bind(gameplay, fire, Binding::MouseButton(0))?;
    map.bind(gameplay, walk, Binding::keys(KeyCode::A, KeyCode::D))?;
    map.bind(gameplay, zoom, Binding::scroll_y())?;
    map.bind(menu, fire, Binding::Key(KeyCode::Escape))?;
    let (mut windows, _script) = HeadlessWindows::new(1);
    let window = windows
        .create_window(&WindowDesc {
            title: "t",
            width: 8,
            height: 8,
        })
        .map_err(|_| InputError::TooMany)?;
    Ok(Rig {
        input: Input::new(map, KeyMap::windows()),
        window,
        fire,
        walk,
        zoom,
        gameplay,
        menu,
    })
}

fn key(rig: &Rig, scancode: u32, action: Action) -> Event {
    Event::Key {
        window: rig.window,
        scancode,
        action,
        mods: mods::SHIFT,
    }
}

fn button(rig: &Rig, button: u8, action: Action) -> Event {
    Event::MouseButton {
        window: rig.window,
        button,
        action,
    }
}

#[test]
fn nothing_is_active_before_any_event() -> Result<(), InputError> {
    let rig = rig()?;
    assert_eq!(rig.input.unmapped_keys(), 0);
    assert!(!rig.input.held(rig.fire));
    assert!(!rig.input.pressed(rig.fire));
    assert!(!rig.input.released(rig.fire));
    assert!((rig.input.axis(rig.walk)).abs() < f32::EPSILON);
    assert_eq!(rig.input.pointer(), Vec2::ZERO);
    assert_eq!(rig.input.context(), Some(rig.gameplay));
    Ok(())
}

#[test]
fn a_press_is_pressed_for_one_frame_and_held_until_the_release() -> Result<(), InputError> {
    let mut rig = rig()?;
    rig.input.begin_frame();
    let press = key(&rig, SPACE, Action::Press);
    rig.input.apply(&press);
    assert!(rig.input.pressed(rig.fire) && rig.input.held(rig.fire));
    assert!(!rig.input.released(rig.fire));

    rig.input.begin_frame();
    assert!(!rig.input.pressed(rig.fire));
    assert!(rig.input.held(rig.fire));

    rig.input.begin_frame();
    let release = key(&rig, SPACE, Action::Release);
    rig.input.apply(&release);
    assert!(rig.input.released(rig.fire));
    assert!(!rig.input.held(rig.fire) && !rig.input.pressed(rig.fire));

    rig.input.begin_frame();
    assert!(!rig.input.released(rig.fire));
    Ok(())
}

#[test]
fn a_tap_inside_one_frame_is_not_lost() -> Result<(), InputError> {
    let mut rig = rig()?;
    rig.input.begin_frame();
    let (press, release) = (
        key(&rig, SPACE, Action::Press),
        key(&rig, SPACE, Action::Release),
    );
    rig.input.apply(&press);
    rig.input.apply(&release);
    assert!(rig.input.pressed(rig.fire) && rig.input.released(rig.fire));
    assert!(!rig.input.held(rig.fire));
    Ok(())
}

#[test]
fn a_key_repeat_makes_no_new_press() -> Result<(), InputError> {
    let mut rig = rig()?;
    let press = key(&rig, SPACE, Action::Press);
    rig.input.begin_frame();
    rig.input.apply(&press);
    rig.input.begin_frame();
    let repeat = key(&rig, SPACE, Action::Repeat);
    rig.input.apply(&repeat);
    assert!(!rig.input.pressed(rig.fire));
    assert!(rig.input.held(rig.fire));
    Ok(())
}

#[test]
fn a_second_press_of_a_key_that_is_down_makes_no_new_edge() -> Result<(), InputError> {
    let mut rig = rig()?;
    let press = key(&rig, SPACE, Action::Press);
    rig.input.begin_frame();
    rig.input.apply(&press);
    rig.input.begin_frame();
    rig.input.apply(&press);
    assert!(!rig.input.pressed(rig.fire));
    Ok(())
}

#[test]
fn a_release_of_a_key_that_is_up_makes_no_edge() -> Result<(), InputError> {
    let mut rig = rig()?;
    rig.input.begin_frame();
    let release = key(&rig, SPACE, Action::Release);
    rig.input.apply(&release);
    assert!(!rig.input.released(rig.fire));
    Ok(())
}

#[test]
fn a_mouse_button_drives_the_same_action_as_a_key() -> Result<(), InputError> {
    let mut rig = rig()?;
    rig.input.begin_frame();
    let click = button(&rig, 0, Action::Press);
    rig.input.apply(&click);
    assert!(rig.input.pressed(rig.fire) && rig.input.held(rig.fire));
    rig.input.begin_frame();
    let other = button(&rig, 1, Action::Press);
    rig.input.apply(&other);
    assert!(!rig.input.pressed(rig.fire));
    let up = button(&rig, 0, Action::Release);
    rig.input.apply(&up);
    assert!(rig.input.released(rig.fire) && !rig.input.held(rig.fire));
    Ok(())
}

#[test]
fn two_inputs_of_one_action_hold_it_until_both_are_up() -> Result<(), InputError> {
    let mut rig = rig()?;
    rig.input.begin_frame();
    let (space, click) = (
        key(&rig, SPACE, Action::Press),
        button(&rig, 0, Action::Press),
    );
    rig.input.apply(&space);
    rig.input.apply(&click);
    rig.input.begin_frame();
    let space_up = key(&rig, SPACE, Action::Release);
    rig.input.apply(&space_up);
    assert!(rig.input.held(rig.fire));
    Ok(())
}

#[test]
fn a_key_pair_gives_minus_one_zero_or_one() -> Result<(), InputError> {
    let mut rig = rig()?;
    let (a_down, d_down) = (
        key(&rig, KEY_A, Action::Press),
        key(&rig, KEY_D, Action::Press),
    );
    rig.input.begin_frame();
    rig.input.apply(&a_down);
    assert!((rig.input.axis(rig.walk) + 1.0).abs() < f32::EPSILON);
    rig.input.apply(&d_down);
    assert!(rig.input.axis(rig.walk).abs() < f32::EPSILON);
    let a_up = key(&rig, KEY_A, Action::Release);
    rig.input.apply(&a_up);
    assert!((rig.input.axis(rig.walk) - 1.0).abs() < f32::EPSILON);
    Ok(())
}

#[test]
fn the_wheel_is_an_axis_that_resets_every_frame() -> Result<(), InputError> {
    let mut rig = rig()?;
    rig.input.begin_frame();
    let scroll = Event::Scroll {
        window: rig.window,
        dx: 0.5,
        dy: 2.0,
    };
    rig.input.apply(&scroll);
    rig.input.apply(&scroll);
    assert!((rig.input.axis(rig.zoom) - 4.0).abs() < f32::EPSILON);
    assert_eq!(rig.input.scroll(), Vec2::new(1.0, 4.0));
    rig.input.begin_frame();
    assert!(rig.input.axis(rig.zoom).abs() < f32::EPSILON);
    Ok(())
}

#[test]
fn the_pointer_has_a_position_and_a_delta_per_frame() -> Result<(), InputError> {
    let mut rig = rig()?;
    let at = |x: f32, y: f32| Event::MouseMove {
        window: rig.window,
        x,
        y,
    };
    rig.input.begin_frame();
    rig.input.apply(&at(10.0, 20.0));
    assert_eq!(rig.input.pointer(), Vec2::new(10.0, 20.0));
    assert_eq!(rig.input.pointer_delta(), Vec2::ZERO);
    rig.input.begin_frame();
    rig.input.apply(&at(13.0, 18.0));
    rig.input.apply(&at(14.0, 15.0));
    assert_eq!(rig.input.pointer(), Vec2::new(14.0, 15.0));
    assert_eq!(rig.input.pointer_delta(), Vec2::new(4.0, -5.0));
    rig.input.begin_frame();
    assert_eq!(rig.input.pointer_delta(), Vec2::ZERO);
    assert_eq!(rig.input.pointer(), Vec2::new(14.0, 15.0));
    Ok(())
}

#[test]
fn switching_the_context_switches_the_bindings() -> Result<(), InputError> {
    let mut rig = rig()?;
    rig.input.begin_frame();
    let (space, esc) = (
        key(&rig, SPACE, Action::Press),
        key(&rig, ESCAPE, Action::Press),
    );
    rig.input.apply(&space);
    rig.input.apply(&esc);
    assert!(rig.input.held(rig.fire));
    rig.input.set_context(rig.menu)?;
    assert_eq!(rig.input.context(), Some(rig.menu));
    assert!(rig.input.held(rig.fire));
    let space_up = key(&rig, SPACE, Action::Release);
    rig.input.apply(&space_up);
    assert!(
        rig.input.held(rig.fire),
        "escape is still down and bound in the menu"
    );
    rig.input.set_context(rig.gameplay)?;
    assert!(!rig.input.held(rig.fire));
    Ok(())
}

#[test]
fn a_context_of_another_map_is_refused() -> Result<(), InputError> {
    let mut rig = rig()?;
    let mut bigger = InputMap::new();
    for name in ["a", "b", "c"] {
        bigger.add_context(name)?;
    }
    let far = bigger.context("c").ok_or(InputError::UnknownContext)?;
    assert_eq!(rig.input.set_context(far), Err(InputError::UnknownContext));
    assert_eq!(rig.input.context(), Some(rig.gameplay));
    Ok(())
}

#[test]
fn losing_focus_releases_everything_that_is_down() -> Result<(), InputError> {
    let mut rig = rig()?;
    rig.input.begin_frame();
    let (space, click) = (
        key(&rig, SPACE, Action::Press),
        button(&rig, 0, Action::Press),
    );
    rig.input.apply(&space);
    rig.input.apply(&click);
    rig.input.begin_frame();
    rig.input.apply(&Event::Focus {
        window: rig.window,
        gained: false,
    });
    assert!(!rig.input.held(rig.fire));
    assert!(rig.input.released(rig.fire));
    Ok(())
}

#[test]
fn gaining_focus_changes_nothing() -> Result<(), InputError> {
    let mut rig = rig()?;
    rig.input.begin_frame();
    let space = key(&rig, SPACE, Action::Press);
    rig.input.apply(&space);
    rig.input.apply(&Event::Focus {
        window: rig.window,
        gained: true,
    });
    assert!(rig.input.held(rig.fire));
    Ok(())
}

#[test]
fn unmapped_scancodes_are_counted_and_do_nothing() -> Result<(), InputError> {
    let mut rig = rig()?;
    rig.input.begin_frame();
    let strange = key(&rig, UNMAPPED, Action::Press);
    rig.input.apply(&strange);
    assert_eq!(rig.input.unmapped_keys(), 1);
    assert!(!rig.input.held(rig.fire));
    let (repeat, release) = (
        key(&rig, UNMAPPED, Action::Repeat),
        key(&rig, UNMAPPED, Action::Release),
    );
    rig.input.apply(&repeat);
    rig.input.apply(&release);
    assert_eq!(rig.input.unmapped_keys(), 1, "only a press is counted");
    Ok(())
}

#[test]
fn a_quit_request_stays_latched() -> Result<(), InputError> {
    let mut rig = rig()?;
    assert!(!rig.input.quit_requested());
    rig.input
        .apply(&Event::CloseRequested { window: rig.window });
    rig.input.begin_frame();
    assert!(rig.input.quit_requested());
    let mut other = self::rig()?;
    other.input.apply(&Event::Quit);
    assert!(other.input.quit_requested());
    Ok(())
}

#[test]
fn an_action_of_another_map_reads_as_inactive() -> Result<(), InputError> {
    let mut rig = rig()?;
    let mut bigger = InputMap::new();
    bigger.add_action("a", ActionKind::Button)?;
    bigger.add_action("b", ActionKind::Button)?;
    bigger.add_action("c", ActionKind::Button)?;
    bigger.add_action("d", ActionKind::Axis)?;
    let far = bigger.action("d").ok_or(InputError::UnknownAction)?;
    rig.input.begin_frame();
    assert!(!rig.input.held(far) && !rig.input.pressed(far) && !rig.input.released(far));
    assert!(rig.input.axis(far).abs() < f32::EPSILON);
    Ok(())
}

#[test]
fn two_key_pairs_on_one_axis_are_limited_to_one() -> Result<(), InputError> {
    let mut map = InputMap::new();
    let walk = map.add_action("walk", ActionKind::Axis)?;
    let ctx = map.add_context("c")?;
    map.bind(ctx, walk, Binding::keys(KeyCode::A, KeyCode::D))?;
    map.bind(ctx, walk, Binding::keys(KeyCode::Left, KeyCode::Right))?;
    let mut rig = rig()?;
    rig.input = Input::new(map, KeyMap::windows());
    rig.input.begin_frame();
    for scancode in [KEY_D, 0x14D] {
        let event = key(&rig, scancode, Action::Press);
        rig.input.apply(&event);
    }
    assert!((rig.input.axis(walk) - 1.0).abs() < f32::EPSILON);
    assert_eq!(rig.input.map().action_count(), 1);
    Ok(())
}
