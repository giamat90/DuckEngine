//! Profiling hook: scope markers that reach one sink installed by the application.
//!
//! `base` reads no clock. A scope only reports "entered `name`" and "left `name`" to the
//! [`ProfileSink`] that layer 4 installs once with [`install_sink`]; the sink takes the
//! timestamp and the thread id. With no sink installed a scope costs one atomic load.
//!
//! Code marks a region with [`profile_scope!`](crate::profile_scope). The macro expands to
//! nothing unless the `profiling` feature of `base` is on, so a build without it pays nothing
//! and does not evaluate the name expression. [`scope`] is the function behind the macro and
//! is always available, for code that wants a scope regardless of the feature.

use core::fmt;
use core::marker::PhantomData;
use std::sync::OnceLock;

/// Receives scope events. Called from any thread, so it is `Sync` and takes `&self`.
///
/// A sink is observe-only: it must never feed anything back into the code it measures (simulation
/// stays deterministic). It must tolerate unbalanced events (`mem::forget` on a guard loses the
/// exit) and must not open a scope from inside `enter` or `exit`.
pub trait ProfileSink: Sync {
    /// A scope named `name` started on the calling thread.
    fn enter(&self, name: &'static str);
    /// The scope named `name` ended on the thread that started it (a guard is not `Send`).
    fn exit(&self, name: &'static str);
}

static SINK: OnceLock<&'static dyn ProfileSink> = OnceLock::new();

/// A sink was already installed; it cannot be replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkAlreadyInstalled;

impl fmt::Display for SinkAlreadyInstalled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a profiling sink is already installed")
    }
}

impl std::error::Error for SinkAlreadyInstalled {}

/// Installs the process-wide sink. Meant to be called once at startup by the application.
///
/// # Errors
///
/// Returns [`SinkAlreadyInstalled`] if a sink was installed before; the first one stays.
pub fn install_sink(sink: &'static dyn ProfileSink) -> Result<(), SinkAlreadyInstalled> {
    SINK.set(sink).map_err(|_| SinkAlreadyInstalled)
}

/// Whether a sink is installed.
#[must_use]
pub fn sink_installed() -> bool {
    SINK.get().is_some()
}

/// Ends its scope when dropped. It remembers the sink it entered, so an exit always pairs with its enter.
#[must_use = "a scope ends when the guard is dropped; bind it to a variable"]
pub struct ScopeGuard {
    name: &'static str,
    sink: Option<&'static dyn ProfileSink>,
    /// Makes the guard `!Send`, so `enter` and `exit` come from the same thread.
    not_send: PhantomData<*const ()>,
}

impl fmt::Debug for ScopeGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScopeGuard")
            .field("name", &self.name)
            .field("active", &self.sink.is_some())
            .finish()
    }
}

impl Drop for ScopeGuard {
    #[inline]
    fn drop(&mut self) {
        if let Some(sink) = self.sink {
            sink.exit(self.name);
        }
    }
}

/// Starts a scope named `name`; it ends when the returned guard is dropped.
///
/// Bind the guard (`let _guard = scope("x");`); `let _ = scope("x");` drops it at once and the scope is empty.
#[inline]
pub fn scope(name: &'static str) -> ScopeGuard {
    let sink = SINK.get().copied();
    if let Some(sink) = sink {
        sink.enter(name);
    }
    ScopeGuard {
        name,
        sink,
        not_send: PhantomData,
    }
}

/// Marks the rest of the enclosing block as a profiling scope named by a `&'static str`.
///
/// Use it as a statement with a trailing semicolon (it expands to a `let`, or to nothing when the
/// `profiling` feature of `base` is off, so it does not work as a block tail or in a match arm).
#[cfg(feature = "profiling")]
#[macro_export]
macro_rules! profile_scope {
    ($name:expr $(,)?) => {
        let _profile_scope_guard = $crate::profile::scope($name);
    };
}

/// Marks the rest of the enclosing block as a profiling scope named by a `&'static str`.
///
/// Use it as a statement with a trailing semicolon (it expands to a `let`, or to nothing when the
/// `profiling` feature of `base` is off, so it does not work as a block tail or in a match arm).
#[cfg(not(feature = "profiling"))]
#[macro_export]
macro_rules! profile_scope {
    ($name:expr $(,)?) => {};
}

#[cfg(test)]
mod tests {
    use super::{ScopeGuard, SinkAlreadyInstalled};
    use core::marker::PhantomData;

    // Compile-time check that `ScopeGuard` is not `Send`: if it were, the two blanket impls would overlap
    // and `some_item` would be ambiguous.
    trait AmbiguousIfSend<A> {
        fn some_item() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    impl<T: ?Sized + Send> AmbiguousIfSend<u8> for T {}
    const _: fn() = || {
        let _ = <ScopeGuard as AmbiguousIfSend<_>>::some_item;
    };

    #[test]
    fn the_refusal_explains_itself() {
        assert_eq!(
            SinkAlreadyInstalled.to_string(),
            "a profiling sink is already installed"
        );
        let as_error: &dyn std::error::Error = &SinkAlreadyInstalled;
        assert!(as_error.source().is_none());
    }

    #[test]
    fn a_guard_without_a_sink_shows_its_name_and_that_it_is_inactive() {
        let guard = ScopeGuard {
            name: "quiet",
            sink: None,
            not_send: PhantomData,
        };
        assert_eq!(
            format!("{guard:?}"),
            r#"ScopeGuard { name: "quiet", active: false }"#
        );
    }
}
