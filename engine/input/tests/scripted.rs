//! A scripted session replayed through the Headless platform: events pushed to the script come
//! out of `pump` in order and drive the input state frame by frame, with no display.

use input::{ActionId, ActionKind, Binding, Input, InputError, InputMap, KeyCode, KeyMap};
use platform::headless::{HeadlessWindows, ScriptHandle};
use platform::window::{Action, Event, EventQueue, IWindowSystem, WindowDesc, WindowId, mods};

const KEY_D: u32 = 0x20;
const SPACE: u32 = 0x39;

struct Session {
    input: Input,
    windows: HeadlessWindows,
    script: ScriptHandle,
    window: WindowId,
    queue: EventQueue,
    fire: ActionId,
    walk: ActionId,
}

fn session() -> Result<Session, InputError> {
    let mut map = InputMap::new();
    let fire = map.add_action("fire", ActionKind::Button)?;
    let walk = map.add_action("walk", ActionKind::Axis)?;
    let ctx = map.add_context("play")?;
    map.bind(ctx, fire, Binding::Key(KeyCode::Space))?;
    map.bind(ctx, walk, Binding::keys(KeyCode::A, KeyCode::D))?;
    let (mut windows, script) = HeadlessWindows::new(1);
    let window = windows
        .create_window(&WindowDesc {
            title: "scripted",
            width: 64,
            height: 64,
        })
        .map_err(|_| InputError::TooMany)?;
    Ok(Session {
        input: Input::new(map, KeyMap::windows()),
        windows,
        script,
        window,
        queue: EventQueue::with_capacity(16),
        fire,
        walk,
    })
}

impl Session {
    fn key(&self, scancode: u32, action: Action) -> Event {
        Event::Key {
            window: self.window,
            scancode,
            action,
            mods: mods::SHIFT,
        }
    }

    /// Pushes `events`, pumps them through the platform and applies them as one frame.
    fn frame(&mut self, events: &[Event]) {
        for event in events {
            self.script.push(*event);
        }
        let pumped = self.windows.pump(&mut self.queue);
        assert_eq!(pumped.dropped, 0);
        self.input.begin_frame();
        while let Some(event) = self.queue.pop() {
            self.input.apply(&event);
        }
    }

    fn state(&self) -> (f32, bool, bool, bool) {
        (
            self.input.axis(self.walk),
            self.input.pressed(self.fire),
            self.input.held(self.fire),
            self.input.released(self.fire),
        )
    }
}

#[test]
fn a_scripted_session_gives_the_expected_state_in_every_frame() -> Result<(), InputError> {
    let mut s = session()?;
    // Frame 0: D goes down. Frame 1: space goes down. Frame 2: nothing. Frame 3: both go up.
    let frames = [
        vec![s.key(KEY_D, Action::Press)],
        vec![s.key(SPACE, Action::Press)],
        vec![],
        vec![s.key(KEY_D, Action::Release), s.key(SPACE, Action::Release)],
    ];
    // (walk, fire pressed, fire held, fire released) after each frame.
    let expected = [
        (1.0, false, false, false),
        (1.0, true, true, false),
        (1.0, false, true, false),
        (0.0, false, false, true),
    ];
    for (events, want) in frames.iter().zip(expected) {
        s.frame(events);
        assert_eq!(s.state(), want);
    }
    Ok(())
}

fn replay() -> Result<Vec<(bool, bool)>, InputError> {
    let mut s = session()?;
    let mut seen = Vec::new();
    for step in 0..6_u8 {
        let action = if step % 2 == 0 {
            Action::Press
        } else {
            Action::Release
        };
        let event = s.key(SPACE, action);
        s.frame(&[event]);
        seen.push((s.input.pressed(s.fire), s.input.released(s.fire)));
    }
    Ok(seen)
}

#[test]
fn the_same_script_twice_gives_the_same_states() -> Result<(), InputError> {
    assert_eq!(replay()?, replay()?);
    Ok(())
}
