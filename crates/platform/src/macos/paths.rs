//! The user's folders, in the home folder's `Library`.

use std::env;
use std::path::PathBuf;

use crate::{PlatformError, UserDirs};

/// Finds the game's folders. The studio name is not part of the paths on
/// macOS.
pub(crate) fn user_dirs(_studio: &str, game: &str) -> Result<UserDirs, PlatformError> {
    // `home_dir` reads `HOME`, and asks the user database if it is unset or
    // empty. In a sandboxed app, `HOME` is the app's container, which is
    // where its `Library` is.
    resolve(game, env::home_dir())
}

/// Finds the game's folders inside the home folder `home`.
fn resolve(game: &str, home: Option<PathBuf>) -> Result<UserDirs, PlatformError> {
    let home = home
        .filter(|home| home.is_absolute())
        .ok_or(PlatformError::NoHomeDirectory)?;
    let library = home.join("Library");
    let data = library.join("Application Support").join(game);
    Ok(UserDirs {
        config: data.clone(),
        data,
        logs: library.join("Logs").join(game),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_and_data_share_application_support_and_logs_have_their_own() {
        let dirs = resolve("Sandbox", Some(PathBuf::from("/Users/ada"))).expect("a home");
        assert_eq!(
            dirs.config,
            PathBuf::from("/Users/ada/Library/Application Support/Sandbox")
        );
        assert_eq!(dirs.data, dirs.config);
        assert_eq!(dirs.logs, PathBuf::from("/Users/ada/Library/Logs/Sandbox"));
    }

    #[test]
    fn an_unknown_or_relative_home_is_an_error() {
        for home in [None, Some(PathBuf::new()), Some(PathBuf::from("Users/ada"))] {
            assert_eq!(
                resolve("Sandbox", home.clone()),
                Err(PlatformError::NoHomeDirectory),
                "{home:?}"
            );
        }
    }
}
