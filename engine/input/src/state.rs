//! The per-frame input state.

use crate::error::InputError;
use crate::key::KeyMap;
use crate::map::{ActionId, Binding, ContextId, InputMap};
use base::math::Vec2;
use platform::window::{Action, Event};

/// The bit of key or button `index`; none for an index past the width of the set.
fn mask(index: u32) -> u128 {
    1_u128.checked_shl(index).unwrap_or(0)
}

/// Which keys (or buttons) are down, and which went down or up since the frame began.
#[derive(Debug, Clone, Copy, Default)]
struct Switches {
    down: u128,
    pressed: u128,
    released: u128,
}

impl Switches {
    fn press(&mut self, index: u32) {
        let bit = mask(index);
        if self.down & bit == 0 {
            self.pressed |= bit;
        }
        self.down |= bit;
    }

    fn release(&mut self, index: u32) {
        let bit = mask(index);
        if self.down & bit != 0 {
            self.released |= bit;
        }
        self.down &= !bit;
    }

    fn release_all(&mut self) {
        self.released |= self.down;
        self.down = 0;
    }

    fn begin_frame(&mut self) {
        self.pressed = 0;
        self.released = 0;
    }

    fn follow(&mut self, index: u32, action: Action) {
        match action {
            Action::Press => self.press(index),
            Action::Release => self.release(index),
            Action::Repeat => {}
        }
    }

    fn is_down(&self, index: u32) -> bool {
        self.down & mask(index) != 0
    }

    fn went_down(&self, index: u32) -> bool {
        self.pressed & mask(index) != 0
    }

    fn went_up(&self, index: u32) -> bool {
        self.released & mask(index) != 0
    }
}

/// The state of the input: raw keys and buttons, the pointer, and the map that turns them into
/// actions. Events of every window count (an application has one; the pointer is shared, so a move
/// from one window to another gives one large delta).
///
/// A frame is `begin_frame`, then `apply` for each event of the frame, then the queries. All of
/// it allocates nothing. An action id from another map reads as inactive.
#[derive(Debug, Clone)]
pub struct Input {
    map: InputMap,
    scancodes: KeyMap,
    active: Option<ContextId>,
    keys: Switches,
    buttons: Switches,
    pointer: Vec2,
    has_pointer: bool,
    pointer_delta: Vec2,
    scroll: Vec2,
    quit: bool,
    unmapped: u32,
}

impl Input {
    /// Input state over `map`, reading scancodes with `scancodes`. The first context, if any, is
    /// active.
    #[must_use]
    pub fn new(map: InputMap, scancodes: KeyMap) -> Self {
        let active = (map.context_count() > 0).then_some(ContextId(0));
        Self {
            map,
            scancodes,
            active,
            keys: Switches::default(),
            buttons: Switches::default(),
            pointer: Vec2::ZERO,
            has_pointer: false,
            pointer_delta: Vec2::ZERO,
            scroll: Vec2::ZERO,
            quit: false,
            unmapped: 0,
        }
    }

    /// Starts a frame: clears the edges (pressed, released), the wheel and the pointer delta.
    /// Keys that are down stay down.
    pub fn begin_frame(&mut self) {
        self.keys.begin_frame();
        self.buttons.begin_frame();
        self.pointer_delta = Vec2::ZERO;
        self.scroll = Vec2::ZERO;
    }

    /// Follows one platform event. A key repeat changes nothing; losing focus releases every key
    /// and button that is down, so that none stays stuck.
    pub fn apply(&mut self, event: &Event) {
        match *event {
            Event::Key {
                scancode, action, ..
            } => self.key(scancode, action),
            Event::MouseButton { button, action, .. } => {
                self.buttons.follow(u32::from(button), action);
            }
            Event::MouseMove { x, y, .. } => self.move_pointer(Vec2::new(x, y)),
            Event::Scroll { dx, dy, .. } => self.scroll += Vec2::new(dx, dy),
            Event::Focus { gained: false, .. } => {
                self.keys.release_all();
                self.buttons.release_all();
            }
            Event::Quit | Event::CloseRequested { .. } => self.quit = true,
            Event::Focus { .. } | Event::Resized { .. } => {}
        }
    }

    fn key(&mut self, scancode: u32, action: Action) {
        match self.scancodes.key(scancode) {
            Some(key) => self.keys.follow(key.index(), action),
            None if action == Action::Press => self.unmapped = self.unmapped.saturating_add(1),
            None => {}
        }
    }

    fn move_pointer(&mut self, to: Vec2) {
        if self.has_pointer {
            self.pointer_delta += to - self.pointer;
        }
        self.pointer = to;
        self.has_pointer = true;
    }

    /// Makes `context` the active one. Keys and edges are kept as they are, so a key that is
    /// down reads as held in the new context without a press there, and an edge of this frame
    /// is read with the new bindings.
    ///
    /// # Errors
    /// [`InputError::UnknownContext`] for a context of another map.
    pub fn set_context(&mut self, context: ContextId) -> Result<(), InputError> {
        if usize::from(context.0) >= self.map.context_count() {
            return Err(InputError::UnknownContext);
        }
        self.active = Some(context);
        Ok(())
    }

    /// The map this state reads.
    #[must_use]
    pub fn map(&self) -> &InputMap {
        &self.map
    }

    /// The active context.
    #[must_use]
    pub fn context(&self) -> Option<ContextId> {
        self.active
    }

    fn bindings_of(&self, action: ActionId) -> impl Iterator<Item = Binding> + '_ {
        let all = self.active.map_or(&[][..], |c| self.map.bindings(c));
        all.iter()
            .filter(move |(id, _)| *id == action)
            .map(|(_, binding)| *binding)
    }

    /// True while a button action is down: any of its bindings in the active context is.
    #[must_use]
    pub fn held(&self, action: ActionId) -> bool {
        self.bindings_of(action).any(|b| self.down(b))
    }

    /// True in the frame where a button action went down: any of its bindings went down, also
    /// while another one was already down.
    #[must_use]
    pub fn pressed(&self, action: ActionId) -> bool {
        self.bindings_of(action)
            .any(|b| self.edge(b, Switches::went_down))
    }

    /// True in the frame where a button action went up: any of its bindings went up, also while
    /// another one is still down.
    #[must_use]
    pub fn released(&self, action: ActionId) -> bool {
        self.bindings_of(action)
            .any(|b| self.edge(b, Switches::went_up))
    }

    fn down(&self, binding: Binding) -> bool {
        self.edge(binding, Switches::is_down)
    }

    fn edge(&self, binding: Binding, test: fn(&Switches, u32) -> bool) -> bool {
        match binding {
            Binding::Key(key) => test(&self.keys, key.index()),
            Binding::MouseButton(button) => test(&self.buttons, u32::from(button)),
            Binding::KeyAxis { .. } | Binding::ScrollY => false,
        }
    }

    /// The value of an axis action. Key pairs add up and are limited to -1..1 (both keys of a pair
    /// down cancel out); the wheel adds the amount scrolled this frame, not limited. Asking a
    /// button action reads 0, and asking an axis for an edge reads false.
    #[must_use]
    pub fn axis(&self, action: ActionId) -> f32 {
        let keys: f32 = self.bindings_of(action).map(|b| self.pair_value(b)).sum();
        let wheel: f32 = self
            .bindings_of(action)
            .filter(|b| matches!(b, Binding::ScrollY))
            .map(|_| self.scroll.y)
            .sum();
        keys.clamp(-1.0, 1.0) + wheel
    }

    fn pair_value(&self, binding: Binding) -> f32 {
        match binding {
            Binding::KeyAxis { negative, positive } => {
                f32::from(u8::from(self.keys.is_down(positive.index())))
                    - f32::from(u8::from(self.keys.is_down(negative.index())))
            }
            Binding::Key(_) | Binding::MouseButton(_) | Binding::ScrollY => 0.0,
        }
    }

    /// The pointer position in screen units (the origin until it first moves).
    #[must_use]
    pub fn pointer(&self) -> Vec2 {
        self.pointer
    }

    /// How far the pointer moved this frame.
    #[must_use]
    pub fn pointer_delta(&self) -> Vec2 {
        self.pointer_delta
    }

    /// The wheel amount of this frame, horizontal and vertical.
    #[must_use]
    pub fn scroll(&self) -> Vec2 {
        self.scroll
    }

    /// True once the application was asked to quit (a latch: `begin_frame` does not clear it).
    #[must_use]
    pub fn quit_requested(&self) -> bool {
        self.quit
    }

    /// Key presses whose scancode the table did not know.
    #[must_use]
    pub fn unmapped_keys(&self) -> u32 {
        self.unmapped
    }
}
