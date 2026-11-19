//! The controls of the sample, through the `input` crate: window events go in, named actions are
//! read, and what the user asked for comes out as plain [`Requests`]. No window and no device is
//! touched here, so scripted events test it.
//!
//! | Input | Action |
//! |---|---|
//! | `1`, `2`, `3` | choose sphere, box or capsule |
//! | `Space`, primary button | drop the chosen piece |
//! | secondary button and drag | orbit the camera |
//! | wheel | zoom |
//! | `R` | reset |
//! | `Escape` | quit |

use base::math::Vec2;
use input::{ActionId, ActionKind, Binding, Input, InputError, InputMap, KeyCode, KeyMap};
use platform::window::Event;

/// Mouse button 0 is the primary one.
const PRIMARY: u8 = 0;
/// Mouse button 1 is the secondary one.
const SECONDARY: u8 = 1;

/// What a piece dropped into the box is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Piece {
    /// A ball.
    #[default]
    Sphere,
    /// A cuboid.
    Box,
    /// A capsule.
    Capsule,
}

/// What the user asked for in a frame.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Requests {
    /// Start the box again.
    pub reset: bool,
    /// Drop a piece.
    pub drop: Option<Piece>,
    /// How far the pointer dragged the view, in pixels.
    pub orbit: Vec2,
    /// Wheel notches (positive comes closer).
    pub zoom: f32,
}

/// The actions the sample reads.
#[derive(Clone, Copy, Debug)]
struct Actions {
    sphere: ActionId,
    cuboid: ActionId,
    capsule: ActionId,
    drop: ActionId,
    look: ActionId,
    zoom: ActionId,
    reset: ActionId,
    quit: ActionId,
}

/// The input state and the choice of piece.
#[derive(Clone, Debug)]
pub struct Controls {
    input: Input,
    actions: Actions,
    chosen: Piece,
    quit: bool,
}

/// The map of the sample: one context with every action bound.
fn build_map() -> Result<(InputMap, Actions), InputError> {
    let mut map = InputMap::new();
    let play = map.add_context("play")?;
    let mut button = |name: &str, binding: Binding| -> Result<ActionId, InputError> {
        let action = map.add_action(name, ActionKind::Button)?;
        map.bind(play, action, binding)?;
        Ok(action)
    };
    let sphere = button("sphere", Binding::Key(KeyCode::Digit1))?;
    let cuboid = button("box", Binding::Key(KeyCode::Digit2))?;
    let capsule = button("capsule", Binding::Key(KeyCode::Digit3))?;
    let reset = button("reset", Binding::Key(KeyCode::R))?;
    let quit = button("quit", Binding::Key(KeyCode::Escape))?;
    let look = button("look", Binding::MouseButton(SECONDARY))?;
    let drop = button("drop", Binding::Key(KeyCode::Space))?;
    map.bind(play, drop, Binding::MouseButton(PRIMARY))?;
    let zoom = map.add_action("zoom", ActionKind::Axis)?;
    map.bind(play, zoom, Binding::scroll_y())?;
    let actions = Actions {
        sphere,
        cuboid,
        capsule,
        drop,
        look,
        zoom,
        reset,
        quit,
    };
    Ok((map, actions))
}

impl Controls {
    /// A fresh state: the sphere is chosen.
    ///
    /// # Errors
    /// The error of the input map (it does not fail for this fixed map).
    pub fn new() -> Result<Self, InputError> {
        let (map, actions) = build_map()?;
        Ok(Self {
            input: Input::new(map, KeyMap::host()),
            actions,
            chosen: Piece::Sphere,
            quit: false,
        })
    }

    /// Starts a frame; call it before the events of the frame.
    pub fn begin_frame(&mut self) {
        self.input.begin_frame();
    }

    /// Follows a window event.
    pub fn apply(&mut self, event: &Event) {
        self.input.apply(event);
    }

    /// True once the user asked to leave.
    #[must_use]
    pub fn quit(&self) -> bool {
        self.quit || self.input.quit_requested() || self.input.pressed(self.actions.quit)
    }

    /// The piece a number key chose in this frame, if one did.
    fn choice(&self) -> Option<Piece> {
        let actions = self.actions;
        [
            (actions.sphere, Piece::Sphere),
            (actions.cuboid, Piece::Box),
            (actions.capsule, Piece::Capsule),
        ]
        .into_iter()
        .find(|(action, _)| self.input.pressed(*action))
        .map(|(_, piece)| piece)
    }

    /// What the events of this frame ask for.
    pub fn requests(&mut self) -> Requests {
        self.chosen = self.choice().unwrap_or(self.chosen);
        let actions = self.actions;
        let input = &self.input;
        self.quit |= input.pressed(actions.quit);
        Requests {
            reset: input.pressed(actions.reset),
            drop: input.pressed(actions.drop).then_some(self.chosen),
            orbit: if input.held(actions.look) {
                input.pointer_delta()
            } else {
                Vec2::ZERO
            },
            zoom: input.axis(actions.zoom),
        }
    }
}

#[cfg(test)]
mod tests;
