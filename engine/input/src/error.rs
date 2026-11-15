//! Errors of the input map: values, never panics (digest section 4).

use core::fmt;

/// Why a change of the input map was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputError {
    /// An action or a context with that name exists already.
    DuplicateName,
    /// The action id does not belong to this map.
    UnknownAction,
    /// The context id does not belong to this map.
    UnknownContext,
    /// The binding does not fit the kind of the action: keys and mouse buttons bind button
    /// actions, key pairs and the scroll wheel bind axis actions.
    WrongKind,
    /// A mouse button number of 8 or more (the window port reports up to eight).
    BadButton,
    /// More actions or contexts than a 16-bit id can name.
    TooMany,
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::DuplicateName => "an action or context with that name exists already",
            Self::UnknownAction => "the action does not belong to this map",
            Self::UnknownContext => "the context does not belong to this map",
            Self::WrongKind => "the binding does not fit the kind of the action",
            Self::BadButton => "mouse buttons are numbered 0 to 7",
            Self::TooMany => "too many actions or contexts",
        };
        f.write_str(text)
    }
}

impl std::error::Error for InputError {}
