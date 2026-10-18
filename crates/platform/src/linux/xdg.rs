//! The user's folders, from the XDG Base Directory variables.

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

use crate::{PlatformError, UserDirs};

/// Finds the game's folders from this process's environment. The studio
/// name is not part of the paths on Linux.
pub(crate) fn user_dirs(_studio: &str, game: &str) -> Result<UserDirs, PlatformError> {
    // `home_dir` reads `HOME`, and asks the user database if it is unset
    // or empty.
    resolve(game, |name| env::var_os(name), env::home_dir)
}

/// Finds the game's folders. `var` reads an environment variable, and
/// `home` finds the home directory; `home` is called only if a path needs
/// it.
fn resolve(
    game: &str,
    var: impl Fn(&str) -> Option<OsString>,
    home: impl FnOnce() -> Option<PathBuf>,
) -> Result<UserDirs, PlatformError> {
    let config_home = absolute(var("XDG_CONFIG_HOME"));
    let data_home = absolute(var("XDG_DATA_HOME"));
    let (config_home, data_home) = match (config_home, data_home) {
        (Some(config_home), Some(data_home)) => (config_home, data_home),
        (config_home, data_home) => {
            let home = home()
                .filter(|home| home.is_absolute())
                .ok_or(PlatformError::NoHomeDirectory)?;
            (
                config_home.unwrap_or_else(|| home.join(".config")),
                data_home.unwrap_or_else(|| home.join(".local/share")),
            )
        }
    };
    let data = data_home.join(game);
    Ok(UserDirs {
        config: config_home.join(game),
        logs: data.clone(),
        data,
    })
}

/// The path in a variable, if it is one the XDG specification lets us use:
/// "All paths set in these environment variables must be absolute. If an
/// implementation encounters a relative path in any of these variables it
/// should consider the path invalid and ignore it." An empty value is
/// treated as unset.
fn absolute(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|path| path.is_absolute())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// An environment with these variables and nothing else.
    fn env<'a>(vars: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            vars.iter()
                .find(|(var, _)| *var == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    fn home(path: &str) -> impl FnOnce() -> Option<PathBuf> {
        let path = PathBuf::from(path);
        move || Some(path)
    }

    fn dirs(config: &str, data: &str) -> UserDirs {
        UserDirs {
            config: PathBuf::from(config),
            data: PathBuf::from(data),
            logs: PathBuf::from(data),
        }
    }

    #[test]
    fn without_xdg_variables_the_defaults_are_in_the_home_directory() {
        assert_eq!(
            resolve("Sandbox", env(&[]), home("/home/ada")),
            Ok(dirs(
                "/home/ada/.config/Sandbox",
                "/home/ada/.local/share/Sandbox"
            ))
        );
    }

    #[test]
    fn the_xdg_variables_override_the_defaults() {
        let vars = [("XDG_CONFIG_HOME", "/cfg"), ("XDG_DATA_HOME", "/srv/data")];
        assert_eq!(
            resolve("Sandbox", env(&vars), home("/home/ada")),
            Ok(dirs("/cfg/Sandbox", "/srv/data/Sandbox"))
        );
    }

    #[test]
    fn each_variable_is_used_on_its_own() {
        let config_only = [("XDG_CONFIG_HOME", "/cfg")];
        assert_eq!(
            resolve("Sandbox", env(&config_only), home("/home/ada")),
            Ok(dirs("/cfg/Sandbox", "/home/ada/.local/share/Sandbox"))
        );
        let data_only = [("XDG_DATA_HOME", "/data")];
        assert_eq!(
            resolve("Sandbox", env(&data_only), home("/home/ada")),
            Ok(dirs("/home/ada/.config/Sandbox", "/data/Sandbox"))
        );
    }

    #[test]
    fn empty_and_relative_variables_are_ignored() {
        for value in ["", "relative/path", "./cfg", "~/.config"] {
            let vars = [("XDG_CONFIG_HOME", value), ("XDG_DATA_HOME", value)];
            assert_eq!(
                resolve("Sandbox", env(&vars), home("/home/ada")),
                Ok(dirs(
                    "/home/ada/.config/Sandbox",
                    "/home/ada/.local/share/Sandbox"
                )),
                "{value:?}"
            );
        }
    }

    #[test]
    fn the_home_directory_is_not_needed_when_both_variables_are_set() {
        let vars = [("XDG_CONFIG_HOME", "/cfg"), ("XDG_DATA_HOME", "/data")];
        let no_home = || -> Option<PathBuf> { panic!("the home directory was looked up") };
        assert_eq!(
            resolve("Sandbox", env(&vars), no_home),
            Ok(dirs("/cfg/Sandbox", "/data/Sandbox"))
        );
    }

    #[test]
    fn an_unknown_or_relative_home_is_an_error_when_a_default_needs_it() {
        let data_only = [("XDG_DATA_HOME", "/data")];
        assert_eq!(
            resolve("Sandbox", env(&data_only), || None),
            Err(PlatformError::NoHomeDirectory)
        );
        for relative in ["", "home/ada"] {
            assert_eq!(
                resolve("Sandbox", env(&[]), home(relative)),
                Err(PlatformError::NoHomeDirectory),
                "{relative:?}"
            );
        }
    }

    #[test]
    fn the_game_name_is_the_last_folder_and_logs_are_kept_with_the_data() {
        let found = resolve("My Game", env(&[]), home("/home/ada")).expect("a home");
        assert_eq!(found.config.file_name(), Some("My Game".as_ref()));
        assert_eq!(
            found.data.parent(),
            Some(Path::new("/home/ada/.local/share"))
        );
        assert_eq!(found.logs, found.data);
    }

    #[test]
    fn the_real_environment_gives_absolute_paths() {
        // Whatever this machine's environment is, a found folder is
        // absolute and ends with the game's name.
        if let Ok(found) = user_dirs("Studio", "Game") {
            for path in [&found.config, &found.data, &found.logs] {
                assert!(path.is_absolute(), "{}", path.display());
                assert!(path.ends_with("Game"), "{}", path.display());
            }
        }
    }
}
