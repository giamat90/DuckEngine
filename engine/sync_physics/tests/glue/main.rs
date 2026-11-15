//! Tests of the glue between the ECS, the scene and the physics world, in one test program (the
//! counting allocator of `no_allocation` is the global allocator of all of them).

mod attach;
mod common;
mod frame_loop;
mod no_allocation;
mod sync;
