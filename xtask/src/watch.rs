// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! `cargo xtask cook --watch [--force]` (ADR-0042): cooks `assets/` into
//! `target/cooked/` again whenever a file in it changes, for a game that
//! reloads what changed.
//!
//! 1. Builds `nadir-cook` in release once, and runs that executable for
//!    every cook, so that no cook waits on cargo. A change to the cook's
//!    own code needs the watch started again.
//! 2. Cooks, with `--force` if asked: the first cook alone is forced.
//! 3. Every [`INTERVAL`], looks at what the cook's walk sees below
//!    `assets/`: files, folders and links, which are not followed, with
//!    names starting with `.` left out. A file or a link is known by its
//!    time of change and its length, compared for equality, since a time
//!    can go back (a checkout, a copy that keeps times). Something that
//!    cannot be read is marked so, and the next look tells; only
//!    `assets/` itself must be read.
//! 4. Once something changed since the last cook, and nothing more by the
//!    next look, cooks again, and says how long it took and how long since
//!    the change was seen. A cook that fails is printed, and the watch goes
//!    on: the next change cooks again.
//!
//! It runs until stopped, with Ctrl+C. That leaves no output half written:
//! the cook writes each through a file it then renames.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crate::error::Error;
use crate::{cargo, site};

/// How often the source folder is looked at.
pub(crate) const INTERVAL: Duration = Duration::from_millis(100);

/// The cargo arguments that build the cook.
const BUILD: &[&str] = &[
    "build",
    "--release",
    "--package",
    "nadir-cook",
    "--message-format",
    "json-render-diagnostics",
];

/// What a path below the folder watched is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A file, or anything else that is not a folder or a link, which the
    /// cook takes as a file.
    File,
    /// A folder: what it holds is looked at too. One that comes or goes is
    /// a change, since the cook refuses a folder whose name differs from
    /// another's only in case, even an empty one.
    Folder,
    /// A link, which is not followed: the cook refuses one it would read.
    Link,
    /// Something that could not be read on this look, such as a folder
    /// taken away while it was looked at.
    Unread,
}

/// A path as it was seen: what it is, when it last changed, if the system
/// says, and its length. A folder's are left out, as what it holds is
/// looked at.
type Stamp = (Kind, Option<SystemTime>, u64);

/// What is below a folder, by path.
pub(crate) type Snapshot = BTreeMap<PathBuf, Stamp>;

/// Runs `cargo xtask cook --watch` on the workspace at `root`, until
/// stopped ([module docs](self)).
///
/// # Errors
///
/// The cook cannot be built or run, or `assets/` cannot be read. A cook
/// that runs and fails is not an error.
pub(crate) fn command(force: bool, root: &Path, out: &mut dyn Write) -> Result<bool, Error> {
    let source = root.join("assets");
    say(out, "cook --watch: building nadir-cook in release")?;
    let messages = cargo::capture(root, BUILD)?;
    let cook =
        site::executable(&messages, "nadir-cook").ok_or_else(|| Error::UnexpectedOutput {
            command: cargo::display(BUILD),
            detail: "no executable named `nadir-cook` among the artifacts".to_string(),
        })?;
    // What the first cook sees is taken before it runs, so that a change
    // made while it runs cooks again.
    let mut watch = Watch::new(snapshot(&source)?);
    cook_once(&cook, root, force, None, out)?;
    say(
        out,
        &format!(
            "cook --watch: watching {} every {} ms; Ctrl+C stops",
            source.display(),
            INTERVAL.as_millis()
        ),
    )?;
    loop {
        thread::sleep(INTERVAL);
        if let Some(seen) = watch.poll(snapshot(&source)?, Instant::now()) {
            cook_once(&cook, root, false, Some(seen), out)?;
        }
    }
}

/// Runs the executable `cook` on the workspace's `assets/` and
/// `target/cooked/`, which prints to the terminal, then says how it went,
/// with how long since the change it cooks was `seen`.
fn cook_once(
    cook: &Path,
    root: &Path,
    force: bool,
    seen: Option<Instant>,
    out: &mut dyn Write,
) -> Result<(), Error> {
    let mut command = Command::new(cook);
    command.current_dir(root);
    if force {
        command.arg("--force");
    }
    command.args(["assets", "target/cooked"]);
    let started = Instant::now();
    let result = site::run(&mut command);
    let took = started.elapsed().as_secs_f64();
    let line = match (result, seen) {
        (Ok(()), None) => format!("cook --watch: cooked in {took:.2} s"),
        (Ok(()), Some(seen)) => format!(
            "cook --watch: cooked in {took:.2} s, {:.2} s after the change was seen",
            seen.elapsed().as_secs_f64()
        ),
        (Err(error @ Error::CommandFailed { .. }), _) => {
            format!("cook --watch: {error}; watching still")
        }
        (Err(error), _) => return Err(error),
    };
    say(out, &line)
}

/// Writes `line` to `out` at once, before a cook prints to the terminal.
fn say(out: &mut dyn Write, line: &str) -> Result<(), Error> {
    writeln!(out, "{line}")
        .and_then(|()| out.flush())
        .map_err(Error::Output)
}

/// What the cook's walk sees below `folder`: files, folders, and links,
/// not followed, with names starting with `.` left out. What goes while it
/// is looked at is left out, and what cannot be read is marked
/// [`Kind::Unread`], so that the next look tells whether it was a moment's
/// change.
///
/// # Errors
///
/// `folder` itself cannot be read.
pub(crate) fn snapshot(folder: &Path) -> Result<Snapshot, Error> {
    let mut seen = Snapshot::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(dir) = folders.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(source) if dir == folder => return Err(Error::Read { path: dir, source }),
            Err(error) => {
                unread(&mut seen, dir, &error);
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    unread(&mut seen, dir.clone(), &error);
                    break;
                }
            };
            if entry.file_name().as_encoded_bytes().starts_with(b".") {
                continue;
            }
            let path = entry.path();
            let kind = match entry.file_type() {
                Ok(kind) if kind.is_dir() => Kind::Folder,
                Ok(kind) if kind.is_symlink() => Kind::Link,
                Ok(_) => Kind::File,
                Err(error) => {
                    unread(&mut seen, path, &error);
                    continue;
                }
            };
            if kind == Kind::Folder {
                seen.insert(path.clone(), (kind, None, 0));
                folders.push(path);
                continue;
            }
            // A link's own, not what it names.
            match entry.metadata() {
                Ok(metadata) => {
                    seen.insert(path, (kind, metadata.modified().ok(), metadata.len()));
                }
                Err(error) => unread(&mut seen, path, &error),
            }
        }
    }
    Ok(seen)
}

/// Marks `path` as not read on this look, for `error`, unless it went.
fn unread(seen: &mut Snapshot, path: PathBuf, error: &io::Error) {
    if error.kind() != ErrorKind::NotFound {
        seen.insert(path, (Kind::Unread, None, 0));
    }
}

/// When to cook, from what each look at the files saw.
pub(crate) struct Watch {
    /// What the last cook saw.
    cooked: Snapshot,
    /// What the last look saw, when it differs from that, and when a
    /// change was first seen.
    changed: Option<(Snapshot, Instant)>,
}

impl Watch {
    /// A watch whose last cook saw `cooked`.
    pub(crate) fn new(cooked: Snapshot) -> Self {
        Self {
            cooked,
            changed: None,
        }
    }

    /// Takes what a look at `at` saw. A cook is due when the files differ
    /// from what the last cook saw and are as the look before saw them:
    /// then this gives when the change was first seen, and that cook sees
    /// the files as this look did. Files changed and put back before the
    /// next look cook nothing.
    pub(crate) fn poll(&mut self, files: Snapshot, at: Instant) -> Option<Instant> {
        if files == self.cooked {
            self.changed = None;
            return None;
        }
        match self.changed.take() {
            Some((before, seen)) if before == files => {
                self.cooked = files;
                Some(seen)
            }
            Some((_, seen)) => {
                self.changed = Some((files, seen));
                None
            }
            None => {
                self.changed = Some((files, at));
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Files of the lengths given, with no times.
    fn files(lengths: &[(&str, u64)]) -> Snapshot {
        lengths
            .iter()
            .map(|&(name, length)| (PathBuf::from(name), (Kind::File, None, length)))
            .collect()
    }

    #[test]
    fn a_cook_is_due_once_a_change_is_still_there_a_look_later() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut watch = Watch::new(files(&[("hero.png", 1)]));
        assert_eq!(watch.poll(files(&[("hero.png", 1)]), at(100)), None);

        // Due a look after the change, and timed from when it was seen.
        assert_eq!(watch.poll(files(&[("hero.png", 2)]), at(200)), None);
        assert_eq!(
            watch.poll(files(&[("hero.png", 2)]), at(300)),
            Some(at(200))
        );
        // That cook saw it, so it is not due again.
        assert_eq!(watch.poll(files(&[("hero.png", 2)]), at(400)), None);

        // An export saved as its sheet, then its data: one cook, once both
        // are still, timed from the first.
        assert_eq!(watch.poll(files(&[("hero.png", 3)]), at(500)), None);
        let both = files(&[("hero.json", 7), ("hero.png", 3)]);
        assert_eq!(watch.poll(both.clone(), at(600)), None);
        assert_eq!(watch.poll(both.clone(), at(700)), Some(at(500)));

        // Changed and put back between two looks: nothing to cook.
        assert_eq!(
            watch.poll(files(&[("hero.json", 8), ("hero.png", 3)]), at(800)),
            None
        );
        assert_eq!(watch.poll(both.clone(), at(900)), None);
        assert_eq!(watch.poll(both, at(1000)), None);

        // A file that goes is a change.
        assert_eq!(watch.poll(files(&[("hero.png", 3)]), at(1100)), None);
        assert_eq!(
            watch.poll(files(&[("hero.png", 3)]), at(1200)),
            Some(at(1100))
        );
    }

    #[test]
    fn a_look_sees_what_the_cook_s_walk_sees() {
        let dir = std::env::temp_dir().join(format!("nadir-xtask-watch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sprites")).unwrap();
        fs::create_dir_all(dir.join(".git")).unwrap();
        let hero = dir.join("sprites/hero.png");
        fs::write(&hero, b"hero").unwrap();
        // What the cook does not read: hidden files and folders.
        fs::write(dir.join(".hero.png.swp"), b"swap").unwrap();
        fs::write(dir.join(".git/index"), b"index").unwrap();
        // Times long past, set, so that a change shows on any clock.
        let long_ago = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let set_time = |path: &Path, time| {
            fs::File::options()
                .write(true)
                .open(path)
                .and_then(|file| file.set_modified(time))
                .unwrap();
        };
        let folder = |path: PathBuf| (path, (Kind::Folder, None, 0));
        let file = |path: &Path, length| (path.to_path_buf(), (Kind::File, Some(long_ago), length));
        set_time(&hero, long_ago);
        let first = snapshot(&dir).unwrap();
        assert_eq!(
            first,
            Snapshot::from([folder(dir.join("sprites")), file(&hero, 4)])
        );

        // A new time alone is a change.
        set_time(&hero, long_ago + Duration::from_secs(1));
        assert_ne!(snapshot(&dir).unwrap(), first);
        // So is a new length at the same time, as a coarse clock can give.
        fs::write(&hero, b"a hero").unwrap();
        set_time(&hero, long_ago);
        let second = snapshot(&dir).unwrap();
        assert_eq!(
            second,
            Snapshot::from([folder(dir.join("sprites")), file(&hero, 6)])
        );
        // And a file added, in new folders.
        fs::create_dir_all(dir.join("data/prefabs")).unwrap();
        let tree = dir.join("data/prefabs/tree.jsonc");
        fs::write(&tree, b"{}").unwrap();
        set_time(&tree, long_ago);
        assert_eq!(
            snapshot(&dir).unwrap(),
            Snapshot::from([
                folder(dir.join("data")),
                folder(dir.join("data/prefabs")),
                file(&tree, 2),
                folder(dir.join("sprites")),
                file(&hero, 6),
            ])
        );
        // An empty folder that comes is a change, and one that goes too,
        // since the cook refuses one named as another but for case, such
        // as `Sprites` beside `sprites`, empty or not. (Not made here: a
        // system that ignores case would not make it.)
        fs::remove_dir_all(dir.join("data")).unwrap();
        assert_eq!(snapshot(&dir).unwrap(), second);
        fs::create_dir(dir.join("fonts")).unwrap();
        let with_folder = snapshot(&dir).unwrap();
        assert_ne!(with_folder, second);
        assert_eq!(
            with_folder.get(&dir.join("fonts")),
            Some(&(Kind::Folder, None, 0))
        );
        fs::remove_dir(dir.join("fonts")).unwrap();
        assert_eq!(snapshot(&dir).unwrap(), second);

        // A folder that is not there cannot be watched.
        assert!(matches!(
            snapshot(&dir.join("none")),
            Err(Error::Read { .. })
        ));
        fs::remove_dir_all(&dir).unwrap();
    }
}
