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

//! `debug_ui.jsonc`, the debug UI's windows where they were left, in the
//! user config directory (ADR-0058):
//!
//! ```text
//! // Where the debug UI's windows were left (development builds).
//! {
//!   "version": 1,
//!   "windows": [
//!     { "name": "Sandbox", "x": 20, "y": 30, "w": 200, "h": 150, "collapsed": false }
//!   ]
//! }
//! ```
//!
//! A window's name is its explicit ID or its title, and its rectangle is in
//! the debug UI's units. The reader is lenient: a file that is not there
//! is no places; one that is too large, not JSONC, or of another version is
//! a warning and no places; a window without a name, a finite rectangle or
//! its collapse is left out, with a warning; members it does not know are
//! ignored. The file is written whole, to a temporary file beside it,
//! synced, then renamed over it, so that a crash leaves the old file or the
//! new.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use nadir_codec::Limits;
use nadir_codec::json::{self, Value};
use nadir_core::log::warn;
use nadir_core::{Rect, Vec2};
use nadir_debug::WindowPlace;

/// The file's name.
pub(crate) const FILE_NAME: &str = "debug_ui.jsonc";

/// The version written and read.
const VERSION: i64 = 1;

/// The largest file read: far more than the places of hundreds of windows.
const MAX_BYTES: u64 = 1024 * 1024;

/// A window's place, as the file keeps it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Place {
    pub(crate) name: String,
    pub(crate) rect: Rect,
    pub(crate) collapsed: bool,
}

/// The places in the file at `path`: none, after a warning, when it cannot
/// be read, and none, quietly, when it is not there.
pub(crate) fn read(path: &Path) -> Vec<Place> {
    let text = match read_text(path) {
        Ok(Some(text)) => text,
        Ok(None) => return Vec::new(),
        Err(cause) => {
            warn!(
                "the debug UI's windows start where the game puts them: {} cannot be read: {cause}",
                path.display()
            );
            return Vec::new();
        }
    };
    match parse(&text) {
        Ok((places, skipped)) => {
            for reason in skipped {
                warn!("{}: {reason}", path.display());
            }
            places
        }
        Err(reason) => {
            warn!(
                "the debug UI's windows start where the game puts them: {}: {reason}",
                path.display()
            );
            Vec::new()
        }
    }
}

/// The file's text, or `None` if it is not there.
fn read_text(path: &Path) -> Result<Option<String>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(format!("it is larger than {MAX_BYTES} bytes"));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| "it is not UTF-8".to_string())
}

/// The places in `text`, with the reasons for each window left out; or why
/// the whole file is not read.
pub(crate) fn parse(text: &str) -> Result<(Vec<Place>, Vec<String>), String> {
    let value = json::parse(text, Limits::default()).map_err(|error| error.to_string())?;
    if value.as_object().is_none() {
        return Err("it is not an object".to_string());
    }
    match value.get("version").and_then(Value::as_i64) {
        Some(VERSION) => {}
        Some(other) => {
            return Err(format!(
                "its version is {other}, and only {VERSION} is read"
            ));
        }
        None => return Err("it has no whole `version`".to_string()),
    }
    let Some(windows) = value.get("windows").and_then(Value::as_array) else {
        return Err("it has no `windows` list".to_string());
    };
    let mut places = Vec::with_capacity(windows.len());
    let mut skipped = Vec::new();
    for (index, window) in windows.iter().enumerate() {
        match place(window) {
            Some(place) => places.push(place),
            None => skipped.push(format!(
                "windows[{index}] is left out: it needs a `name`, finite `x`, `y`, `w` and `h`, \
                 and `collapsed`"
            )),
        }
    }
    Ok((places, skipped))
}

/// The place `window` describes, if it is whole.
fn place(window: &Value) -> Option<Place> {
    let name = window.get("name")?.as_str()?;
    let number = |key: &str| {
        let value = window.get(key)?.as_f64()? as f32;
        value.is_finite().then_some(value)
    };
    let min = Vec2::new(number("x")?, number("y")?);
    let size = Vec2::new(number("w")?, number("h")?);
    let collapsed = window.get("collapsed")?.as_bool()?;
    Some(Place {
        name: name.to_string(),
        rect: Rect::from_min_size(min, size),
        collapsed,
    })
}

/// The file's text for `places`.
pub(crate) fn to_text<'a>(places: impl Iterator<Item = WindowPlace<'a>>) -> String {
    let windows = places
        .map(|place| {
            let size = place.rect.size();
            let number = |value: f32| Value::Float(f64::from(value));
            Value::Object(vec![
                ("name".to_string(), Value::String(place.title.to_string())),
                ("x".to_string(), number(place.rect.min.x)),
                ("y".to_string(), number(place.rect.min.y)),
                ("w".to_string(), number(size.x)),
                ("h".to_string(), number(size.y)),
                ("collapsed".to_string(), Value::Bool(place.collapsed)),
            ])
        })
        .collect();
    let file = Value::Object(vec![
        ("version".to_string(), Value::Int(VERSION)),
        ("windows".to_string(), Value::Array(windows)),
    ]);
    format!(
        "// Where the debug UI's windows were left (development builds).\n{}\n",
        json::to_string_pretty(&file)
    )
}

/// Writes `text` to `path`, through a temporary file beside it, making its
/// folder if it is not there.
pub(crate) fn write(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut temporary = PathBuf::from(path);
    temporary.as_mut_os_string().push(".tmp");
    let mut file = File::create(&temporary)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::process;

    use nadir_core::{GenIndex, Handle};
    use nadir_debug::{DebugUi, TextMetrics};

    use super::*;

    struct Mono;

    impl TextMetrics for Mono {
        fn line_height(&self) -> f32 {
            8.0
        }

        fn caret_x(&self, text: &str, byte: usize) -> f32 {
            6.0 * text[..byte].chars().count() as f32
        }
    }

    fn handle<T>() -> Handle<T> {
        Handle::new(GenIndex::new(0, std::num::NonZeroU32::MIN))
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(Vec2::new(x, y), Vec2::new(w, h))
    }

    #[test]
    fn places_written_read_back_as_they_were() {
        let mut ui = DebugUi::new(handle(), Mono, handle());
        ui.set_place("Tools", rect(10.5, -4.0, 120.0, 80.0), false);
        ui.set_place("Stats##stats", rect(300.0, 20.0, 90.0, 60.0), true);
        let text = to_text(ui.places());
        assert!(text.starts_with("// Where"), "{text}");
        let (places, skipped) = parse(&text).expect("the file it wrote");
        assert!(skipped.is_empty());
        assert_eq!(
            places,
            [
                Place {
                    name: "Tools".to_string(),
                    rect: rect(10.5, -4.0, 120.0, 80.0),
                    collapsed: false,
                },
                Place {
                    name: "stats".to_string(),
                    rect: rect(300.0, 20.0, 90.0, 60.0),
                    collapsed: true,
                },
            ]
        );
    }

    #[test]
    fn a_window_not_whole_is_left_out_and_unknown_members_are_ignored() {
        let text = r#"
            // A comment, and members the reader does not know.
            { "version": 1, "theme": "dark", "windows": [
                { "name": "A", "x": 1, "y": 2, "w": 3.5, "h": 4, "collapsed": true, "pinned": 1 },
                { "name": "B", "x": 1, "y": 2, "w": 3, "collapsed": false },
                { "name": 7, "x": 1, "y": 2, "w": 3, "h": 4, "collapsed": false },
                { "name": "C", "x": null, "y": 2, "w": 3, "h": 4, "collapsed": false },
                { "name": "D", "x": 1, "y": 2, "w": 3, "h": 4, "collapsed": "no" },
                [] ] }
        "#;
        let (places, skipped) = parse(text).expect("a file it reads");
        assert_eq!(
            places,
            [Place {
                name: "A".to_string(),
                rect: rect(1.0, 2.0, 3.5, 4.0),
                collapsed: true,
            }]
        );
        assert_eq!(skipped.len(), 5, "{skipped:?}");
        assert!(
            skipped[0].starts_with("windows[1] is left out"),
            "{skipped:?}"
        );
    }

    #[test]
    fn a_file_of_another_shape_or_version_is_not_read() {
        for (text, reason) in [
            ("", "JSON"),
            ("[]", "it is not an object"),
            (r#"{ "windows": [] }"#, "no whole `version`"),
            (r#"{ "version": 1.0, "windows": [] }"#, "no whole `version`"),
            (r#"{ "version": 2, "windows": [] }"#, "its version is 2"),
            (r#"{ "version": 1 }"#, "no `windows` list"),
            (r#"{ "version": 1, "windows": {} }"#, "no `windows` list"),
        ] {
            let error = parse(text).expect_err(text);
            assert!(
                error.contains(reason) || reason == "JSON",
                "{text}: {error}"
            );
        }
    }

    #[test]
    fn the_file_is_written_with_its_folder_and_read_back() {
        let dir = env::temp_dir().join(format!("nadir-engine-places-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("config").join(FILE_NAME);
        // Not there yet: no places, and no file made.
        assert!(read(&path).is_empty());
        assert!(!path.exists());
        let mut ui = DebugUi::new(handle(), Mono, handle());
        ui.set_place("Tools", rect(1.0, 2.0, 100.0, 50.0), false);
        write(&path, &to_text(ui.places())).expect("written");
        assert_eq!(read(&path).len(), 1);
        // Written again over it, with no temporary file left.
        ui.set_place("More", rect(1.0, 2.0, 100.0, 50.0), true);
        write(&path, &to_text(ui.places())).expect("written again");
        assert_eq!(read(&path).len(), 2);
        let names: Vec<_> = fs::read_dir(path.parent().expect("a folder"))
            .expect("the folder")
            .map(|entry| entry.expect("an entry").file_name())
            .collect();
        assert_eq!(names, [FILE_NAME]);
        // Damaged, or too large: no places.
        fs::write(&path, "{ \"version\": 1, \"windows\": [").expect("damaged");
        assert!(read(&path).is_empty());
        fs::write(&path, vec![b' '; MAX_BYTES as usize + 1]).expect("large");
        assert!(read(&path).is_empty());
        fs::remove_dir_all(&dir).expect("remove the folder");
    }
}
