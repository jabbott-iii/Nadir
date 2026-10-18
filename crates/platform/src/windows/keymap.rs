//! From scan codes to [`Key`].
//!
//! A key message carries the key's scan code (PC set 1) in bits 16 to 23 of
//! its `lParam`, and in bit 24 whether the code had the `0xE0` prefix of an
//! extended key. The code names a physical key and does not depend on the
//! keyboard layout. Here an extended code has `0xE0` in its high byte, as
//! Microsoft writes them: Right Control is `0xE01D`.
//!
//! The codes are those of Microsoft's table of scan codes ("Keyboard Input
//! Overview", <https://learn.microsoft.com/windows/win32/inputdev/about-keyboard-input>),
//! as key messages deliver them. Two keys differ there from the scan codes
//! the keyboard sends: Num Lock arrives as `0xE045`, and Pause as `0x45`.
//!
//! Every [`Key`] has a code here: this backend has no keys it cannot report.
//! Japanese, Korean and Brazilian keys, media keys, the fake Shift presses
//! that Windows adds around some keypad keys (`0xE02A`, `0xE036`), and the
//! codes that Alt+Print Screen and Ctrl+Pause send (`0x54`, `0xE046`) have
//! no [`Key`], and are ignored.

use nadir_core::Key;
use nadir_sys::windows::types::LPARAM;

/// The bit of a key message's `lParam` that marks an extended key.
const EXTENDED: LPARAM = 1 << 24;

/// The prefix of an extended scan code, in the high byte.
const EXTENDED_PREFIX: u16 = 0xE000;

/// Returns the scan code of a key message, from its `lParam`.
pub(super) fn scan_code(lparam: LPARAM) -> u16 {
    // Bits 16 to 23. The mask keeps the cast from losing anything.
    let code = ((lparam >> 16) & 0xFF) as u16;
    if lparam & EXTENDED != 0 {
        EXTENDED_PREFIX | code
    } else {
        code
    }
}

/// Returns the key for a scan code, extended codes having `0xE0` in their
/// high byte.
pub(super) fn from_scan_code(code: u16) -> Option<Key> {
    Some(match code {
        0x001E => Key::A,
        0x0030 => Key::B,
        0x002E => Key::C,
        0x0020 => Key::D,
        0x0012 => Key::E,
        0x0021 => Key::F,
        0x0022 => Key::G,
        0x0023 => Key::H,
        0x0017 => Key::I,
        0x0024 => Key::J,
        0x0025 => Key::K,
        0x0026 => Key::L,
        0x0032 => Key::M,
        0x0031 => Key::N,
        0x0018 => Key::O,
        0x0019 => Key::P,
        0x0010 => Key::Q,
        0x0013 => Key::R,
        0x001F => Key::S,
        0x0014 => Key::T,
        0x0016 => Key::U,
        0x002F => Key::V,
        0x0011 => Key::W,
        0x002D => Key::X,
        0x0015 => Key::Y,
        0x002C => Key::Z,
        0x0002 => Key::Digit1,
        0x0003 => Key::Digit2,
        0x0004 => Key::Digit3,
        0x0005 => Key::Digit4,
        0x0006 => Key::Digit5,
        0x0007 => Key::Digit6,
        0x0008 => Key::Digit7,
        0x0009 => Key::Digit8,
        0x000A => Key::Digit9,
        0x000B => Key::Digit0,
        0x001C => Key::Enter,
        0x0001 => Key::Escape,
        0x000E => Key::Backspace,
        0x000F => Key::Tab,
        0x0039 => Key::Space,
        0x000C => Key::Minus,
        0x000D => Key::Equal,
        0x001A => Key::LeftBracket,
        0x001B => Key::RightBracket,
        // Also the key left of Enter on an ISO keyboard, which has a HID
        // usage of its own.
        0x002B => Key::Backslash,
        0x0027 => Key::Semicolon,
        0x0028 => Key::Quote,
        0x0029 => Key::Grave,
        0x0033 => Key::Comma,
        0x0034 => Key::Period,
        0x0035 => Key::Slash,
        0x003A => Key::CapsLock,
        0x003B => Key::F1,
        0x003C => Key::F2,
        0x003D => Key::F3,
        0x003E => Key::F4,
        0x003F => Key::F5,
        0x0040 => Key::F6,
        0x0041 => Key::F7,
        0x0042 => Key::F8,
        0x0043 => Key::F9,
        0x0044 => Key::F10,
        0x0057 => Key::F11,
        0x0058 => Key::F12,
        0xE037 => Key::PrintScreen,
        0x0046 => Key::ScrollLock,
        0x0045 => Key::Pause,
        0xE052 => Key::Insert,
        0xE047 => Key::Home,
        0xE049 => Key::PageUp,
        0xE053 => Key::Delete,
        0xE04F => Key::End,
        0xE051 => Key::PageDown,
        0xE04D => Key::Right,
        0xE04B => Key::Left,
        0xE050 => Key::Down,
        0xE048 => Key::Up,
        0xE045 => Key::NumLock,
        0xE035 => Key::NumpadDivide,
        0x0037 => Key::NumpadMultiply,
        0x004A => Key::NumpadSubtract,
        0x004E => Key::NumpadAdd,
        0xE01C => Key::NumpadEnter,
        0x004F => Key::Numpad1,
        0x0050 => Key::Numpad2,
        0x0051 => Key::Numpad3,
        0x004B => Key::Numpad4,
        0x004C => Key::Numpad5,
        0x004D => Key::Numpad6,
        0x0047 => Key::Numpad7,
        0x0048 => Key::Numpad8,
        0x0049 => Key::Numpad9,
        0x0052 => Key::Numpad0,
        0x0053 => Key::NumpadDecimal,
        0x0056 => Key::IntlBackslash,
        0xE05D => Key::Menu,
        0x0059 => Key::NumpadEqual,
        0x0064 => Key::F13,
        0x0065 => Key::F14,
        0x0066 => Key::F15,
        0x0067 => Key::F16,
        0x0068 => Key::F17,
        0x0069 => Key::F18,
        0x006A => Key::F19,
        0x006B => Key::F20,
        0x006C => Key::F21,
        0x006D => Key::F22,
        0x006E => Key::F23,
        0x0076 => Key::F24,
        0x001D => Key::LeftControl,
        0x002A => Key::LeftShift,
        0x0038 => Key::LeftAlt,
        0xE05B => Key::LeftSuper,
        0xE01D => Key::RightControl,
        0x0036 => Key::RightShift,
        0xE038 => Key::RightAlt,
        0xE05C => Key::RightSuper,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Microsoft's table, by HID usage id (keyboard page) and the scan code
    /// a key message delivers, for every usage that has a [`Key`]. It is
    /// written out here by usage, so that it does not depend on the names
    /// the table above uses.
    #[rustfmt::skip]
    const MICROSOFT: [(u8, u16); 118] = [
        (0x04, 0x001E), (0x05, 0x0030), (0x06, 0x002E), (0x07, 0x0020),
        (0x08, 0x0012), (0x09, 0x0021), (0x0A, 0x0022), (0x0B, 0x0023),
        (0x0C, 0x0017), (0x0D, 0x0024), (0x0E, 0x0025), (0x0F, 0x0026),
        (0x10, 0x0032), (0x11, 0x0031), (0x12, 0x0018), (0x13, 0x0019),
        (0x14, 0x0010), (0x15, 0x0013), (0x16, 0x001F), (0x17, 0x0014),
        (0x18, 0x0016), (0x19, 0x002F), (0x1A, 0x0011), (0x1B, 0x002D),
        (0x1C, 0x0015), (0x1D, 0x002C), (0x1E, 0x0002), (0x1F, 0x0003),
        (0x20, 0x0004), (0x21, 0x0005), (0x22, 0x0006), (0x23, 0x0007),
        (0x24, 0x0008), (0x25, 0x0009), (0x26, 0x000A), (0x27, 0x000B),
        (0x28, 0x001C), (0x29, 0x0001), (0x2A, 0x000E), (0x2B, 0x000F),
        (0x2C, 0x0039), (0x2D, 0x000C), (0x2E, 0x000D), (0x2F, 0x001A),
        (0x30, 0x001B), (0x31, 0x002B), (0x33, 0x0027), (0x34, 0x0028),
        (0x35, 0x0029), (0x36, 0x0033), (0x37, 0x0034), (0x38, 0x0035),
        (0x39, 0x003A), (0x3A, 0x003B), (0x3B, 0x003C), (0x3C, 0x003D),
        (0x3D, 0x003E), (0x3E, 0x003F), (0x3F, 0x0040), (0x40, 0x0041),
        (0x41, 0x0042), (0x42, 0x0043), (0x43, 0x0044), (0x44, 0x0057),
        (0x45, 0x0058), (0x46, 0xE037), (0x47, 0x0046), (0x48, 0x0045),
        (0x49, 0xE052), (0x4A, 0xE047), (0x4B, 0xE049), (0x4C, 0xE053),
        (0x4D, 0xE04F), (0x4E, 0xE051), (0x4F, 0xE04D), (0x50, 0xE04B),
        (0x51, 0xE050), (0x52, 0xE048), (0x53, 0xE045), (0x54, 0xE035),
        (0x55, 0x0037), (0x56, 0x004A), (0x57, 0x004E), (0x58, 0xE01C),
        (0x59, 0x004F), (0x5A, 0x0050), (0x5B, 0x0051), (0x5C, 0x004B),
        (0x5D, 0x004C), (0x5E, 0x004D), (0x5F, 0x0047), (0x60, 0x0048),
        (0x61, 0x0049), (0x62, 0x0052), (0x63, 0x0053), (0x64, 0x0056),
        (0x65, 0xE05D), (0x67, 0x0059), (0x68, 0x0064), (0x69, 0x0065),
        (0x6A, 0x0066), (0x6B, 0x0067), (0x6C, 0x0068), (0x6D, 0x0069),
        (0x6E, 0x006A), (0x6F, 0x006B), (0x70, 0x006C), (0x71, 0x006D),
        (0x72, 0x006E), (0x73, 0x0076), (0xE0, 0x001D), (0xE1, 0x002A),
        (0xE2, 0x0038), (0xE3, 0xE05B), (0xE4, 0xE01D), (0xE5, 0x0036),
        (0xE6, 0xE038), (0xE7, 0xE05C),
    ];

    #[test]
    fn every_key_has_the_code_microsoft_gives_its_usage() {
        for &key in Key::ALL {
            let entry = MICROSOFT.iter().find(|&&(usage, _)| usage == key.usage());
            let Some(&(_, code)) = entry else {
                panic!("{key:?} is not in Microsoft's table");
            };
            assert_eq!(from_scan_code(code), Some(key), "code {code:#06x}");
        }
        // Every entry is a key, so the table has no usage twice.
        assert_eq!(MICROSOFT.len(), Key::ALL.len());
    }

    #[test]
    fn every_key_has_exactly_one_code() {
        for &key in Key::ALL {
            let codes: Vec<u16> = (0..=u16::MAX)
                .filter(|&code| from_scan_code(code) == Some(key))
                .collect();
            assert_eq!(codes.len(), 1, "{key:?} has the codes {codes:x?}");
        }
    }

    #[test]
    fn codes_that_are_not_keys_of_ours_map_to_nothing() {
        // Nothing, Alt+Print Screen, Ctrl+Pause, the fake Shifts, the
        // Japanese Katakana/Hiragana key, mute, and the ends of the range.
        for code in [
            0x0000, 0x0054, 0xE046, 0xE02A, 0xE036, 0x0070, 0xE020, 0x00FF, 0xFFFF,
        ] {
            assert_eq!(from_scan_code(code), None, "code {code:#06x}");
        }
    }

    #[test]
    fn the_scan_code_is_read_from_bits_16_to_24() {
        // A press of A: repeat count 1, scan code 0x1E.
        assert_eq!(scan_code(0x001E_0001), 0x001E);
        // Its release sets bits 30 and 31 as well.
        assert_eq!(scan_code(0xC01E_0001_u32 as i32 as LPARAM), 0x001E);
        // Right Control is extended.
        assert_eq!(scan_code(0x011D_0001), 0xE01D);
        // An auto-repeat with Alt held: context and previous-state bits.
        assert_eq!(scan_code(0x6038_0001), 0x0038);
        // The upper half of a 64-bit `lParam` is not part of it.
        assert_eq!(scan_code(0x7FFF_FFFF_001E_0001), 0x001E);
    }
}
