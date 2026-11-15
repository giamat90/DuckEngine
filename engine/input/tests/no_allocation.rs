//! Frames of input make no allocator call on the calling thread (digest section 4: zero heap
//! allocations per frame on hot paths; counting allocator with per-thread counters).

use base::alloc::{CountingAllocator, count_allocations};
use input::{ActionId, ActionKind, Binding, Input, InputError, InputMap, KeyCode, KeyMap};
use platform::headless::HeadlessWindows;
use platform::window::{Action, Event, IWindowSystem, WindowDesc, WindowId, mods};
use std::alloc::System;

#[global_allocator]
static ALLOCATOR: CountingAllocator<System> = CountingAllocator::new(System);

#[test]
fn the_counter_sees_an_allocation() {
    let ((), counts) = count_allocations(|| drop(std::hint::black_box(vec![1_u8; 16])));
    assert!(counts.allocations >= 1);
}

fn window() -> Result<WindowId, InputError> {
    let (mut windows, _script) = HeadlessWindows::new(1);
    windows
        .create_window(&WindowDesc {
            title: "alloc",
            width: 8,
            height: 8,
        })
        .map_err(|_| InputError::TooMany)
}

fn events(window: WindowId) -> [Event; 6] {
    let key = |action, mods| Event::Key {
        window,
        scancode: 0x39,
        action,
        mods,
    };
    [
        key(Action::Press, mods::CONTROL),
        Event::MouseMove {
            window,
            x: 3.0,
            y: 4.0,
        },
        Event::MouseButton {
            window,
            button: 0,
            action: Action::Press,
        },
        Event::Scroll {
            window,
            dx: 0.0,
            dy: 1.0,
        },
        key(Action::Release, 0),
        Event::Focus {
            window,
            gained: false,
        },
    ]
}

fn frame(input: &mut Input, events: &[Event], fire: ActionId, walk: ActionId) -> usize {
    input.begin_frame();
    for event in events {
        input.apply(event);
    }
    usize::from(input.pressed(fire))
        + usize::from(input.held(fire))
        + usize::from(input.released(fire))
        + input.axis(walk).to_bits() as usize
}

#[test]
fn a_frame_of_events_and_queries_does_not_allocate() -> Result<(), InputError> {
    let mut map = InputMap::new();
    let fire = map.add_action("fire", ActionKind::Button)?;
    let walk = map.add_action("walk", ActionKind::Axis)?;
    let ctx = map.add_context("play")?;
    map.bind(ctx, fire, Binding::Key(KeyCode::Space))?;
    map.bind(ctx, fire, Binding::MouseButton(0))?;
    map.bind(ctx, walk, Binding::keys(KeyCode::A, KeyCode::D))?;
    let mut input = Input::new(map, KeyMap::windows());
    let events = events(window()?);
    let warm = frame(&mut input, &events, fire, walk);
    let (again, counts) = count_allocations(|| {
        (0..100)
            .map(|_| frame(&mut input, &events, fire, walk))
            .sum::<usize>()
    });
    assert_eq!(again, warm * 100);
    assert_eq!(counts.allocations, 0);
    Ok(())
}
