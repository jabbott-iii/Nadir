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

//! The controls page's bindings: the binding each row shows and rebinds,
//! its name for the player, and placing a new one, swapped with the row
//! that had its key or button.
//!
//! Only the gameplay actions, [`Act::Move`] and [`Act::Pause`], are
//! rebound. The menus' own actions keep their keys, which they share with
//! the gameplay's on purpose (the arrows move the person and the focus), so
//! a conflict is looked for among the rows alone.

use std::fmt::Write as _;

use nadir::prelude::*;

use crate::Act;

/// Which part of a binding a row shows: all of it, or one key of a
/// composite ([`Binding::KeyComposite2`]), which is rebound a key at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Whole,
    Up,
    Down,
    Left,
    Right,
}

/// A row's binding: an action's slot, or one key of the composite there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Place {
    pub(crate) action: Act,
    pub(crate) slot: usize,
    pub(crate) part: Part,
}

const fn place(action: Act, slot: usize, part: Part) -> Place {
    Place { action, slot, part }
}

/// The rows of the controls page, top to bottom, with their labels: every
/// gameplay binding the sandbox has ([`crate::actions`]).
pub(crate) const ROWS: [(&str, Place); 11] = [
    ("Move up", place(Act::Move, 0, Part::Up)),
    ("Move down", place(Act::Move, 0, Part::Down)),
    ("Move left", place(Act::Move, 0, Part::Left)),
    ("Move right", place(Act::Move, 0, Part::Right)),
    ("Move up, 2nd", place(Act::Move, 1, Part::Up)),
    ("Move down, 2nd", place(Act::Move, 1, Part::Down)),
    ("Move left, 2nd", place(Act::Move, 1, Part::Left)),
    ("Move right, 2nd", place(Act::Move, 1, Part::Right)),
    ("Move, pad", place(Act::Move, 2, Part::Whole)),
    ("Pause", place(Act::Pause, 0, Part::Whole)),
    ("Pause, pad", place(Act::Pause, 1, Part::Whole)),
];

impl Place {
    /// The kind of press a rebinding of the row listens for
    /// ([`Input::begin_rebind`]): a stick for a direction, else a key or a
    /// button.
    pub(crate) fn listens_for(self) -> ActionKind {
        match self.part {
            Part::Whole => self.action.kind(),
            Part::Up | Part::Down | Part::Left | Part::Right => ActionKind::Button,
        }
    }

    /// The row's binding now: the slot's, or the composite's key there.
    pub(crate) fn get(self, map: &ActionMap<Act>) -> Option<Binding> {
        let binding = map.bindings(self.action)[self.slot];
        match (self.part, binding) {
            (Part::Whole, binding) => binding,
            (
                part,
                Some(Binding::KeyComposite2 {
                    up,
                    down,
                    left,
                    right,
                }),
            ) => Some(Binding::Key(match part {
                Part::Up => up,
                Part::Down => down,
                Part::Left => left,
                _ => right,
            })),
            _ => None,
        }
    }

    /// Makes the row's binding `binding`, or none. A part of a composite
    /// takes a key alone, and only where the slot has a composite.
    fn set(self, map: &mut ActionMap<Act>, binding: Option<Binding>) -> Result<(), ()> {
        if self.part == Part::Whole {
            match binding {
                Some(binding) => map.bind(self.action, self.slot, binding).map_err(|_| ())?,
                None => map.unbind(self.action, self.slot),
            };
            return Ok(());
        }
        let (
            Some(Binding::Key(key)),
            Some(Binding::KeyComposite2 {
                up,
                down,
                left,
                right,
            }),
        ) = (binding, map.bindings(self.action)[self.slot])
        else {
            return Err(());
        };
        let [mut up, mut down, mut left, mut right] = [up, down, left, right];
        *match self.part {
            Part::Up => &mut up,
            Part::Down => &mut down,
            Part::Left => &mut left,
            _ => &mut right,
        } = key;
        let composite = Binding::KeyComposite2 {
            up,
            down,
            left,
            right,
        };
        map.bind(self.action, self.slot, composite)
            .map_err(|_| ())?;
        Ok(())
    }
}

/// What placing a binding did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Placed {
    /// It is the row's, and no other row had its key or button.
    Bound,
    /// It is the row's, and the other row that had it took the row's old
    /// binding, or none.
    Swapped(Place),
    /// It was the row's already.
    Same,
    /// The other row that has it cannot take the row's old binding (a stick
    /// cannot take a key): nothing changed.
    Taken(Place),
    /// A key of a composite takes a key alone: nothing changed.
    KeysOnly,
}

/// Makes `binding` the binding of the row at `at`. If another row has its
/// key or button, the two swap: the other takes the row's old binding, or
/// none if it had none. Nothing changes if that cannot be done.
pub(crate) fn rebind(map: &mut ActionMap<Act>, at: Place, binding: Binding) -> Placed {
    if at.part != Part::Whole && !matches!(binding, Binding::Key(_)) {
        return Placed::KeysOnly;
    }
    let old = at.get(map);
    if old == Some(binding) {
        return Placed::Same;
    }
    let other = ROWS.iter().map(|&(_, place)| place).find(|&other| {
        other != at
            && other
                .get(map)
                .is_some_and(|theirs| shares(&theirs, &binding))
    });
    let mut next = map.clone();
    if let Some(other) = other
        && other.set(&mut next, old).is_err()
    {
        return Placed::Taken(other);
    }
    if at.set(&mut next, Some(binding)).is_err() {
        return Placed::Taken(at);
    }
    *map = next;
    other.map_or(Placed::Bound, Placed::Swapped)
}

/// Whether two of the rows' bindings read the same key or button: a stick
/// shares each half of its axes.
fn shares(a: &Binding, b: &Binding) -> bool {
    let half_of = |axis: GamepadAxis, stick: Stick| stick.axes().contains(&axis);
    match (*a, *b) {
        (Binding::PadAxis { axis, .. }, Binding::PadStick(stick))
        | (Binding::PadStick(stick), Binding::PadAxis { axis, .. }) => half_of(axis, stick),
        _ => a == b,
    }
}

/// Writes the player's name for `binding` into `out`: a key by its name, a
/// pad's button, trigger or stick by what `family`'s pads print on it
/// (`prompts`, or the engine's name for it where the table did not load),
/// and "None".
pub(crate) fn name(
    binding: Option<Binding>,
    prompts: Option<&ButtonPrompts>,
    family: GamepadFamily,
    out: &mut String,
) {
    // Writing to a `String` cannot fail.
    let _ = match binding {
        None => write!(out, "None"),
        Some(Binding::Key(key)) => write!(out, "{key:?}"),
        Some(Binding::MouseButton(button)) => write!(out, "Mouse {button:?}"),
        Some(Binding::KeyComposite2 {
            up,
            down,
            left,
            right,
        }) => write!(out, "{up:?} {left:?} {down:?} {right:?}"),
        Some(Binding::PadButton(button)) => match prompts {
            Some(prompts) => write!(out, "{}", prompts.button(family, button)),
            None => write!(out, "{button:?}"),
        },
        Some(Binding::PadStick(stick)) => match prompts {
            Some(prompts) => write!(out, "{}", prompts.stick(family, stick)),
            None => write!(out, "{stick:?} stick"),
        },
        Some(Binding::PadAxis { axis, sign }) => {
            let trigger = prompts.and_then(|prompts| prompts.trigger(family, axis));
            match (trigger, stick_of(axis)) {
                (Some(label), _) => write!(out, "{label}"),
                (None, Some(stick)) => {
                    let way = match (axis, sign) {
                        (GamepadAxis::LeftX | GamepadAxis::RightX, AxisSign::Negative) => "left",
                        (GamepadAxis::LeftX | GamepadAxis::RightX, AxisSign::Positive) => "right",
                        (_, AxisSign::Negative) => "up",
                        (_, AxisSign::Positive) => "down",
                    };
                    match prompts {
                        Some(prompts) => write!(out, "{} {way}", prompts.stick(family, stick)),
                        None => write!(out, "{stick:?} stick {way}"),
                    }
                }
                (None, None) => write!(out, "{axis:?}"),
            }
        }
    };
}

/// The stick `axis` is one of, or `None` for a trigger.
fn stick_of(axis: GamepadAxis) -> Option<Stick> {
    Stick::ALL
        .iter()
        .copied()
        .find(|stick| stick.axes().contains(&axis))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> ActionMap<Act> {
        crate::actions().expect("the sandbox's bindings fit")
    }

    fn row(label: &str) -> Place {
        ROWS.iter()
            .find(|(name, _)| *name == label)
            .map(|&(_, place)| place)
            .expect("a row")
    }

    fn key(key: Key) -> Binding {
        Binding::Key(key)
    }

    #[test]
    fn every_row_shows_a_binding_of_the_defaults_and_listens_for_its_kind() {
        let map = map();
        let shown: Vec<_> = ROWS.iter().map(|(_, place)| place.get(&map)).collect();
        assert_eq!(
            shown,
            [
                Some(key(Key::W)),
                Some(key(Key::S)),
                Some(key(Key::A)),
                Some(key(Key::D)),
                Some(key(Key::Up)),
                Some(key(Key::Down)),
                Some(key(Key::Left)),
                Some(key(Key::Right)),
                Some(Binding::PadStick(Stick::Left)),
                Some(key(Key::Escape)),
                Some(Binding::PadButton(GamepadButton::Start)),
            ]
        );
        assert_eq!(row("Move, pad").listens_for(), ActionKind::Axis2);
        assert_eq!(row("Move up").listens_for(), ActionKind::Button);
        assert_eq!(row("Pause").listens_for(), ActionKind::Button);
    }

    #[test]
    fn a_free_key_is_bound_in_place_of_the_old() {
        let mut map = map();
        assert_eq!(rebind(&mut map, row("Move up"), key(Key::I)), Placed::Bound);
        assert_eq!(row("Move up").get(&map), Some(key(Key::I)));
        assert_eq!(
            row("Move down").get(&map),
            Some(key(Key::S)),
            "its neighbors stay"
        );
        assert_eq!(rebind(&mut map, row("Move up"), key(Key::I)), Placed::Same);
    }

    #[test]
    fn a_taken_key_swaps_with_its_row_in_the_composite_or_out_of_it() {
        let mut map = map();
        // Within the composite: up takes D, right takes W.
        let up = row("Move up");
        assert_eq!(
            rebind(&mut map, up, key(Key::D)),
            Placed::Swapped(row("Move right"))
        );
        assert_eq!(row("Move right").get(&map), Some(key(Key::W)));
        // Across the slots, and with Pause.
        assert_eq!(
            rebind(&mut map, up, key(Key::Up)),
            Placed::Swapped(row("Move up, 2nd"))
        );
        assert_eq!(row("Move up, 2nd").get(&map), Some(key(Key::D)));
        assert_eq!(
            rebind(&mut map, row("Pause"), key(Key::Up)),
            Placed::Swapped(up)
        );
        assert_eq!(up.get(&map), Some(key(Key::Escape)));
    }

    #[test]
    fn a_row_that_cannot_take_the_old_binding_leaves_everything_as_it_was() {
        let mut map = map();
        let before = map.clone();
        // The stick's half is the pad's move: it cannot take Escape.
        let half = Binding::PadAxis {
            axis: GamepadAxis::LeftY,
            sign: AxisSign::Negative,
        };
        assert_eq!(
            rebind(&mut map, row("Pause"), half),
            Placed::Taken(row("Move, pad"))
        );
        // A key of a composite takes keys alone.
        let button = Binding::PadButton(GamepadButton::A);
        assert_eq!(rebind(&mut map, row("Move up"), button), Placed::KeysOnly);
        for &(_, place) in &ROWS {
            assert_eq!(place.get(&map), place.get(&before));
        }
        // The pad's pause swaps with nothing: it takes the free X button.
        let x = Binding::PadButton(GamepadButton::X);
        assert_eq!(rebind(&mut map, row("Pause, pad"), x), Placed::Bound);
        // Given to the keyboard's pause, the other row is left with none.
        assert_eq!(
            rebind(&mut map, row("Pause"), x),
            Placed::Swapped(row("Pause, pad"))
        );
        assert_eq!(row("Pause, pad").get(&map), Some(key(Key::Escape)));
    }

    fn named(binding: Option<Binding>, prompts: Option<&ButtonPrompts>) -> String {
        let mut out = String::new();
        name(binding, prompts, GamepadFamily::PlayStation, &mut out);
        out
    }

    #[test]
    fn a_binding_is_named_as_the_player_knows_it() {
        let half = |axis, sign| Some(Binding::PadAxis { axis, sign });
        let cases = [
            (None, "None"),
            (Some(key(Key::LeftShift)), "LeftShift"),
            (
                Some(Binding::MouseButton(MouseButton::Right)),
                "Mouse Right",
            ),
            (Some(Binding::PadButton(GamepadButton::A)), "A"),
            (Some(Binding::PadStick(Stick::Right)), "Right stick"),
            (
                half(GamepadAxis::LeftY, AxisSign::Negative),
                "Left stick up",
            ),
            (
                half(GamepadAxis::RightX, AxisSign::Positive),
                "Right stick right",
            ),
            (
                half(GamepadAxis::LeftTrigger, AxisSign::Positive),
                "LeftTrigger",
            ),
        ];
        for (binding, expected) in cases {
            assert_eq!(named(binding, None), expected, "{binding:?}");
        }
    }

    #[test]
    fn with_the_cooked_prompts_a_pad_s_binding_is_named_as_its_family_prints_it() {
        let app = crate::tests::scene(crate::Scene::Field);
        let mut server = app.world().resource_mut::<AssetServer>();
        let prompts = server
            .load_now::<ButtonPrompts>(ButtonPrompts::PATH, &mut FinalizeCtx::new())
            .expect("the cooked prompts load");
        let prompts = server.get(prompts);
        assert_eq!(
            named(Some(Binding::PadButton(GamepadButton::A)), prompts),
            "\u{2715}"
        );
        let trigger = Binding::PadAxis {
            axis: GamepadAxis::RightTrigger,
            sign: AxisSign::Positive,
        };
        assert_eq!(named(Some(trigger), prompts), "R2");
        let up = Binding::PadAxis {
            axis: GamepadAxis::LeftY,
            sign: AxisSign::Negative,
        };
        assert_eq!(named(Some(up), prompts), "Left stick up");
    }
}
