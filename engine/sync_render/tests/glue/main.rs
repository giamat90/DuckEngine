//! Tests of the glue between the ECS, the scene and the render scene, in one test program (the
//! counting allocator of `no_allocation` is the global allocator of all of them).

mod common;
mod extract;
mod frame_loop;
mod no_allocation;
