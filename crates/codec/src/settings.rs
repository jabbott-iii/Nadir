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

//! The settings file, `settings.jsonc` in the game's config folder: what a
//! player chose in the game's settings menus, kept from one run to the
//! next (`docs/subsystems/assets-and-codecs.md`, "Settings file reader and
//! writer"; ADR-0077). The facade, nadir_engine, reads the file as the game starts
//! and writes it as the settings change. This module turns its text into
//! [`Settings`] with [`read`], and [`Settings`] back into text with
//! [`write`](fn@write).
//!
//! ```text
//! {
//!   "version": 1,
//!   "display": { "fullscreen": false, "vsync": true },
//!   "audio": { "music": 100, "sfx": 80, "ui": 100, "ambience": 60 },
//!   "ui": { "scale": null, "language": "en" },
//!   "input": {
//!     "deadzones": { "stick_inner": 0.2, "stick_outer": 0.95, "trigger": 0.1,
//!                    "press": 0.5, "release": 0.4 },
//!     "bindings": {
//!       "move": [{ "keys": { "up": "W", "down": "S", "left": "A", "right": "D" } },
//!                { "stick": "Left" }, null, null],
//!       "shoot": [{ "pad_axis": "RightTrigger", "sign": "+" }, { "mouse": "Left" }]
//!     }
//!   },
//!   "log": { "level": "info", "targets": { "nadir_render": "debug" } }
//! }
//! ```
//!
//! # The settings
//!
//! | Setting | Takes |
//! | :- | :- |
//! | `display.fullscreen`, `display.vsync` | `true` or `false` |
//! | `audio.music`, `audio.sfx`, `audio.ui`, `audio.ambience` | a bus's volume in percent: a whole number from 0 to 100 |
//! | `ui.scale` | `null`, for a scale from the window's scale factor, or a number from 1/8 to 64 ([`MIN_UI_SCALE`], [`MAX_UI_SCALE`]) |
//! | `ui.language` | a language tag: 1 to 35 ASCII letters, digits and `-` ([`is_language`]) |
//! | `input.deadzones.stick_inner`, `stick_outer`, `trigger`, `press`, `release` | a number from 0 to 1 |
//! | `input.bindings.<action>` | an array of at most [`SLOTS`] slots, each `null` (empty) or a binding (below); slots past its end are empty |
//! | `log.level` | the level of every target without one of its own: `"error"`, `"warn"`, `"info"`, `"debug"` or `"trace"` |
//! | `log.targets.<module path>` | that target's level, a name as above, for a module path such as `nadir_render::batch` ([`is_module_path`]) |
//!
//! The deadzones' rules between each other, such as `stick_inner` below
//! `stick_outer`, are nadir-input's to check: this module checks each
//! number alone.
//!
//! # Bindings
//!
//! A binding is an object whose member says what it is. Keys, buttons and
//! axes go by their names in nadir-core ([`Key::name`],
//! [`MouseButton::name`], [`GamepadButton::name`], [`GamepadAxis::name`]),
//! which are case-sensitive.
//!
//! | Binding | Object |
//! | :- | :- |
//! | a key | `{"key": "W"}` |
//! | a mouse button | `{"mouse": "Left"}` |
//! | four keys that give a direction | `{"keys": {"up": "W", "down": "S", "left": "A", "right": "D"}}` |
//! | a pad's button | `{"pad": "A"}` |
//! | half of a pad's axis | `{"pad_axis": "RightTrigger", "sign": "+"}`, or `"-"`, which a trigger does not have |
//! | a pad's stick | `{"stick": "Left"}` or `"Right"` |
//!
//! Which bindings an action's kind takes is the game's to check: this
//! module does not know the kinds.
//!
//! # Reading: each setting falls back alone
//!
//! A player may edit the file by hand, and another version of the game may
//! have written it, so [`read`] is lenient. It starts from the game's
//! defaults and takes from the file each setting it can:
//!
//! - **The file as a whole** is refused, with a [`CodecError`], if it is
//!   larger than [`MAX_BYTES`], is not JSONC, is not an object, or has no
//!   `version` that is a whole number of at least 1
//!   ([`ErrorKind::NoVersion`]); and if its version is newer than
//!   [`VERSION`] ([`ErrorKind::NewerVersion`]). The game then uses its
//!   defaults.
//! - **A missing setting or section** keeps its default, quietly.
//! - **A setting of the wrong type or out of its range** keeps its default,
//!   with a [`Warning`] ([`WarningKind::Invalid`]). So does each setting of
//!   a section that is not an object.
//! - **A binding that is not one**, in a slot, keeps the default's binding
//!   in that slot; an action whose value is not an array of at most
//!   [`SLOTS`] keeps all its defaults.
//! - **A member that is not a setting**, at the top, in a section or in
//!   `deadzones`, is a [`WarningKind::Unknown`] warning, and stays in the
//!   source for [`write`](fn@write) to keep. So are the bindings of an
//!   action the defaults do not have ([`WarningKind::UnknownAction`]): a
//!   game's that renamed the action, or another game's.
//! - **A log target** that is not a module path, or whose level is not a
//!   level's name, is left out ([`WarningKind::LeftOut`]).
//!
//! # Writing: what it does not know stays
//!
//! [`write`](fn@write) writes settings over the value the file was read
//! into: each setting replaces its member where it is, a missing one is
//! added at the end of its object, and every member it does not know stays
//! where it was. A member of a known setting that held something wrong is
//! written over with the setting's value. Comments are not in the value
//! tree, so the file's own are lost; [`write`](fn@write) starts the file
//! with [`HEADER`].

use std::fmt;

use nadir_core::log::Level;
use nadir_core::{GamepadAxis, GamepadButton, Key, MouseButton};

use crate::json::{self, Value};
use crate::{CodecError, Limits};

/// The version [`write`](fn@write) writes, and the newest [`read`] reads.
pub const VERSION: i64 = 1;

/// How many slots an action has: as many as nadir-input's `ActionMap`,
/// whose controls menus show them as columns.
pub const SLOTS: usize = 4;

/// The largest file [`read`] reads: far more than the settings of a game
/// with thousands of actions.
pub const MAX_BYTES: usize = 1024 * 1024;

/// The smallest UI scale the file takes: 1/8, where nadir-render's
/// `UiScale::Free` holds a smaller one.
pub const MIN_UI_SCALE: f64 = 0.125;

/// The largest UI scale the file takes: 64, where nadir-render's
/// `UiScale::Free` holds a larger one.
pub const MAX_UI_SCALE: f64 = 64.0;

/// The longest language tag the file takes, in bytes: room for a language,
/// a script and a region, as in `zh-Hant-TW`, and more.
pub const MAX_LANGUAGE_BYTES: usize = 35;

/// The longest log target the file takes, in bytes.
pub const MAX_TARGET_BYTES: usize = 256;

/// The comment [`write`](fn@write) starts the file with.
pub const HEADER: &str = "// The game's settings. The game reads this file as it starts, and\n\
                          // writes it again when a setting changes: it keeps the members it does\n\
                          // not know, but not comments.\n";

// The messages below say "at most 4".
const _: () = assert!(SLOTS == 4);

/// Every setting the file holds, by its sections.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// The window.
    pub display: Display,
    /// The mixer's buses.
    pub audio: Audio,
    /// The interface.
    pub ui: Ui,
    /// The gamepads' deadzones and the actions' bindings.
    pub input: Input,
    /// What the log records.
    pub log: Log,
}

/// The `display` section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Display {
    /// Whether the window fills the screen, in borderless fullscreen.
    pub fullscreen: bool,
    /// Whether each frame waits for the display's refresh.
    pub vsync: bool,
}

/// The `audio` section: each bus's volume, in percent, from 0 to 100.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Audio {
    /// The music's bus.
    pub music: u8,
    /// The sound effects' bus.
    pub sfx: u8,
    /// The interface's sounds' bus.
    pub ui: u8,
    /// The ambient loops' bus.
    pub ambience: u8,
}

/// The `ui` section.
#[derive(Clone, Debug, PartialEq)]
pub struct Ui {
    /// The free UI scale, in output pixels per UI unit, from
    /// [`MIN_UI_SCALE`] to [`MAX_UI_SCALE`]; or `None`, for one from the
    /// window's scale factor.
    pub scale: Option<f64>,
    /// The language's tag, which [`is_language`] takes.
    pub language: String,
}

/// The `input` section.
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    /// Where the pads' sticks and triggers start to count.
    pub deadzones: Deadzones,
    /// Each action's bindings, by its name, in the order of the game's
    /// actions.
    pub bindings: Vec<ActionBindings>,
}

/// The `input.deadzones` section: each a number from 0 to 1, as
/// nadir-input's `Deadzones` has them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Deadzones {
    /// How far from the center a stick must be to count.
    pub stick_inner: f64,
    /// How far from the center a stick reads as length 1.
    pub stick_outer: f64,
    /// How far a trigger must be pulled to count.
    pub trigger: f64,
    /// Where a half of an axis bound to a button presses.
    pub press: f64,
    /// Where it lets go again.
    pub release: f64,
}

/// One action's bindings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionBindings {
    /// The action's name, as the game's action set names it.
    pub action: String,
    /// Its slots, `None` where one is empty.
    pub slots: [Option<Binding>; SLOTS],
}

/// A binding, as the file writes it (see the module's docs, "Bindings").
/// The facade turns it into nadir-input's `Binding`, which has the same
/// cases.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Binding {
    /// A key: `{"key": "W"}`.
    Key(Key),
    /// A mouse button: `{"mouse": "Left"}`.
    Mouse(MouseButton),
    /// Four keys that give a direction:
    /// `{"keys": {"up": "W", "down": "S", "left": "A", "right": "D"}}`.
    Keys {
        /// The key for up.
        up: Key,
        /// The key for down.
        down: Key,
        /// The key for left.
        left: Key,
        /// The key for right.
        right: Key,
    },
    /// A pad's button: `{"pad": "A"}`.
    Pad(GamepadButton),
    /// One half of a pad's axis:
    /// `{"pad_axis": "RightTrigger", "sign": "+"}`. A trigger's is always
    /// [`Sign::Positive`].
    PadAxis {
        /// The axis.
        axis: GamepadAxis,
        /// Which half of it.
        sign: Sign,
    },
    /// A pad's stick: `{"stick": "Left"}`.
    Stick(Stick),
}

/// Which half of an axis a [`Binding::PadAxis`] reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sign {
    /// Toward -1, `"-"`: a stick pushed left or up.
    Negative,
    /// Toward 1, `"+"`: a stick pushed right or down, or a trigger pulled.
    Positive,
}

/// One of a pad's two sticks, for [`Binding::Stick`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stick {
    /// `"Left"`.
    Left,
    /// `"Right"`.
    Right,
}

/// The `log` section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Log {
    /// The level of every target without one of its own.
    pub level: Level,
    /// Each target's own level, by its module path, in the file's order.
    pub targets: Vec<(String, Level)>,
}

/// What [`read`] read: the settings, what it did not take and why, and the
/// file's value tree, for [`write`](fn@write) to keep what it does not know.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    /// The settings: the file's where it gave them, the defaults' where it
    /// did not.
    pub settings: Settings,
    /// Each member it did not take, or kept without knowing it, in the
    /// file's order.
    pub warnings: Vec<Warning>,
    /// The file as it was read.
    pub source: Value,
}

/// A member of the file that [`read`] did not take as it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// Where it is, such as `audio.music` or `input.bindings.move[1]`.
    pub at: String,
    /// What is wrong, and what was done.
    pub kind: WarningKind,
}

/// What is wrong with a member, and what [`read`] did about it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WarningKind {
    /// It is not a setting: it is kept as it is.
    Unknown,
    /// It names an action the game does not have: its bindings are kept as
    /// they are.
    UnknownAction,
    /// It is not what its setting takes: the setting keeps its default.
    Invalid {
        /// What the setting takes.
        expected: &'static str,
    },
    /// A log target that is not a module path, or with no level's name: it
    /// is left out.
    LeftOut {
        /// What a target takes.
        expected: &'static str,
    },
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // A member's name comes from the file, so control characters in it
        // are escaped, not written into the log.
        let at = self.at.escape_debug();
        match self.kind {
            WarningKind::Unknown => write!(f, "`{at}` is not a setting: it is kept as it is"),
            WarningKind::UnknownAction => write!(
                f,
                "`{at}` is not one of the game's actions: its bindings are kept as they are"
            ),
            WarningKind::Invalid { expected } => {
                write!(f, "`{at}` is not {expected}: the setting keeps its default")
            }
            WarningKind::LeftOut { expected } => {
                write!(f, "`{at}` is not {expected}: it is left out")
            }
        }
    }
}

/// Why [`read`] refused the whole file. See [`CodecError::Settings`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The text is longer than [`MAX_BYTES`].
    TooLarge {
        /// [`MAX_BYTES`].
        limit: usize,
    },
    /// The text is JSONC, but not an object.
    NotAnObject,
    /// It has no `version`, or one that is not a whole number of at least
    /// 1.
    NoVersion,
    /// Its version is newer than [`VERSION`]: a newer game wrote it.
    NewerVersion {
        /// The file's version.
        found: i64,
    },
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { limit } => write!(f, "it is larger than {limit} bytes"),
            Self::NotAnObject => write!(f, "it is not an object"),
            Self::NoVersion => write!(f, "it has no `version` that is a whole number of 1 or more"),
            Self::NewerVersion { found } => write!(
                f,
                "its version is {found}, and this game reads {VERSION} and older"
            ),
        }
    }
}

/// Reads the settings file's `text` over `defaults`: each setting the file
/// gives, and the defaults' where it gives none or one that is wrong
/// (the module's docs, "Reading"). `defaults.input.bindings` names the
/// game's actions; the file's bindings of any other action are kept in the
/// source alone.
///
/// # Errors
///
/// - [`CodecError::Json`] if the text is not JSONC, or breaks `limits`;
/// - [`CodecError::Settings`] if it is too large, is not an object, or has
///   no version this module reads.
pub fn read(text: &str, defaults: &Settings, limits: Limits) -> Result<Loaded, CodecError> {
    let refuse = |kind| Err(CodecError::Settings { kind });
    if text.len() > MAX_BYTES {
        return refuse(ErrorKind::TooLarge { limit: MAX_BYTES });
    }
    let source = json::parse(text, limits)?;
    let Some(members) = source.as_object() else {
        return refuse(ErrorKind::NotAnObject);
    };
    match source.get("version").and_then(Value::as_i64) {
        Some(VERSION) => {}
        Some(found) if found > VERSION => return refuse(ErrorKind::NewerVersion { found }),
        _ => return refuse(ErrorKind::NoVersion),
    }
    let mut reader = Reader::default();
    let mut settings = defaults.clone();
    for (name, value) in members {
        match name.as_str() {
            "version" => {}
            "display" => reader.display(value, &mut settings.display),
            "audio" => reader.audio(value, &mut settings.audio),
            "ui" => reader.ui(value, &mut settings.ui),
            "input" => reader.input(value, &mut settings.input),
            "log" => reader.log(value, &mut settings.log),
            _ => reader.warn(name.clone(), WarningKind::Unknown),
        }
    }
    Ok(Loaded {
        settings,
        warnings: reader.warnings,
        source,
    })
}

/// The settings file's text for `settings`, written over `source`, the
/// value [`read`] read the file into, if there was one: what `source` holds
/// that is not a setting stays (the module's docs, "Writing"). It starts
/// with [`HEADER`] and ends with a line break.
///
/// Written one member to a line, a file grows: one that keeping what
/// `source` holds would make larger than [`MAX_BYTES`], which [`read`]
/// refuses, is written without it, from the settings alone.
pub fn write(settings: &Settings, source: Option<&Value>) -> String {
    let text = write_over(settings, source);
    if text.len() > MAX_BYTES && source.is_some() {
        return write_over(settings, None);
    }
    text
}

/// [`write`]'s text, over `source` whatever its length.
fn write_over(settings: &Settings, source: Option<&Value>) -> String {
    let mut root = match source {
        Some(Value::Object(members)) => members.clone(),
        _ => Vec::new(),
    };
    set(&mut root, "version", Value::Int(VERSION));
    let display = settings.display;
    edit(&mut root, "display", |members| {
        set(members, "fullscreen", Value::Bool(display.fullscreen));
        set(members, "vsync", Value::Bool(display.vsync));
    });
    let audio = settings.audio;
    edit(&mut root, "audio", |members| {
        for (name, percent) in [
            ("music", audio.music),
            ("sfx", audio.sfx),
            ("ui", audio.ui),
            ("ambience", audio.ambience),
        ] {
            set(members, name, Value::Int(i64::from(percent)));
        }
    });
    let ui = &settings.ui;
    edit(&mut root, "ui", |members| {
        set(members, "scale", ui.scale.map_or(Value::Null, Value::Float));
        set(members, "language", Value::String(ui.language.clone()));
    });
    let input = &settings.input;
    edit(&mut root, "input", |members| {
        let deadzones = input.deadzones;
        edit(members, "deadzones", |members| {
            for (name, value) in [
                ("stick_inner", deadzones.stick_inner),
                ("stick_outer", deadzones.stick_outer),
                ("trigger", deadzones.trigger),
                ("press", deadzones.press),
                ("release", deadzones.release),
            ] {
                set(members, name, Value::Float(value));
            }
        });
        edit(members, "bindings", |members| {
            for action in &input.bindings {
                let slots = action
                    .slots
                    .iter()
                    .map(|slot| slot.as_ref().map_or(Value::Null, binding_value))
                    .collect();
                set(members, &action.action, Value::Array(slots));
            }
        });
    });
    let log = &settings.log;
    edit(&mut root, "log", |members| {
        set(
            members,
            "level",
            Value::String(level_name(log.level).to_string()),
        );
        let targets = log
            .targets
            .iter()
            .map(|(target, level)| {
                (
                    target.clone(),
                    Value::String(level_name(*level).to_string()),
                )
            })
            .collect();
        set(members, "targets", Value::Object(targets));
    });
    format!("{HEADER}{}\n", json::to_string_pretty(&Value::Object(root)))
}

/// Whether `text` is a language tag the file takes: 1 to
/// [`MAX_LANGUAGE_BYTES`] ASCII letters, digits and `-`, as in `en`,
/// `pt-BR` or `zh-Hant-TW`. That it names a language the game has is the
/// game's to check.
pub fn is_language(text: &str) -> bool {
    (1..=MAX_LANGUAGE_BYTES).contains(&text.len())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// Whether `text` is a log target the file takes: a module path of 1 to
/// [`MAX_TARGET_BYTES`] bytes, its parts ASCII letters, digits and `_`,
/// each one at least, joined by `::`, as in `nadir_render::batch`.
pub fn is_module_path(text: &str) -> bool {
    (1..=MAX_TARGET_BYTES).contains(&text.len())
        && text.split("::").all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
}

/// The name the file gives `level`: `"error"`, `"warn"`, `"info"`,
/// `"debug"` or `"trace"`.
pub const fn level_name(level: Level) -> &'static str {
    match level {
        Level::Error => "error",
        Level::Warn => "warn",
        Level::Info => "info",
        Level::Debug => "debug",
        Level::Trace => "trace",
    }
}

/// The level that [`level_name`] names `name`, if one does.
pub fn level_from_name(name: &str) -> Option<Level> {
    [
        Level::Error,
        Level::Warn,
        Level::Info,
        Level::Debug,
        Level::Trace,
    ]
    .into_iter()
    .find(|&level| level_name(level) == name)
}

/// What a setting of each type takes, for its warning.
const A_BOOL: &str = "`true` or `false`";
const A_VOLUME: &str = "a whole number from 0 to 100";
const A_SCALE: &str = "`null`, or a number from 0.125 to 64";
const A_LANGUAGE: &str = "a language tag: 1 to 35 ASCII letters, digits and `-`";
const A_DEADZONE: &str = "a number from 0 to 1";
const AN_OBJECT: &str = "an object";
const SLOTS_ARRAY: &str = "an array of at most 4 slots, each `null` or a binding";
const A_LEVEL: &str = "a level's name: `error`, `warn`, `info`, `debug` or `trace`";
const A_TARGET: &str = "a module path, such as `nadir_render::batch`, with a level's name";
const A_BINDING: &str = "`null` or a binding: an object of `key`, `mouse`, `keys`, `pad`, \
                         `pad_axis` and `sign`, or `stick`";
const A_KEY: &str = "a binding of a key's name, such as `{\"key\": \"W\"}`";
const A_MOUSE: &str = "a binding of a mouse button's name: `Left`, `Right`, `Middle`, `Back` \
                       or `Forward`";
const FOUR_KEYS: &str = "a binding of four keys' names, `up`, `down`, `left` and `right`";
const A_PAD: &str = "a binding of a pad button's name, such as `{\"pad\": \"A\"}`";
const A_PAD_AXIS: &str = "a binding of a pad axis's name, such as `LeftX` or `RightTrigger`, \
                          and a `sign` of `+` or `-`, which a trigger does not have";
const A_STICK: &str = "a binding of a stick: `Left` or `Right`";

/// The warnings so far.
#[derive(Default)]
struct Reader {
    warnings: Vec<Warning>,
}

impl Reader {
    fn warn(&mut self, at: String, kind: WarningKind) {
        self.warnings.push(Warning { at, kind });
    }

    fn invalid(&mut self, at: String, expected: &'static str) {
        self.warn(at, WarningKind::Invalid { expected });
    }

    /// The members of the section at `at`, if it is an object; a warning if
    /// it is not.
    fn section<'v>(&mut self, at: &str, value: &'v Value) -> Option<&'v [(String, Value)]> {
        let members = value.as_object();
        if members.is_none() {
            self.invalid(at.to_string(), AN_OBJECT);
        }
        members
    }

    /// Sets `out` to `value` if `take` takes it, or says why not.
    fn take<T>(
        &mut self,
        at: String,
        value: &Value,
        out: &mut T,
        expected: &'static str,
        take: impl FnOnce(&Value) -> Option<T>,
    ) {
        match take(value) {
            Some(taken) => *out = taken,
            None => self.invalid(at, expected),
        }
    }

    fn display(&mut self, value: &Value, out: &mut Display) {
        let Some(members) = self.section("display", value) else {
            return;
        };
        for (name, value) in members {
            let at = format!("display.{name}");
            match name.as_str() {
                "fullscreen" => self.take(at, value, &mut out.fullscreen, A_BOOL, Value::as_bool),
                "vsync" => self.take(at, value, &mut out.vsync, A_BOOL, Value::as_bool),
                _ => self.warn(at, WarningKind::Unknown),
            }
        }
    }

    fn audio(&mut self, value: &Value, out: &mut Audio) {
        let Some(members) = self.section("audio", value) else {
            return;
        };
        let volume = |value: &Value| {
            value
                .as_i64()
                .and_then(|percent| u8::try_from(percent).ok())
                .filter(|&percent| percent <= 100)
        };
        for (name, value) in members {
            let at = format!("audio.{name}");
            let bus = match name.as_str() {
                "music" => &mut out.music,
                "sfx" => &mut out.sfx,
                "ui" => &mut out.ui,
                "ambience" => &mut out.ambience,
                _ => {
                    self.warn(at, WarningKind::Unknown);
                    continue;
                }
            };
            self.take(at, value, bus, A_VOLUME, volume);
        }
    }

    fn ui(&mut self, value: &Value, out: &mut Ui) {
        let Some(members) = self.section("ui", value) else {
            return;
        };
        for (name, value) in members {
            let at = format!("ui.{name}");
            match name.as_str() {
                "scale" => self.take(at, value, &mut out.scale, A_SCALE, |value| match value {
                    Value::Null => Some(None),
                    _ => value
                        .as_f64()
                        .filter(|scale| (MIN_UI_SCALE..=MAX_UI_SCALE).contains(scale))
                        .map(Some),
                }),
                "language" => self.take(at, value, &mut out.language, A_LANGUAGE, |value| {
                    value
                        .as_str()
                        .filter(|tag| is_language(tag))
                        .map(str::to_string)
                }),
                _ => self.warn(at, WarningKind::Unknown),
            }
        }
    }

    fn input(&mut self, value: &Value, out: &mut Input) {
        let Some(members) = self.section("input", value) else {
            return;
        };
        for (name, value) in members {
            match name.as_str() {
                "deadzones" => self.deadzones(value, &mut out.deadzones),
                "bindings" => self.bindings(value, &mut out.bindings),
                _ => self.warn(format!("input.{name}"), WarningKind::Unknown),
            }
        }
    }

    fn deadzones(&mut self, value: &Value, out: &mut Deadzones) {
        let Some(members) = self.section("input.deadzones", value) else {
            return;
        };
        let part = |value: &Value| value.as_f64().filter(|part| (0.0..=1.0).contains(part));
        for (name, value) in members {
            let at = format!("input.deadzones.{name}");
            let deadzone = match name.as_str() {
                "stick_inner" => &mut out.stick_inner,
                "stick_outer" => &mut out.stick_outer,
                "trigger" => &mut out.trigger,
                "press" => &mut out.press,
                "release" => &mut out.release,
                _ => {
                    self.warn(at, WarningKind::Unknown);
                    continue;
                }
            };
            self.take(at, value, deadzone, A_DEADZONE, part);
        }
    }

    /// Each action of `out`, the defaults', that the file names takes its
    /// slots from it.
    fn bindings(&mut self, value: &Value, out: &mut [ActionBindings]) {
        let Some(members) = self.section("input.bindings", value) else {
            return;
        };
        for (name, value) in members {
            let at = format!("input.bindings.{name}");
            let Some(action) = out.iter_mut().find(|action| action.action == *name) else {
                self.warn(at, WarningKind::UnknownAction);
                continue;
            };
            let Some(slots) = value.as_array().filter(|slots| slots.len() <= SLOTS) else {
                self.invalid(at, SLOTS_ARRAY);
                continue;
            };
            for (index, slot) in action.slots.iter_mut().enumerate() {
                *slot = match slots.get(index) {
                    None | Some(Value::Null) => None,
                    Some(value) => match binding(value) {
                        Ok(binding) => Some(binding),
                        Err(expected) => {
                            self.invalid(format!("{at}[{index}]"), expected);
                            // The default's, which the slot still holds.
                            *slot
                        }
                    },
                };
            }
        }
    }

    fn log(&mut self, value: &Value, out: &mut Log) {
        let Some(members) = self.section("log", value) else {
            return;
        };
        for (name, value) in members {
            let at = format!("log.{name}");
            match name.as_str() {
                "level" => self.take(at, value, &mut out.level, A_LEVEL, |value| {
                    value.as_str().and_then(level_from_name)
                }),
                "targets" => {
                    let Some(targets) = self.section(&at, value) else {
                        continue;
                    };
                    out.targets.clear();
                    for (target, level) in targets {
                        match level.as_str().and_then(level_from_name) {
                            Some(level) if is_module_path(target) => {
                                out.targets.push((target.clone(), level));
                            }
                            _ => self.warn(
                                format!("log.targets.{target}"),
                                WarningKind::LeftOut { expected: A_TARGET },
                            ),
                        }
                    }
                }
                _ => self.warn(at, WarningKind::Unknown),
            }
        }
    }
}

/// The binding `value` is, or what a binding of its kind takes.
fn binding(value: &Value) -> Result<Binding, &'static str> {
    let members = value.as_object().ok_or(A_BINDING)?;
    // Whether the object has exactly these members.
    let exactly = |names: &[&str]| {
        members.len() == names.len() && names.iter().all(|name| value.get(name).is_some())
    };
    if exactly(&["key"]) {
        return value
            .get("key")
            .and_then(Value::as_str)
            .and_then(Key::from_name)
            .map(Binding::Key)
            .ok_or(A_KEY);
    }
    if exactly(&["mouse"]) {
        return value
            .get("mouse")
            .and_then(Value::as_str)
            .and_then(MouseButton::from_name)
            .map(Binding::Mouse)
            .ok_or(A_MOUSE);
    }
    if exactly(&["keys"]) {
        let keys = value.get("keys").ok_or(FOUR_KEYS)?;
        let four = keys.as_object().is_some_and(|members| members.len() == 4);
        let key = |name: &str| {
            keys.get(name)
                .and_then(Value::as_str)
                .and_then(Key::from_name)
                .ok_or(FOUR_KEYS)
        };
        if !four {
            return Err(FOUR_KEYS);
        }
        return Ok(Binding::Keys {
            up: key("up")?,
            down: key("down")?,
            left: key("left")?,
            right: key("right")?,
        });
    }
    if exactly(&["pad"]) {
        return value
            .get("pad")
            .and_then(Value::as_str)
            .and_then(GamepadButton::from_name)
            .map(Binding::Pad)
            .ok_or(A_PAD);
    }
    if exactly(&["pad_axis", "sign"]) {
        let axis = value
            .get("pad_axis")
            .and_then(Value::as_str)
            .and_then(GamepadAxis::from_name)
            .ok_or(A_PAD_AXIS)?;
        let sign = match value.get("sign").and_then(Value::as_str) {
            Some("+") => Sign::Positive,
            Some("-") if !axis.is_trigger() => Sign::Negative,
            _ => return Err(A_PAD_AXIS),
        };
        return Ok(Binding::PadAxis { axis, sign });
    }
    if exactly(&["stick"]) {
        return match value.get("stick").and_then(Value::as_str) {
            Some("Left") => Ok(Binding::Stick(Stick::Left)),
            Some("Right") => Ok(Binding::Stick(Stick::Right)),
            _ => Err(A_STICK),
        };
    }
    Err(A_BINDING)
}

/// The object the file writes `binding` as.
fn binding_value(binding: &Binding) -> Value {
    let name = |text: &str| Value::String(text.to_string());
    let one = |member: &str, value: Value| Value::Object(vec![(member.to_string(), value)]);
    match *binding {
        Binding::Key(key) => one("key", name(key.name())),
        Binding::Mouse(button) => one("mouse", name(button.name())),
        Binding::Keys {
            up,
            down,
            left,
            right,
        } => one(
            "keys",
            Value::Object(
                [("up", up), ("down", down), ("left", left), ("right", right)]
                    .into_iter()
                    .map(|(direction, key)| (direction.to_string(), name(key.name())))
                    .collect(),
            ),
        ),
        Binding::Pad(button) => one("pad", name(button.name())),
        Binding::PadAxis { axis, sign } => Value::Object(vec![
            ("pad_axis".to_string(), name(axis.name())),
            (
                "sign".to_string(),
                name(match sign {
                    Sign::Negative => "-",
                    Sign::Positive => "+",
                }),
            ),
        ]),
        Binding::Stick(stick) => one(
            "stick",
            name(match stick {
                Stick::Left => "Left",
                Stick::Right => "Right",
            }),
        ),
    }
}

/// Sets the member `name` of `members` to `value`, where it is, or at the
/// end if there is none.
fn set(members: &mut Vec<(String, Value)>, name: &str, value: Value) {
    match members.iter_mut().find(|(member, _)| member == name) {
        Some((_, old)) => *old = value,
        None => members.push((name.to_string(), value)),
    }
}

/// Calls `change` on the members of the object `name` of `members`: the
/// one there, or a new one at the end. A member of that name that is not an
/// object becomes an empty one first.
fn edit(
    members: &mut Vec<(String, Value)>,
    name: &str,
    change: impl FnOnce(&mut Vec<(String, Value)>),
) {
    let index = match members.iter().position(|(member, _)| member == name) {
        Some(index) => index,
        None => {
            members.push((name.to_string(), Value::Object(Vec::new())));
            members.len() - 1
        }
    };
    if let Some((_, value)) = members.get_mut(index) {
        if !matches!(value, Value::Object(_)) {
            *value = Value::Object(Vec::new());
        }
        if let Value::Object(inner) = value {
            change(inner);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A game's defaults: two actions, one of each shape the tests need.
    fn defaults() -> Settings {
        Settings {
            display: Display {
                fullscreen: false,
                vsync: true,
            },
            audio: Audio {
                music: 100,
                sfx: 100,
                ui: 100,
                ambience: 100,
            },
            ui: Ui {
                scale: None,
                language: "en".to_string(),
            },
            input: Input {
                deadzones: Deadzones {
                    stick_inner: 0.2,
                    stick_outer: 0.95,
                    trigger: 0.1,
                    press: 0.5,
                    release: 0.4,
                },
                bindings: vec![
                    ActionBindings {
                        action: "move".to_string(),
                        slots: [
                            Some(Binding::Keys {
                                up: Key::W,
                                down: Key::S,
                                left: Key::A,
                                right: Key::D,
                            }),
                            Some(Binding::Stick(Stick::Left)),
                            None,
                            None,
                        ],
                    },
                    ActionBindings {
                        action: "confirm".to_string(),
                        slots: [
                            Some(Binding::Key(Key::Enter)),
                            Some(Binding::Pad(GamepadButton::A)),
                            None,
                            None,
                        ],
                    },
                ],
            },
            log: Log {
                level: Level::Info,
                targets: Vec::new(),
            },
        }
    }

    fn read_text(text: &str) -> Loaded {
        read(text, &defaults(), Limits::default()).unwrap_or_else(|error| panic!("{error}"))
    }

    /// The file `{"version": 1, <members>}`.
    fn file(members: &str) -> String {
        if members.is_empty() {
            r#"{"version": 1}"#.to_string()
        } else {
            format!(r#"{{"version": 1, {members}}}"#)
        }
    }

    fn invalid(at: &str, expected: &'static str) -> Warning {
        Warning {
            at: at.to_string(),
            kind: WarningKind::Invalid { expected },
        }
    }

    fn unknown(at: &str) -> Warning {
        Warning {
            at: at.to_string(),
            kind: WarningKind::Unknown,
        }
    }

    fn refused(text: &str) -> CodecError {
        match read(text, &defaults(), Limits::default()) {
            Ok(loaded) => panic!("read: {:?}", loaded.settings),
            Err(error) => error,
        }
    }

    #[test]
    fn a_file_of_only_its_version_is_the_defaults_without_a_warning() {
        let loaded = read_text(&file(""));
        assert_eq!(loaded.settings, defaults());
        assert_eq!(loaded.warnings, []);
    }

    #[test]
    fn every_setting_is_read_from_the_file() {
        let loaded = read_text(&file(
            r#""display": {"fullscreen": true, "vsync": false},
               "audio": {"music": 0, "sfx": 50, "ui": 100, "ambience": 7},
               "ui": {"scale": 1.5, "language": "pt-BR"},
               "input": {
                 "deadzones": {"stick_inner": 0, "stick_outer": 1, "trigger": 0.25,
                               "press": 0.6, "release": 0.3},
                 "bindings": {
                   "move": [{"stick": "Right"}, null,
                            {"keys": {"up": "Up", "down": "Down", "left": "Left", "right": "Right"}}],
                   "confirm": [{"mouse": "Left"}, {"pad_axis": "LeftY", "sign": "-"},
                               {"pad_axis": "RightTrigger", "sign": "+"}, {"pad": "Start"}]
                 }
               },
               "log": {"level": "warn", "targets": {"nadir_render::batch": "trace", "sandbox": "debug"}}"#,
        ));
        assert_eq!(loaded.warnings, []);
        let settings = loaded.settings;
        assert_eq!(
            settings.display,
            Display {
                fullscreen: true,
                vsync: false
            }
        );
        assert_eq!(
            settings.audio,
            Audio {
                music: 0,
                sfx: 50,
                ui: 100,
                ambience: 7
            }
        );
        assert_eq!(settings.ui.scale, Some(1.5));
        assert_eq!(settings.ui.language, "pt-BR");
        assert_eq!(
            settings.input.deadzones,
            Deadzones {
                stick_inner: 0.0,
                stick_outer: 1.0,
                trigger: 0.25,
                press: 0.6,
                release: 0.3
            }
        );
        assert_eq!(
            settings.input.bindings[0].slots,
            [
                Some(Binding::Stick(Stick::Right)),
                None,
                Some(Binding::Keys {
                    up: Key::Up,
                    down: Key::Down,
                    left: Key::Left,
                    right: Key::Right
                }),
                None,
            ],
            "a slot past the array's end is empty"
        );
        assert_eq!(
            settings.input.bindings[1].slots,
            [
                Some(Binding::Mouse(MouseButton::Left)),
                Some(Binding::PadAxis {
                    axis: GamepadAxis::LeftY,
                    sign: Sign::Negative
                }),
                Some(Binding::PadAxis {
                    axis: GamepadAxis::RightTrigger,
                    sign: Sign::Positive
                }),
                Some(Binding::Pad(GamepadButton::Start)),
            ]
        );
        assert_eq!(settings.log.level, Level::Warn);
        assert_eq!(
            settings.log.targets,
            [
                ("nadir_render::batch".to_string(), Level::Trace),
                ("sandbox".to_string(), Level::Debug)
            ]
        );
    }

    #[test]
    fn a_missing_section_or_setting_keeps_its_default_quietly() {
        let loaded = read_text(&file(r#""display": {"vsync": false}, "audio": {}"#));
        assert_eq!(loaded.warnings, []);
        let mut expected = defaults();
        expected.display.vsync = false;
        assert_eq!(loaded.settings, expected);
    }

    #[test]
    fn a_setting_of_the_wrong_type_keeps_its_default_with_a_warning() {
        let loaded = read_text(&file(
            r#""display": {"fullscreen": "yes", "vsync": false},
               "audio": {"music": 50.0, "sfx": "80"},
               "ui": {"scale": "2", "language": 3},
               "input": {"deadzones": {"trigger": "0.2"}},
               "log": {"level": 2}"#,
        ));
        let mut expected = defaults();
        expected.display.vsync = false;
        assert_eq!(loaded.settings, expected, "only vsync was right");
        assert_eq!(
            loaded.warnings,
            [
                invalid("display.fullscreen", A_BOOL),
                invalid("audio.music", A_VOLUME),
                invalid("audio.sfx", A_VOLUME),
                invalid("ui.scale", A_SCALE),
                invalid("ui.language", A_LANGUAGE),
                invalid("input.deadzones.trigger", A_DEADZONE),
                invalid("log.level", A_LEVEL),
            ]
        );
    }

    #[test]
    fn a_setting_out_of_its_range_keeps_its_default_with_a_warning() {
        let loaded = read_text(&file(
            r#""audio": {"music": 101, "sfx": -1, "ui": 256, "ambience": 100},
               "ui": {"scale": 0.124, "language": "en_US"},
               "input": {"deadzones": {"stick_inner": -0.1, "stick_outer": 1.01, "press": 1}},
               "log": {"level": "verbose"}"#,
        ));
        let mut expected = defaults();
        expected.input.deadzones.press = 1.0;
        assert_eq!(loaded.settings, expected, "100 and 1 are in range");
        assert_eq!(
            loaded.warnings,
            [
                invalid("audio.music", A_VOLUME),
                invalid("audio.sfx", A_VOLUME),
                invalid("audio.ui", A_VOLUME),
                invalid("ui.scale", A_SCALE),
                invalid("ui.language", A_LANGUAGE),
                invalid("input.deadzones.stick_inner", A_DEADZONE),
                invalid("input.deadzones.stick_outer", A_DEADZONE),
                invalid("log.level", A_LEVEL),
            ]
        );
        for (scale, taken) in [(0.125, true), (64.0, true), (64.001, false), (0.0, false)] {
            let text = file(&format!(r#""ui": {{"scale": {scale:?}}}"#));
            let loaded = read_text(&text);
            assert_eq!(loaded.settings.ui.scale.is_some(), taken, "{scale}");
        }
        let loaded = read_text(&file(r#""ui": {"scale": 2.0}"#));
        assert_eq!(loaded.settings.ui.scale, Some(2.0));
        let loaded = read_text(&file(r#""ui": {"scale": null}"#));
        assert_eq!(loaded.settings.ui.scale, None);
        assert_eq!(loaded.warnings, []);
    }

    #[test]
    fn a_section_that_is_not_an_object_keeps_all_its_defaults() {
        let loaded = read_text(&file(
            r#""display": true, "audio": [], "ui": null, "input": 1,
               "log": {"targets": []}"#,
        ));
        assert_eq!(loaded.settings, defaults());
        assert_eq!(
            loaded.warnings,
            [
                invalid("display", AN_OBJECT),
                invalid("audio", AN_OBJECT),
                invalid("ui", AN_OBJECT),
                invalid("input", AN_OBJECT),
                invalid("log.targets", AN_OBJECT),
            ]
        );
        let loaded = read_text(&file(r#""input": {"deadzones": 0, "bindings": []}"#));
        assert_eq!(loaded.settings, defaults());
        assert_eq!(
            loaded.warnings,
            [
                invalid("input.deadzones", AN_OBJECT),
                invalid("input.bindings", AN_OBJECT),
            ]
        );
    }

    #[test]
    fn members_that_are_not_settings_are_kept_with_a_warning() {
        let text = file(
            r#""dispaly": {"fullscreen": true},
               "display": {"fulscreen": true, "vsync": false},
               "audio": {"master": 50}, "ui": {"font": "big"},
               "input": {"mouse": {"invert": true}, "deadzones": {"dead": 0.5}},
               "log": {"file": false}"#,
        );
        let loaded = read_text(&text);
        let mut expected = defaults();
        expected.display.vsync = false;
        assert_eq!(loaded.settings, expected);
        assert_eq!(
            loaded.warnings,
            [
                unknown("dispaly"),
                unknown("display.fulscreen"),
                unknown("audio.master"),
                unknown("ui.font"),
                unknown("input.mouse"),
                unknown("input.deadzones.dead"),
                unknown("log.file"),
            ]
        );
        // Written back, each is where it was, and the settings follow
        // their own.
        let written = write(&loaded.settings, Some(&loaded.source));
        let value = json::parse(&written, Limits::default()).expect("JSON");
        let keys: Vec<&str> = value
            .as_object()
            .expect("an object")
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(
            keys,
            [
                "version", "dispaly", "display", "audio", "ui", "input", "log"
            ]
        );
        let display = value.get("display").expect("display");
        assert_eq!(display.get("fulscreen"), Some(&Value::Bool(true)));
        assert_eq!(display.get("fullscreen"), Some(&Value::Bool(false)));
        assert_eq!(
            value
                .get("dispaly")
                .and_then(|section| section.get("fullscreen")),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            value.get("audio").and_then(|audio| audio.get("master")),
            Some(&Value::Int(50))
        );
        let input = value.get("input").expect("input");
        assert_eq!(
            input.get("mouse").and_then(|mouse| mouse.get("invert")),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            input
                .get("deadzones")
                .and_then(|deadzones| deadzones.get("dead")),
            Some(&Value::Float(0.5))
        );
        assert_eq!(
            value.get("log").and_then(|log| log.get("file")),
            Some(&Value::Bool(false))
        );
    }

    #[test]
    fn the_bindings_of_an_action_the_game_lacks_are_kept_with_a_warning() {
        let text = file(r#""input": {"bindings": {"jump": [{"key": "Space"}], "confirm": []}}"#);
        let loaded = read_text(&text);
        assert_eq!(
            loaded.warnings,
            [Warning {
                at: "input.bindings.jump".to_string(),
                kind: WarningKind::UnknownAction
            }]
        );
        assert_eq!(
            loaded.settings.input.bindings[1].slots, [None; SLOTS],
            "an empty array empties every slot"
        );
        let written = write(&loaded.settings, Some(&loaded.source));
        let value = json::parse(&written, Limits::default()).expect("JSON");
        let bindings = value
            .get("input")
            .and_then(|input| input.get("bindings"))
            .expect("bindings");
        let names: Vec<&str> = bindings
            .as_object()
            .expect("an object")
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, ["jump", "confirm", "move"]);
        assert_eq!(
            bindings.get("jump"),
            Some(&Value::Array(vec![Value::Object(vec![(
                "key".to_string(),
                Value::String("Space".to_string())
            )])]))
        );
    }

    #[test]
    fn an_action_whose_slots_are_not_an_array_of_four_keeps_its_defaults() {
        let loaded = read_text(&file(
            r#""input": {"bindings": {
                 "move": {"key": "W"},
                 "confirm": [null, null, null, null, {"key": "Space"}]}}"#,
        ));
        assert_eq!(loaded.settings, defaults());
        assert_eq!(
            loaded.warnings,
            [
                invalid("input.bindings.move", SLOTS_ARRAY),
                invalid("input.bindings.confirm", SLOTS_ARRAY),
            ]
        );
    }

    #[test]
    fn a_slot_that_is_not_a_binding_keeps_the_defaults_binding() {
        // The defaults' confirm: Enter, the pad's A, then two empty slots.
        let loaded = read_text(&file(
            r#""input": {"bindings": {"confirm": [{"key": "enter"}, {"pad": "A", "key": "B"},
                                                  {"stick": "Middle"}, 3]}}"#,
        ));
        assert_eq!(loaded.settings, defaults());
        assert_eq!(
            loaded.warnings,
            [
                invalid("input.bindings.confirm[0]", A_KEY),
                invalid("input.bindings.confirm[1]", A_BINDING),
                invalid("input.bindings.confirm[2]", A_STICK),
                invalid("input.bindings.confirm[3]", A_BINDING),
            ]
        );
    }

    #[test]
    fn each_kind_of_binding_refuses_what_it_does_not_take() {
        let cases: &[(&str, &'static str)] = &[
            (r#"{}"#, A_BINDING),
            (r#"{"button": "A"}"#, A_BINDING),
            (r#"{"key": "W", "sign": "+"}"#, A_BINDING),
            (r#"{"key": 4}"#, A_KEY),
            (r#"{"key": "w"}"#, A_KEY),
            (r#"{"mouse": "Wheel"}"#, A_MOUSE),
            (r#"{"keys": ["W", "S", "A", "D"]}"#, FOUR_KEYS),
            (
                r#"{"keys": {"up": "W", "down": "S", "left": "A"}}"#,
                FOUR_KEYS,
            ),
            (
                r#"{"keys": {"up": "W", "down": "S", "left": "A", "right": "D", "jump": "Space"}}"#,
                FOUR_KEYS,
            ),
            (
                r#"{"keys": {"up": "W", "down": "S", "left": "A", "rihgt": "D"}}"#,
                FOUR_KEYS,
            ),
            (
                r#"{"keys": {"up": "W", "down": "S", "left": "A", "right": "Dee"}}"#,
                FOUR_KEYS,
            ),
            (r#"{"pad": "Triangle"}"#, A_PAD),
            (r#"{"pad_axis": "LeftX"}"#, A_BINDING),
            (r#"{"pad_axis": "LeftZ", "sign": "+"}"#, A_PAD_AXIS),
            (r#"{"pad_axis": "LeftX", "sign": "plus"}"#, A_PAD_AXIS),
            (r#"{"pad_axis": "LeftTrigger", "sign": "-"}"#, A_PAD_AXIS),
            (r#"{"stick": "left"}"#, A_STICK),
        ];
        for &(text, expected) in cases {
            let value = json::parse(text, Limits::default()).expect("JSON");
            assert_eq!(binding(&value), Err(expected), "{text}");
        }
    }

    #[test]
    fn a_log_target_that_is_not_a_module_path_or_lacks_a_level_is_left_out() {
        let loaded = read_text(&file(
            r#""log": {"targets": {"nadir_audio": "debug", "": "info", "a::": "info",
                                   "a b": "info", "nadir_ecs": "loud", "nadir_gfx": null}}"#,
        ));
        assert_eq!(
            loaded.settings.log.targets,
            [("nadir_audio".to_string(), Level::Debug)]
        );
        let left_out = |at: &str| Warning {
            at: at.to_string(),
            kind: WarningKind::LeftOut { expected: A_TARGET },
        };
        assert_eq!(
            loaded.warnings,
            [
                left_out("log.targets."),
                left_out("log.targets.a::"),
                left_out("log.targets.a b"),
                left_out("log.targets.nadir_ecs"),
                left_out("log.targets.nadir_gfx"),
            ]
        );
        assert!(is_module_path("a::b_2::C"));
        assert!(!is_module_path("::a"));
        assert!(!is_module_path(&"a".repeat(MAX_TARGET_BYTES + 1)));
        assert!(is_module_path(&"a".repeat(MAX_TARGET_BYTES)));
    }

    #[test]
    fn language_tags_are_ascii_letters_digits_and_dashes() {
        for tag in [
            "en",
            "pt-BR",
            "zh-Hant-TW",
            "419",
            &"a".repeat(MAX_LANGUAGE_BYTES),
        ] {
            assert!(is_language(tag), "{tag}");
        }
        for tag in [
            "",
            "en_US",
            "fr ",
            "日本",
            &"a".repeat(MAX_LANGUAGE_BYTES + 1),
        ] {
            assert!(!is_language(tag), "{tag}");
        }
    }

    #[test]
    fn the_whole_file_is_refused_when_it_is_not_a_settings_file_of_this_version() {
        let settings = |kind| CodecError::Settings { kind };
        assert_eq!(refused("[]"), settings(ErrorKind::NotAnObject));
        assert_eq!(refused("1"), settings(ErrorKind::NotAnObject));
        assert_eq!(refused("{}"), settings(ErrorKind::NoVersion));
        assert_eq!(
            refused(r#"{"version": "1"}"#),
            settings(ErrorKind::NoVersion)
        );
        assert_eq!(
            refused(r#"{"version": 1.0}"#),
            settings(ErrorKind::NoVersion)
        );
        assert_eq!(refused(r#"{"version": 0}"#), settings(ErrorKind::NoVersion));
        assert_eq!(
            refused(r#"{"version": -3}"#),
            settings(ErrorKind::NoVersion)
        );
        assert_eq!(
            refused(r#"{"version": 2, "display": {}}"#),
            settings(ErrorKind::NewerVersion { found: 2 })
        );
        assert!(matches!(refused("{"), CodecError::Json { .. }));
        assert!(matches!(refused(""), CodecError::Json { .. }));
        let large = format!(r#"{{"version": 1, "pad": "{}"}}"#, "x".repeat(MAX_BYTES));
        assert_eq!(
            refused(&large),
            settings(ErrorKind::TooLarge { limit: MAX_BYTES })
        );
    }

    #[test]
    fn jsonc_comments_and_trailing_commas_are_read() {
        let loaded = read_text(
            "// mine\n{\n  \"version\": 1, /* the window */\n  \"display\": {\"vsync\": false,},\n}\n",
        );
        assert_eq!(loaded.warnings, []);
        assert!(!loaded.settings.display.vsync);
    }

    #[test]
    fn a_file_written_from_nothing_reads_back_as_the_settings() {
        let mut settings = defaults();
        settings.display.fullscreen = true;
        settings.audio.sfx = 30;
        settings.ui.scale = Some(0.2);
        settings.ui.language = "de".to_string();
        settings.input.deadzones.trigger = 0.15;
        settings.input.bindings[1].slots[3] = Some(Binding::PadAxis {
            axis: GamepadAxis::LeftX,
            sign: Sign::Negative,
        });
        settings
            .log
            .targets
            .push(("nadir_input".to_string(), Level::Trace));
        let text = write(&settings, None);
        assert!(text.starts_with(HEADER));
        assert!(text.ends_with("}\n"));
        assert!(text.contains("\"scale\": 0.2,"), "{text}");
        let loaded = read_text(&text);
        assert_eq!(loaded.warnings, []);
        assert_eq!(loaded.settings, settings);
        assert_eq!(
            write(&loaded.settings, Some(&loaded.source)),
            text,
            "written over itself, the file is the same"
        );
    }

    #[test]
    fn what_the_file_does_not_know_is_left_out_where_keeping_it_is_too_large() {
        // A 0.6 MiB array of zeros grows to 2.4 MiB written one to a line.
        let zeros = vec!["0"; 300_000].join(",");
        let loaded = read_text(&file(&format!(r#""theme": [{zeros}]"#)));
        assert_eq!(loaded.warnings, [unknown("theme")]);
        let written = write(&loaded.settings, Some(&loaded.source));
        assert_eq!(written, write(&loaded.settings, None));
        assert!(written.len() <= MAX_BYTES);
        // One that fits keeps it.
        let loaded = read_text(&file(r#""theme": [0, 0]"#));
        assert!(write(&loaded.settings, Some(&loaded.source)).contains("\"theme\""));
    }

    #[test]
    fn a_setting_that_was_wrong_is_written_over_with_its_value() {
        let loaded = read_text(&file(r#""audio": {"music": "loud"}, "ui": 4"#));
        let written = write(&loaded.settings, Some(&loaded.source));
        let value = json::parse(&written, Limits::default()).expect("JSON");
        assert_eq!(
            value.get("audio").and_then(|audio| audio.get("music")),
            Some(&Value::Int(100))
        );
        assert_eq!(
            value.get("ui").and_then(|ui| ui.get("language")),
            Some(&Value::String("en".to_string()))
        );
    }

    #[test]
    fn warnings_name_the_member_and_what_was_done() {
        assert_eq!(
            invalid("audio.music", A_VOLUME).to_string(),
            "`audio.music` is not a whole number from 0 to 100: the setting keeps its default"
        );
        assert_eq!(
            unknown("dispaly").to_string(),
            "`dispaly` is not a setting: it is kept as it is"
        );
        assert_eq!(
            unknown("a\nWARN fake").to_string(),
            "`a\\nWARN fake` is not a setting: it is kept as it is",
            "a line break from the file is escaped"
        );
    }

    #[test]
    fn level_names_read_back() {
        for level in [
            Level::Error,
            Level::Warn,
            Level::Info,
            Level::Debug,
            Level::Trace,
        ] {
            assert_eq!(level_from_name(level_name(level)), Some(level));
        }
        assert_eq!(level_from_name("INFO"), None);
    }
}
