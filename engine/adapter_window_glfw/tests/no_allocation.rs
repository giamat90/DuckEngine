//! The per-frame calls of the GLFW window system make no allocator call on the Rust side.
//! Needs the GLFW library and a display; otherwise it reports "not run: device not set up".

use adapter_window_glfw::GlfwWindows;
use base::alloc::{CountingAllocator, count_allocations};
use platform::window::{EventQueue, IWindowSystem, WindowDesc};
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn pump_info_and_token_do_not_allocate() {
    let mut windows = match GlfwWindows::load_default(1) {
        Ok(windows) => windows,
        Err(error) => {
            assert!(
                std::env::var_os("DUCK_REQUIRE_GLFW").is_none(),
                "GLFW is required here but cannot start: {error}"
            );
            eprintln!("not run: device not set up (GLFW: {error})");
            return;
        }
    };
    let desc = WindowDesc {
        title: "frame",
        width: 320,
        height: 240,
    };
    let Ok(id) = windows.create_window(&desc) else {
        panic!("a window fits");
    };
    let mut queue = EventQueue::with_capacity(64);
    // The first pumps may start things up inside GLFW; the counted frames come after.
    windows.pump(&mut queue);
    windows.pump(&mut queue);
    let ((), counts) = count_allocations(|| {
        for _ in 0..8 {
            windows.pump(&mut queue);
            let _ = windows.window_info(id);
            let _ = windows.native_token(id);
            drain(&mut queue);
        }
    });
    assert_eq!(counts.allocations + counts.reallocations, 0);
}

fn drain(queue: &mut EventQueue) {
    while queue.pop().is_some() {}
}
