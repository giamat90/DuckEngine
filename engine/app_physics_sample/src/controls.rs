//! The sample's input as plain data and pure functions: window events go in, requests come out
//! (spawn a body at the cursor, reset the arena, toggle the debug drawing, quit). No device and no
//! window is touched here.
//!
//! The keys are hardware scancodes (layout-independent key codes come with the input module in
//! phase 7), so the table in [`keys`] is per operating system, as in `app_sprite_sample`. The cursor
//! is in window screen units, which equal framebuffer pixels where the display scale is 1.

use crate::mapping::to_world;
use base::math::Vec2;
use platform::window::{Action, Event};

/// Hardware scancodes of the keys the sample uses. Windows scancodes come from GLFW; the other
/// table is the Linux/X11 one (evdev code plus 8) and is not run on this machine.
pub mod keys {
    /// Escape.
    #[cfg(windows)]
    pub const ESCAPE: u32 = 0x01;
    /// R: reset the arena.
    #[cfg(windows)]
    pub const RESET: u32 = 0x13;
    /// D: toggle the debug drawing.
    #[cfg(windows)]
    pub const DEBUG: u32 = 0x20;
    /// P: switch between the stream and the pyramid scene.
    #[cfg(windows)]
    pub const SCENE: u32 = 0x19;
    /// S: switch between the default and the finer sub-steps of the solver.
    #[cfg(windows)]
    pub const SUB_STEPS: u32 = 0x1F;

    /// Escape.
    #[cfg(not(windows))]
    pub const ESCAPE: u32 = 9;
    /// R: reset the arena.
    #[cfg(not(windows))]
    pub const RESET: u32 = 27;
    /// D: toggle the debug drawing.
    #[cfg(not(windows))]
    pub const DEBUG: u32 = 40;
    #[cfg(not(windows))]
    pub const SCENE: u32 = 33;
    #[cfg(not(windows))]
    pub const SUB_STEPS: u32 = 39;
}

/// Mouse button 0 is the primary one.
const PRIMARY: u8 = 0;

/// The requests a key press makes, as bits of [`Controls`]'s `asked`.
const ASK_RESET: u8 = 1;
const ASK_SCENE: u8 = 2;
const ASK_SUB_STEPS: u8 = 4;

/// A pixel count as `f32`; window sizes beyond 65535 are not expected and are capped there.
fn pixels(count: u32) -> f32 {
    f32::from(u16::try_from(count).unwrap_or(u16::MAX))
}

/// The state of the input between frames.
#[derive(Clone, Debug)]
pub struct Controls {
    view: (u32, u32),
    cursor: Vec2,
    debug: bool,
    spawn: Option<Vec2>,
    /// Requests made by a key press and not yet taken: a set of [`ASK_RESET`] and friends.
    asked: u8,
    zoom_out: u32,
    quit: bool,
}

impl Controls {
    /// A fresh state for a view of `view` pixels; the debug drawing starts on.
    #[must_use]
    pub fn new(view: (u32, u32)) -> Self {
        Self {
            view,
            cursor: Vec2::ZERO,
            debug: true,
            spawn: None,
            asked: 0,
            zoom_out: 1,
            quit: false,
        }
    }

    /// Follows a window event. Events of any window count: the sample has one.
    pub fn apply(&mut self, event: &Event) {
        match *event {
            Event::Key {
                scancode, action, ..
            } => self.key(scancode, action),
            Event::MouseMove { x, y, .. } => self.mouse_move(x, y),
            Event::MouseButton { button, action, .. } => self.button(button, action),
            Event::Resized { width, height, .. } => self.view = (width, height),
            Event::CloseRequested { .. } | Event::Quit => self.quit = true,
            Event::Focus { .. } | Event::Scroll { .. } => {}
        }
    }

    fn mouse_move(&mut self, x: f32, y: f32) {
        self.cursor = Vec2::new(x - pixels(self.view.0) / 2.0, pixels(self.view.1) / 2.0 - y);
    }

    fn button(&mut self, button: u8, action: Action) {
        if button == PRIMARY && action == Action::Press {
            self.spawn = Some(self.cursor_in_world());
        }
    }

    fn key(&mut self, scancode: u32, action: Action) {
        if action != Action::Press {
            return;
        }
        match scancode {
            keys::ESCAPE => self.quit = true,
            keys::RESET => self.asked |= ASK_RESET,
            keys::DEBUG => self.debug = !self.debug,
            keys::SCENE => self.asked |= ASK_SCENE,
            keys::SUB_STEPS => self.asked |= ASK_SUB_STEPS,
            _ => {}
        }
    }

    /// Follows a change of the view size, in pixels.
    pub fn resize(&mut self, view: (u32, u32)) {
        self.view = view;
    }

    /// True while the debug drawing is on.
    #[must_use]
    pub fn debug(&self) -> bool {
        self.debug
    }

    /// True once the user asked to leave.
    #[must_use]
    pub fn quit(&self) -> bool {
        self.quit
    }

    /// The place, in metres, a click asked a body at; asked for once per click.
    pub fn take_spawn(&mut self) -> Option<Vec2> {
        self.spawn.take()
    }

    /// True once per press of the reset key.
    pub fn take_reset(&mut self) -> bool {
        self.take(ASK_RESET)
    }

    /// True once per press of the scene key.
    pub fn take_scene(&mut self) -> bool {
        self.take(ASK_SCENE)
    }

    /// True once per press of the sub-steps key.
    pub fn take_sub_steps(&mut self) -> bool {
        self.take(ASK_SUB_STEPS)
    }

    /// True once after the request `ask` was made.
    fn take(&mut self, ask: u8) -> bool {
        let made = self.asked & ask != 0;
        self.asked &= !ask;
        made
    }

    /// Sets how many times more of the world a pixel covers (1 is 20 pixels to the metre); a
    /// click is placed with it.
    pub fn set_zoom_out(&mut self, zoom_out: u32) {
        self.zoom_out = zoom_out.max(1);
    }

    /// Switches the debug drawing on or off.
    pub fn set_debug(&mut self, debug: bool) {
        self.debug = debug;
    }

    fn cursor_in_world(&self) -> Vec2 {
        let zoom = f32::from(u16::try_from(self.zoom_out).unwrap_or(u16::MAX));
        to_world(self.cursor * zoom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform::window::WindowId;

    const VIEW: (u32, u32) = (800, 450);

    fn key(scancode: u32, action: Action) -> Event {
        Event::Key {
            window: WindowId::from_raw(0, 0),
            scancode,
            action,
            mods: 0,
        }
    }

    fn mouse(x: f32, y: f32) -> Event {
        Event::MouseMove {
            window: WindowId::from_raw(0, 0),
            x,
            y,
        }
    }

    fn click(button: u8, action: Action) -> Event {
        Event::MouseButton {
            window: WindowId::from_raw(0, 0),
            button,
            action,
        }
    }

    #[test]
    fn a_fresh_state_asks_for_nothing_and_draws_the_debug_view() {
        let mut controls = Controls::new(VIEW);
        assert!(controls.debug());
        assert!(!controls.quit());
        assert_eq!(controls.take_spawn(), None);
        assert!(!controls.take_reset());
    }

    #[test]
    fn a_click_asks_for_a_body_where_the_cursor_is_in_metres() {
        let mut controls = Controls::new(VIEW);
        // The middle of the window is the origin; one metre right of it is 20 pixels, and
        // y grows downwards on the screen but upwards in the world.
        controls.apply(&mouse(400.0 + 20.0, 225.0 - 40.0));
        controls.apply(&click(PRIMARY, Action::Press));
        assert_eq!(controls.take_spawn(), Some(Vec2::new(1.0, 2.0)));
        // Asked for once: a second look finds nothing, and the release asks for nothing.
        assert_eq!(controls.take_spawn(), None);
        controls.apply(&click(PRIMARY, Action::Release));
        assert_eq!(controls.take_spawn(), None);
    }

    #[test]
    fn other_buttons_do_not_spawn() {
        let mut controls = Controls::new(VIEW);
        controls.apply(&click(1, Action::Press));
        assert_eq!(controls.take_spawn(), None);
    }

    #[test]
    fn the_cursor_follows_a_resized_view() {
        let mut controls = Controls::new(VIEW);
        controls.resize((400, 200));
        controls.apply(&mouse(200.0 + 20.0, 100.0));
        controls.apply(&click(PRIMARY, Action::Press));
        assert_eq!(controls.take_spawn(), Some(Vec2::new(1.0, 0.0)));
        controls.apply(&Event::Resized {
            window: WindowId::from_raw(0, 0),
            width: 800,
            height: 450,
        });
        controls.apply(&mouse(400.0, 225.0));
        controls.apply(&click(PRIMARY, Action::Press));
        assert_eq!(controls.take_spawn(), Some(Vec2::ZERO));
    }

    #[test]
    fn the_reset_key_asks_once_per_press() {
        let mut controls = Controls::new(VIEW);
        controls.apply(&key(keys::RESET, Action::Press));
        assert!(controls.take_reset());
        assert!(!controls.take_reset());
        controls.apply(&key(keys::RESET, Action::Repeat));
        controls.apply(&key(keys::RESET, Action::Release));
        assert!(!controls.take_reset());
    }

    #[test]
    fn the_debug_key_toggles_on_press_only() {
        let mut controls = Controls::new(VIEW);
        controls.apply(&key(keys::DEBUG, Action::Press));
        assert!(!controls.debug());
        controls.apply(&key(keys::DEBUG, Action::Repeat));
        controls.apply(&key(keys::DEBUG, Action::Release));
        assert!(!controls.debug());
        controls.apply(&key(keys::DEBUG, Action::Press));
        assert!(controls.debug());
    }

    #[test]
    fn escape_and_closing_the_window_quit() {
        let mut controls = Controls::new(VIEW);
        controls.apply(&key(keys::ESCAPE, Action::Release));
        assert!(!controls.quit());
        controls.apply(&key(keys::ESCAPE, Action::Press));
        assert!(controls.quit());
        let mut closing = Controls::new(VIEW);
        closing.apply(&Event::CloseRequested {
            window: WindowId::from_raw(0, 0),
        });
        assert!(closing.quit());
        let mut quitting = Controls::new(VIEW);
        quitting.apply(&Event::Quit);
        assert!(quitting.quit());
    }

    #[test]
    fn other_keys_and_focus_change_nothing() {
        let mut controls = Controls::new(VIEW);
        controls.apply(&key(0xFFFF, Action::Press));
        controls.apply(&Event::Focus {
            window: WindowId::from_raw(0, 0),
            gained: true,
        });
        assert!(controls.debug());
        assert!(!controls.quit());
        assert!(!controls.take_reset());
    }

    #[test]
    fn the_scene_and_sub_steps_keys_ask_once_per_press() {
        let mut controls = Controls::new(VIEW);
        assert!(!controls.take_scene() && !controls.take_sub_steps());
        controls.apply(&key(keys::SCENE, Action::Press));
        assert!(controls.take_scene());
        assert!(!controls.take_scene());
        assert!(!controls.take_sub_steps());
        controls.apply(&key(keys::SUB_STEPS, Action::Press));
        controls.apply(&key(keys::SUB_STEPS, Action::Repeat));
        assert!(controls.take_sub_steps());
        assert!(!controls.take_sub_steps());
        controls.apply(&key(keys::SCENE, Action::Release));
        assert!(!controls.take_scene());
    }

    #[test]
    fn a_zoomed_out_view_places_a_click_further_out_in_the_world() {
        let mut controls = Controls::new(VIEW);
        controls.set_zoom_out(2);
        // 20 pixels right of the middle is 2 m at 10 pixels to the metre.
        controls.apply(&mouse(400.0 + 20.0, 225.0));
        controls.apply(&click(PRIMARY, Action::Press));
        assert_eq!(controls.take_spawn(), Some(Vec2::new(2.0, 0.0)));
        controls.set_zoom_out(0);
        controls.apply(&click(PRIMARY, Action::Press));
        assert_eq!(controls.take_spawn(), Some(Vec2::new(1.0, 0.0)));
    }

    #[test]
    fn the_debug_drawing_can_be_set_directly() {
        let mut controls = Controls::new(VIEW);
        controls.set_debug(false);
        assert!(!controls.debug());
        controls.set_debug(true);
        assert!(controls.debug());
    }
}
