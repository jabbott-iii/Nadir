//! The folders where a game keeps its settings, saves and logs.

use std::path::PathBuf;

use crate::PlatformError;

#[cfg(target_os = "linux")]
use crate::linux::user_dirs as system_dirs;
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
use crate::unsupported::user_dirs as system_dirs;
#[cfg(target_os = "windows")]
use crate::windows::user_dirs as system_dirs;

/// The longest name allowed, in bytes. Linux allows 255 bytes in one file
/// name, and Windows 255 UTF-16 units, which is never fewer.
const MAX_NAME_BYTES: usize = 255;

/// The characters Windows does not allow in a file name, besides control
/// characters. `/` separates folders on every system.
const FORBIDDEN: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// The folders where a game keeps what it writes, one set per game and
/// user.
///
/// Two of them can be the same folder: on Linux and Windows the logs are
/// kept with the data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserDirs {
    /// Settings.
    pub config: PathBuf,
    /// Saves, crash reports and other data.
    pub data: PathBuf,
    /// Log files.
    pub logs: PathBuf,
}

/// Finds the folders where the game `game`, made by `studio`, keeps its
/// settings, saves and logs.
///
/// It only works out the paths: it creates no folder. Create one before
/// writing into it.
///
/// | System | Settings | Data | Logs |
/// | :- | :- | :- | :- |
/// | Linux | `$XDG_CONFIG_HOME/<game>` | `$XDG_DATA_HOME/<game>` | as data |
/// | Windows | `%APPDATA%\<studio>\<game>` | `%LOCALAPPDATA%\<studio>\<game>` | as data |
/// | macOS | `~/Library/Application Support/<game>` | as settings | `~/Library/Logs/<game>` |
///
/// On Linux, an `XDG_` variable that is unset, empty or not an absolute
/// path counts as unset, and its default applies: `~/.config` and
/// `~/.local/share`.
///
/// # Errors
///
/// - [`PlatformError::InvalidName`] if `studio` or `game` cannot be a
///   folder name on all three systems, even if this system would accept
///   it. A name must be 1 to 255 bytes long and must not:
///   - contain a control character or any of `< > : " / \ | ? *`
///   - start with a space, or end with a space or a dot
///   - be a name that Windows keeps for a device, such as `CON`, `NUL` or
///     `COM1`, with or without an extension
/// - [`PlatformError::NoHomeDirectory`] if the home directory is unknown
///   and a path needs it.
/// - [`PlatformError::CallFailed`] on Windows, if the shell does not know
///   the folders.
/// - [`PlatformError::Unsupported`] on macOS, until it has a backend.
pub fn user_dirs(studio: &str, game: &str) -> Result<UserDirs, PlatformError> {
    check_name(studio)?;
    check_name(game)?;
    system_dirs(studio, game)
}

/// Fails unless `name` can be one folder name on every system.
fn check_name(name: &str) -> Result<(), PlatformError> {
    // The part before the extension, which is what Windows compares with
    // its device names.
    let stem = name.split('.').next().unwrap_or(name).trim_end_matches(' ');
    let valid = !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && !name
            .chars()
            .any(|c| c.is_control() || FORBIDDEN.contains(&c))
        && !name.starts_with(' ')
        && !name.ends_with([' ', '.'])
        && !is_device_name(stem);
    if valid {
        Ok(())
    } else {
        Err(PlatformError::InvalidName {
            name: name.to_string(),
        })
    }
}

/// Whether Windows keeps `stem` for a device. Case does not matter.
fn is_device_name(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let Some(number) = upper
        .strip_prefix("COM")
        .or_else(|| upper.strip_prefix("LPT"))
    else {
        return false;
    };
    let mut chars = number.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some('0'..='9' | '¹' | '²' | '³'), None)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_names_are_accepted() {
        for name in [
            "Sandbox",
            "My Game",
            "Nadir Studio",
            "Café",
            "ゲーム",
            "game-2",
            "v1.0",
            ".hidden",
            "CONSOLE",
            "Contra",
            "COM10",
            "LPT",
            "NULL",
        ] {
            assert_eq!(check_name(name), Ok(()), "{name:?}");
        }
        assert_eq!(check_name(&"x".repeat(MAX_NAME_BYTES)), Ok(()));
    }

    #[test]
    fn names_that_some_system_refuses_are_rejected() {
        let long = "x".repeat(MAX_NAME_BYTES + 1);
        for name in [
            "",
            ".",
            "..",
            "a/b",
            "/abs",
            "a\\b",
            "C:",
            "what?",
            "star*",
            "<x>",
            "\"quoted\"",
            "a|b",
            "tab\there",
            "line\nbreak",
            "nul\0",
            " leading",
            "trailing ",
            "trailing.",
            "CON",
            "con",
            "Con.txt",
            "NUL",
            "aux.tar.gz",
            "AUX .log",
            "prn",
            "COM1",
            "com0",
            "LPT9",
            "COM¹",
            "lpt³",
            &long,
        ] {
            assert_eq!(
                check_name(name),
                Err(PlatformError::InvalidName {
                    name: name.to_string()
                }),
                "{name:?}"
            );
        }
    }

    #[test]
    fn both_names_are_checked() {
        let invalid = |name: &str| {
            Err(PlatformError::InvalidName {
                name: name.to_string(),
            })
        };
        assert_eq!(user_dirs("a/b", "Game"), invalid("a/b"));
        assert_eq!(user_dirs("Studio", ".."), invalid(".."));
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    #[test]
    fn other_systems_have_no_paths_yet() {
        assert_eq!(user_dirs("Studio", "Game"), Err(PlatformError::Unsupported));
    }
}
