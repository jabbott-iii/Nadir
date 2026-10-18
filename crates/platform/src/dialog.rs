//! Telling the player about an error that stops the game.

use std::io::{self, Write};

/// Tells the player about an error that stops the game, such as a window
/// that will not open. `title` names the game, and `message` says what went
/// wrong.
///
/// On Linux the message goes to stderr, as one line: `<title>: <message>`.
/// The game never starts another program, such as `zenity`, to show it. On
/// Windows and macOS it goes to stderr too, until those systems have
/// backends.
pub fn show_error_dialog(title: &str, message: &str) {
    // There is nowhere left to report a failure to write to stderr.
    let _ = write_error(&mut io::stderr().lock(), title, message);
}

/// Writes the error as one line of text.
fn write_error(out: &mut impl Write, title: &str, message: &str) -> io::Result<()> {
    writeln!(out, "{title}: {message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_follows_the_title_on_one_line() {
        let mut out = Vec::new();
        write_error(&mut out, "Sandbox", "could not open the window").expect("write to a Vec");
        assert_eq!(out, b"Sandbox: could not open the window\n");
    }
}
