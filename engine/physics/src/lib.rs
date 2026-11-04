//! Layer 2 physics crate (design: `wiki/architecture/physics-and-jobs-v0.md`, decision: ADR-0018).
//!
//! One crate with the modules [`core`] (the dimension-generic world, bodies, handles and state hash,
//! generic over the narrow [`core::Dim`] trait) and [`d2`] (the 2D instance and the API applications
//! use). A 3D instance (`d3`) is phase 8 and is not built; a test instance in `tests/` proves that
//! the core compiles and steps for a second dimension.
//!
//! Rules kept here: errors are values, no panic, no `unsafe`; capacities are fixed when the world is
//! built and no call allocates afterwards; iteration order is slot-index order; floating-point work
//! is plain `+ - * /` and `sqrt` in a fixed order, and angle conversions go through `base::dmath`.

pub mod core;
pub mod d2;
