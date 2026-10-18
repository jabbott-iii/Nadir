//! From the Mac's virtual key codes to [`Key`].
//!
//! `-[NSEvent keyCode]` names a physical key, whatever the keyboard layout.
//! The table follows the place of each key: `kVK_Help` is the key in the
//! place of Insert on Apple's extended keyboards, and `kVK_ANSI_KeypadClear`
//! the one in the place of Num Lock.
//!
//! One pair of keys is placed by keyboard type, not by position. On an ANSI
//! keyboard the key left of `1` reports `kVK_ANSI_Grave`, which is
//! [`Key::Grave`], and the table follows that. On an ISO keyboard Apple names
//! the codes by the key labels, so that key reports `kVK_ISO_Section`, and
//! the key right of the left Shift, [`Key::IntlBackslash`], reports
//! `kVK_ANSI_Grave`: the two come out swapped. Putting them back needs the
//! keyboard type from HIToolbox, as SDL does, which is not done yet.
//!
//! These keys have no code on macOS, and are never reported: Print Screen,
//! Scroll Lock, Pause, Menu, and F21 to F24. Apple's keyboards have F13 to
//! F15 in the places of the first three, and no Menu key; `HIToolbox/Events.h`
//! names no code for the PC Menu key, or for F21 to F24.

use nadir_core::Key;
use nadir_sys::macos::hitoolbox as vk;

/// Returns the key for a virtual key code.
pub(super) fn from_key_code(code: u16) -> Option<Key> {
    Some(match code {
        vk::kVK_ANSI_A => Key::A,
        vk::kVK_ANSI_B => Key::B,
        vk::kVK_ANSI_C => Key::C,
        vk::kVK_ANSI_D => Key::D,
        vk::kVK_ANSI_E => Key::E,
        vk::kVK_ANSI_F => Key::F,
        vk::kVK_ANSI_G => Key::G,
        vk::kVK_ANSI_H => Key::H,
        vk::kVK_ANSI_I => Key::I,
        vk::kVK_ANSI_J => Key::J,
        vk::kVK_ANSI_K => Key::K,
        vk::kVK_ANSI_L => Key::L,
        vk::kVK_ANSI_M => Key::M,
        vk::kVK_ANSI_N => Key::N,
        vk::kVK_ANSI_O => Key::O,
        vk::kVK_ANSI_P => Key::P,
        vk::kVK_ANSI_Q => Key::Q,
        vk::kVK_ANSI_R => Key::R,
        vk::kVK_ANSI_S => Key::S,
        vk::kVK_ANSI_T => Key::T,
        vk::kVK_ANSI_U => Key::U,
        vk::kVK_ANSI_V => Key::V,
        vk::kVK_ANSI_W => Key::W,
        vk::kVK_ANSI_X => Key::X,
        vk::kVK_ANSI_Y => Key::Y,
        vk::kVK_ANSI_Z => Key::Z,
        vk::kVK_ANSI_1 => Key::Digit1,
        vk::kVK_ANSI_2 => Key::Digit2,
        vk::kVK_ANSI_3 => Key::Digit3,
        vk::kVK_ANSI_4 => Key::Digit4,
        vk::kVK_ANSI_5 => Key::Digit5,
        vk::kVK_ANSI_6 => Key::Digit6,
        vk::kVK_ANSI_7 => Key::Digit7,
        vk::kVK_ANSI_8 => Key::Digit8,
        vk::kVK_ANSI_9 => Key::Digit9,
        vk::kVK_ANSI_0 => Key::Digit0,
        vk::kVK_Return => Key::Enter,
        vk::kVK_Escape => Key::Escape,
        vk::kVK_Delete => Key::Backspace,
        vk::kVK_Tab => Key::Tab,
        vk::kVK_Space => Key::Space,
        vk::kVK_ANSI_Minus => Key::Minus,
        vk::kVK_ANSI_Equal => Key::Equal,
        vk::kVK_ANSI_LeftBracket => Key::LeftBracket,
        vk::kVK_ANSI_RightBracket => Key::RightBracket,
        vk::kVK_ANSI_Backslash => Key::Backslash,
        vk::kVK_ANSI_Semicolon => Key::Semicolon,
        vk::kVK_ANSI_Quote => Key::Quote,
        vk::kVK_ANSI_Grave => Key::Grave,
        vk::kVK_ANSI_Comma => Key::Comma,
        vk::kVK_ANSI_Period => Key::Period,
        vk::kVK_ANSI_Slash => Key::Slash,
        vk::kVK_CapsLock => Key::CapsLock,
        vk::kVK_F1 => Key::F1,
        vk::kVK_F2 => Key::F2,
        vk::kVK_F3 => Key::F3,
        vk::kVK_F4 => Key::F4,
        vk::kVK_F5 => Key::F5,
        vk::kVK_F6 => Key::F6,
        vk::kVK_F7 => Key::F7,
        vk::kVK_F8 => Key::F8,
        vk::kVK_F9 => Key::F9,
        vk::kVK_F10 => Key::F10,
        vk::kVK_F11 => Key::F11,
        vk::kVK_F12 => Key::F12,
        vk::kVK_Help => Key::Insert,
        vk::kVK_Home => Key::Home,
        vk::kVK_PageUp => Key::PageUp,
        vk::kVK_ForwardDelete => Key::Delete,
        vk::kVK_End => Key::End,
        vk::kVK_PageDown => Key::PageDown,
        vk::kVK_RightArrow => Key::Right,
        vk::kVK_LeftArrow => Key::Left,
        vk::kVK_DownArrow => Key::Down,
        vk::kVK_UpArrow => Key::Up,
        vk::kVK_ANSI_KeypadClear => Key::NumLock,
        vk::kVK_ANSI_KeypadDivide => Key::NumpadDivide,
        vk::kVK_ANSI_KeypadMultiply => Key::NumpadMultiply,
        vk::kVK_ANSI_KeypadMinus => Key::NumpadSubtract,
        vk::kVK_ANSI_KeypadPlus => Key::NumpadAdd,
        vk::kVK_ANSI_KeypadEnter => Key::NumpadEnter,
        vk::kVK_ANSI_Keypad1 => Key::Numpad1,
        vk::kVK_ANSI_Keypad2 => Key::Numpad2,
        vk::kVK_ANSI_Keypad3 => Key::Numpad3,
        vk::kVK_ANSI_Keypad4 => Key::Numpad4,
        vk::kVK_ANSI_Keypad5 => Key::Numpad5,
        vk::kVK_ANSI_Keypad6 => Key::Numpad6,
        vk::kVK_ANSI_Keypad7 => Key::Numpad7,
        vk::kVK_ANSI_Keypad8 => Key::Numpad8,
        vk::kVK_ANSI_Keypad9 => Key::Numpad9,
        vk::kVK_ANSI_Keypad0 => Key::Numpad0,
        vk::kVK_ANSI_KeypadDecimal => Key::NumpadDecimal,
        vk::kVK_ISO_Section => Key::IntlBackslash,
        vk::kVK_ANSI_KeypadEquals => Key::NumpadEqual,
        vk::kVK_F13 => Key::F13,
        vk::kVK_F14 => Key::F14,
        vk::kVK_F15 => Key::F15,
        vk::kVK_F16 => Key::F16,
        vk::kVK_F17 => Key::F17,
        vk::kVK_F18 => Key::F18,
        vk::kVK_F19 => Key::F19,
        vk::kVK_F20 => Key::F20,
        vk::kVK_Control => Key::LeftControl,
        vk::kVK_Shift => Key::LeftShift,
        vk::kVK_Option => Key::LeftAlt,
        vk::kVK_Command => Key::LeftSuper,
        vk::kVK_RightControl => Key::RightControl,
        vk::kVK_RightShift => Key::RightShift,
        vk::kVK_RightOption => Key::RightAlt,
        vk::kVK_RightCommand => Key::RightSuper,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keys with no code: see the module's documentation.
    const EXCEPTIONS: [Key; 8] = [
        Key::PrintScreen,
        Key::ScrollLock,
        Key::Pause,
        Key::Menu,
        Key::F21,
        Key::F22,
        Key::F23,
        Key::F24,
    ];

    #[test]
    fn every_key_but_the_exceptions_has_exactly_one_code() {
        for &key in Key::ALL {
            let codes: Vec<u16> = (0..=u16::MAX)
                .filter(|&code| from_key_code(code) == Some(key))
                .collect();
            let expected = usize::from(!EXCEPTIONS.contains(&key));
            assert_eq!(codes.len(), expected, "{key:?} has the codes {codes:x?}");
        }
        assert_eq!(
            (0..=u16::MAX)
                .filter(|&code| from_key_code(code).is_some())
                .count(),
            Key::ALL.len() - EXCEPTIONS.len()
        );
    }

    #[test]
    fn codes_match_the_header() {
        // The numbers are those of `HIToolbox/Events.h`, written out here so
        // that this does not depend on the constants the table uses.
        let expected = [
            (0x00, Key::A),
            (0x01, Key::S),
            (0x06, Key::Z),
            (0x0A, Key::IntlBackslash),
            (0x0C, Key::Q),
            (0x0D, Key::W),
            (0x12, Key::Digit1),
            (0x16, Key::Digit6),
            (0x17, Key::Digit5),
            (0x1D, Key::Digit0),
            (0x24, Key::Enter),
            (0x30, Key::Tab),
            (0x31, Key::Space),
            (0x32, Key::Grave),
            (0x33, Key::Backspace),
            (0x35, Key::Escape),
            (0x36, Key::RightSuper),
            (0x37, Key::LeftSuper),
            (0x38, Key::LeftShift),
            (0x39, Key::CapsLock),
            (0x3A, Key::LeftAlt),
            (0x3B, Key::LeftControl),
            (0x3C, Key::RightShift),
            (0x3D, Key::RightAlt),
            (0x3E, Key::RightControl),
            (0x47, Key::NumLock),
            (0x4C, Key::NumpadEnter),
            (0x51, Key::NumpadEqual),
            (0x52, Key::Numpad0),
            (0x5C, Key::Numpad9),
            (0x60, Key::F5),
            (0x69, Key::F13),
            (0x6F, Key::F12),
            (0x72, Key::Insert),
            (0x75, Key::Delete),
            (0x7A, Key::F1),
            (0x7B, Key::Left),
            (0x7E, Key::Up),
        ];
        for (code, key) in expected {
            assert_eq!(from_key_code(code), Some(key), "code {code:#04x}");
        }
    }

    #[test]
    fn codes_that_are_not_keys_of_ours_map_to_nothing() {
        // Fn, the volume keys, the Japanese Yen and Kana keys, and the end of
        // the range.
        for code in [0x3F, 0x48, 0x49, 0x4A, 0x5D, 0x68, 0x7F, u16::MAX] {
            assert_eq!(from_key_code(code), None, "code {code:#04x}");
        }
    }
}
