//! The window port: windows, lifecycle and input as one port with pulled events.
//!
//! [`IWindowSystem`] creates windows (handles, so a stale id is detected) and polls the operating
//! system into a caller-owned [`EventQueue`] of fixed capacity. Lifecycle (quit, focus) and input
//! (keys, mouse) are event kinds of the one stream. Events are plain `Copy` data; nothing is
//! pushed into engine code and nothing allocates per frame.
//!
//! The port is used from the thread that created it (GLFW, for one, allows window calls from the
//! main thread only), so it is not `Send`, and a [`Platform`](crate::platform::Platform) that
//! holds one is not `Send` either. A caller must check [`PumpResult::dropped`]: an event that does
//! not fit in the queue is lost. Events of a window that was destroyed meanwhile can still be in
//! the queue, so consumers tolerate stale [`WindowId`]s. A close request is latched in
//! [`WindowInfo::close_requested`] and cannot be cancelled in v0.

use crate::error::PlatformError;
use base::pool::Handle;
use base::ring::Ring;

/// Identifies a window; a stale id (the window was destroyed) is rejected by the port.
pub type WindowId = Handle<WindowInfo>;

/// What to create.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowDesc<'a> {
    /// Title of the window.
    pub title: &'a str,
    /// Width in screen units.
    pub width: u32,
    /// Height in screen units.
    pub height: u32,
}

/// The state of a window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowInfo {
    /// Width in screen units.
    pub width: u32,
    /// Height in screen units.
    pub height: u32,
    /// Width of the framebuffer in pixels.
    pub framebuffer_width: u32,
    /// Height of the framebuffer in pixels.
    pub framebuffer_height: u32,
    /// Pixels per screen unit.
    pub scale: f32,
    /// The window has input focus.
    pub focused: bool,
    /// The user asked to close the window.
    pub close_requested: bool,
}

/// An opaque value only the matching render adapter can interpret; no OS type is visible here.
/// Zero is the null token ([`NativeToken::NULL`]) and is never given to a live window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NativeToken(pub u64);

impl NativeToken {
    /// The token that names no window.
    pub const NULL: Self = Self(0);
}

/// Modifier bits of [`Event::Key`], combined with `|`.
pub mod mods {
    /// A shift key is held.
    pub const SHIFT: u8 = 1;
    /// A control key is held.
    pub const CONTROL: u8 = 2;
    /// An alt key is held.
    pub const ALT: u8 = 4;
    /// A super (logo) key is held.
    pub const SUPER: u8 = 8;
}

/// Press, release or key repeat.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Action {
    /// The key or button went down.
    #[default]
    Press,
    /// The key or button went up.
    Release,
    /// The key is held and the system repeats it.
    Repeat,
}

/// One thing that happened, as plain data.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Event {
    /// The application was asked to quit.
    #[default]
    Quit,
    /// The user asked to close a window.
    CloseRequested {
        /// The window.
        window: WindowId,
    },
    /// A window gained or lost input focus.
    Focus {
        /// The window.
        window: WindowId,
        /// True when focus was gained.
        gained: bool,
    },
    /// A window changed size. The framebuffer size and the DPI scale are read with
    /// [`IWindowSystem::window_info`]; they may get their own events with the GLFW adapter.
    Resized {
        /// The window.
        window: WindowId,
        /// New width in screen units.
        width: u32,
        /// New height in screen units.
        height: u32,
    },
    /// A key changed state. The scancode is the hardware key; layout-independent key codes come
    /// with the input module (phase 7).
    Key {
        /// The window.
        window: WindowId,
        /// Hardware scancode.
        scancode: u32,
        /// Press, release or repeat.
        action: Action,
        /// Modifier bits, see [`mods`].
        mods: u8,
    },
    /// A mouse button changed state.
    MouseButton {
        /// The window.
        window: WindowId,
        /// Button number, 0 is the primary button.
        button: u8,
        /// Press or release.
        action: Action,
    },
    /// The mouse moved.
    MouseMove {
        /// The window.
        window: WindowId,
        /// Position in screen units from the left edge.
        x: f32,
        /// Position in screen units from the top edge.
        y: f32,
    },
    /// The wheel or trackpad scrolled.
    Scroll {
        /// The window.
        window: WindowId,
        /// Horizontal amount.
        dx: f32,
        /// Vertical amount.
        dy: f32,
    },
}

/// A caller-owned FIFO of events with a fixed capacity, allocated once.
#[derive(Clone, Debug)]
pub struct EventQueue {
    events: Ring<Event>,
}

impl EventQueue {
    /// A queue that holds at most `capacity` events.
    #[must_use]
    pub fn with_capacity(capacity: u32) -> Self {
        Self {
            events: Ring::with_capacity(capacity),
        }
    }

    /// Adds an event at the back.
    ///
    /// # Errors
    /// Returns the event back when the queue is full.
    pub fn push(&mut self, event: Event) -> Result<(), Event> {
        self.events.push(event)
    }

    /// Removes and returns the oldest event.
    pub fn pop(&mut self) -> Option<Event> {
        self.events.pop()
    }

    /// Number of events waiting.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// True when no event is waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Number of events the queue can hold.
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.events.capacity()
    }
}

/// What one [`IWindowSystem::pump`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PumpResult {
    /// Events appended to the queue.
    pub delivered: u32,
    /// Events that did not fit and were dropped (never silent: the caller must check this).
    pub dropped: u32,
}

/// The window system; see the module documentation. The port is used from one thread.
pub trait IWindowSystem {
    /// True when windows can be created.
    fn has_window(&self) -> bool;

    /// Creates a window.
    ///
    /// # Errors
    /// [`PlatformError::Unsupported`] without a window system, [`PlatformError::InvalidPath`]
    /// (the shared "invalid argument" error) for a zero width or height,
    /// [`PlatformError::QueueFull`] when the system's capacity for windows is used up,
    /// [`PlatformError::Io`] on a system failure.
    fn create_window(&mut self, desc: &WindowDesc<'_>) -> Result<WindowId, PlatformError>;

    /// Destroys a window.
    ///
    /// # Errors
    /// [`PlatformError::NotFound`] for a stale or unknown id.
    fn destroy_window(&mut self, id: WindowId) -> Result<(), PlatformError>;

    /// The state of a window.
    ///
    /// # Errors
    /// [`PlatformError::NotFound`] for a stale or unknown id.
    fn window_info(&self, id: WindowId) -> Result<WindowInfo, PlatformError>;

    /// The opaque handle for the render adapter.
    ///
    /// # Errors
    /// [`PlatformError::NotFound`] for a stale or unknown id.
    fn native_token(&self, id: WindowId) -> Result<NativeToken, PlatformError>;

    /// Polls the system and appends the events to `queue`; events that do not fit are dropped and
    /// counted in the result. Never appends more than the queue's free room.
    fn pump(&mut self, queue: &mut EventQueue) -> PumpResult;
}

#[cfg(test)]
mod tests {
    use super::*;
    use base::pool::Pool;

    /// A window id as a real pool issues it.
    fn some_window() -> WindowId {
        let info = WindowInfo {
            width: 1,
            height: 1,
            framebuffer_width: 1,
            framebuffer_height: 1,
            scale: 1.0,
            focused: false,
            close_requested: false,
        };
        Pool::with_capacity(1)
            .insert(info)
            .unwrap_or_else(|_| panic!("a pool with room takes one value"))
    }

    fn event(n: u32) -> Event {
        Event::Resized {
            window: some_window(),
            width: n,
            height: n,
        }
    }

    #[test]
    fn a_new_queue_is_empty_and_reports_its_capacity() {
        let queue = EventQueue::with_capacity(3);
        assert!(queue.is_empty());
        assert_eq!((queue.len(), queue.capacity()), (0, 3));
    }

    #[test]
    fn events_come_out_in_the_order_they_went_in() {
        let mut queue = EventQueue::with_capacity(4);
        for n in 1..=3 {
            assert_eq!(queue.push(event(n)), Ok(()));
        }
        assert!(!queue.is_empty());
        assert_eq!(queue.len(), 3);
        for n in 1..=3 {
            assert_eq!(queue.pop(), Some(event(n)));
        }
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn a_full_queue_hands_the_event_back() {
        let mut queue = EventQueue::with_capacity(1);
        assert_eq!(queue.push(event(1)), Ok(()));
        assert_eq!(queue.push(event(2)), Err(event(2)));
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn the_modifier_bits_are_distinct_single_bits() {
        let all = [mods::SHIFT, mods::CONTROL, mods::ALT, mods::SUPER];
        assert!(all.iter().all(|m| m.is_power_of_two()));
        assert_eq!(all.iter().fold(0, |acc, m| acc | m).count_ones(), 4);
        assert_eq!(NativeToken::NULL, NativeToken(0));
    }

    #[test]
    fn the_default_event_is_quit_and_the_default_action_is_press() {
        assert_eq!(Event::default(), Event::Quit);
        assert_eq!(Action::default(), Action::Press);
    }
}
