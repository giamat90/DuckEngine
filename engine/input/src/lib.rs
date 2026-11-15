//! Layer 2 input crate (design: `wiki/architecture/ecs-and-sandbox-v0.md` section 4):
//! layout-independent key codes with the scancode tables of Windows and Linux, named actions
//! (button or axis), contexts of bindings that can be switched, and the per-frame input state fed
//! by the events of the window port. It reads the platform event type and nothing else, so scripted
//! events on the Headless platform test it with no display. No ECS dependency, no `unsafe` code.

pub mod error;
pub mod key;
pub mod map;
pub mod state;

pub use error::InputError;
pub use key::{KeyCode, KeyMap};
pub use map::{ActionId, ActionKind, Binding, ContextId, InputMap};
pub use state::Input;
