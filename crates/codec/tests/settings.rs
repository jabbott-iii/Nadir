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

//! Settings file tests: the hand-written vectors in `tests/settings/`.
//!
//! - `every_setting.jsonc`: every setting away from its default, with
//!   comments and trailing commas, as a player might have edited it;
//! - `written.jsonc`: the text the writer writes for those settings, from
//!   nothing, which pins the format;
//! - `fallbacks.jsonc`: one setting of each kind the reader does not
//!   take, and members it keeps without knowing them;
//! - `version_2.jsonc`: a newer game's file.

use nadir_codec::json::{self, Value};
use nadir_codec::settings::{
    self, ActionBindings, Audio, Binding, Deadzones, Display, ErrorKind, Input, Log, Settings,
    Sign, Stick, Ui, Warning, WarningKind,
};
use nadir_codec::{CodecError, Limits};
use nadir_core::log::Level;
use nadir_core::{GamepadAxis, GamepadButton, Key, MouseButton};

fn vector(name: &str) -> String {
    let path = format!("{}/tests/settings/{name}.jsonc", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// A game's defaults, with the two actions the vectors bind.
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

/// What `every_setting.jsonc` holds.
fn every_setting() -> Settings {
    Settings {
        display: Display {
            fullscreen: true,
            vsync: false,
        },
        audio: Audio {
            music: 40,
            sfx: 75,
            ui: 0,
            ambience: 100,
        },
        ui: Ui {
            scale: Some(2.5),
            language: "pt-BR".to_string(),
        },
        input: Input {
            deadzones: Deadzones {
                stick_inner: 0.15,
                stick_outer: 0.9,
                trigger: 0.05,
                press: 0.6,
                release: 0.45,
            },
            bindings: vec![
                ActionBindings {
                    action: "move".to_string(),
                    slots: [
                        Some(Binding::Keys {
                            up: Key::Up,
                            down: Key::Down,
                            left: Key::Left,
                            right: Key::Right,
                        }),
                        Some(Binding::Stick(Stick::Right)),
                        None,
                        Some(Binding::Keys {
                            up: Key::W,
                            down: Key::S,
                            left: Key::A,
                            right: Key::D,
                        }),
                    ],
                },
                ActionBindings {
                    action: "confirm".to_string(),
                    slots: [
                        Some(Binding::Key(Key::Space)),
                        Some(Binding::Pad(GamepadButton::B)),
                        Some(Binding::Mouse(MouseButton::Right)),
                        Some(Binding::PadAxis {
                            axis: GamepadAxis::RightTrigger,
                            sign: Sign::Positive,
                        }),
                    ],
                },
            ],
        },
        log: Log {
            level: Level::Debug,
            targets: vec![
                ("nadir_render::batch".to_string(), Level::Trace),
                ("nadir_audio".to_string(), Level::Error),
            ],
        },
    }
}

#[test]
fn a_hand_edited_file_reads_every_setting() {
    let loaded =
        settings::read(&vector("every_setting"), &defaults(), Limits::default()).expect("it reads");
    assert_eq!(loaded.warnings, []);
    assert_eq!(loaded.settings, every_setting());
}

#[test]
fn the_writer_writes_the_fixture_and_reads_it_back() {
    let fixture = vector("written");
    assert_eq!(settings::write(&every_setting(), None), fixture);
    let loaded = settings::read(&fixture, &defaults(), Limits::default()).expect("it reads");
    assert_eq!(loaded.warnings, []);
    assert_eq!(loaded.settings, every_setting());
    // The hand-edited file, written over itself, is the fixture without its
    // comments: the same members in the same order.
    let edited =
        settings::read(&vector("every_setting"), &defaults(), Limits::default()).expect("it reads");
    assert_eq!(
        settings::write(&edited.settings, Some(&edited.source)),
        fixture
    );
}

#[test]
fn each_setting_the_reader_does_not_take_keeps_its_default() {
    let loaded =
        settings::read(&vector("fallbacks"), &defaults(), Limits::default()).expect("it reads");
    let mut expected = defaults();
    expected.display.vsync = false;
    // The key, which only the game knows `move` cannot take, then an empty
    // slot, where the file's array ends; its stick is not one, so the slot
    // keeps the default's.
    expected.input.bindings[0].slots = [
        Some(Binding::Key(Key::W)),
        Some(Binding::Stick(Stick::Left)),
        None,
        None,
    ];
    expected.log.targets = vec![("nadir_gfx".to_string(), Level::Info)];
    assert_eq!(loaded.settings, expected);
    // Each warning, in the file's order, with what was done. What each
    // invalid setting expected is the unit tests'.
    let found: Vec<(&str, &str)> = loaded
        .warnings
        .iter()
        .map(|warning| {
            let done = match warning.kind {
                WarningKind::Invalid { .. } => "default",
                WarningKind::Unknown | WarningKind::UnknownAction => "kept",
                WarningKind::LeftOut { .. } => "left out",
                _ => "other",
            };
            (warning.at.as_str(), done)
        })
        .collect();
    assert_eq!(
        found,
        [
            ("display.fullscreen", "default"),
            ("audio.music", "default"),
            ("audio.sfx", "default"),
            ("audio.master", "kept"),
            ("ui.scale", "default"),
            ("ui.language", "default"),
            ("input.deadzones.trigger", "default"),
            ("input.bindings.move[1]", "default"),
            ("input.bindings.confirm", "default"),
            ("input.bindings.jump", "kept"),
            ("log.level", "default"),
            ("log.targets.nadir audio", "left out"),
            ("theme", "kept"),
        ]
    );
    assert_eq!(
        loaded.warnings[9],
        Warning {
            at: "input.bindings.jump".to_string(),
            kind: WarningKind::UnknownAction
        }
    );
    // Written back, what it does not know is still there.
    let written = settings::write(&loaded.settings, Some(&loaded.source));
    let value = json::parse(&written, Limits::default()).expect("JSON");
    assert_eq!(value.get("theme"), Some(&Value::String("dark".to_string())));
    assert_eq!(
        value.get("audio").and_then(|audio| audio.get("master")),
        Some(&Value::Int(80))
    );
    assert!(
        value
            .get("input")
            .and_then(|input| input.get("bindings"))
            .and_then(|bindings| bindings.get("jump"))
            .is_some()
    );
    let again = settings::read(&written, &defaults(), Limits::default()).expect("it reads");
    assert_eq!(again.settings, loaded.settings);
    let kept: Vec<&str> = again
        .warnings
        .iter()
        .map(|warning| warning.at.as_str())
        .collect();
    assert_eq!(
        kept,
        ["audio.master", "input.bindings.jump", "theme"],
        "only what it does not know is left to warn of"
    );
}

#[test]
fn a_newer_games_file_is_refused_as_a_whole() {
    assert_eq!(
        settings::read(&vector("version_2"), &defaults(), Limits::default()),
        Err(CodecError::Settings {
            kind: ErrorKind::NewerVersion { found: 2 }
        })
    );
    assert_eq!(
        CodecError::Settings {
            kind: ErrorKind::NewerVersion { found: 2 }
        }
        .to_string(),
        "settings file error: its version is 2, and this game reads 1 and older"
    );
}
