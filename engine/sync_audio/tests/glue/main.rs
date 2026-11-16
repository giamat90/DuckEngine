//! Tests of the glue between the ECS, the scene and the audio engine, in one test program (the
//! counting allocator of `no_allocation` is the global allocator of all of them).

mod common;
mod cue;
mod frame_loop;
mod listener;
mod no_allocation;
mod play;
