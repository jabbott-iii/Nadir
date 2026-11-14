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

//! The debug UI's logic with no GPU: synthetic input in, what the widgets
//! said and what the frame pushed out (`debug-tools.md`, "Tests").
//!
//! The test font is `metrics::tests::Fixed`: 8 high, each character 6
//! wide and a space 3. A row is then 12 high (2 of padding above and
//! below), with 2 between rows. The window `W` is at (0, 0) to (200, 150):
//! its title bar is 12 high, its rows start at y 14 and are 14 apart, from
//! x 2, and a control takes 117 of its 196 units, its label from x 121.

use std::num::NonZeroU32;

use nadir_core::{Color, GenIndex, Handle, Key, MouseButton, Rect, UVec2, Vec2};
use nadir_render::{SortMode, UiList, UiScale, UiView};

use crate::metrics::tests::Fixed;
use crate::{DebugInput, DebugUi, Frame, Plot, Response, Ui};

const SIZE: UVec2 = UVec2::new(400, 300);
const WINDOW: Rect = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(200.0, 150.0));

fn handle<T>(index: u32) -> Handle<T> {
    Handle::new(GenIndex::new(index, NonZeroU32::MIN))
}

/// The point at `x` in the middle of row `row` of the window `W`.
fn row(row: u32, x: f32) -> Vec2 {
    Vec2::new(x, 20.0 + 14.0 * row as f32)
}

struct Harness {
    ui: DebugUi,
    input: DebugInput,
    out: UiList,
    size: UVec2,
}

impl Harness {
    fn new() -> Self {
        Self {
            ui: DebugUi::new(handle(0), Fixed, handle(1)),
            input: DebugInput::new(),
            out: UiList::new(),
            size: SIZE,
        }
    }

    /// A frame of `windows`, finished into `out`.
    fn frame(&mut self, windows: impl FnOnce(&mut Frame<'_>)) {
        let mut frame = self.ui.frame(&mut self.input, self.size, self.size);
        windows(&mut frame);
        self.out.clear();
        frame.finish(&mut self.out);
    }

    /// A frame of the window `W`, filled by `add`.
    fn show(&mut self, add: impl FnOnce(&mut Ui<'_>)) {
        self.frame(|frame| {
            frame.window("W", WINDOW, add);
        });
    }

    fn move_to(&mut self, at: Vec2) {
        self.input.pointer_moved(at);
    }

    fn press(&mut self, at: Vec2) {
        self.input.pointer_moved(at);
        self.input.button(MouseButton::Left, true);
    }

    fn release(&mut self, at: Vec2) {
        self.input.pointer_moved(at);
        self.input.button(MouseButton::Left, false);
    }

    fn click(&mut self, at: Vec2) {
        self.press(at);
        self.release(at);
    }

    fn tap(&mut self, key: Key) {
        self.input.key(key, true, false);
        self.input.key(key, false, false);
    }

    /// The texts the last frame pushed, with their positions.
    fn texts(&self) -> Vec<(String, Vec2)> {
        self.out
            .iter()
            .filter_map(|(draw, _)| match draw {
                nadir_render::UiDraw::Text(draw, text) => Some((text.to_string(), draw.position)),
                _ => None,
            })
            .collect()
    }
}

/// Draws a button `Go` for each frame of `script`: each a closure that
/// queues input after the frame before. The button's response of each
/// frame.
fn button_frames(script: &[&dyn Fn(&mut Harness)]) -> Vec<Response> {
    let mut harness = Harness::new();
    let mut responses = Vec::new();
    for step in script {
        step(&mut harness);
        let mut response = Response::default();
        harness.show(|ui| response = ui.button("Go"));
        responses.push(response);
    }
    responses
}

#[test]
fn a_button_clicks_on_release_only_when_the_press_started_on_it() {
    // The button `Go` is 20 wide: (2, 14) to (22, 26).
    let on = row(0, 10.0);
    let off = row(3, 100.0);
    let nothing: &dyn Fn(&mut Harness) = &|_| {};
    // Pressed and released on it, in one frame or two.
    let one = button_frames(&[nothing, &|h| h.click(on)]);
    assert!(one[1].clicked);
    let two = button_frames(&[nothing, &|h| h.press(on), &|h| h.release(on)]);
    assert!(!two[1].clicked && two[1].active);
    assert!(two[2].clicked);
    // Pressed elsewhere, released on it.
    let elsewhere = button_frames(&[nothing, &|h| h.press(off), &|h| h.release(on)]);
    assert!(elsewhere.iter().all(|response| !response.clicked));
    // Pressed on it, released elsewhere.
    let away = button_frames(&[nothing, &|h| h.press(on), &|h| h.release(off)]);
    assert!(away.iter().all(|response| !response.clicked));
    // In one frame: pressed elsewhere then released on it, or pressed on it
    // then released elsewhere. Each edge is judged where it happened.
    let late = button_frames(&[nothing, &|h| {
        h.press(off);
        h.release(on);
    }]);
    assert!(!late[1].clicked);
    let flick = button_frames(&[nothing, &|h| {
        h.press(on);
        h.release(off);
    }]);
    assert!(!flick[1].clicked);
    // Hovered while nothing is held.
    let hovered = button_frames(&[nothing, &|h| h.move_to(on)]);
    assert!(hovered[1].hovered && !hovered[1].clicked);
}

#[test]
fn a_window_in_front_takes_the_pointer_from_the_one_behind() {
    let mut harness = Harness::new();
    let mut clicked = false;
    let front = Rect::new(Vec2::new(5.0, 5.0), Vec2::new(100.0, 100.0));
    for _ in 0..2 {
        harness.frame(|frame| {
            frame.window("W", WINDOW, |ui| clicked |= ui.button("Go").clicked);
            frame.window("Front", front, |_| {});
        });
        harness.click(row(0, 10.0));
    }
    assert!(!clicked);
    assert!(harness.ui.wants_mouse());
}

#[test]
fn a_frame_dropped_unfinished_spends_its_input() {
    let mut harness = Harness::new();
    harness.show(|ui| {
        ui.button("Go");
    });
    harness.click(row(0, 10.0));
    let mut clicks = 0;
    for finished in [false, true] {
        let mut frame = harness.ui.frame(&mut harness.input, SIZE, SIZE);
        frame.window("W", WINDOW, |ui| {
            clicks += u32::from(ui.button("Go").clicked)
        });
        if finished {
            frame.finish(&mut harness.out);
        }
    }
    assert_eq!(clicks, 1);
}

/// Draws a slider for each frame of `script`, as `button_frames` does;
/// the value after each frame.
fn slide<T: Copy>(
    start: T,
    draw: impl Fn(&mut Ui<'_>, &mut T),
    script: &[&dyn Fn(&mut Harness)],
) -> Vec<T> {
    let mut harness = Harness::new();
    let mut value = start;
    let mut values = Vec::new();
    for step in script {
        step(&mut harness);
        harness.show(|ui| draw(ui, &mut value));
        values.push(value);
    }
    values
}

#[test]
fn a_slider_holds_its_value_in_its_range() {
    // The slider's control is (2, 14) to (119, 26): 117 wide.
    let nothing: &dyn Fn(&mut Harness) = &|_| {};
    let script: [&dyn Fn(&mut Harness); 5] = [
        nothing,
        &|h| h.press(row(0, 60.0)),
        &|h| h.move_to(row(0, 1000.0)),
        &|h| h.move_to(row(0, -50.0)),
        &|h| h.release(row(0, -50.0)),
    ];
    let slider = |range: std::ops::RangeInclusive<f32>| {
        move |ui: &mut Ui<'_>, value: &mut f32| {
            ui.slider_f32("S", value, range.clone());
        }
    };
    let values = slide(3.0, slider(0.0..=10.0), &script);
    assert!((values[1] - 58.0 / 117.0 * 10.0).abs() < 1e-4, "{values:?}");
    assert_eq!(values[2..], [10.0, 0.0, 0.0]);
    // A reversed range is the same range.
    assert_eq!(
        slide(3.0, slider(10.0..=0.0), &script)[2..],
        [10.0, 0.0, 0.0]
    );
    // A NaN bound changes nothing.
    let nan = slide(3.0, slider(0.0..=f32::NAN), &script);
    assert!(nan.iter().all(|&value| value == 3.0));
    // A value out of its range is left alone until the slider is moved.
    let kept = slide(
        50.0,
        slider(0.0..=10.0),
        &[nothing, &|h| h.move_to(row(0, 60.0))],
    );
    assert_eq!(kept, [50.0, 50.0]);

    // Whole numbers, over every i32: the ends exactly, and rounded between.
    let whole = |range: std::ops::RangeInclusive<i32>| {
        move |ui: &mut Ui<'_>, value: &mut i32| {
            ui.slider_i32("I", value, range.clone());
        }
    };
    let values = slide(0, whole(i32::MIN..=i32::MAX), &script);
    assert_eq!(values[2..4], [i32::MAX, i32::MIN]);
    assert_eq!(
        slide(0, whole(0..=10), &script)[1],
        5,
        "58/117 of 10 is 4.96"
    );
    let reversed = std::ops::RangeInclusive::new(10, 0);
    assert_eq!(slide(0, whole(reversed), &script)[2..4], [10, 0]);
}

/// A harness with the text field `T` (its control (2, 14) to (119, 26))
/// and a button `Go` under it, drawn by `field` each frame.
struct Field {
    harness: Harness,
    value: String,
    responses: Vec<Response>,
}

impl Field {
    fn new(value: &str) -> Self {
        let mut field = Self {
            harness: Harness::new(),
            value: value.to_string(),
            responses: Vec::new(),
        };
        field.frame(true);
        field
    }

    /// A frame, with the field drawn or not.
    fn frame(&mut self, drawn: bool) -> Response {
        let mut response = Response::default();
        let value = &mut self.value;
        self.harness.show(|ui| {
            if drawn {
                response = ui.text_input("T", value);
            }
            ui.button("Go");
        });
        self.responses.push(response);
        response
    }

    fn focus(&mut self) -> Response {
        self.harness.click(row(0, 10.0));
        self.frame(true)
    }

    fn typed(&mut self, text: &str) -> Response {
        self.harness.input.text(text);
        self.frame(true)
    }

    fn keys(&mut self, keys: &[Key]) -> Response {
        for &key in keys {
            self.harness.tap(key);
        }
        self.frame(true)
    }
}

#[test]
fn a_text_field_edits_its_text() {
    let mut field = Field::new("");
    assert!(!field.harness.ui.wants_keyboard());
    field.harness.click(row(0, 10.0));
    field.harness.input.text("abc");
    assert!(field.frame(true).changed);
    assert!(field.harness.ui.wants_keyboard());
    assert_eq!(field.value, "abc");

    field.keys(&[Key::Left, Key::Left]);
    field.typed("X");
    assert_eq!(field.value, "aXbc");
    field.keys(&[Key::Backspace]);
    assert_eq!(field.value, "abc");
    field.keys(&[Key::Home, Key::Delete]);
    assert_eq!(field.value, "bc");

    // Shift and End select to the end; typing replaces it.
    field.harness.input.key(Key::LeftShift, true, false);
    field.harness.tap(Key::End);
    field.harness.input.key(Key::LeftShift, false, false);
    field.typed("Z");
    assert_eq!(field.value, "Z");
    // Control and A select all.
    field.harness.input.key(Key::LeftControl, true, false);
    field.harness.tap(Key::A);
    field.harness.input.key(Key::LeftControl, false, false);
    field.typed("héé");
    assert_eq!(field.value, "héé");
    field.keys(&[Key::Backspace]);
    assert_eq!(field.value, "hé");

    // A key repeated is pressed again.
    field.harness.input.key(Key::Backspace, true, false);
    field.harness.input.key(Key::Backspace, true, true);
    field.harness.input.key(Key::Backspace, false, false);
    field.frame(true);
    assert_eq!(field.value, "");
}

#[test]
fn enter_submits_and_keeps_the_keyboard_and_escape_or_a_press_elsewhere_ends_it() {
    let mut field = Field::new("go");
    field.focus();
    let enter = field.keys(&[Key::Enter]);
    assert!(enter.submitted && !enter.changed && !enter.lost_focus);
    assert!(field.harness.ui.wants_keyboard());
    let escape = field.keys(&[Key::Escape]);
    assert!(escape.lost_focus);
    assert!(!field.harness.ui.wants_keyboard());
    // Typing with no focus does nothing.
    field.typed("x");
    assert_eq!(field.value, "go");

    field.focus();
    field.harness.click(row(1, 10.0));
    assert!(field.frame(true).lost_focus);
    assert!(!field.harness.ui.wants_keyboard());
    // A press outside every window ends it too.
    field.focus();
    field.harness.click(Vec2::new(300.0, 250.0));
    assert!(field.frame(true).lost_focus);
}

#[test]
fn keys_a_field_does_not_use_reach_its_caller() {
    let mut field = Field::new("");
    field.focus();
    field.harness.tap(Key::Up);
    field.harness.tap(Key::Tab);
    field.harness.input.text("\t");
    let mut seen = (false, false, false);
    let value = &mut field.value;
    field.harness.show(|ui| {
        ui.text_input("T", value);
        seen = (
            ui.key_pressed(Key::Up),
            ui.key_pressed(Key::Tab),
            ui.key_pressed(Key::Down),
        );
    });
    assert_eq!(seen, (true, true, false));
    assert_eq!(field.value, "", "a tab is not typed");
}

#[test]
fn a_text_the_caller_changes_moves_the_caret_to_its_end() {
    let mut field = Field::new("");
    field.focus();
    field.typed("é");
    // The caret was at byte 2; the text is now one byte long.
    field.value = "a".to_string();
    field.typed("b");
    assert_eq!(field.value, "ab");
    // Changed while a selection is held.
    field.harness.input.key(Key::LeftShift, true, false);
    field.harness.tap(Key::Home);
    field.harness.input.key(Key::LeftShift, false, false);
    field.frame(true);
    field.value = "xyz".to_string();
    field.typed("!");
    assert_eq!(field.value, "xyz!");
}

#[test]
fn a_field_not_drawn_lets_the_keyboard_go_and_focus_next_takes_it() {
    let mut field = Field::new("");
    field.focus();
    assert!(field.harness.ui.wants_keyboard());
    field.frame(false);
    assert!(!field.harness.ui.wants_keyboard());
    field.typed("a");
    assert_eq!(field.value, "", "the field has no keyboard");

    let value = &mut field.value;
    field.harness.show(|ui| {
        ui.focus_next();
        ui.text_input("T", value);
    });
    assert!(field.harness.ui.wants_keyboard());
    field.typed("a");
    assert_eq!(field.value, "a");
}

#[test]
fn a_click_puts_the_caret_where_it_was() {
    let mut field = Field::new("abcd");
    // The text starts at x 4: c starts at 16, d at 22.
    field.harness.click(row(0, 21.0));
    field.harness.input.text("-");
    field.frame(true);
    assert_eq!(field.value, "abc-d");
}

/// The window `W`'s place.
fn place_of(ui: &DebugUi, name: &str) -> (Rect, bool) {
    let place = ui
        .places()
        .find(|place| place.title == name)
        .expect("a place");
    (place.rect, place.collapsed)
}

#[test]
fn a_window_drags_by_its_title_bar_and_resizes_by_its_grip() {
    let mut harness = Harness::new();
    harness.show(|_| {});
    let revision = harness.ui.places_revision();
    harness.press(Vec2::new(50.0, 5.0));
    harness.show(|_| {});
    harness.move_to(Vec2::new(80.0, 25.0));
    harness.show(|_| {});
    let moved = Rect::new(Vec2::new(30.0, 20.0), Vec2::new(230.0, 170.0));
    assert_eq!(place_of(&harness.ui, "W").0, moved);
    assert_eq!(harness.ui.places_revision(), revision, "while held");
    harness.release(Vec2::new(80.0, 25.0));
    harness.show(|_| {});
    assert_eq!(harness.ui.places_revision(), revision + 1);

    // The grip, at the bottom-right corner, 6 a side.
    harness.press(Vec2::new(227.0, 167.0));
    harness.show(|_| {});
    harness.move_to(Vec2::new(127.0, 117.0));
    harness.show(|_| {});
    assert_eq!(place_of(&harness.ui, "W").0.size(), Vec2::new(100.0, 100.0));
    // No smaller than the style's smallest.
    harness.move_to(Vec2::new(0.0, 0.0));
    harness.show(|_| {});
    assert_eq!(place_of(&harness.ui, "W").0.size(), Vec2::new(60.0, 40.0));
    harness.release(Vec2::ZERO);
    harness.show(|_| {});
    assert_eq!(harness.ui.places_revision(), revision + 2);
}

#[test]
fn a_collapsed_window_is_its_title_bar_and_fills_nothing() {
    let mut harness = Harness::new();
    let mut open = (true, false);
    harness.frame(|frame| open.0 = frame.window("W", WINDOW, |_| open.1 = true));
    assert_eq!(open, (true, true));
    harness.click(Vec2::new(6.0, 6.0));
    open = (true, false);
    harness.frame(|frame| open.0 = frame.window("W", WINDOW, |_| open.1 = true));
    assert_eq!(open, (false, false));
    assert!(place_of(&harness.ui, "W").1);
    // Below its title bar is the game's.
    harness.move_to(Vec2::new(50.0, 100.0));
    harness.show(|_| {});
    assert!(!harness.ui.wants_mouse());
    harness.move_to(Vec2::new(50.0, 6.0));
    harness.show(|_| {});
    assert!(harness.ui.wants_mouse());
}

#[test]
fn the_wheel_and_the_thumb_scroll_a_tall_window_and_hold_it_in_range() {
    let mut harness = Harness::new();
    let rows = |ui: &mut Ui<'_>| {
        for index in 0..30 {
            ui.label(format_args!("row {index}"));
        }
    };
    harness.show(rows);
    harness.show(rows);
    // 30 rows from y 14, 14 apart, and 2 of padding below: 424 high in a
    // view of 138, so it scrolls 286.
    harness.move_to(Vec2::new(50.0, 50.0));
    harness.input.wheel(Vec2::new(0.0, -100.0));
    harness.show(rows);
    let last = harness
        .texts()
        .into_iter()
        .find(|(text, _)| text == "row 29")
        .expect("the last row drawn");
    assert_eq!(last.1.y, 14.0 + 29.0 * 14.0 - 286.0 + 2.0);
    // Rows out of view are not drawn.
    assert!(harness.texts().iter().all(|(text, _)| text != "row 0"));
    harness.input.wheel(Vec2::new(0.0, 100.0));
    harness.show(rows);
    assert!(
        harness
            .texts()
            .iter()
            .any(|(text, at)| text == "row 0" && at.y == 16.0)
    );

    // The thumb, at the top of the track (194, 12) to (200, 144): moved by
    // its room, it scrolls all the way.
    harness.press(Vec2::new(197.0, 20.0));
    harness.show(rows);
    harness.move_to(Vec2::new(197.0, 400.0));
    harness.show(rows);
    let last = harness
        .texts()
        .into_iter()
        .find(|(text, _)| text == "row 29")
        .expect("the last row drawn");
    assert_eq!(last.1.y, 14.0 + 29.0 * 14.0 - 286.0 + 2.0);
    harness.release(Vec2::new(197.0, 400.0));
    harness.show(rows);
}

#[test]
fn a_window_pressed_comes_to_the_front() {
    let mut harness = Harness::new();
    let back = Rect::new(Vec2::ZERO, Vec2::new(100.0, 100.0));
    let front = Rect::new(Vec2::new(50.0, 50.0), Vec2::new(150.0, 150.0));
    let draw = |frame: &mut Frame<'_>| {
        frame.window("A", back, |_| {});
        frame.window("B", front, |_| {});
    };
    harness.frame(draw);
    // The panels, back to front: the last pushed is in front.
    let panels = |harness: &Harness| -> Vec<Vec2> {
        harness
            .out
            .iter()
            .filter_map(|(draw, _)| match draw {
                nadir_render::UiDraw::Sprite(sprite) if sprite.size.y == 100.0 => {
                    Some(sprite.position)
                }
                _ => None,
            })
            .collect()
    };
    assert_eq!(panels(&harness), [Vec2::ZERO, Vec2::splat(50.0)]);
    harness.click(Vec2::new(20.0, 20.0));
    harness.frame(draw);
    assert_eq!(panels(&harness), [Vec2::splat(50.0), Vec2::ZERO]);
}

#[test]
fn places_are_data_kept_as_left_and_named_by_the_explicit_id() {
    let mut harness = Harness::new();
    let saved = Rect::new(Vec2::new(10.0, 20.0), Vec2::new(110.0, 120.0));
    harness.ui.set_place("Tools", saved, true);
    // Not finite: ignored. Too small: grown.
    harness
        .ui
        .set_place("Tools", Rect::new(Vec2::splat(f32::NAN), Vec2::ONE), false);
    harness
        .ui
        .set_place("Tiny", Rect::from_min_size(Vec2::ZERO, Vec2::ONE), false);
    assert_eq!(place_of(&harness.ui, "Tools"), (saved, true));
    assert_eq!(
        place_of(&harness.ui, "Tiny").0.size(),
        Vec2::new(60.0, 40.0)
    );
    // Shown later at its place, not at its first rect; its title changes,
    // and it is one window.
    for count in 0..3 {
        harness.frame(|frame| {
            frame.window(format_args!("Tools {count}##Tools"), WINDOW, |_| {});
        });
    }
    assert_eq!(place_of(&harness.ui, "Tools"), (saved, true));
    assert_eq!(harness.ui.places().len(), 2);

    // A screen too small to hold it draws it on screen, and keeps its place.
    harness.size = UVec2::new(50.0 as u32, 30);
    harness.frame(|frame| {
        frame.window("Tools", WINDOW, |_| {});
    });
    assert_eq!(place_of(&harness.ui, "Tools"), (saved, true));
    // No screen at all: nothing shown, nothing wanted.
    harness.size = UVec2::ZERO;
    let mut open = true;
    harness.frame(|frame| open = frame.window("Tools", WINDOW, |_| {}));
    assert!(!open && !harness.ui.wants_mouse());
}

#[test]
fn one_text_in_two_scopes_is_two_widgets_and_an_explicit_id_outlives_its_text() {
    let mut harness = Harness::new();
    let mut clicked = [false; 2];
    let draw = |harness: &mut Harness, clicked: &mut [bool; 2]| {
        harness.show(|ui| {
            for (index, clicked) in clicked.iter_mut().enumerate() {
                ui.with_id(index, |ui| *clicked = ui.button("Go").clicked);
            }
        });
    };
    draw(&mut harness, &mut clicked);
    harness.click(row(1, 10.0));
    draw(&mut harness, &mut clicked);
    assert_eq!(clicked, [false, true]);

    // Sliders of one name under two tree nodes.
    let mut harness = Harness::new();
    let mut values = [0.0f32; 2];
    let trees = |harness: &mut Harness, values: &mut [f32; 2]| {
        harness.show(|ui| {
            for (name, value) in ["A", "B"].into_iter().zip(values.iter_mut()) {
                ui.tree_node(name, |ui| {
                    ui.slider_f32("x", value, 0.0..=10.0);
                });
            }
        });
    };
    trees(&mut harness, &mut values);
    harness.click(row(0, 10.0));
    trees(&mut harness, &mut values);
    harness.click(row(2, 10.0));
    trees(&mut harness, &mut values);
    // A's slider is row 1, B's node row 2 and its slider row 3.
    harness.press(row(1, 60.0));
    trees(&mut harness, &mut values);
    harness.move_to(row(1, 1000.0));
    trees(&mut harness, &mut values);
    assert_eq!(values, [10.0, 0.0]);
    harness.release(row(1, 1000.0));
    trees(&mut harness, &mut values);

    // A button whose text changes between the press and the release.
    let mut harness = Harness::new();
    let mut clicked = false;
    for frame in 0..3 {
        harness.show(|ui| clicked |= ui.button(format_args!("Go {frame}##go")).clicked);
        match frame {
            0 => harness.press(row(0, 10.0)),
            1 => harness.release(row(0, 10.0)),
            _ => {}
        }
    }
    assert!(clicked);
}

#[test]
fn checkboxes_drags_trees_combos_plots_and_colors_answer() {
    let mut harness = Harness::new();
    let mut on = false;
    harness.show(|ui| {
        ui.checkbox("On", &mut on);
    });
    // On its text, after the box.
    harness.click(row(0, 20.0));
    harness.show(|ui| {
        assert!(ui.checkbox("On", &mut on).changed);
    });
    assert!(on);

    // A drag of 10 units right at 0.5 a unit; a NaN step does nothing.
    let mut harness = Harness::new();
    let mut values = (1.0f32, 1.0f32);
    let drags = |harness: &mut Harness, values: &mut (f32, f32)| {
        harness.show(|ui| {
            ui.drag_f32("D", &mut values.0, 0.5);
            ui.drag_f32("N", &mut values.1, f32::NAN);
        });
    };
    drags(&mut harness, &mut values);
    harness.press(row(0, 60.0));
    drags(&mut harness, &mut values);
    harness.move_to(row(0, 70.0));
    drags(&mut harness, &mut values);
    assert_eq!(values.0, 6.0);
    harness.release(row(0, 70.0));
    harness.press(row(1, 60.0));
    drags(&mut harness, &mut values);
    harness.move_to(row(1, 90.0));
    drags(&mut harness, &mut values);
    assert_eq!(values.1, 1.0);

    // A tree node opens, keeps open, and closes.
    let mut harness = Harness::new();
    let mut shown = 0;
    let tree = |harness: &mut Harness, shown: &mut u32| {
        harness.show(|ui| {
            ui.tree_node("Tree", |ui| {
                *shown += 1;
                ui.label("child");
            });
        });
    };
    tree(&mut harness, &mut shown);
    harness.click(row(0, 10.0));
    tree(&mut harness, &mut shown);
    tree(&mut harness, &mut shown);
    assert_eq!(shown, 2);
    harness.click(row(0, 10.0));
    tree(&mut harness, &mut shown);
    tree(&mut harness, &mut shown);
    assert_eq!(shown, 2);
}

#[test]
fn a_combo_opens_picks_and_closes() {
    let items = ["one", "two", "three"];
    let mut harness = Harness::new();
    let mut selected = 0;
    let mut changed = false;
    let combo = |harness: &mut Harness, selected: &mut usize, changed: &mut bool| {
        harness.show(|ui| {
            *changed |= ui.combo("C", selected, &items).changed;
            ui.button("Go");
        });
    };
    combo(&mut harness, &mut selected, &mut changed);
    harness.click(row(0, 10.0));
    combo(&mut harness, &mut selected, &mut changed);
    // Its list: under the combo, a row each, over the button.
    assert!(
        harness
            .texts()
            .iter()
            .any(|(text, at)| text == "three" && at.y == 52.0)
    );
    combo(&mut harness, &mut selected, &mut changed);
    harness.click(Vec2::new(10.0, 44.0));
    combo(&mut harness, &mut selected, &mut changed);
    assert_eq!((selected, changed), (1, true));
    // Drawn in the frame of the pick, closed from the next.
    combo(&mut harness, &mut selected, &mut changed);
    assert!(
        harness.texts().iter().all(|(text, _)| text != "three"),
        "closed"
    );

    // A press elsewhere closes it, and picks nothing.
    harness.click(row(0, 10.0));
    combo(&mut harness, &mut selected, &mut changed);
    harness.click(Vec2::new(300.0, 250.0));
    combo(&mut harness, &mut selected, &mut changed);
    combo(&mut harness, &mut selected, &mut changed);
    assert!(harness.texts().iter().all(|(text, _)| text != "three"));
    assert_eq!(selected, 1);

    // No items, or an index past them: nothing shown, nothing changed.
    let mut harness = Harness::new();
    let mut past = 7;
    harness.show(|ui| {
        ui.combo("E", &mut past, &[]);
        ui.combo("F", &mut past, &items);
    });
    assert_eq!(past, 7);
}

#[test]
fn a_plot_says_which_value_is_under_the_pointer_and_which_was_clicked() {
    // Over (2, 14) to (119, 44): eleven values, 11.7 apart.
    let values: Vec<f32> = (0..=10).map(|v| v as f32).collect();
    let mut harness = Harness::new();
    let mut responses = Vec::new();
    for step in 0..3 {
        let mut response = None;
        harness.show(|ui| {
            let mut plot = Plot::new(0.0..=10.0);
            plot.selected = Some(3);
            response = Some(ui.plot("P", values.iter().copied(), plot));
        });
        responses.push(response.expect("a plot"));
        match step {
            0 => harness.move_to(Vec2::new(60.5, 30.0)),
            1 => harness.click(Vec2::new(118.0, 30.0)),
            _ => {}
        }
    }
    assert_eq!(responses[1].hovered, Some(5));
    assert_eq!(responses[2].clicked, Some(10));

    // NaN values and bounds, and no values, draw and say nothing wrong.
    let mut harness = Harness::new();
    harness.show(|ui| {
        ui.plot(
            "A",
            [1.0, f32::NAN, f32::INFINITY, 2.0],
            Plot::new(f32::NAN..=1.0),
        );
        assert_eq!(ui.plot("B", [], Plot::new(0.0..=1.0)).hovered, None);
    });
    let lines_finite = harness.out.iter().all(|(draw, _)| match draw {
        nadir_render::UiDraw::Sprite(sprite) => {
            sprite.position.x.is_finite()
                && sprite.size.x.is_finite()
                && sprite.rotation.is_finite()
        }
        _ => true,
    });
    assert!(lines_finite);
}

#[test]
fn a_color_edit_sets_each_channel() {
    let mut harness = Harness::new();
    let mut color = Color::rgba(10, 20, 30, 40);
    harness.show(|ui| {
        ui.color_edit("Tint", &mut color);
    });
    // Its swatch row, then R, G, B and A, each control to x 119.
    harness.click(row(1, 118.9));
    harness.show(|ui| {
        assert!(ui.color_edit("Tint", &mut color).changed);
    });
    assert_eq!(color, Color::rgba(255, 20, 30, 40));
    harness.click(row(4, 2.0));
    harness.show(|ui| {
        ui.color_edit("Tint", &mut color);
    });
    assert_eq!(color, Color::rgba(255, 20, 30, 0));
}

#[test]
fn the_mouse_is_the_ui_s_over_its_windows_unless_the_game_pressed_first() {
    let mut harness = Harness::new();
    harness.move_to(Vec2::new(300.0, 250.0));
    harness.show(|_| {});
    assert!(!harness.ui.wants_mouse());
    harness.move_to(Vec2::new(50.0, 50.0));
    harness.show(|_| {});
    assert!(harness.ui.wants_mouse());
    // Pressed outside, then dragged over a window: still the game's.
    harness.press(Vec2::new(300.0, 250.0));
    harness.show(|_| {});
    harness.move_to(Vec2::new(50.0, 50.0));
    let mut hovered = true;
    harness.show(|ui| hovered = ui.button("Go").hovered);
    assert!(!harness.ui.wants_mouse());
    harness.move_to(row(0, 10.0));
    harness.show(|ui| hovered = ui.button("Go").hovered);
    assert!(!hovered);
    // The release is the game's too; the mouse is the UI's from the next
    // frame.
    harness.release(row(0, 10.0));
    harness.show(|ui| hovered = ui.button("Go").hovered);
    assert!(!harness.ui.wants_mouse() && !hovered);
    harness.show(|ui| hovered = ui.button("Go").hovered);
    assert!(harness.ui.wants_mouse() && hovered);
    // A widget held, the pointer gone off every window.
    harness.press(row(0, 10.0));
    harness.show(|ui| {
        ui.button("Go");
    });
    harness.move_to(Vec2::new(300.0, 250.0));
    harness.show(|ui| {
        ui.button("Go");
    });
    assert!(harness.ui.wants_mouse());
    // Losing the focus lets it go, with no click.
    harness.input.focus_lost();
    let mut response = Response::default();
    harness.show(|ui| response = ui.button("Go"));
    assert!(response.active && !response.clicked);
    harness.show(|_| {});
    assert!(!harness.ui.wants_mouse());
}

#[test]
fn a_point_is_over_the_windows_the_last_frame_drew() {
    let mut harness = Harness::new();
    // Before any frame, nothing is drawn.
    assert!(!harness.ui.is_over(Vec2::new(50.0, 50.0)));
    harness.move_to(Vec2::new(300.0, 250.0));
    harness.show(|_| {});
    // The pointer ended outside, but a point over the window is over it.
    assert!(!harness.ui.wants_mouse());
    assert!(harness.ui.is_over(Vec2::new(50.0, 50.0)));
    assert!(!harness.ui.is_over(Vec2::new(300.0, 250.0)));
    // At twice the scale, the window covers twice the output's pixels.
    harness.ui.style_mut().scale = UiScale::Free(2.0);
    harness.show(|_| {});
    assert!(harness.ui.is_over(Vec2::new(390.0, 290.0)));
    harness.ui.style_mut().scale = UiScale::Free(1.0);
    harness.show(|_| {});
    // Collapsed, only its title bar.
    harness.click(Vec2::new(4.0, 6.0));
    harness.show(|_| {});
    harness.show(|_| {});
    assert!(harness.ui.is_over(Vec2::new(50.0, 6.0)));
    assert!(!harness.ui.is_over(Vec2::new(50.0, 50.0)));
    // A window not drawn in the last frame is not hit.
    harness.frame(|_| {});
    assert!(!harness.ui.is_over(Vec2::new(50.0, 6.0)));
}

#[test]
fn a_frame_pushes_its_windows_in_front_of_the_game_s_ui() {
    let mut harness = Harness::new();
    // The game's UI, at the top layer and depth, pushed first.
    let game_view = UiView::new(UiScale::Pixel(1));
    harness.out.set_view(game_view);
    let mut frame = harness.ui.frame(&mut harness.input, SIZE, SIZE);
    frame.window("W", WINDOW, |ui| {
        ui.label("{c:#f00}not markup");
        ui.button("Go");
    });
    frame.finish(&mut harness.out);
    assert_eq!(harness.out.view(), game_view, "the view is put back");
    let mut texts = 0;
    for (draw, view) in harness.out.iter() {
        assert_eq!(view.scale(), UiScale::Free(1.0));
        assert!(view.clip().is_some());
        match draw {
            nadir_render::UiDraw::Sprite(sprite) => {
                assert_eq!(
                    (sprite.layer, sprite.sort),
                    (u8::MAX, SortMode::Fixed(u32::MAX))
                );
            }
            nadir_render::UiDraw::Text(draw, text) => {
                texts += 1;
                assert_eq!(
                    (draw.layer, draw.sort),
                    (u8::MAX, SortMode::Fixed(u32::MAX))
                );
                assert!(!draw.layout.markup, "{text}");
            }
            _ => {}
        }
    }
    // The title, the collapse mark, the label and the button.
    assert_eq!(texts, 4);
}

/// The sprites the last frame pushed: position, size, color.
fn sprites(harness: &Harness) -> Vec<(Vec2, Vec2, Color)> {
    harness
        .out
        .iter()
        .filter_map(|(draw, _)| match draw {
            nadir_render::UiDraw::Sprite(sprite) => {
                Some((sprite.position, sprite.size, sprite.color))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_drag_ends_where_the_button_went_up_however_the_pointer_moves_after() {
    let mut values = Vec::new();
    let mut harness = Harness::new();
    let mut value = 3.0f32;
    for step in 0..3 {
        harness.show(|ui| {
            ui.slider_f32("S", &mut value, 0.0..=10.0);
        });
        values.push(value);
        if step == 0 {
            harness.press(row(0, 10.0));
        } else if step == 1 {
            harness.release(row(0, 60.0));
            harness.move_to(row(0, 1000.0));
        }
    }
    assert!((values[2] - 58.0 / 117.0 * 10.0).abs() < 1e-4, "{values:?}");

    let mut harness = Harness::new();
    harness.show(|_| {});
    harness.press(Vec2::new(50.0, 5.0));
    harness.show(|_| {});
    harness.release(Vec2::new(80.0, 5.0));
    harness.move_to(Vec2::new(300.0, 200.0));
    harness.show(|_| {});
    assert_eq!(place_of(&harness.ui, "W").0.min, Vec2::new(30.0, 0.0));
}

#[test]
fn a_window_in_two_calls_keeps_its_scroll_and_its_second_part_its_clip() {
    let mut harness = Harness::new();
    let draw = |harness: &mut Harness| {
        harness.frame(|frame| {
            frame.window("W", WINDOW, |ui| {
                ui.label("a0");
                ui.label("a1");
            });
            frame.window("W", WINDOW, |ui| {
                for index in 0..30 {
                    ui.label(format_args!("b{index}"));
                }
            });
        });
    };
    draw(&mut harness);
    draw(&mut harness);
    harness.move_to(Vec2::new(50.0, 50.0));
    harness.input.wheel(Vec2::new(0.0, -100.0));
    draw(&mut harness);
    // 32 rows: 452 high, scrolled 314. The last, row 31, at the bottom.
    let bottom = 14.0 + 31.0 * 14.0 - 314.0 + 2.0;
    for _ in 0..2 {
        let last = harness.texts().into_iter().find(|(text, _)| text == "b29");
        assert_eq!(last.map(|(_, at)| at.y), Some(bottom));
        draw(&mut harness);
    }
    // Every row of the content left of the scrollbar, under the title bar.
    for (draw, view) in harness.out.iter() {
        if let nadir_render::UiDraw::Text(_, text) = draw
            && (text.starts_with('a') || text.starts_with('b'))
        {
            let clip = view.clip().expect("a clip");
            assert!(
                clip.max.x <= 194.0 && clip.min.y >= 12.0,
                "{text}: {clip:?}"
            );
        }
    }
}

#[test]
fn what_was_typed_before_a_press_goes_to_the_field_that_had_the_keyboard() {
    // Typed, then a press outside, in one frame: typed into the field,
    // which then lets the keyboard go.
    let mut field = Field::new("");
    field.focus();
    field.harness.input.text("abc");
    field.harness.click(Vec2::new(300.0, 250.0));
    field.frame(true);
    let response = field.frame(true);
    assert_eq!(field.value, "abc");
    assert!(response.lost_focus || field.responses.iter().rev().take(2).any(|r| r.lost_focus));
    // Typed with no field focused, then a press on one: not typed into it.
    let mut field = Field::new("");
    field.harness.input.text("q");
    field.harness.click(row(0, 10.0));
    field.frame(true);
    field.frame(true);
    assert_eq!(field.value, "");
    assert!(field.harness.ui.wants_keyboard());
    // The key that gives a field the keyboard is not typed into it.
    let mut field = Field::new("");
    field.harness.input.text("`");
    let value = &mut field.value;
    field.harness.show(|ui| {
        ui.focus_next();
        ui.text_input("T", value);
    });
    assert_eq!(field.value, "");
}

#[test]
fn a_field_hears_that_another_took_the_keyboard() {
    let mut harness = Harness::new();
    let (mut b, mut a) = (String::new(), String::new());
    let mut lost = (false, false);
    let mut draw = |harness: &mut Harness, steal: bool, lost: &mut (bool, bool)| {
        harness.show(|ui| {
            if steal {
                ui.focus_next();
            }
            lost.0 |= ui.text_input("B", &mut b).lost_focus;
            lost.1 |= ui.text_input("A", &mut a).lost_focus;
        });
    };
    draw(&mut harness, false, &mut lost);
    harness.click(row(1, 10.0));
    draw(&mut harness, false, &mut lost);
    // B, called first, takes it: A hears it.
    harness.click(row(0, 10.0));
    draw(&mut harness, false, &mut lost);
    assert_eq!(lost, (false, true));
    // A takes it back with a press; B hears it from A's press.
    harness.click(row(1, 10.0));
    draw(&mut harness, false, &mut lost);
    assert_eq!(lost, (true, true));
    // focus_next gives it to B: A hears it.
    lost = (false, false);
    draw(&mut harness, true, &mut lost);
    assert_eq!(lost, (false, true));
}

#[test]
fn a_window_drawn_on_screen_follows_its_drag_from_where_it_is_drawn() {
    let mut harness = Harness::new();
    harness.ui.set_place(
        "W",
        Rect::from_min_size(Vec2::new(-500.0, 0.0), WINDOW.size()),
        false,
    );
    harness.show(|_| {});
    // Kept on screen with 24 of its width: drawn from x -176.
    assert!(
        sprites(&harness)
            .iter()
            .any(|(at, size, _)| *at == Vec2::new(-176.0, 0.0) && *size == WINDOW.size())
    );
    harness.press(Vec2::new(10.0, 5.0));
    harness.show(|_| {});
    harness.move_to(Vec2::new(110.0, 5.0));
    harness.show(|_| {});
    assert!(
        sprites(&harness)
            .iter()
            .any(|(at, size, _)| *at == Vec2::new(-76.0, 0.0) && *size == WINDOW.size())
    );
    harness.release(Vec2::new(110.0, 5.0));
    harness.show(|_| {});
    assert_eq!(place_of(&harness.ui, "W").0.min, Vec2::new(-76.0, 0.0));
    // A place far off a screen is drawn on it.
    harness.ui.set_place(
        "W",
        Rect::from_min_size(Vec2::new(1000.0, 10.0), WINDOW.size()),
        false,
    );
    harness.show(|_| {});
    assert!(
        sprites(&harness)
            .iter()
            .any(|(at, _, _)| *at == Vec2::new(376.0, 10.0))
    );
}

#[test]
fn a_press_on_a_window_keeps_the_mouse_until_its_release() {
    let mut harness = Harness::new();
    harness.show(|_| {});
    // Its empty body: no widget.
    harness.press(Vec2::new(100.0, 100.0));
    harness.show(|_| {});
    harness.move_to(Vec2::new(300.0, 250.0));
    harness.show(|_| {});
    assert!(harness.ui.wants_mouse());
    harness.release(Vec2::new(300.0, 250.0));
    harness.show(|_| {});
    harness.show(|_| {});
    assert!(!harness.ui.wants_mouse());
}

#[test]
fn a_frame_with_nothing_drawn_keeps_the_windows_stacked() {
    let mut harness = Harness::new();
    let back = Rect::new(Vec2::ZERO, Vec2::new(100.0, 100.0));
    let front = Rect::new(Vec2::new(50.0, 50.0), Vec2::new(150.0, 150.0));
    let draw = |frame: &mut Frame<'_>| {
        frame.window("A", back, |_| {});
        frame.window("B", front, |_| {});
    };
    harness.frame(draw);
    harness.click(Vec2::new(20.0, 20.0));
    harness.frame(draw);
    fn order(harness: &Harness) -> Vec<String> {
        harness
            .ui
            .places()
            .map(|place| place.title.to_string())
            .collect()
    }
    assert_eq!(order(&harness), ["B", "A"]);
    harness.size = UVec2::ZERO;
    harness.frame(draw);
    harness.size = SIZE;
    harness.frame(draw);
    assert_eq!(order(&harness), ["B", "A"]);
    // Shown again while the other is drawn: in front.
    harness.frame(|frame| {
        frame.window("A", back, |_| {});
    });
    harness.frame(draw);
    assert_eq!(order(&harness), ["A", "B"]);
}

#[test]
fn a_plot_draws_only_finite_shapes_and_marks_its_last_value_inside() {
    let mut harness = Harness::new();
    harness.show(|ui| {
        let mut tall = Plot::new(0.0..=1.0);
        tall.height = f32::INFINITY;
        ui.plot("T", [0.5], tall);
        let mut last = Plot::new(0.0..=10.0);
        last.selected = Some(10);
        ui.plot("L", (0..=10).map(|v| v as f32), last);
    });
    for (at, size, _) in sprites(&harness) {
        assert!(at.x.is_finite() && at.y.is_finite() && size.x.is_finite() && size.y.is_finite());
    }
    // The second plot, under the first's 30 rows: its mark at x 118.
    assert!(
        sprites(&harness)
            .iter()
            .any(|(at, size, _)| at.x == 118.0 && *size == Vec2::new(1.0, 30.0))
    );
}

#[test]
fn a_field_scrolls_to_its_caret_and_back_when_its_text_shrinks() {
    let mut field = Field::new("abcdefghijklmnopqrstuvwxyz");
    field.focus();
    // 26 characters, 156 wide, in a field of 113 from x 4: End scrolls 44.
    field.keys(&[Key::End]);
    let text_at = |field: &Field| {
        field
            .harness
            .texts()
            .into_iter()
            .find(|(text, _)| text.starts_with("abc"))
            .map(|(_, at)| at.x)
    };
    assert_eq!(text_at(&field), Some(-40.0));
    // The caret at x 116, then, selected back two, a selection from 104.
    assert!(
        sprites(&field.harness)
            .iter()
            .any(|(at, size, _)| at.x == 116.0 && size.x == 1.0)
    );
    field.harness.input.key(Key::LeftShift, true, false);
    field.harness.tap(Key::Left);
    field.harness.tap(Key::Left);
    field.harness.input.key(Key::LeftShift, false, false);
    field.frame(true);
    let drawn = sprites(&field.harness);
    assert!(
        drawn
            .iter()
            .any(|(at, size, _)| at.x == 104.0 && size.x == 12.0),
        "{drawn:?}"
    );
    assert!(
        drawn
            .iter()
            .any(|(at, size, _)| at.x == 104.0 && size.x == 1.0)
    );
    // All selected, then one letter: back at the field's start.
    field.harness.input.key(Key::LeftControl, true, false);
    field.harness.tap(Key::A);
    field.harness.input.key(Key::LeftControl, false, false);
    field.typed("a");
    assert_eq!(field.value, "a");
    assert_eq!(
        field
            .harness
            .texts()
            .into_iter()
            .find(|(text, _)| text == "a")
            .map(|(_, at)| at.x),
        Some(4.0)
    );
}

#[test]
fn the_thumb_scrolls_its_range_over_its_room_and_hidden_widgets_take_no_press() {
    let mut harness = Harness::new();
    let mut clicked = false;
    let rows = |harness: &mut Harness, clicked: &mut bool| {
        harness.show(|ui| {
            *clicked |= ui.button("Go").clicked;
            for index in 0..29 {
                ui.label(format_args!("row {index}"));
            }
        });
    };
    rows(&mut harness, &mut clicked);
    rows(&mut harness, &mut clicked);
    // 30 rows: 424 high in 138, a range of 286; the thumb 42.96 of a track
    // of 132, so 89.04 of room. Dragged half its room: scrolled 143.
    let room = 132.0 - 138.0 / 424.0 * 132.0;
    harness.press(Vec2::new(197.0, 20.0));
    rows(&mut harness, &mut clicked);
    harness.move_to(Vec2::new(197.0, 20.0 + room * 0.5));
    rows(&mut harness, &mut clicked);
    let row_9 = harness
        .texts()
        .into_iter()
        .find(|(text, _)| text == "row 9");
    let expected = 14.0 + 10.0 * 14.0 - 143.0 + 2.0;
    assert!(row_9.is_some_and(|(_, at)| (at.y - expected).abs() < 1e-3));
    harness.release(Vec2::new(197.0, 20.0 + room * 0.5));
    rows(&mut harness, &mut clicked);

    // Scrolled back to 10: the button, at y 4 to 16, is half under the
    // title bar, where a press does not reach it.
    harness.move_to(Vec2::new(50.0, 50.0));
    harness.input.wheel(Vec2::new(0.0, 100.0));
    rows(&mut harness, &mut clicked);
    harness.input.wheel(Vec2::new(0.0, -10.0 / 36.0));
    rows(&mut harness, &mut clicked);
    harness.click(Vec2::new(15.0, 8.0));
    rows(&mut harness, &mut clicked);
    assert!(!clicked);
    harness.click(Vec2::new(15.0, 14.0));
    rows(&mut harness, &mut clicked);
    assert!(clicked);
}

#[test]
fn texts_are_cut_to_their_control_and_a_list_is_drawn_over_every_window() {
    let mut harness = Harness::new();
    let items = ["one", "two", "three"];
    let mut selected = 0;
    let mut value = 3.0;
    let mut draw = |harness: &mut Harness, selected: &mut usize| {
        harness.frame(|frame| {
            frame.window("W", WINDOW, |ui| {
                ui.combo("C", selected, &items);
                ui.slider_f32("S", &mut value, 0.0..=10.0);
            });
            frame.window(
                "Front",
                Rect::new(Vec2::new(60.0, 20.0), Vec2::new(260.0, 170.0)),
                |_| {},
            );
        });
    };
    draw(&mut harness, &mut selected);
    harness.click(row(0, 10.0));
    draw(&mut harness, &mut selected);
    let texts: Vec<(String, Rect)> = harness
        .out
        .iter()
        .filter_map(|(draw, view)| match draw {
            nadir_render::UiDraw::Text(_, text) => {
                Some((text.to_string(), view.clip().expect("a clip")))
            }
            _ => None,
        })
        .collect();
    let at = |name: &str| texts.iter().position(|(text, _)| text == name).expect(name);
    assert!(
        at("three") > at("Front"),
        "the list after the window in front"
    );
    // The slider's value, cut to its control.
    let (_, clip) = &texts[at("3.000")];
    assert!(clip.max.x <= 119.0 && clip.min.x >= 2.0, "{clip:?}");
}

#[test]
fn the_front_title_bar_is_the_front_window_s_and_a_still_click_changes_no_place() {
    let mut harness = Harness::new();
    let style = *harness.ui.style();
    let back = Rect::new(Vec2::ZERO, Vec2::new(100.0, 100.0));
    let front = Rect::new(Vec2::new(150.0, 0.0), Vec2::new(250.0, 100.0));
    let bars = |harness: &Harness| -> Vec<(f32, Color)> {
        sprites(harness)
            .into_iter()
            .filter(|(_, size, _)| size.y == 12.0 && size.x == 100.0)
            .map(|(at, _, color)| (at.x, color))
            .collect()
    };
    harness.frame(|frame| {
        frame.window("A", back, |_| {});
        frame.window("B", front, |_| {});
    });
    assert_eq!(
        bars(&harness),
        [(0.0, style.title), (150.0, style.title_front)]
    );
    // B not drawn: A is in front.
    harness.frame(|frame| {
        frame.window("A", back, |_| {});
    });
    assert_eq!(bars(&harness), [(0.0, style.title_front)]);
    // A click on a title bar, not moved, changes no place.
    let revision = harness.ui.places_revision();
    harness.click(Vec2::new(50.0, 5.0));
    harness.frame(|frame| {
        frame.window("A", back, |_| {});
    });
    assert_eq!(harness.ui.places_revision(), revision);

    // A drag of no step changes nothing.
    let mut harness = Harness::new();
    let mut value = 1.0f32;
    let mut changed = false;
    for step in 0..3 {
        harness.show(|ui| changed |= ui.drag_f32("D", &mut value, 0.0).changed);
        match step {
            0 => harness.press(row(0, 60.0)),
            _ => harness.move_to(row(0, 90.0)),
        }
    }
    assert!(!changed && value == 1.0);

    // A combo with no items opens an empty list, and picks nothing.
    let mut harness = Harness::new();
    let mut past = 7;
    for step in 0..3 {
        harness.show(|ui| {
            assert!(!ui.combo("E", &mut past, &[]).changed);
        });
        match step {
            0 => harness.click(row(0, 10.0)),
            _ => harness.click(row(1, 10.0)),
        }
    }
    assert_eq!(past, 7);
}

#[test]
fn a_held_widget_or_an_open_list_not_drawn_in_a_frame_lets_go() {
    // A slider held, then not drawn for a frame: the button still down,
    // it does not follow the pointer when it is drawn again.
    let mut harness = Harness::new();
    let mut value = 3.0f32;
    let slider = |harness: &mut Harness, drawn: bool, value: &mut f32| {
        harness.show(|ui| {
            if drawn {
                ui.slider_f32("S", value, 0.0..=10.0);
            }
        });
    };
    slider(&mut harness, true, &mut value);
    harness.press(row(0, 10.0));
    slider(&mut harness, true, &mut value);
    let held = value;
    slider(&mut harness, false, &mut value);
    harness.move_to(row(0, 1000.0));
    slider(&mut harness, true, &mut value);
    assert_eq!(value, held);
    harness.release(row(0, 1000.0));
    slider(&mut harness, true, &mut value);

    // A combo's list, its combo not drawn for a frame: closed.
    let items = ["one", "two", "three"];
    let mut harness = Harness::new();
    let mut selected = 0;
    let combo = |harness: &mut Harness, drawn: bool, selected: &mut usize| {
        harness.show(|ui| {
            if drawn {
                ui.combo("C", selected, &items);
            }
        });
    };
    combo(&mut harness, true, &mut selected);
    harness.click(row(0, 10.0));
    combo(&mut harness, true, &mut selected);
    assert!(harness.texts().iter().any(|(text, _)| text == "three"));
    combo(&mut harness, false, &mut selected);
    combo(&mut harness, true, &mut selected);
    assert!(harness.texts().iter().all(|(text, _)| text != "three"));
}

#[test]
fn while_one_widget_is_held_no_other_is_hovered_and_scopes_keep_them_apart() {
    let mut harness = Harness::new();
    let mut responses = [Response::default(); 2];
    let draw = |harness: &mut Harness, responses: &mut [Response; 2]| {
        harness.show(|ui| {
            for (index, response) in responses.iter_mut().enumerate() {
                ui.with_id(index, |ui| *response = ui.button("Go"));
            }
        });
    };
    draw(&mut harness, &mut responses);
    harness.press(row(1, 10.0));
    draw(&mut harness, &mut responses);
    assert!(!responses[0].active && responses[1].active, "{responses:?}");
    // Held, then over the other: not hovered.
    harness.move_to(row(0, 10.0));
    draw(&mut harness, &mut responses);
    assert!(!responses[0].hovered && responses[1].active);
    harness.release(row(0, 10.0));
    draw(&mut harness, &mut responses);
    assert!(!responses[0].clicked && !responses[1].clicked);
}

#[test]
fn a_window_not_drawn_takes_no_pointer() {
    let mut harness = Harness::new();
    let cover = Rect::new(Vec2::ZERO, Vec2::new(100.0, 100.0));
    let mut clicked = false;
    harness.frame(|frame| {
        frame.window("W", WINDOW, |ui| clicked |= ui.button("Go").clicked);
        frame.window("Cover", cover, |_| {});
    });
    // The cover hidden: the button under where it was takes the click.
    harness.frame(|frame| {
        frame.window("W", WINDOW, |ui| clicked |= ui.button("Go").clicked);
    });
    harness.click(row(0, 10.0));
    harness.frame(|frame| {
        frame.window("W", WINDOW, |ui| clicked |= ui.button("Go").clicked);
    });
    assert!(clicked);
}

#[test]
fn two_color_edits_in_a_window_keep_their_sliders_apart() {
    let mut harness = Harness::new();
    let mut colors = [Color::BLACK; 2];
    let draw = |harness: &mut Harness, colors: &mut [Color; 2]| {
        harness.show(|ui| {
            ui.color_edit("A", &mut colors[0]);
            ui.color_edit("B", &mut colors[1]);
        });
    };
    draw(&mut harness, &mut colors);
    draw(&mut harness, &mut colors);
    // A's R slider is row 1: pressed, and dragged past its end.
    harness.press(row(1, 10.0));
    draw(&mut harness, &mut colors);
    harness.move_to(row(1, 1000.0));
    draw(&mut harness, &mut colors);
    harness.release(row(1, 1000.0));
    draw(&mut harness, &mut colors);
    assert_eq!(colors, [Color::rgb(255, 0, 0), Color::BLACK]);
}
