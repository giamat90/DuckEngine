//! The interactive example's state as plain data and pure functions (card 3-16): window events
//! go in, the sprites of the frame come out. No device and no window is touched here, so the
//! tests run on any machine.
//!
//! The keys are hardware scancodes (the layout-independent key codes come with the input module in
//! phase 7), so the table in [`keys`] is per operating system.

use crate::scene::SceneTextures;
use base::math::Vec2;
use platform::window::{Action, Event};
use render::sprite::{Sprite, UvRect};

/// Hardware scancodes of the keys the example uses: each direction has the letter key and the
/// arrow key. Windows scancodes come from GLFW (the arrows carry its extended-key bit); the other
/// table is the Linux/X11 one (evdev code plus 8) and is not run on this machine.
pub mod keys {
    /// Escape.
    #[cfg(windows)]
    pub const ESCAPE: u32 = 0x01;
    /// W and the up arrow.
    #[cfg(windows)]
    pub const UP: [u32; 2] = [0x11, 0x148];
    /// S and the down arrow.
    #[cfg(windows)]
    pub const DOWN: [u32; 2] = [0x1F, 0x150];
    /// A and the left arrow.
    #[cfg(windows)]
    pub const LEFT: [u32; 2] = [0x1E, 0x14B];
    /// D and the right arrow.
    #[cfg(windows)]
    pub const RIGHT: [u32; 2] = [0x20, 0x14D];

    /// Escape.
    #[cfg(not(windows))]
    pub const ESCAPE: u32 = 9;
    /// W and the up arrow.
    #[cfg(not(windows))]
    pub const UP: [u32; 2] = [25, 111];
    /// S and the down arrow.
    #[cfg(not(windows))]
    pub const DOWN: [u32; 2] = [39, 116];
    /// A and the left arrow.
    #[cfg(not(windows))]
    pub const LEFT: [u32; 2] = [38, 113];
    /// D and the right arrow.
    #[cfg(not(windows))]
    pub const RIGHT: [u32; 2] = [40, 114];
}

/// Side of the player sprite, in pixels.
pub const PLAYER_SIZE: f32 = 48.0;
/// Side of the marker sprite that follows the cursor, in pixels.
pub const MARKER_SIZE: f32 = 24.0;
/// Pixels the player moves in a frame while a direction key is held.
pub const SPEED: f32 = 4.0;
/// The marker tint before a click and after it (RGBA).
const MARKER_OFF: [u8; 4] = [60, 255, 60, 220];
const MARKER_ON: [u8; 4] = [255, 60, 60, 220];
const WHITE: [u8; 4] = [255, 255, 255, 255];
/// Mouse button 0 is the primary one.
const PRIMARY: u8 = 0;

/// The direction keys held now, one bit each.
#[derive(Clone, Copy, Debug, Default)]
struct Held(u8);

impl Held {
    const UP: u8 = 1;
    const DOWN: u8 = 2;
    const LEFT: u8 = 4;
    const RIGHT: u8 = 8;

    /// The bit of the direction `scancode` stands for, 0 for any other key.
    fn bit(scancode: u32) -> u8 {
        [
            (keys::UP, Self::UP),
            (keys::DOWN, Self::DOWN),
            (keys::LEFT, Self::LEFT),
            (keys::RIGHT, Self::RIGHT),
        ]
        .into_iter()
        .find(|(codes, _)| codes.contains(&scancode))
        .map_or(0, |(_, bit)| bit)
    }

    /// Sets or clears the direction `scancode` stands for; other keys change nothing.
    fn set(&mut self, scancode: u32, down: bool) {
        let bit = Self::bit(scancode);
        if down {
            self.0 |= bit;
        } else {
            self.0 &= !bit;
        }
    }

    /// The direction as -1, 0 or 1 on each axis (y up).
    fn axes(self) -> (f32, f32) {
        let axis = |plus: u8, minus: u8| {
            f32::from(i8::from(self.0 & plus != 0) - i8::from(self.0 & minus != 0))
        };
        (axis(Self::RIGHT, Self::LEFT), axis(Self::UP, Self::DOWN))
    }
}

/// A pixel count as `f32`; window sizes beyond 65535 are not expected and are capped there.
fn pixels(count: u32) -> f32 {
    f32::from(u16::try_from(count).unwrap_or(u16::MAX))
}

/// The state of the example between frames.
#[derive(Clone, Debug)]
pub struct Controls {
    view: (u32, u32),
    held: Held,
    player: Vec2,
    cursor: Vec2,
    marked: bool,
    quit: bool,
}

impl Controls {
    /// A fresh state for a view of `view` pixels: the player in the middle.
    #[must_use]
    pub fn new(view: (u32, u32)) -> Self {
        Self {
            view,
            held: Held::default(),
            player: Vec2::new(0.0, 0.0),
            cursor: Vec2::new(0.0, 0.0),
            marked: false,
            quit: false,
        }
    }

    /// Follows a window event. Events of any window count: the example has one.
    pub fn apply(&mut self, event: &Event) {
        match *event {
            Event::Key {
                scancode, action, ..
            } => self.key(scancode, action),
            Event::MouseMove { x, y, .. } => {
                self.cursor = Vec2::new(x - self.half_view().x, self.half_view().y - y);
            }
            Event::MouseButton { button, action, .. } => {
                self.marked ^= button == PRIMARY && action == Action::Press;
            }
            Event::Resized { width, height, .. } => self.view = (width, height),
            Event::CloseRequested { .. } | Event::Quit => self.quit = true,
            Event::Focus { .. } | Event::Scroll { .. } => {}
        }
    }

    fn key(&mut self, scancode: u32, action: Action) {
        if scancode == keys::ESCAPE && action == Action::Press {
            self.quit = true;
        }
        self.held.set(scancode, action != Action::Release);
    }

    /// Half the view, in pixels.
    fn half_view(&self) -> Vec2 {
        Vec2::new(pixels(self.view.0) / 2.0, pixels(self.view.1) / 2.0)
    }

    /// The player's position, in view pixels from the middle (y up).
    #[must_use]
    pub fn player(&self) -> Vec2 {
        self.player
    }

    /// The cursor, in view pixels from the middle (y up).
    #[must_use]
    pub fn cursor(&self) -> Vec2 {
        self.cursor
    }

    /// True after an odd number of primary clicks.
    #[must_use]
    pub fn marked(&self) -> bool {
        self.marked
    }

    /// True once the user asked to leave.
    #[must_use]
    pub fn quit(&self) -> bool {
        self.quit
    }

    /// Moves the player one frame on, and keeps it inside the view.
    pub fn step(&mut self) {
        let (dx, dy) = self.held.axes();
        let limit = self.half_view();
        let reach = |centre: f32, delta: f32, half: f32| {
            let edge = (half - PLAYER_SIZE / 2.0).max(0.0);
            (centre + delta * SPEED).clamp(-edge, edge)
        };
        self.player = Vec2::new(
            reach(self.player.x, dx, limit.x),
            reach(self.player.y, dy, limit.y),
        );
    }

    /// Replaces the contents of `out` with the sprites of the frame: the player (the checker)
    /// and the marker under the cursor, which changes colour with each click.
    pub fn fill_sprites(&self, textures: &SceneTextures, out: &mut Vec<Sprite>) {
        let sprite = |texture, position, size: f32, tint, layer| Sprite {
            texture,
            position,
            size: Vec2::new(size, size),
            angle: 0.0,
            uv: UvRect::FULL,
            tint,
            layer,
        };
        let marker = if self.marked { MARKER_ON } else { MARKER_OFF };
        out.clear();
        out.push(sprite(textures.checker, self.player, PLAYER_SIZE, WHITE, 1));
        out.push(sprite(textures.white, self.cursor, MARKER_SIZE, marker, 2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform::window::{Action, WindowId};
    use render::desc::{TextureDesc, TextureFormat};
    use render::device::IRenderDevice;
    use render::null::NullDevice;

    const VIEW: (u32, u32) = (800, 450);

    fn window() -> WindowId {
        WindowId::from_raw(0, 0)
    }

    fn key(scancode: u32, action: Action) -> Event {
        Event::Key {
            window: window(),
            scancode,
            action,
            mods: 0,
        }
    }

    #[test]
    fn the_player_starts_in_the_middle_and_stays_without_input() {
        let mut controls = Controls::new(VIEW);
        controls.step();
        assert_eq!(controls.player(), Vec2::new(0.0, 0.0));
    }

    #[test]
    fn a_held_key_moves_the_player_one_speed_a_frame_and_a_release_stops_it() {
        let mut controls = Controls::new(VIEW);
        controls.apply(&key(keys::RIGHT[0], Action::Press));
        controls.step();
        controls.step();
        assert_eq!(controls.player(), Vec2::new(2.0 * SPEED, 0.0));
        controls.apply(&key(keys::RIGHT[0], Action::Release));
        controls.step();
        assert_eq!(controls.player(), Vec2::new(2.0 * SPEED, 0.0));
    }

    fn mouse(x: f32, y: f32) -> Event {
        Event::MouseMove {
            window: window(),
            x,
            y,
        }
    }

    fn click(button: u8, action: Action) -> Event {
        Event::MouseButton {
            window: window(),
            button,
            action,
        }
    }

    fn textures() -> SceneTextures {
        let mut device = NullDevice::new();
        let mut make = || {
            device
                .create_texture(TextureDesc {
                    width: 1,
                    height: 1,
                    mip_count: 1,
                    format: TextureFormat::Rgba8,
                })
                .unwrap_or_else(|_| unreachable!())
        };
        SceneTextures {
            white: make(),
            checker: make(),
        }
    }

    /// A fresh state that applies each event and then steps one frame.
    fn replay(events: &[Event]) -> Controls {
        let mut controls = Controls::new(VIEW);
        for event in events {
            controls.apply(event);
            controls.step();
        }
        controls
    }

    fn held(scancodes: &[u32], frames: u32) -> Vec2 {
        let mut controls = Controls::new(VIEW);
        for code in scancodes {
            controls.apply(&key(*code, Action::Press));
        }
        for _ in 0..frames {
            controls.step();
        }
        controls.player()
    }

    #[test]
    fn opposite_keys_cancel_and_the_up_key_goes_up() {
        assert_eq!(
            held(&[keys::LEFT[0], keys::RIGHT[1]], 3),
            Vec2::new(0.0, 0.0)
        );
        assert_eq!(held(&[keys::UP[1]], 1), Vec2::new(0.0, SPEED));
        assert_eq!(held(&[keys::DOWN[0]], 1), Vec2::new(0.0, -SPEED));
        assert_eq!(held(&[keys::LEFT[1]], 1), Vec2::new(-SPEED, 0.0));
    }

    #[test]
    fn the_player_stays_inside_the_view() {
        let edge = Vec2::new(
            pixels(VIEW.0) / 2.0 - PLAYER_SIZE / 2.0,
            pixels(VIEW.1) / 2.0 - PLAYER_SIZE / 2.0,
        );
        assert_eq!(held(&[keys::RIGHT[0], keys::UP[0]], 1000), edge);
        assert_eq!(
            held(&[keys::LEFT[0], keys::DOWN[0]], 1000),
            Vec2::new(-edge.x, -edge.y)
        );
    }

    #[test]
    fn a_smaller_view_after_a_resize_pulls_the_player_back_in() {
        let mut controls = Controls::new(VIEW);
        controls.apply(&key(keys::RIGHT[0], Action::Press));
        for _ in 0..1000 {
            controls.step();
        }
        controls.apply(&Event::Resized {
            window: window(),
            width: 200,
            height: 100,
        });
        controls.step();
        assert_eq!(controls.player(), Vec2::new(100.0 - PLAYER_SIZE / 2.0, 0.0));
    }

    #[test]
    fn the_cursor_is_in_view_pixels_from_the_middle_with_y_up() {
        let mut controls = Controls::new(VIEW);
        controls.apply(&mouse(400.0, 225.0));
        assert_eq!(controls.cursor(), Vec2::new(0.0, 0.0));
        controls.apply(&mouse(500.0, 125.0));
        assert_eq!(controls.cursor(), Vec2::new(100.0, 100.0));
    }

    #[test]
    fn only_a_primary_press_toggles_the_marker() {
        let mut controls = Controls::new(VIEW);
        assert!(!controls.marked());
        controls.apply(&click(0, Action::Press));
        assert!(controls.marked());
        controls.apply(&click(0, Action::Release));
        controls.apply(&click(1, Action::Press));
        assert!(controls.marked());
        controls.apply(&click(0, Action::Press));
        assert!(!controls.marked());
    }

    #[test]
    fn escape_or_a_close_request_asks_to_quit() {
        let mut controls = Controls::new(VIEW);
        controls.apply(&key(keys::ESCAPE, Action::Release));
        assert!(!controls.quit());
        controls.apply(&key(keys::ESCAPE, Action::Press));
        assert!(controls.quit());
        let mut closed = Controls::new(VIEW);
        closed.apply(&Event::CloseRequested { window: window() });
        assert!(closed.quit());
    }

    #[test]
    fn the_sprites_are_the_player_and_the_marker() {
        let textures = textures();
        let mut controls = Controls::new(VIEW);
        controls.apply(&mouse(500.0, 125.0));
        let mut sprites = Vec::new();
        controls.fill_sprites(&textures, &mut sprites);
        assert_eq!(sprites.len(), 2);
        assert_eq!(sprites[0].texture, textures.checker);
        assert_eq!(sprites[0].position, controls.player());
        assert_eq!(sprites[1].texture, textures.white);
        assert_eq!(sprites[1].position, Vec2::new(100.0, 100.0));
        assert!(sprites[0].layer < sprites[1].layer);
        controls.apply(&click(0, Action::Press));
        let before = sprites[1].tint;
        controls.fill_sprites(&textures, &mut sprites);
        assert_eq!(sprites.len(), 2, "the list is replaced, not appended");
        assert_ne!(sprites[1].tint, before);
    }

    #[test]
    fn the_same_events_give_the_same_sprites() {
        let textures = textures();
        let events = [
            key(keys::RIGHT[0], Action::Press),
            mouse(10.0, 20.0),
            click(0, Action::Press),
            key(keys::UP[0], Action::Press),
            key(keys::RIGHT[0], Action::Release),
        ];
        let run = || {
            let controls = replay(&events);
            let mut sprites = Vec::new();
            controls.fill_sprites(&textures, &mut sprites);
            sprites
        };
        assert_eq!(run(), run());
    }
}
