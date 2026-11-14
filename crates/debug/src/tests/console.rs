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

//! The console in its window, driven by typed text, keys and the wheel.
//!
//! In the window `W`, (0, 0) to (200, 150), nine rows fit, from y 14, 14
//! apart: eight of the log, its lines' text 2 below each row's top, and the
//! command line, at y 126 to 138.

use nadir_core::{Key, Vec2};

use super::{Harness, sprites};
use crate::{Console, ConsoleError};

/// A console of one command, `tp <x> <y>`, which records each run.
fn console() -> Console<u8> {
    let mut console = Console::new();
    console
        .register("tp <x> <y>", "moves the player", 1)
        .expect("a usage");
    console
        .register("timescale <scale>", "scales the time", 2)
        .expect("a usage");
    console
}

/// A frame of the console in the window `W`; the commands it ran.
fn show(harness: &mut Harness, console: &mut Console<u8>) -> Vec<(u8, Vec<String>)> {
    let mut ran = Vec::new();
    harness.show(|ui| {
        console.show(ui, |command, args, out| {
            let words = (0..args.len())
                .filter_map(|i| args.get(i))
                .map(String::from);
            ran.push((command, words.collect()));
            out.print("done");
            Ok::<(), ConsoleError>(())
        });
    });
    ran
}

/// The log's lines drawn, starting with `start`, and their tops.
fn drawn(harness: &Harness, start: &str) -> Vec<(String, f32)> {
    harness
        .texts()
        .into_iter()
        .filter(|(text, _)| text.starts_with(start))
        .map(|(text, at)| (text, at.y))
        .collect()
}

#[test]
fn shown_the_command_line_takes_the_keyboard_and_enter_runs_it() {
    let mut harness = Harness::new();
    let mut console = console();
    // The key that showed the console is typed in the frame it shows.
    harness.input.text("`");
    show(&mut harness, &mut console);
    assert!(harness.ui.wants_keyboard());
    assert_eq!(console.line(), "");
    harness.input.text("tp 1 2");
    assert!(show(&mut harness, &mut console).is_empty());
    assert_eq!(console.line(), "tp 1 2");
    harness.tap(Key::Enter);
    let ran = show(&mut harness, &mut console);
    assert_eq!(ran, [(1, vec!["1".to_string(), "2".to_string()])]);
    assert_eq!(console.line(), "");
    assert!(harness.ui.wants_keyboard(), "Enter keeps the keyboard");
    assert_eq!(console.lines().collect::<Vec<_>>(), ["> tp 1 2", "done"]);

    // Escape lets the keyboard go; shown again after a frame it was not,
    // the line takes it back.
    harness.tap(Key::Escape);
    show(&mut harness, &mut console);
    assert!(!harness.ui.wants_keyboard());
    show(&mut harness, &mut console);
    assert!(
        !harness.ui.wants_keyboard(),
        "still shown: it stays as it is"
    );
    harness.show(|_| {});
    show(&mut harness, &mut console);
    assert!(harness.ui.wants_keyboard());
}

#[test]
fn the_log_fills_the_window_from_its_bottom_with_the_lines_in_view_only() {
    let mut harness = Harness::new();
    let mut console = console();
    for n in 0..3 {
        console.print(format_args!("line {n}"));
    }
    show(&mut harness, &mut console);
    // Three lines in eight rows: the last three, above the command line.
    assert_eq!(
        drawn(&harness, "line"),
        [
            ("line 0".to_string(), 86.0),
            ("line 1".to_string(), 100.0),
            ("line 2".to_string(), 114.0)
        ]
    );
    for n in 0..50 {
        console.print(format_args!("more {n}"));
    }
    show(&mut harness, &mut console);
    let shown = drawn(&harness, "more");
    // Only the eight newest are formatted and drawn.
    assert_eq!(shown.len(), 8, "{shown:?}");
    assert!(drawn(&harness, "line").is_empty());
    assert_eq!(shown[0], ("more 42".to_string(), 16.0));
    assert_eq!(shown[7], ("more 49".to_string(), 114.0));
    // They fit: the window has no scrollbar, whose track would start at
    // (194, 12).
    show(&mut harness, &mut console);
    assert!(
        sprites(&harness)
            .iter()
            .all(|(at, _, _)| *at != Vec2::new(194.0, 12.0))
    );
}

#[test]
fn the_wheel_and_page_keys_scroll_the_log_which_holds_its_lines_as_more_come() {
    let mut harness = Harness::new();
    let mut console = console();
    for n in 0..50 {
        console.print(format_args!("line {n}"));
    }
    show(&mut harness, &mut console);
    let first = |harness: &Harness| drawn(harness, "line")[0].0.clone();
    assert_eq!(first(&harness), "line 42");
    // Three lines a line of the wheel, over the log.
    harness.move_to(Vec2::new(50.0, 50.0));
    harness.input.wheel(Vec2::new(0.0, 1.0));
    show(&mut harness, &mut console);
    assert_eq!(first(&harness), "line 39");
    // Over the command line, it does nothing.
    harness.move_to(Vec2::new(50.0, 132.0));
    harness.input.wheel(Vec2::new(0.0, 1.0));
    show(&mut harness, &mut console);
    assert_eq!(first(&harness), "line 39");
    // Lines that come keep the view where it is.
    console.print("line 50\nline 51");
    show(&mut harness, &mut console);
    assert_eq!(first(&harness), "line 39");
    // A page down, past the bottom, sticks to the newest again.
    harness.tap(Key::PageDown);
    show(&mut harness, &mut console);
    assert_eq!(first(&harness), "line 44");
    console.print("line 52");
    show(&mut harness, &mut console);
    assert_eq!(first(&harness), "line 45");
    harness.tap(Key::PageUp);
    show(&mut harness, &mut console);
    assert_eq!(first(&harness), "line 37");
    // Up past the top, it stops at the oldest.
    harness.move_to(Vec2::new(50.0, 50.0));
    harness.input.wheel(Vec2::new(0.0, 100.0));
    show(&mut harness, &mut console);
    assert_eq!(first(&harness), "line 0");
    harness.input.wheel(Vec2::new(0.0, -100.0));
    show(&mut harness, &mut console);
    assert_eq!(first(&harness), "line 45");
}

#[test]
fn tab_up_and_down_work_while_the_command_line_has_the_keyboard() {
    let mut harness = Harness::new();
    let mut console = console();
    show(&mut harness, &mut console);
    harness.input.text("ti");
    harness.tap(Key::Tab);
    show(&mut harness, &mut console);
    assert_eq!(console.line(), "timescale ");
    harness.input.text("2");
    harness.tap(Key::Enter);
    assert_eq!(show(&mut harness, &mut console).len(), 1);
    harness.input.text("tp");
    show(&mut harness, &mut console);
    harness.tap(Key::Up);
    show(&mut harness, &mut console);
    assert_eq!(console.line(), "timescale 2");
    harness.tap(Key::Down);
    show(&mut harness, &mut console);
    assert_eq!(console.line(), "tp", "back to the line typed");

    // Without the keyboard, the keys are the game's.
    harness.tap(Key::Escape);
    show(&mut harness, &mut console);
    harness.tap(Key::Up);
    harness.tap(Key::Tab);
    show(&mut harness, &mut console);
    assert_eq!(console.line(), "tp");
    // A click on the command line gives it back.
    harness.click(Vec2::new(50.0, 132.0));
    show(&mut harness, &mut console);
    harness.tap(Key::Tab);
    show(&mut harness, &mut console);
    assert_eq!(console.line(), "tp ");
}

#[test]
fn enter_runs_what_was_typed_before_it_and_what_follows_goes_to_the_next_line() {
    let mut harness = Harness::new();
    let mut console = console();
    show(&mut harness, &mut console);
    // In one frame: a line, Enter, and the start of the next.
    harness.input.text("tp 1 2");
    harness.tap(Key::Enter);
    harness.input.text("5");
    let ran = show(&mut harness, &mut console);
    assert_eq!(ran, [(1, vec!["1".to_string(), "2".to_string()])]);
    assert!(show(&mut harness, &mut console).is_empty());
    assert_eq!(console.line(), "5");
    // Up, then Enter, in one frame: the line recalled runs.
    harness.tap(Key::Escape);
    show(&mut harness, &mut console);
    harness.click(Vec2::new(50.0, 132.0));
    show(&mut harness, &mut console);
    console.set_line("");
    harness.tap(Key::Up);
    harness.tap(Key::Enter);
    show(&mut harness, &mut console);
    assert_eq!(console.line(), "tp 1 2");
    let ran = show(&mut harness, &mut console);
    assert_eq!(ran, [(1, vec!["1".to_string(), "2".to_string()])]);
    assert_eq!(console.line(), "");
}

#[test]
fn the_keys_of_the_frame_that_shows_the_console_are_not_its() {
    let mut harness = Harness::new();
    let mut console = console();
    console.run_line("tp 1 2", |_, _, _| Ok(()));
    console.set_line("t");
    let before = console.lines().len();
    // Tab pressed in the frame the console shows lists nothing.
    harness.tap(Key::Tab);
    show(&mut harness, &mut console);
    show(&mut harness, &mut console);
    assert_eq!(console.lines().len(), before, "Tab listed nothing");
    assert_eq!(console.line(), "t");
    // Up pressed in the frame it shows again recalls nothing; pressed once
    // it shows, it does. (PageUp is held back the same way, by the same
    // flag.)
    harness.show(|_| {});
    harness.tap(Key::Up);
    show(&mut harness, &mut console);
    assert_eq!(console.line(), "t");
    harness.tap(Key::Up);
    show(&mut harness, &mut console);
    assert_eq!(console.line(), "tp 1 2");
}

#[test]
fn a_turn_of_the_wheel_that_is_not_finite_spoils_no_later_turn() {
    let mut harness = Harness::new();
    let mut console = console();
    for n in 0..50 {
        console.print(format_args!("line {n}"));
    }
    harness.move_to(Vec2::new(50.0, 50.0));
    for bad in [f32::NAN, f32::INFINITY, f32::MAX] {
        harness.input.wheel(Vec2::new(0.0, bad));
        show(&mut harness, &mut console);
        harness.input.wheel(Vec2::new(0.0, -100.0));
        show(&mut harness, &mut console);
        assert_eq!(drawn(&harness, "line")[0].0, "line 42", "{bad}");
        harness.input.wheel(Vec2::new(0.0, 1.0));
        show(&mut harness, &mut console);
        assert_eq!(drawn(&harness, "line")[0].0, "line 39", "{bad}");
        harness.input.wheel(Vec2::new(0.0, -1.0));
        show(&mut harness, &mut console);
    }
}

#[test]
fn a_line_run_brings_a_log_scrolled_up_back_to_its_newest() {
    let mut harness = Harness::new();
    let mut console = console();
    for n in 0..50 {
        console.print(format_args!("line {n}"));
    }
    show(&mut harness, &mut console);
    harness.move_to(Vec2::new(50.0, 50.0));
    harness.input.wheel(Vec2::new(0.0, 5.0));
    show(&mut harness, &mut console);
    assert_eq!(drawn(&harness, "line")[0].0, "line 27");
    harness.input.text("tp 1 2");
    harness.tap(Key::Enter);
    show(&mut harness, &mut console);
    show(&mut harness, &mut console);
    let shown = drawn(&harness, "");
    assert!(
        shown.iter().any(|(text, _)| text == "> tp 1 2"),
        "{shown:?}"
    );
    assert_eq!(
        shown.iter().rev().nth(1).map(|(text, _)| text.as_str()),
        Some("done"),
        "{shown:?}"
    );
}
