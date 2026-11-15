//! Layout-independent key codes and the scancode tables of the operating systems.
//!
//! A [`KeyCode`] names the physical key by its position on a PC keyboard (the key at the place of
//! `Q` on a US layout is [`KeyCode::Q`], also on a French one), as the web's `KeyboardEvent.code`
//! does. The platform event carries only the hardware scancode, which differs per operating
//! system; a [`KeyMap`] turns it into a key code.

/// One row per key: the code, the Windows scancode (as GLFW reports it: the PC set-1 make code,
/// plus `0x100` for the extended keys) and the Linux evdev code (the X11 scancode is that plus 8).
macro_rules! keys {
    ($($name:ident => ($windows:expr, $evdev:expr)),* $(,)?) => {
        /// A physical key, named by its position on a PC keyboard.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u8)]
        pub enum KeyCode {
            $(
                #[doc = concat!("The `", stringify!($name), "` key.")]
                $name,
            )*
        }

        impl KeyCode {
            /// Every key code, in the order of its [`index`](Self::index).
            pub const ALL: &'static [KeyCode] = &[$(KeyCode::$name),*];

            /// The key, its Windows scancode and its evdev code.
            const CODES: &'static [(KeyCode, u32, u32)] =
                &[$((KeyCode::$name, $windows, $evdev)),*];
        }
    };
}

keys! {
    Escape => (0x01, 1),
    Digit1 => (0x02, 2), Digit2 => (0x03, 3), Digit3 => (0x04, 4), Digit4 => (0x05, 5),
    Digit5 => (0x06, 6), Digit6 => (0x07, 7), Digit7 => (0x08, 8), Digit8 => (0x09, 9),
    Digit9 => (0x0A, 10), Digit0 => (0x0B, 11),
    Backspace => (0x0E, 14), Tab => (0x0F, 15),
    Q => (0x10, 16), W => (0x11, 17), E => (0x12, 18), R => (0x13, 19), T => (0x14, 20),
    Y => (0x15, 21), U => (0x16, 22), I => (0x17, 23), O => (0x18, 24), P => (0x19, 25),
    Enter => (0x1C, 28), LeftControl => (0x1D, 29),
    A => (0x1E, 30), S => (0x1F, 31), D => (0x20, 32), F => (0x21, 33), G => (0x22, 34),
    H => (0x23, 35), J => (0x24, 36), K => (0x25, 37), L => (0x26, 38),
    LeftShift => (0x2A, 42),
    Z => (0x2C, 44), X => (0x2D, 45), C => (0x2E, 46), V => (0x2F, 47), B => (0x30, 48),
    N => (0x31, 49), M => (0x32, 50),
    RightShift => (0x36, 54), LeftAlt => (0x38, 56), Space => (0x39, 57),
    F1 => (0x3B, 59), F2 => (0x3C, 60), F3 => (0x3D, 61), F4 => (0x3E, 62), F5 => (0x3F, 63),
    F6 => (0x40, 64), F7 => (0x41, 65), F8 => (0x42, 66), F9 => (0x43, 67), F10 => (0x44, 68),
    F11 => (0x57, 87), F12 => (0x58, 88),
    RightControl => (0x11D, 97), RightAlt => (0x138, 100),
    Up => (0x148, 103), Left => (0x14B, 105), Right => (0x14D, 106), Down => (0x150, 108),
}

impl KeyCode {
    /// The position of the key in [`KeyCode::ALL`]: dense, from zero, below 128.
    #[must_use]
    pub fn index(self) -> u32 {
        self as u32
    }
}

/// Offset between an evdev code and the scancode an X11 or Wayland window system reports.
const EVDEV_TO_X11: u32 = 8;

/// A table from hardware scancode to key code.
#[derive(Debug, Clone)]
pub struct KeyMap {
    entries: Vec<(u32, KeyCode)>,
}

impl KeyMap {
    /// The table of Windows, with GLFW scancodes (the extended keys, which are the arrows and the
    /// right-hand control and alt, follow the PC set-1 convention and are not yet checked on
    /// hardware; the main block is the one the physics sample already uses).
    #[must_use]
    pub fn windows() -> Self {
        Self::build(|windows, _| windows)
    }

    /// The table of Linux (X11 and Wayland, evdev code plus 8; not run on hardware yet, derived
    /// from the evdev key list and checked against the table of the physics sample).
    #[must_use]
    pub fn linux() -> Self {
        Self::build(|_, evdev| evdev + EVDEV_TO_X11)
    }

    /// The table of the operating system this build runs on (Linux for everything but Windows).
    #[must_use]
    pub fn host() -> Self {
        if cfg!(windows) {
            Self::windows()
        } else {
            Self::linux()
        }
    }

    /// A table that names no key: every key event then counts as unmapped.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn build(scancode_of: impl Fn(u32, u32) -> u32) -> Self {
        let mut entries: Vec<(u32, KeyCode)> = KeyCode::CODES
            .iter()
            .map(|(key, windows, evdev)| (scancode_of(*windows, *evdev), *key))
            .collect();
        entries.sort_by_key(|(scancode, _)| *scancode);
        Self { entries }
    }

    /// The key at `scancode`, if the table knows it.
    #[must_use]
    pub fn key(&self, scancode: u32) -> Option<KeyCode> {
        let at = self
            .entries
            .binary_search_by_key(&scancode, |(code, _)| *code)
            .ok()?;
        self.entries.get(at).map(|(_, key)| *key)
    }

    /// How many keys the table names.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when the table names no key.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
