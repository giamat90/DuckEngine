//! The input map: named actions, contexts and the bindings of a context.

use crate::error::InputError;
use crate::key::KeyCode;

/// Whether an action is on or off, or has a value between -1 and 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// Pressed, held or released.
    Button,
    /// A number: key pairs give -1 to 1 together; the wheel adds its amount (not limited).
    Axis,
}

/// Names an action of one [`InputMap`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionId(pub(crate) u16);

/// Names a context of one [`InputMap`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextId(pub(crate) u16);

/// What drives an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    /// A key, for a button action.
    Key(KeyCode),
    /// A mouse button (0 is the primary one), for a button action.
    MouseButton(u8),
    /// Two keys, for an axis action: the positive one gives 1, the negative one -1. Several
    /// key pairs on one action add up, then are limited to -1..1.
    KeyAxis {
        /// The key that gives -1.
        negative: KeyCode,
        /// The key that gives 1.
        positive: KeyCode,
    },
    /// The vertical wheel, for an axis action.
    ScrollY,
}

impl Binding {
    /// A key pair for an axis action.
    #[must_use]
    pub fn keys(negative: KeyCode, positive: KeyCode) -> Self {
        Self::KeyAxis { negative, positive }
    }

    /// The vertical wheel for an axis action.
    #[must_use]
    pub fn scroll_y() -> Self {
        Self::ScrollY
    }
}

/// One action: its name and kind.
#[derive(Debug, Clone)]
struct ActionDef {
    name: String,
    kind: ActionKind,
}

/// One context: its name and the bindings that are live while it is active.
#[derive(Debug, Clone)]
struct Context {
    name: String,
    bindings: Vec<(ActionId, Binding)>,
}

/// How many mouse buttons a binding may name.
pub(crate) const MOUSE_BUTTONS: u8 = 8;

/// Whether `binding` can drive an action of `kind`.
fn fits(kind: ActionKind, binding: Binding) -> bool {
    matches!(
        (kind, binding),
        (
            ActionKind::Button,
            Binding::Key(_) | Binding::MouseButton(_)
        ) | (ActionKind::Axis, Binding::KeyAxis { .. } | Binding::ScrollY)
    )
}

/// The actions, the contexts and the bindings of each context. Built once at start-up (it
/// allocates); reading it during a frame does not.
#[derive(Debug, Clone, Default)]
pub struct InputMap {
    actions: Vec<ActionDef>,
    contexts: Vec<Context>,
}

impl InputMap {
    /// An empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an action. Action names are unique among the actions (a context may share one).
    ///
    /// # Errors
    /// [`InputError::DuplicateName`] and [`InputError::TooMany`].
    pub fn add_action(&mut self, name: &str, kind: ActionKind) -> Result<ActionId, InputError> {
        if self.action(name).is_some() {
            return Err(InputError::DuplicateName);
        }
        let id = u16::try_from(self.actions.len()).map_err(|_| InputError::TooMany)?;
        self.actions.push(ActionDef {
            name: name.to_owned(),
            kind,
        });
        Ok(ActionId(id))
    }

    /// Adds a context. Context names are unique among the contexts.
    ///
    /// # Errors
    /// [`InputError::DuplicateName`] and [`InputError::TooMany`].
    pub fn add_context(&mut self, name: &str) -> Result<ContextId, InputError> {
        if self.context(name).is_some() {
            return Err(InputError::DuplicateName);
        }
        let id = u16::try_from(self.contexts.len()).map_err(|_| InputError::TooMany)?;
        self.contexts.push(Context {
            name: name.to_owned(),
            bindings: Vec::new(),
        });
        Ok(ContextId(id))
    }

    /// Binds `binding` to `action` in `context`. An action may have several bindings in a
    /// context; it is active while any of them is.
    ///
    /// # Errors
    /// [`InputError::UnknownAction`], [`InputError::UnknownContext`] and [`InputError::WrongKind`].
    pub fn bind(
        &mut self,
        context: ContextId,
        action: ActionId,
        binding: Binding,
    ) -> Result<(), InputError> {
        let kind = self.action_kind(action).ok_or(InputError::UnknownAction)?;
        let target = self
            .contexts
            .get_mut(usize::from(context.0))
            .ok_or(InputError::UnknownContext)?;
        if matches!(binding, Binding::MouseButton(b) if b >= MOUSE_BUTTONS) {
            return Err(InputError::BadButton);
        }
        if !fits(kind, binding) {
            return Err(InputError::WrongKind);
        }
        target.bindings.push((action, binding));
        Ok(())
    }

    /// The action called `name`.
    #[must_use]
    pub fn action(&self, name: &str) -> Option<ActionId> {
        let at = self.actions.iter().position(|a| a.name == name)?;
        u16::try_from(at).ok().map(ActionId)
    }

    /// The context called `name`.
    #[must_use]
    pub fn context(&self, name: &str) -> Option<ContextId> {
        let at = self.contexts.iter().position(|c| c.name == name)?;
        u16::try_from(at).ok().map(ContextId)
    }

    /// The name of an action.
    #[must_use]
    pub fn action_name(&self, action: ActionId) -> Option<&str> {
        self.actions
            .get(usize::from(action.0))
            .map(|a| a.name.as_str())
    }

    /// The kind of an action.
    #[must_use]
    pub fn action_kind(&self, action: ActionId) -> Option<ActionKind> {
        self.actions.get(usize::from(action.0)).map(|a| a.kind)
    }

    /// How many actions there are.
    #[must_use]
    pub fn action_count(&self) -> usize {
        self.actions.len()
    }

    /// How many contexts there are.
    #[must_use]
    pub fn context_count(&self) -> usize {
        self.contexts.len()
    }

    /// The bindings of a context (none for an id of another map).
    pub(crate) fn bindings(&self, context: ContextId) -> &[(ActionId, Binding)] {
        self.contexts
            .get(usize::from(context.0))
            .map_or(&[], |c| c.bindings.as_slice())
    }
}
