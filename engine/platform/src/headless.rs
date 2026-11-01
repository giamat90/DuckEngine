//! The Headless window system: virtual windows (bookkeeping only, no display) and scripted events.
//!
//! A test or a headless app pushes events through a [`ScriptHandle`]; [`HeadlessWindows::pump`]
//! replays them in order into the caller's queue and applies the ones that change window state
//! (resize, focus, close request). This makes input and lifecycle logic testable with no display
//! and no device, on every platform and in CI. `pump` drains the whole script: the state changes
//! are applied even when the event itself does not fit in the queue and is dropped.

use crate::error::PlatformError;
use crate::window::{
    Event, EventQueue, IWindowSystem, NativeToken, PumpResult, WindowDesc, WindowId, WindowInfo,
};
use base::pool::Pool;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

type Script = Arc<Mutex<VecDeque<Event>>>;

fn locked(script: &Script) -> MutexGuard<'_, VecDeque<Event>> {
    // A poisoned lock only means another thread panicked while holding it; the queue is still a
    // valid queue of plain data.
    script
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Feeds events to a [`HeadlessWindows`] from the test or the headless app.
#[derive(Clone, Debug)]
pub struct ScriptHandle {
    script: Script,
}

impl ScriptHandle {
    /// Appends an event to the script; the next `pump` delivers it.
    pub fn push(&self, event: Event) {
        locked(&self.script).push_back(event);
    }

    /// Number of scripted events not yet delivered.
    #[must_use]
    pub fn pending(&self) -> usize {
        locked(&self.script).len()
    }
}

/// The Null window system: virtual windows and a script of events.
pub struct HeadlessWindows {
    windows: Pool<WindowInfo>,
    script: Script,
}

impl HeadlessWindows {
    /// A system that holds at most `max_windows` windows, and the handle that scripts its events.
    #[must_use]
    pub fn new(max_windows: u32) -> (Self, ScriptHandle) {
        let script = Script::default();
        let handle = ScriptHandle {
            script: Arc::clone(&script),
        };
        let windows = Self {
            windows: Pool::with_capacity(max_windows),
            script,
        };
        (windows, handle)
    }

    fn next_event(&self) -> Option<Event> {
        locked(&self.script).pop_front()
    }

    fn apply(&mut self, event: Event) {
        match event {
            Event::Resized {
                window,
                width,
                height,
            } => self.update(window, |info| {
                info.width = width;
                info.height = height;
                info.framebuffer_width = width;
                info.framebuffer_height = height;
            }),
            Event::Focus { window, gained } => self.update(window, |info| info.focused = gained),
            Event::CloseRequested { window } => {
                self.update(window, |info| info.close_requested = true);
            }
            _ => {}
        }
    }

    fn update(&mut self, id: WindowId, change: impl FnOnce(&mut WindowInfo)) {
        if let Some(info) = self.windows.get_mut(id) {
            change(info);
        }
    }
}

impl IWindowSystem for HeadlessWindows {
    fn has_window(&self) -> bool {
        true
    }

    fn create_window(&mut self, desc: &WindowDesc<'_>) -> Result<WindowId, PlatformError> {
        if desc.width == 0 || desc.height == 0 {
            return Err(PlatformError::InvalidPath);
        }
        let info = WindowInfo {
            width: desc.width,
            height: desc.height,
            framebuffer_width: desc.width,
            framebuffer_height: desc.height,
            scale: 1.0,
            focused: false,
            close_requested: false,
        };
        self.windows
            .insert(info)
            .map_err(|_| PlatformError::QueueFull)
    }

    fn destroy_window(&mut self, id: WindowId) -> Result<(), PlatformError> {
        self.windows
            .remove(id)
            .map(|_| ())
            .ok_or(PlatformError::NotFound)
    }

    fn window_info(&self, id: WindowId) -> Result<WindowInfo, PlatformError> {
        self.windows.get(id).copied().ok_or(PlatformError::NotFound)
    }

    fn native_token(&self, id: WindowId) -> Result<NativeToken, PlatformError> {
        self.window_info(id)?;
        let packed = (u64::from(id.generation()) << 32) | u64::from(id.index());
        // Plus one, so that no live window has the null token.
        Ok(NativeToken(packed.saturating_add(1)))
    }

    fn pump(&mut self, queue: &mut EventQueue) -> PumpResult {
        let mut result = PumpResult::default();
        while let Some(event) = self.next_event() {
            self.apply(event);
            let accepted = queue.push(event).is_ok();
            result.delivered += u32::from(accepted);
            result.dropped += u32::from(!accepted);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESC: WindowDesc<'static> = WindowDesc {
        title: "t",
        width: 10,
        height: 20,
    };

    fn one_window() -> (HeadlessWindows, ScriptHandle, WindowId) {
        let (mut windows, script) = HeadlessWindows::new(2);
        let Ok(id) = windows.create_window(&DESC) else {
            panic!("a window fits");
        };
        (windows, script, id)
    }

    fn pumped(windows: &mut HeadlessWindows) -> EventQueue {
        let mut queue = EventQueue::with_capacity(8);
        windows.pump(&mut queue);
        queue
    }

    #[test]
    fn a_new_window_has_the_requested_size_and_no_focus() {
        let (windows, _script, id) = one_window();
        let info = windows.window_info(id);
        assert_eq!(info.map(|i| (i.width, i.height)), Ok((10, 20)));
        assert_eq!(
            info.map(|i| (i.framebuffer_width, i.framebuffer_height)),
            Ok((10, 20))
        );
        assert_eq!(
            info.map(|i| (i.focused, i.close_requested)),
            Ok((false, false))
        );
        assert_eq!(info.map(|i| i.scale), Ok(1.0));
    }

    #[test]
    fn resize_focus_and_close_events_update_the_window() {
        let (mut windows, script, window) = one_window();
        script.push(Event::Resized {
            window,
            width: 30,
            height: 40,
        });
        script.push(Event::Focus {
            window,
            gained: true,
        });
        script.push(Event::CloseRequested { window });
        pumped(&mut windows);
        let info = windows.window_info(window);
        assert_eq!(info.map(|i| (i.width, i.height)), Ok((30, 40)));
        assert_eq!(
            info.map(|i| (i.framebuffer_width, i.framebuffer_height)),
            Ok((30, 40))
        );
        assert_eq!(
            info.map(|i| (i.focused, i.close_requested)),
            Ok((true, true))
        );
        script.push(Event::Focus {
            window,
            gained: false,
        });
        pumped(&mut windows);
        assert_eq!(windows.window_info(window).map(|i| i.focused), Ok(false));
    }

    #[test]
    fn input_events_pass_through_without_changing_the_window() {
        let (mut windows, script, window) = one_window();
        let before = windows.window_info(window);
        script.push(Event::MouseMove {
            window,
            x: 1.0,
            y: 2.0,
        });
        let mut queue = pumped(&mut windows);
        assert_eq!(windows.window_info(window), before);
        assert!(matches!(queue.pop(), Some(Event::MouseMove { .. })));
    }

    #[test]
    fn an_event_for_a_destroyed_window_is_delivered_and_changes_nothing() {
        let (mut windows, script, window) = one_window();
        assert_eq!(windows.destroy_window(window), Ok(()));
        script.push(Event::Focus {
            window,
            gained: true,
        });
        let mut queue = pumped(&mut windows);
        assert!(matches!(queue.pop(), Some(Event::Focus { .. })));
        assert_eq!(windows.window_info(window), Err(PlatformError::NotFound));
    }

    #[test]
    fn the_script_reports_what_is_pending_and_pump_empties_it() {
        let (mut windows, script, _window) = one_window();
        script.push(Event::Quit);
        script.push(Event::Quit);
        assert_eq!(script.pending(), 2);
        let result = windows.pump(&mut EventQueue::with_capacity(8));
        assert_eq!((result.delivered, result.dropped), (2, 0));
        assert_eq!(script.pending(), 0);
    }

    #[test]
    fn a_full_pool_refuses_another_window() {
        let (mut windows, _script) = HeadlessWindows::new(1);
        assert!(windows.create_window(&DESC).is_ok());
        assert_eq!(
            windows.create_window(&DESC).err(),
            Some(PlatformError::QueueFull)
        );
    }

    #[test]
    fn the_native_token_packs_generation_and_index() {
        let (mut windows, _script, first) = one_window();
        assert_eq!(windows.native_token(first), Ok(NativeToken(1)));
        assert_eq!(windows.destroy_window(first), Ok(()));
        let Ok(again) = windows.create_window(&DESC) else {
            panic!("a window fits");
        };
        let packed = (u64::from(again.generation()) << 32) | u64::from(again.index());
        assert_ne!(packed, 0);
        assert_eq!(windows.native_token(again), Ok(NativeToken(packed + 1)));
    }

    #[test]
    fn no_live_window_has_the_null_token() {
        let (windows, _script, id) = one_window();
        assert_ne!(windows.native_token(id), Ok(NativeToken::NULL));
    }

    #[test]
    fn a_zero_width_or_height_is_refused() {
        let (mut windows, _script) = HeadlessWindows::new(2);
        for (width, height) in [(0, 5), (5, 0), (0, 0)] {
            let desc = WindowDesc {
                title: "t",
                width,
                height,
            };
            assert_eq!(
                windows.create_window(&desc).err(),
                Some(PlatformError::InvalidPath)
            );
        }
        assert_eq!(windows.windows.len(), 0);
    }

    #[test]
    fn a_reused_slot_gets_a_new_generation_and_the_old_id_stays_stale() {
        let (mut windows, _script, old) = one_window();
        assert_eq!(windows.destroy_window(old), Ok(()));
        let Ok(new) = windows.create_window(&DESC) else {
            panic!("a window fits");
        };
        assert_eq!(new.index(), old.index());
        assert_ne!(new.generation(), old.generation());
        assert_eq!(windows.window_info(old), Err(PlatformError::NotFound));
        assert_eq!(windows.destroy_window(old), Err(PlatformError::NotFound));
        assert_eq!(windows.native_token(old), Err(PlatformError::NotFound));
    }

    #[test]
    fn state_changes_apply_even_when_the_event_is_dropped() {
        let (mut windows, script, window) = one_window();
        script.push(Event::Focus {
            window,
            gained: true,
        });
        script.push(Event::CloseRequested { window });
        let mut full = EventQueue::with_capacity(0);
        let result = windows.pump(&mut full);
        assert_eq!((result.delivered, result.dropped), (0, 2));
        let info = windows.window_info(window);
        assert_eq!(
            info.map(|i| (i.focused, i.close_requested)),
            Ok((true, true))
        );
    }
}
