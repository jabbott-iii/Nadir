//! From the kernel's key codes to [`Key`].
//!
//! An X server on Linux reports a key as the kernel's evdev code plus 8.
//! That holds wherever the server's keyboard rules are `evdev`, which is
//! every current Linux desktop, and XWayland. The code names a physical key
//! and does not depend on the keyboard layout.
//!
//! Every [`Key`] has a code here: this backend has no keys it cannot report.
//! A keyboard's Japanese and Korean keys, and its media keys, have codes but
//! no [`Key`], and are ignored.

use core::ffi::c_uint;

use nadir_core::Key;
use nadir_sys::linux::evdev as ev;

/// What an X server adds to an evdev code to make a keycode.
const X_KEYCODE_OFFSET: c_uint = 8;

/// Returns the key for the keycode of an X key event.
pub(super) fn from_x_keycode(keycode: c_uint) -> Option<Key> {
    let code = keycode.checked_sub(X_KEYCODE_OFFSET)?;
    from_evdev(u16::try_from(code).ok()?)
}

/// Returns the key for an evdev key code.
pub(super) fn from_evdev(code: u16) -> Option<Key> {
    Some(match code {
        ev::KEY_A => Key::A,
        ev::KEY_B => Key::B,
        ev::KEY_C => Key::C,
        ev::KEY_D => Key::D,
        ev::KEY_E => Key::E,
        ev::KEY_F => Key::F,
        ev::KEY_G => Key::G,
        ev::KEY_H => Key::H,
        ev::KEY_I => Key::I,
        ev::KEY_J => Key::J,
        ev::KEY_K => Key::K,
        ev::KEY_L => Key::L,
        ev::KEY_M => Key::M,
        ev::KEY_N => Key::N,
        ev::KEY_O => Key::O,
        ev::KEY_P => Key::P,
        ev::KEY_Q => Key::Q,
        ev::KEY_R => Key::R,
        ev::KEY_S => Key::S,
        ev::KEY_T => Key::T,
        ev::KEY_U => Key::U,
        ev::KEY_V => Key::V,
        ev::KEY_W => Key::W,
        ev::KEY_X => Key::X,
        ev::KEY_Y => Key::Y,
        ev::KEY_Z => Key::Z,
        ev::KEY_1 => Key::Digit1,
        ev::KEY_2 => Key::Digit2,
        ev::KEY_3 => Key::Digit3,
        ev::KEY_4 => Key::Digit4,
        ev::KEY_5 => Key::Digit5,
        ev::KEY_6 => Key::Digit6,
        ev::KEY_7 => Key::Digit7,
        ev::KEY_8 => Key::Digit8,
        ev::KEY_9 => Key::Digit9,
        ev::KEY_0 => Key::Digit0,
        ev::KEY_ENTER => Key::Enter,
        ev::KEY_ESC => Key::Escape,
        ev::KEY_BACKSPACE => Key::Backspace,
        ev::KEY_TAB => Key::Tab,
        ev::KEY_SPACE => Key::Space,
        ev::KEY_MINUS => Key::Minus,
        ev::KEY_EQUAL => Key::Equal,
        ev::KEY_LEFTBRACE => Key::LeftBracket,
        ev::KEY_RIGHTBRACE => Key::RightBracket,
        ev::KEY_BACKSLASH => Key::Backslash,
        ev::KEY_SEMICOLON => Key::Semicolon,
        ev::KEY_APOSTROPHE => Key::Quote,
        ev::KEY_GRAVE => Key::Grave,
        ev::KEY_COMMA => Key::Comma,
        ev::KEY_DOT => Key::Period,
        ev::KEY_SLASH => Key::Slash,
        ev::KEY_CAPSLOCK => Key::CapsLock,
        ev::KEY_F1 => Key::F1,
        ev::KEY_F2 => Key::F2,
        ev::KEY_F3 => Key::F3,
        ev::KEY_F4 => Key::F4,
        ev::KEY_F5 => Key::F5,
        ev::KEY_F6 => Key::F6,
        ev::KEY_F7 => Key::F7,
        ev::KEY_F8 => Key::F8,
        ev::KEY_F9 => Key::F9,
        ev::KEY_F10 => Key::F10,
        ev::KEY_F11 => Key::F11,
        ev::KEY_F12 => Key::F12,
        ev::KEY_SYSRQ => Key::PrintScreen,
        ev::KEY_SCROLLLOCK => Key::ScrollLock,
        ev::KEY_PAUSE => Key::Pause,
        ev::KEY_INSERT => Key::Insert,
        ev::KEY_HOME => Key::Home,
        ev::KEY_PAGEUP => Key::PageUp,
        ev::KEY_DELETE => Key::Delete,
        ev::KEY_END => Key::End,
        ev::KEY_PAGEDOWN => Key::PageDown,
        ev::KEY_RIGHT => Key::Right,
        ev::KEY_LEFT => Key::Left,
        ev::KEY_DOWN => Key::Down,
        ev::KEY_UP => Key::Up,
        ev::KEY_NUMLOCK => Key::NumLock,
        ev::KEY_KPSLASH => Key::NumpadDivide,
        ev::KEY_KPASTERISK => Key::NumpadMultiply,
        ev::KEY_KPMINUS => Key::NumpadSubtract,
        ev::KEY_KPPLUS => Key::NumpadAdd,
        ev::KEY_KPENTER => Key::NumpadEnter,
        ev::KEY_KP1 => Key::Numpad1,
        ev::KEY_KP2 => Key::Numpad2,
        ev::KEY_KP3 => Key::Numpad3,
        ev::KEY_KP4 => Key::Numpad4,
        ev::KEY_KP5 => Key::Numpad5,
        ev::KEY_KP6 => Key::Numpad6,
        ev::KEY_KP7 => Key::Numpad7,
        ev::KEY_KP8 => Key::Numpad8,
        ev::KEY_KP9 => Key::Numpad9,
        ev::KEY_KP0 => Key::Numpad0,
        ev::KEY_KPDOT => Key::NumpadDecimal,
        ev::KEY_102ND => Key::IntlBackslash,
        ev::KEY_COMPOSE => Key::Menu,
        ev::KEY_KPEQUAL => Key::NumpadEqual,
        ev::KEY_F13 => Key::F13,
        ev::KEY_F14 => Key::F14,
        ev::KEY_F15 => Key::F15,
        ev::KEY_F16 => Key::F16,
        ev::KEY_F17 => Key::F17,
        ev::KEY_F18 => Key::F18,
        ev::KEY_F19 => Key::F19,
        ev::KEY_F20 => Key::F20,
        ev::KEY_F21 => Key::F21,
        ev::KEY_F22 => Key::F22,
        ev::KEY_F23 => Key::F23,
        ev::KEY_F24 => Key::F24,
        ev::KEY_LEFTCTRL => Key::LeftControl,
        ev::KEY_LEFTSHIFT => Key::LeftShift,
        ev::KEY_LEFTALT => Key::LeftAlt,
        ev::KEY_LEFTMETA => Key::LeftSuper,
        ev::KEY_RIGHTCTRL => Key::RightControl,
        ev::KEY_RIGHTSHIFT => Key::RightShift,
        ev::KEY_RIGHTALT => Key::RightAlt,
        ev::KEY_RIGHTMETA => Key::RightSuper,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The highest evdev code. `linux/input-event-codes.h`: `KEY_MAX`.
    const KEY_MAX: u16 = 0x2ff;

    #[test]
    fn every_key_has_exactly_one_code() {
        for &key in Key::ALL {
            let codes: Vec<u16> = (0..=KEY_MAX)
                .filter(|&code| from_evdev(code) == Some(key))
                .collect();
            assert_eq!(codes.len(), 1, "{key:?} has the codes {codes:?}");
        }
        let mapped = (0..=u16::MAX)
            .filter(|&code| from_evdev(code).is_some())
            .count();
        assert_eq!(mapped, Key::ALL.len());
    }

    #[test]
    fn codes_match_the_kernel_header() {
        // The numbers are those of `linux/input-event-codes.h`, written out
        // here so that this does not depend on the constants the table uses.
        let expected = [
            (1, Key::Escape),
            (2, Key::Digit1),
            (10, Key::Digit9),
            (11, Key::Digit0),
            (14, Key::Backspace),
            (15, Key::Tab),
            (16, Key::Q),
            (17, Key::W),
            (28, Key::Enter),
            (29, Key::LeftControl),
            (30, Key::A),
            (31, Key::S),
            (32, Key::D),
            (41, Key::Grave),
            (42, Key::LeftShift),
            (43, Key::Backslash),
            (44, Key::Z),
            (50, Key::M),
            (54, Key::RightShift),
            (55, Key::NumpadMultiply),
            (56, Key::LeftAlt),
            (57, Key::Space),
            (58, Key::CapsLock),
            (59, Key::F1),
            (68, Key::F10),
            (69, Key::NumLock),
            (70, Key::ScrollLock),
            (71, Key::Numpad7),
            (75, Key::Numpad4),
            (79, Key::Numpad1),
            (82, Key::Numpad0),
            (83, Key::NumpadDecimal),
            (86, Key::IntlBackslash),
            (87, Key::F11),
            (88, Key::F12),
            (96, Key::NumpadEnter),
            (97, Key::RightControl),
            (98, Key::NumpadDivide),
            (99, Key::PrintScreen),
            (100, Key::RightAlt),
            (102, Key::Home),
            (103, Key::Up),
            (104, Key::PageUp),
            (105, Key::Left),
            (106, Key::Right),
            (107, Key::End),
            (108, Key::Down),
            (109, Key::PageDown),
            (110, Key::Insert),
            (111, Key::Delete),
            (117, Key::NumpadEqual),
            (119, Key::Pause),
            (125, Key::LeftSuper),
            (126, Key::RightSuper),
            (127, Key::Menu),
            (183, Key::F13),
            (194, Key::F24),
        ];
        for (code, key) in expected {
            assert_eq!(from_evdev(code), Some(key), "code {code}");
        }
    }

    /// The kernel's own table from USB HID usage ids (keyboard page) to key
    /// codes, which its HID driver uses for every USB keyboard: Linux,
    /// `drivers/hid/hid-input.c`, `hid_keyboard`. These are its entries 0x00
    /// to 0x73.
    #[rustfmt::skip]
    const HID_KEYBOARD: [u16; 0x74] = [
          0,   0,   0,   0,  30,  48,  46,  32,  18,  33,  34,  35,  23,  36,  37,  38,
         50,  49,  24,  25,  16,  19,  31,  20,  22,  47,  17,  45,  21,  44,   2,   3,
          4,   5,   6,   7,   8,   9,  10,  11,  28,   1,  14,  15,  57,  12,  13,  26,
         27,  43,  43,  39,  40,  41,  51,  52,  53,  58,  59,  60,  61,  62,  63,  64,
         65,  66,  67,  68,  87,  88,  99,  70, 119, 110, 102, 104, 111, 107, 109, 106,
        105, 108, 103,  69,  98,  55,  74,  78,  96,  79,  80,  81,  75,  76,  77,  71,
         72,  73,  82,  83,  86, 127, 116, 117, 183, 184, 185, 186, 187, 188, 189, 190,
        191, 192, 193, 194,
    ];

    /// The same table's entries 0xE0 to 0xE7: the modifier keys.
    const HID_MODIFIERS: [u16; 8] = [29, 42, 56, 125, 97, 54, 100, 126];

    #[test]
    fn every_key_has_the_code_the_kernel_gives_its_usage() {
        // `Key` is numbered by HID usage and the table here goes by evdev
        // code. A USB keyboard sends the first and the kernel turns it into
        // the second, so the two must agree by way of the kernel's table.
        for &key in Key::ALL {
            let usage = usize::from(key.usage());
            let code = match usage {
                0x00..0x74 => HID_KEYBOARD[usage],
                0xE0..=0xE7 => HID_MODIFIERS[usage - 0xE0],
                _ => panic!("{key:?} has a usage outside the kernel's table"),
            };
            assert_eq!(from_evdev(code), Some(key), "usage {usage:#04x}");
        }
        // The key left of Enter on an ISO keyboard has a usage of its own,
        // 0x32, and the same code as the backslash key, 0x31.
        assert_eq!(HID_KEYBOARD[0x32], HID_KEYBOARD[0x31]);
        assert_eq!(from_evdev(HID_KEYBOARD[0x32]), Some(Key::Backslash));
    }

    #[test]
    fn codes_that_are_not_keys_of_ours_map_to_nothing() {
        // Reserved, two Japanese keys, mute, the left mouse button, and the
        // ends of the range.
        for code in [0, 85, 89, 113, 0x110, KEY_MAX, u16::MAX] {
            assert_eq!(from_evdev(code), None, "code {code}");
        }
    }

    #[test]
    fn an_x_keycode_is_the_evdev_code_plus_eight() {
        assert_eq!(from_x_keycode(9), Some(Key::Escape));
        assert_eq!(from_x_keycode(25), Some(Key::W));
        assert_eq!(from_x_keycode(38), Some(Key::A));
        assert_eq!(from_x_keycode(133), Some(Key::LeftSuper));
        for &key in Key::ALL {
            let code = (0..=KEY_MAX).find(|&code| from_evdev(code) == Some(key));
            let keycode = c_uint::from(code.unwrap()) + 8;
            assert_eq!(from_x_keycode(keycode), Some(key));
            // A core X keycode is one byte.
            assert!(keycode <= 255, "{key:?} has the keycode {keycode}");
        }
    }

    #[test]
    fn keycodes_outside_the_table_map_to_nothing() {
        for keycode in [0, 7, 8, 256, 0x2ff + 8, 65536 + 38, c_uint::MAX] {
            assert_eq!(from_x_keycode(keycode), None, "keycode {keycode}");
        }
    }
}
