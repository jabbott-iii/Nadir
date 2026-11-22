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

//! Button prompts: [`Gamepads`], the pads connected and the family of each,
//! and [`ButtonPrompts`], what each family prints on its buttons (roadmap
//! M8.5, ADR-0074).

use nadir_assets::{Asset, AssetError, DecodeCtx, FinalizeCtx};
use nadir_codec::json::Value;
use nadir_codec::kbj;
use nadir_core::{GamepadAxis, GamepadButton};
use nadir_input::Stick;
use nadir_platform::{Event, GamepadFamily, GamepadId};

/// The gamepads connected now, each with its [`GamepadFamily`] (whose
/// labels its buttons carry): a world resource that [`App::run`] keeps as
/// the platform reports pads plugged in and unplugged.
///
/// A game shows the prompt of the device used last: when
/// [`Input::active_device`](nadir_input::Input::active_device) is a pad,
/// [`Gamepads::family`] gives its family, and [`ButtonPrompts`] the labels
/// of that family. A pad's family comes from the platform's
/// [`GamepadState::family`](nadir_platform::GamepadState::family).
///
/// [`App::update`] does not change it: a test or a headless run makes its
/// own events, with no platform to tell a pad's family, so there it holds
/// no pad.
///
/// [`App::run`]: crate::App::run
/// [`App::update`]: crate::App::update
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Gamepads {
    /// In the order they connected.
    pads: Vec<(GamepadId, GamepadFamily)>,
}

impl Gamepads {
    /// The family of the pad `id`, while it is connected.
    pub fn family(&self, id: GamepadId) -> Option<GamepadFamily> {
        self.pads
            .iter()
            .find(|&&(pad, _)| pad == id)
            .map(|&(_, family)| family)
    }

    /// Every pad connected, with its family, in the order they connected.
    pub fn iter(&self) -> impl Iterator<Item = (GamepadId, GamepadFamily)> + '_ {
        self.pads.iter().copied()
    }

    /// Follows the pads that `events` connect and disconnect, asking
    /// `family` each new pad's family. A pad whose family is not known has
    /// gone again within the events, whose disconnection follows: it is
    /// left out.
    pub(crate) fn handle(
        &mut self,
        events: &[Event],
        family: impl Fn(GamepadId) -> Option<GamepadFamily>,
    ) {
        for event in events {
            match *event {
                Event::GamepadConnected(id) => {
                    if let Some(family) = family(id) {
                        self.pads.push((id, family));
                    }
                }
                Event::GamepadDisconnected(id) => self.pads.retain(|&(pad, _)| pad != id),
                _ => {}
            }
        }
    }
}

/// Each button's name in a prompt table, as its variant is spelled.
const BUTTONS: [(GamepadButton, &str); GamepadButton::ALL.len()] = [
    (GamepadButton::A, "A"),
    (GamepadButton::B, "B"),
    (GamepadButton::X, "X"),
    (GamepadButton::Y, "Y"),
    (GamepadButton::LeftBumper, "LeftBumper"),
    (GamepadButton::RightBumper, "RightBumper"),
    (GamepadButton::Back, "Back"),
    (GamepadButton::Start, "Start"),
    (GamepadButton::Guide, "Guide"),
    (GamepadButton::LeftStick, "LeftStick"),
    (GamepadButton::RightStick, "RightStick"),
    (GamepadButton::DPadUp, "DPadUp"),
    (GamepadButton::DPadDown, "DPadDown"),
    (GamepadButton::DPadLeft, "DPadLeft"),
    (GamepadButton::DPadRight, "DPadRight"),
];

/// Each trigger's name in a prompt table, as its axis is spelled.
const TRIGGERS: [(GamepadAxis, &str); 2] = [
    (GamepadAxis::LeftTrigger, "LeftTrigger"),
    (GamepadAxis::RightTrigger, "RightTrigger"),
];

/// Each stick's name in a prompt table, as its variant is spelled.
const STICKS: [(Stick, &str); Stick::ALL.len()] = [(Stick::Left, "Left"), (Stick::Right, "Right")];

/// The three parts of a family's labels, as a prompt table names them.
const PARTS: [&str; 3] = ["buttons", "triggers", "sticks"];

/// What each family of gamepad prints on its buttons, its triggers and its
/// sticks, for a game's button prompts: "Press A" on an Xbox pad, "Press ✕"
/// on a PlayStation one. An asset, read from a cooked data file, by default
/// [`ButtonPrompts::PATH`].
///
/// A [`GamepadButton`] names a place on the pad, so the bottom face button
/// is `A` on every pad, and the table gives each family's label for each
/// place: a Nintendo pad's `A` is labeled "B". A game loads the table as any
/// asset, with [`AssetServer::load`](nadir_assets::AssetServer::load) or,
/// as it sets up, `load_now`, and picks the family from [`Gamepads`].
///
/// # The file
///
/// The source is `assets/data/input/prompts.jsonc`, which the cook's data
/// job writes as KBJ, the engine's binary JSON, to `data/input/prompts.kbj`.
/// It holds an object for each family, named as [`GamepadFamily::name`]
/// spells it, with three objects of labels, each label a string:
///
/// - `buttons`: one for each [`GamepadButton`], named as its variant is
///   spelled (`"A"`, `"LeftBumper"`, `"DPadUp"`).
/// - `triggers`: `"LeftTrigger"` and `"RightTrigger"`, as
///   [`GamepadAxis`] spells them.
/// - `sticks`: `"Left"` and `"Right"`, as [`Stick`] spells them.
///
/// ```jsonc
/// {
///   "Xbox": {
///     "buttons": { "A": "A", "B": "B", "X": "X", "Y": "Y", "LeftBumper": "LB", … },
///     "triggers": { "LeftTrigger": "LT", "RightTrigger": "RT" },
///     "sticks": { "Left": "Left stick", "Right": "Right stick" }
///   },
///   "PlayStation": { … }, "Nintendo": { … }, "Other": { … }
/// }
/// ```
///
/// A table that lacks a family or a label, has a name it does not know, or
/// has a label that is not a string fails to load, as
/// [`AssetError::Invalid`] saying where. So a table that loads has a label
/// for every family, button, trigger and stick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ButtonPrompts {
    /// In the order of [`GamepadFamily::ALL`].
    families: Vec<Labels>,
}

/// One family's labels.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Labels {
    /// In the order of [`GamepadButton::ALL`].
    buttons: Vec<String>,
    /// In the order of [`TRIGGERS`].
    triggers: Vec<String>,
    /// In the order of [`Stick::ALL`].
    sticks: Vec<String>,
}

impl ButtonPrompts {
    /// Where the cook writes the table from `assets/data/input/prompts.jsonc`.
    pub const PATH: &'static str = "data/input/prompts.kbj";

    /// What a pad of the family prints on the button.
    pub fn button(&self, family: GamepadFamily, button: GamepadButton) -> &str {
        &self.labels(family).buttons[button.index()]
    }

    /// What a pad of the family prints on the trigger, or `None` if `axis`
    /// is a stick's axis, not a trigger.
    pub fn trigger(&self, family: GamepadFamily, axis: GamepadAxis) -> Option<&str> {
        let slot = TRIGGERS.iter().position(|&(trigger, _)| trigger == axis)?;
        Some(&self.labels(family).triggers[slot])
    }

    /// What the family calls the stick.
    pub fn stick(&self, family: GamepadFamily, stick: Stick) -> &str {
        let slot = match stick {
            Stick::Left => 0,
            Stick::Right => 1,
        };
        &self.labels(family).sticks[slot]
    }

    fn labels(&self, family: GamepadFamily) -> &Labels {
        &self.families[family.index()]
    }

    /// The table that `value`, a prompt table's whole value, gives, or why
    /// it is not one, naming the place.
    fn from_value(value: &Value) -> Result<Self, String> {
        let table = object(value, "the table")?;
        let names: Vec<&str> = GamepadFamily::ALL
            .iter()
            .map(|family| family.name())
            .collect();
        known(table, &names, "the table", "family")?;
        let families = GamepadFamily::ALL
            .iter()
            .map(|family| {
                let name = family.name();
                let place = format!("`{name}`");
                let parts = object(member(table, name, "the table")?, &place)?;
                known(parts, &PARTS, &place, "part")?;
                Ok(Labels {
                    buttons: labels(parts, name, "buttons", "button", &BUTTONS.map(|(_, n)| n))?,
                    triggers: labels(
                        parts,
                        name,
                        "triggers",
                        "trigger",
                        &TRIGGERS.map(|(_, n)| n),
                    )?,
                    sticks: labels(parts, name, "sticks", "stick", &STICKS.map(|(_, n)| n))?,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(Self { families })
    }
}

/// `value`'s members, or why it is not an object.
fn object<'v>(value: &'v Value, place: &str) -> Result<&'v [(String, Value)], String> {
    value
        .as_object()
        .ok_or_else(|| format!("{place} is not an object"))
}

/// The member `name` of `members`, which are `place`'s, or why it has none.
fn member<'v>(
    members: &'v [(String, Value)],
    name: &str,
    place: &str,
) -> Result<&'v Value, String> {
    members
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
        .ok_or_else(|| format!("{place} has no `{name}`"))
}

/// Refuses a member of `members`, `place`'s, whose name is none of `names`:
/// a misspelled name would otherwise go unseen.
fn known(
    members: &[(String, Value)],
    names: &[&str],
    place: &str,
    what: &str,
) -> Result<(), String> {
    match members
        .iter()
        .find(|(key, _)| !names.contains(&key.as_str()))
    {
        Some((key, _)) => Err(format!(
            "{place} has `{key}`, which is no {what}: the names are {}",
            names.join(", ")
        )),
        None => Ok(()),
    }
}

/// The labels of the part `part` of the family `family`, a string for each
/// of `names`, in their order: the names of `what`, a button, trigger or
/// stick.
fn labels(
    parts: &[(String, Value)],
    family: &str,
    part: &str,
    what: &str,
    names: &[&str],
) -> Result<Vec<String>, String> {
    let place = format!("`{family}.{part}`");
    let members = object(member(parts, part, &format!("`{family}`"))?, &place)?;
    known(members, names, &place, what)?;
    names
        .iter()
        .map(|name| {
            member(members, name, &place)?
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("`{family}.{part}.{name}` is not a string"))
        })
        .collect()
}

impl Asset for ButtonPrompts {
    type Decoded = Self;
    const EXTENSIONS: &'static [&'static str] = &["kbj"];

    fn decode(bytes: &[u8], ctx: &DecodeCtx<'_>) -> Result<Self, AssetError> {
        let value = kbj::decode(bytes, ctx.limits()).map_err(AssetError::Decode)?;
        Self::from_value(&value)
            .map_err(|reason| AssetError::Invalid(format!("not a prompt table: {reason}")))
    }

    fn finalize(prompts: Self, _ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        Ok(prompts)
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process};

    use nadir_assets::{AssetServer, OnFailure};
    use nadir_codec::{Limits, json};

    use super::*;

    /// The sandbox's table, from `assets/`, as its source text.
    fn sandbox_table() -> String {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/data/input/prompts.jsonc"
        );
        fs::read_to_string(path).unwrap_or_else(|error| panic!("read {path}: {error}"))
    }

    /// `text`, a prompt table's JSONC, cooked as the cook's data job writes
    /// it: the same value, in KBJ.
    fn cook(text: &str) -> Vec<u8> {
        let value = json::parse(text, Limits::default()).expect("the table's JSONC");
        kbj::encode(&value, Limits::default()).expect("encodable")
    }

    fn decode(bytes: &[u8]) -> Result<ButtonPrompts, AssetError> {
        ButtonPrompts::decode(
            bytes,
            &DecodeCtx::new(ButtonPrompts::PATH, Limits::default()),
        )
    }

    #[test]
    fn the_names_are_spelled_as_the_variants_are() {
        for (index, &(button, name)) in BUTTONS.iter().enumerate() {
            assert_eq!(button, GamepadButton::ALL[index]);
            assert_eq!(name, format!("{button:?}"));
        }
        for (axis, name) in TRIGGERS {
            assert!(axis.is_trigger(), "{axis:?}");
            assert_eq!(name, format!("{axis:?}"));
        }
        let triggers = GamepadAxis::ALL.iter().filter(|axis| axis.is_trigger());
        assert_eq!(triggers.count(), TRIGGERS.len());
        for (index, &(stick, name)) in STICKS.iter().enumerate() {
            assert_eq!(stick, Stick::ALL[index]);
            assert_eq!(name, format!("{stick:?}"));
        }
    }

    #[test]
    fn the_sandboxs_table_labels_every_button_trigger_and_stick_of_every_family() {
        let prompts = decode(&cook(&sandbox_table())).expect("the sandbox's table");
        for &family in GamepadFamily::ALL {
            for &button in GamepadButton::ALL {
                assert!(
                    !prompts.button(family, button).is_empty(),
                    "{family:?} {button:?}"
                );
            }
            for &axis in GamepadAxis::ALL {
                assert_eq!(
                    prompts.trigger(family, axis).is_some(),
                    axis.is_trigger(),
                    "{family:?} {axis:?}"
                );
            }
            for &stick in Stick::ALL {
                assert!(
                    !prompts.stick(family, stick).is_empty(),
                    "{family:?} {stick:?}"
                );
            }
        }
        // The bottom face button, by each family's label: a Nintendo pad's A
        // is on the right.
        use GamepadFamily::{Nintendo, Other, PlayStation, Xbox};
        let a = GamepadButton::A;
        assert_eq!(prompts.button(Xbox, a), "A");
        assert_eq!(prompts.button(PlayStation, a), "\u{2715}");
        assert_eq!(prompts.button(Nintendo, a), "B");
        assert_eq!(prompts.button(Nintendo, GamepadButton::B), "A");
        assert_eq!(prompts.button(Other, a), "A");
        assert_eq!(
            prompts.trigger(PlayStation, GamepadAxis::RightTrigger),
            Some("R2")
        );
        assert_eq!(
            prompts.trigger(Nintendo, GamepadAxis::LeftTrigger),
            Some("ZL")
        );
        assert_eq!(prompts.trigger(Xbox, GamepadAxis::LeftX), None);
        assert_eq!(prompts.stick(Xbox, Stick::Right), "Right stick");
    }

    #[test]
    fn a_table_that_lacks_a_label_or_has_a_name_it_does_not_know_is_refused() {
        let table = json::parse(&sandbox_table(), Limits::default()).expect("the table");
        let refused = |change: &dyn Fn(&mut Value)| -> String {
            let mut changed = table.clone();
            change(&mut changed);
            let bytes = kbj::encode(&changed, Limits::default()).expect("encodable");
            match decode(&bytes) {
                Err(AssetError::Invalid(reason)) => reason,
                other => panic!("not refused as invalid: {other:?}"),
            }
        };
        fn members(value: &mut Value) -> &mut Vec<(String, Value)> {
            match value {
                Value::Object(members) => members,
                _ => panic!("not an object"),
            }
        }
        fn at<'v>(value: &'v mut Value, path: &[&str]) -> &'v mut Value {
            path.iter().fold(value, |value, key| {
                let members = members(value);
                &mut members
                    .iter_mut()
                    .find(|(name, _)| name == key)
                    .unwrap_or_else(|| panic!("no {key}"))
                    .1
            })
        }

        assert_eq!(
            refused(&|table| members(table).retain(|(name, _)| name != "Nintendo")),
            "not a prompt table: the table has no `Nintendo`"
        );
        assert_eq!(
            refused(&|table| {
                members(at(table, &["Xbox", "buttons"])).retain(|(name, _)| name != "DPadLeft");
            }),
            "not a prompt table: `Xbox.buttons` has no `DPadLeft`"
        );
        assert_eq!(
            refused(&|table| {
                members(at(table, &["Other", "sticks"])).retain(|(name, _)| name != "Right");
            }),
            "not a prompt table: `Other.sticks` has no `Right`"
        );
        assert_eq!(
            refused(&|table| members(table).push(("Sega".to_string(), Value::Null))),
            "not a prompt table: the table has `Sega`, which is no family: the names are Xbox, \
             PlayStation, Nintendo, Other"
        );
        assert!(
            refused(&|table| {
                members(at(table, &["PlayStation", "buttons"]))
                    .push(("LeftBumbper".to_string(), Value::String("L1".to_string())));
            })
            .starts_with(
                "not a prompt table: `PlayStation.buttons` has `LeftBumbper`, which is no button"
            )
        );
        assert!(
            refused(
                &|table| members(at(table, &["Xbox"])).push(("paddles".to_string(), Value::Null))
            )
            .starts_with("not a prompt table: `Xbox` has `paddles`, which is no part")
        );
        assert_eq!(
            refused(&|table| *at(table, &["Nintendo", "triggers", "LeftTrigger"]) = Value::Int(2)),
            "not a prompt table: `Nintendo.triggers.LeftTrigger` is not a string"
        );
        assert_eq!(
            refused(&|table| *at(table, &["Xbox", "sticks"]) = Value::Array(Vec::new())),
            "not a prompt table: `Xbox.sticks` is not an object"
        );
        assert_eq!(
            refused(&|table| *at(table, &["Xbox"]) = Value::Bool(true)),
            "not a prompt table: `Xbox` is not an object"
        );
        assert_eq!(
            refused(&|table| *table = Value::Array(Vec::new())),
            "not a prompt table: the table is not an object"
        );
        // Bytes that are not KBJ are the codec's to refuse.
        assert!(matches!(decode(b"not KBJ"), Err(AssetError::Decode(_))));
    }

    #[test]
    fn the_cooked_table_loads_from_the_asset_folder() {
        let root = env::temp_dir().join(format!("nadir-engine-prompts-{}", process::id()));
        // A folder left by an earlier run of this process's id.
        let _ = fs::remove_dir_all(&root);
        let path = root.join(ButtonPrompts::PATH);
        fs::create_dir_all(path.parent().expect("a folder")).expect("make the folder");
        fs::write(&path, cook(&sandbox_table())).expect("write the table");
        let mut server = AssetServer::new(&root, OnFailure::Nothing);
        let handle = server
            .load_now::<ButtonPrompts>(ButtonPrompts::PATH, &mut FinalizeCtx::new())
            .expect("the table loads");
        let prompts = server.get(handle).expect("made");
        assert_eq!(
            prompts.button(GamepadFamily::PlayStation, GamepadButton::Y),
            "\u{25b3}"
        );
        drop(server);
        fs::remove_dir_all(&root).expect("remove the folder");
    }

    #[test]
    fn gamepads_follow_the_pads_connected() {
        let (one, two, three) = (
            GamepadId::from_raw(1),
            GamepadId::from_raw(2),
            GamepadId::from_raw(3),
        );
        let family = |id: GamepadId| match id.raw() {
            1 => Some(GamepadFamily::PlayStation),
            2 => Some(GamepadFamily::Xbox),
            // Gone again before its family was asked.
            _ => None,
        };
        let mut pads = Gamepads::default();
        pads.handle(
            &[
                Event::GamepadConnected(one),
                Event::Focused(true),
                Event::GamepadConnected(two),
                Event::GamepadConnected(three),
                Event::GamepadDisconnected(three),
            ],
            family,
        );
        assert_eq!(
            pads.iter().collect::<Vec<_>>(),
            [
                (one, GamepadFamily::PlayStation),
                (two, GamepadFamily::Xbox)
            ]
        );
        assert_eq!(pads.family(two), Some(GamepadFamily::Xbox));
        assert_eq!(pads.family(three), None);
        pads.handle(&[Event::GamepadDisconnected(one)], family);
        assert_eq!(pads.family(one), None);
        assert_eq!(
            pads.iter().collect::<Vec<_>>(),
            [(two, GamepadFamily::Xbox)]
        );
    }
}
