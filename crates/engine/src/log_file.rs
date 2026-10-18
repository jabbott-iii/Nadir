//! The log file, in the game's logs folder.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use nadir_core::log::{self, FileConfig, info, warn};
use nadir_platform::user_dirs;

/// The log file's name.
const FILE_NAME: &str = "nadir.log";

/// Each run starts a new file, and the files of the three runs before it
/// are kept as `nadir.log.1` to `nadir.log.3`. A run that writes more than
/// 4 MiB goes on in a new file, which also moves the older ones along.
const ROTATION: FileConfig = FileConfig {
    max_bytes: 4 * 1024 * 1024,
    keep: 3,
    rotate_on_open: true,
};

/// Starts writing the log to [`FILE_NAME`] in the logs folder of `game`,
/// made by `studio`. If that fails, the game goes on with no log file, after
/// a warning that says why.
pub(crate) fn start(studio: &str, game: &str) {
    let dirs = match user_dirs(studio, game) {
        Ok(dirs) => dirs,
        Err(error) => {
            warn!("no log file: {error}");
            return;
        }
    };
    match open_in(&dirs.logs) {
        Ok(path) => info!("logging to {}", path.display()),
        Err(error) => warn!(
            "no log file: could not open one in {}: {error}",
            dirs.logs.display()
        ),
    }
}

/// Creates `dir` if it does not exist, and starts writing the log to
/// [`FILE_NAME`] in it. Returns the file's path.
fn open_in(dir: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let path = dir.join(FILE_NAME);
    log::open_file(&path, ROTATION)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::process;

    use super::*;

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
    }

    // The logger belongs to the whole process, so this is the only test in
    // the crate that opens a log file.
    #[test]
    fn the_log_file_is_created_with_its_folder_and_keeps_the_last_three_runs() {
        let root = env::temp_dir().join(format!("nadir-engine-log-{}", process::id()));
        let dir = root.join("Studio").join("Game");
        log::set_stderr(false);
        for run in 1..=5 {
            let path = open_in(&dir).expect("open the log file");
            assert_eq!(path, dir.join("nadir.log"));
            info!("written by run {run}");
        }
        log::close_file();
        log::set_stderr(true);

        let mut names: Vec<String> = fs::read_dir(&dir)
            .expect("list the log folder")
            .map(|entry| {
                let entry = entry.expect("read a folder entry");
                entry.file_name().to_string_lossy().into_owned()
            })
            .collect();
        names.sort();
        assert_eq!(
            names,
            ["nadir.log", "nadir.log.1", "nadir.log.2", "nadir.log.3"]
        );
        for (name, run) in [
            ("nadir.log", 5),
            ("nadir.log.1", 4),
            ("nadir.log.2", 3),
            ("nadir.log.3", 2),
        ] {
            let text = read(&dir.join(name));
            assert!(
                text.contains(&format!("written by run {run}")),
                "{name}: {text}"
            );
            assert_eq!(text.matches("written by run").count(), 1, "{name}: {text}");
        }

        fs::remove_dir_all(&root).expect("remove the test folder");
    }
}
