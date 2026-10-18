//! The Windows backend: a Win32 window, the user's folders, and message
//! boxes.

mod input;
mod keymap;
mod paths;
mod window;

use core::ptr;

use nadir_sys::windows::user32::{MB_ICONERROR, MB_OK, MessageBoxW};

pub(crate) use paths::user_dirs;
pub(crate) use window::Backend;

/// `text` in UTF-16, ending with a NUL, for Windows. A NUL inside `text`
/// ends the string there, as far as Windows can see.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// Shows an error in a message box, and waits until the player closes it.
pub(crate) fn message_box(title: &str, message: &str) {
    let (title, message) = (wide(title), wide(message));
    // SAFETY: both strings end with a NUL, and a box with no owner window is
    // allowed.
    let chosen = unsafe {
        MessageBoxW(
            ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        )
    };
    // A failure has nowhere to be reported but where the message also went:
    // stderr, written before this.
    let _ = chosen;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_string_is_utf_16_with_a_nul_at_the_end() {
        assert_eq!(wide(""), [0]);
        assert_eq!(wide("Nadir"), [0x4E, 0x61, 0x64, 0x69, 0x72, 0]);
        assert_eq!(wide("é"), [0xE9, 0]);
        // Outside the basic plane: a surrogate pair.
        assert_eq!(wide("\u{1F3AE}"), [0xD83C, 0xDFAE, 0]);
    }
}
