//! Component types and their ids.

/// A component is any plain `'static` type that can be shared between threads (the parallel
/// queries of the next card need `Send + Sync`). There is nothing to implement and no macro: the
/// blanket impl covers every such type, and a world registers a type on first use.
pub trait Component: 'static + Send + Sync {}

impl<T: 'static + Send + Sync> Component for T {}

/// Names a component type inside one world: its registration order. Ids are small, dense and
/// deterministic (the same sequence of first uses gives the same ids).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ComponentId(pub(crate) u32);

impl ComponentId {
    /// The registration number of the component.
    #[must_use]
    pub fn index(self) -> u32 {
        self.0
    }
}
