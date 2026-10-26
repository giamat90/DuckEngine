//! Layer 0 base crate: building blocks every other engine crate may use.
//! Standard library only, no OS access.

pub mod alloc;
pub mod arena;
pub mod assert;
pub mod log;
pub mod pool;
pub mod rng;
pub mod simd;
