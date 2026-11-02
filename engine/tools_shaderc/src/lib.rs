//! `shaderc`, the offline shader compiler driver (tier T2, dev-time only): GLSL 450 sources in,
//! `dshd` shader programs out (card 3-11, ADR-0009).
//!
//! The shipped runtime never links this crate or any shader compiler. The compilers are external
//! programs (glslang, SPIRV-Cross) that this crate runs as child processes with argument vectors,
//! never through a shell, after checking their pinned versions.

pub mod cli;
pub mod compiler;
pub mod contract;
pub mod external;
pub mod json;
pub mod null;
pub mod pack;
pub mod process;
pub mod reflect;
pub mod tools;
