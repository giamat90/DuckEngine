//! The per-frame calls of the window port make no allocator call.

use base::alloc::{CountingAllocator, count_allocations};
use platform::headless::ScriptHandle;
use platform::platform::Platform;
use platform::window::{Event, EventQueue, IWindowSystem, WindowDesc, WindowId};
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn pump_info_token_and_caps_do_not_allocate() {
    let (mut platform, _clock, script) = Platform::headless_windows(2);
    let desc = WindowDesc {
        title: "frame",
        width: 8,
        height: 8,
    };
    let Some(windows) = platform.windows.as_mut() else {
        panic!("headless windows");
    };
    let Ok(id) = windows.create_window(&desc) else {
        panic!("a window fits");
    };
    let mut queue = EventQueue::with_capacity(4);
    // The script's own storage grows at startup, outside the counted frames.
    for _ in 0..4 {
        script.push(Event::Quit);
    }
    assert_eq!(windows.pump(&mut queue).delivered, 4);
    let (delivered, counts) = count_allocations(|| {
        (0..4)
            .map(|_| frame(windows.as_mut(), &mut queue, &script, id))
            .sum::<u32>()
    });
    assert_eq!(delivered, 16);
    assert_eq!(counts.calls(), 0);
}

fn frame(
    windows: &mut dyn IWindowSystem,
    queue: &mut EventQueue,
    script: &ScriptHandle,
    id: WindowId,
) -> u32 {
    while queue.pop().is_some() {}
    for _ in 0..4 {
        script.push(Event::Quit);
    }
    let delivered = windows.pump(queue).delivered;
    let _ = windows.window_info(id);
    let _ = windows.native_token(id);
    delivered
}
