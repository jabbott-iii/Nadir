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

//! The menu kit's tests: focus navigation by keys and by the mouse, the
//! widgets, the layout, the drawing, and reading the input from the world.

use std::num::NonZeroU32;

use nadir_core::{GenIndex, Key, MouseButton, UVec2};
use nadir_input::{ActionKind, ActionMap, Binding};
use nadir_platform::Event;
use nadir_render::UiDraw;

use super::*;
use crate::{App, Config};

fn handle<T>() -> Handle<T> {
    Handle::new(GenIndex::new(0, NonZeroU32::MIN))
}

fn style() -> MenuStyle {
    let uv = Rect::new(Vec2::splat(0.25), Vec2::splat(0.75));
    MenuStyle::new(handle(), handle(), uv)
}

/// A point as `(x, y)`.
fn point(at: Vec2) -> String {
    format!("({}, {})", at.x, at.y)
}

const SIZE: Vec2 = Vec2::new(480.0, 270.0);

/// A tick of nothing pressed, the pointer nowhere, in a UI of 480 × 270.
fn idle() -> MenuInput {
    MenuInput {
        size: SIZE,
        ..MenuInput::NONE
    }
}

const TAP: Press = Press {
    held: true,
    just_pressed: true,
    just_released: false,
};

const HELD: Press = Press {
    held: true,
    just_pressed: false,
    just_released: false,
};

const UP_AGAIN: Press = Press {
    held: false,
    just_pressed: false,
    just_released: true,
};

/// A tap of down, up or confirm, released in the next tick.
fn down() -> MenuInput {
    MenuInput {
        down: TAP,
        ..idle()
    }
}

fn up() -> MenuInput {
    MenuInput { up: TAP, ..idle() }
}

fn confirm() -> MenuInput {
    MenuInput {
        confirm: TAP,
        ..idle()
    }
}

/// What a tick of the test page did.
#[derive(Debug, Default, PartialEq)]
struct Did {
    play: bool,
    settings: bool,
    quit: bool,
    music: bool,
    fullscreen: bool,
    back: bool,
}

/// The values the test page's slider and toggle change.
struct Values {
    music: i32,
    fullscreen: bool,
}

/// A page: a title, Play, a line of text, Settings, a music slider of 0 to
/// 100 by 10, a fullscreen toggle, and Quit. The focusable items are at 1,
/// 3, 4, 5 and 6.
fn page(menu: &mut Menu, input: &MenuInput, values: &mut Values) -> Did {
    let style = style();
    let mut page = menu.begin(input, &style);
    page.title("Sandbox");
    let play = page.button("Play");
    page.text("a line of text");
    let settings = page.button("Settings");
    let music = page.slider("Music", &mut values.music, 0, 100, 10);
    let fullscreen = page.toggle("Fullscreen", &mut values.fullscreen);
    let quit = page.button("Quit");
    let back = page.back();
    Did {
        play,
        settings,
        quit,
        music,
        fullscreen,
        back,
    }
}

fn values() -> Values {
    Values {
        music: 50,
        fullscreen: false,
    }
}

/// Runs the page once for each input, and gives what the last tick did.
fn run(menu: &mut Menu, values: &mut Values, inputs: &[MenuInput]) -> Did {
    let mut did = Did::default();
    for input in inputs {
        did = page(menu, input, values);
    }
    did
}

#[test]
fn the_first_focusable_item_has_the_focus_from_the_first_tick() {
    let (mut menu, mut values) = (Menu::new(), values());
    assert_eq!(menu.focused(), None);
    let did = run(&mut menu, &mut values, &[confirm()]);
    assert_eq!(menu.focused(), Some(1), "Play, under the title");
    assert!(did.play, "confirm in the first tick acts on it");
}

#[test]
fn down_and_up_move_the_focus_past_titles_and_text_and_wrap_around() {
    let (mut menu, mut values) = (Menu::new(), values());
    run(&mut menu, &mut values, &[idle()]);
    let mut seen = Vec::new();
    for _ in 0..6 {
        run(&mut menu, &mut values, &[down(), idle()]);
        seen.push(menu.focused().expect("a focus"));
    }
    assert_eq!(seen, [3, 4, 5, 6, 1, 3], "Play, then past the text");
    seen.clear();
    for _ in 0..3 {
        run(&mut menu, &mut values, &[up(), idle()]);
        seen.push(menu.focused().expect("a focus"));
    }
    assert_eq!(seen, [1, 6, 5], "past the title, then round to Quit");
}

#[test]
fn a_direction_in_the_first_tick_or_after_a_reset_moves_on_from_the_first_item() {
    let (mut menu, mut values) = (Menu::new(), values());
    run(&mut menu, &mut values, &[down()]);
    assert_eq!(menu.focused(), Some(3), "Settings, after Play");
    menu.reset();
    run(&mut menu, &mut values, &[idle(), up()]);
    assert_eq!(menu.focused(), Some(6), "round from Play to Quit");
    menu.reset();
    run(&mut menu, &mut values, &[down()]);
    assert_eq!(menu.focused(), Some(3));
}

#[test]
fn a_held_direction_repeats_after_its_delay_then_at_its_rate() {
    let (mut menu, mut values) = (Menu::new(), values());
    run(&mut menu, &mut values, &[idle()]);
    let held = MenuInput {
        down: HELD,
        ..idle()
    };
    let mut moves = Vec::new();
    let mut focus = menu.focused();
    for tick in 0..(REPEAT_DELAY + 2 * REPEAT_EVERY + 1) {
        let input = if tick == 0 { down() } else { held };
        run(&mut menu, &mut values, &[input]);
        if menu.focused() != focus {
            moves.push(tick);
            focus = menu.focused();
        }
    }
    let delay = REPEAT_DELAY;
    assert_eq!(
        moves,
        [0, delay, delay + REPEAT_EVERY, delay + 2 * REPEAT_EVERY]
    );
    // Let go, and a new press moves at once again.
    run(&mut menu, &mut values, &[idle()]);
    let before = menu.focused();
    run(&mut menu, &mut values, &[down()]);
    assert_ne!(menu.focused(), before);
}

#[test]
fn confirm_activates_the_focused_button_once_while_held() {
    let (mut menu, mut values) = (Menu::new(), values());
    run(&mut menu, &mut values, &[idle(), down(), idle()]);
    assert_eq!(menu.focused(), Some(3));
    let did = run(&mut menu, &mut values, &[confirm()]);
    assert_eq!(
        did,
        Did {
            settings: true,
            ..Did::default()
        }
    );
    let held = MenuInput {
        confirm: HELD,
        ..idle()
    };
    assert_eq!(run(&mut menu, &mut values, &[held]), Did::default());
}

#[test]
fn left_and_right_step_the_slider_within_its_ends_and_flip_the_toggle() {
    let (mut menu, mut values) = (Menu::new(), values());
    run(&mut menu, &mut values, &[idle()]);
    menu.set_focus(4);
    let right = MenuInput {
        right: TAP,
        ..idle()
    };
    let left = MenuInput {
        left: TAP,
        ..idle()
    };
    assert!(run(&mut menu, &mut values, &[right]).music);
    assert_eq!(values.music, 60);
    for _ in 0..8 {
        run(&mut menu, &mut values, &[idle(), right]);
    }
    assert_eq!(values.music, 100, "held at its top");
    assert!(!run(&mut menu, &mut values, &[idle(), right]).music);
    run(&mut menu, &mut values, &[idle(), left]);
    assert_eq!(values.music, 90);
    // Down to the toggle: right flips it, and so does left, and confirm.
    run(&mut menu, &mut values, &[idle(), down()]);
    assert!(run(&mut menu, &mut values, &[idle(), right]).fullscreen);
    assert!(values.fullscreen);
    run(&mut menu, &mut values, &[idle(), left]);
    assert!(!values.fullscreen);
    run(&mut menu, &mut values, &[idle(), confirm()]);
    assert!(values.fullscreen);
    assert_eq!(values.music, 90, "the slider was left alone");
}

#[test]
fn back_is_the_back_action_s_press() {
    let (mut menu, mut values) = (Menu::new(), values());
    let back = MenuInput {
        back: TAP,
        ..idle()
    };
    assert!(run(&mut menu, &mut values, &[back]).back);
    let held = MenuInput {
        back: HELD,
        ..idle()
    };
    assert!(!run(&mut menu, &mut values, &[held]).back);
    assert!(!run(&mut menu, &mut values, &[back.pointer_only()]).back);
}

#[test]
fn the_rows_are_a_column_in_the_middle_from_the_top() {
    let (mut menu, mut values) = (Menu::new(), values());
    run(&mut menu, &mut values, &[idle()]);
    assert_eq!(menu.len(), 7);
    for index in 0..7 {
        let rect = menu.rect(index).expect("an item");
        let top = 32.0 + 16.0 * index as f32;
        assert_eq!(
            rect,
            Rect::new(Vec2::new(140.0, top), Vec2::new(340.0, top + 14.0))
        );
    }
    assert_eq!(menu.rect(7), None);
}

/// The middle of the item at `index`.
fn middle(menu: &Menu, index: usize) -> Vec2 {
    menu.rect(index).expect("an item").center()
}

fn pointing(at: Vec2) -> MenuInput {
    MenuInput {
        pointer: Some(at),
        ..idle()
    }
}

fn clicking(at: Vec2, click: Press) -> MenuInput {
    MenuInput {
        click,
        ..pointing(at)
    }
}

#[test]
fn the_pointer_focuses_what_it_moves_onto_and_not_where_it_rests() {
    let (mut menu, mut values) = (Menu::new(), values());
    run(&mut menu, &mut values, &[idle()]);
    let quit = middle(&menu, 6);
    run(&mut menu, &mut values, &[pointing(quit)]);
    assert_eq!(menu.focused(), Some(6));
    // The keys move it on, while the pointer rests on Quit.
    run(
        &mut menu,
        &mut values,
        &[MenuInput {
            up: TAP,
            ..pointing(quit)
        }],
    );
    assert_eq!(menu.focused(), Some(5));
    run(&mut menu, &mut values, &[pointing(quit)]);
    assert_eq!(menu.focused(), Some(5), "it did not move");
    // Over the title, the text or a gap, it focuses nothing new.
    let title = middle(&menu, 0);
    let gap = Vec2::new(quit.x, menu.rect(6).expect("Quit").min.y - 1.0);
    for at in [title, middle(&menu, 2), gap] {
        run(&mut menu, &mut values, &[pointing(at)]);
        assert_eq!(menu.focused(), Some(5));
    }
}

#[test]
fn a_click_pressed_and_released_on_a_button_activates_it() {
    let (mut menu, mut values) = (Menu::new(), values());
    run(&mut menu, &mut values, &[idle()]);
    let (settings, quit) = (middle(&menu, 3), middle(&menu, 6));
    let did = run(
        &mut menu,
        &mut values,
        &[
            pointing(settings),
            clicking(settings, TAP),
            clicking(settings, HELD),
        ],
    );
    assert_eq!(did, Did::default(), "nothing until it is released");
    let did = run(&mut menu, &mut values, &[clicking(settings, UP_AGAIN)]);
    assert!(did.settings);
    // A tap within a tick acts too.
    let tapped = Press {
        just_released: true,
        ..TAP
    };
    assert!(run(&mut menu, &mut values, &[clicking(quit, tapped)]).quit);
}

#[test]
fn a_click_released_off_the_button_it_was_pressed_on_does_nothing() {
    let (mut menu, mut values) = (Menu::new(), values());
    run(&mut menu, &mut values, &[idle()]);
    let (play, settings) = (middle(&menu, 1), middle(&menu, 3));
    let did = run(
        &mut menu,
        &mut values,
        &[
            clicking(play, TAP),
            clicking(settings, HELD),
            clicking(settings, UP_AGAIN),
        ],
    );
    assert_eq!(did, Did::default());
    // Pressed on Settings and released there, after a press elsewhere.
    let did = run(
        &mut menu,
        &mut values,
        &[clicking(settings, TAP), clicking(settings, UP_AGAIN)],
    );
    assert!(did.settings);
}

#[test]
fn a_click_on_the_slider_sets_it_and_a_drag_moves_it() {
    let (mut menu, mut values) = (Menu::new(), values());
    run(&mut menu, &mut values, &[idle()]);
    let row = menu.rect(4).expect("the slider");
    let track = track(row, &style());
    let at = |across: f32| Vec2::new(track.min.x + track.width() * across, row.center().y);
    assert!(run(&mut menu, &mut values, &[clicking(at(0.0), TAP)]).music);
    assert_eq!(values.music, 0);
    run(&mut menu, &mut values, &[clicking(at(0.74), HELD)]);
    assert_eq!(values.music, 70, "to the nearest step");
    // Dragged off the row and past the track's end, it stays at its top.
    let off = Vec2::new(track.max.x + 50.0, row.max.y + 30.0);
    run(&mut menu, &mut values, &[clicking(off, HELD)]);
    assert_eq!(values.music, 100);
    run(&mut menu, &mut values, &[clicking(off, UP_AGAIN)]);
    run(&mut menu, &mut values, &[pointing(at(0.5))]);
    assert_eq!(values.music, 100, "let go, the pointer moves it no more");
    // Clicked, a toggle flips.
    let toggle = middle(&menu, 5);
    run(&mut menu, &mut values, &[clicking(toggle, TAP)]);
    assert!(!values.fullscreen);
    run(&mut menu, &mut values, &[clicking(toggle, UP_AGAIN)]);
    assert!(values.fullscreen);
}

#[test]
fn a_menu_of_the_mouse_alone_focuses_only_what_is_under_the_pointer() {
    let (mut menu, mut values) = (Menu::new(), values());
    let mouse = |input: MenuInput| input.pointer_only();
    run(&mut menu, &mut values, &[mouse(idle())]);
    assert_eq!(menu.focused(), None);
    let play = middle(&menu, 1);
    run(&mut menu, &mut values, &[mouse(pointing(play))]);
    assert_eq!(menu.focused(), Some(1));
    // Its keys do nothing, and away from the items nothing has the focus.
    let keys = MenuInput {
        confirm: TAP,
        down: TAP,
        ..pointing(play)
    };
    assert_eq!(run(&mut menu, &mut values, &[mouse(keys)]), Did::default());
    assert_eq!(menu.focused(), Some(1));
    run(&mut menu, &mut values, &[mouse(pointing(Vec2::ZERO))]);
    assert_eq!(menu.focused(), None);
    let did = run(
        &mut menu,
        &mut values,
        &[mouse(clicking(play, TAP)), mouse(clicking(play, UP_AGAIN))],
    );
    assert!(did.play);
}

#[test]
fn a_quiet_tick_acts_on_nothing_and_keeps_the_focus() {
    let (mut menu, mut values) = (Menu::new(), values());
    run(&mut menu, &mut values, &[idle(), down(), idle()]);
    let busy = MenuInput {
        down: TAP,
        confirm: TAP,
        click: TAP,
        pointer: Some(Vec2::ZERO),
        ..idle()
    };
    assert_eq!(run(&mut menu, &mut values, &[busy.quiet()]), Did::default());
    assert_eq!(menu.focused(), Some(3));
}

#[test]
fn a_reset_gives_the_focus_back_to_the_first_item() {
    let (mut menu, mut values) = (Menu::new(), values());
    run(&mut menu, &mut values, &[idle(), down(), down(), idle()]);
    assert_eq!(menu.focused(), Some(4));
    menu.reset();
    run(&mut menu, &mut values, &[idle()]);
    assert_eq!(menu.focused(), Some(1));
}

#[test]
fn items_that_go_away_leave_the_focus_on_one_that_is_there() {
    let style = style();
    let mut menu = Menu::new();
    {
        let mut page = menu.begin(&idle(), &style);
        page.button("a");
        page.button("b");
        page.button("c");
    }
    menu.set_focus(2);
    {
        let mut page = menu.begin(&idle(), &style);
        page.title("only a title, then a button");
        page.button("a");
    }
    assert_eq!(menu.focused(), Some(1));
    {
        let mut page = menu.begin(&idle(), &style);
        page.text("nothing to focus");
    }
    assert_eq!(menu.focused(), None);
}

#[test]
fn the_column_goes_left_middle_or_right() {
    let mut style = style();
    for (align, left) in [
        (Align::Left, 6.0),
        (Align::Center, 140.0),
        (Align::Right, 274.0),
    ] {
        style.align = align;
        let mut menu = Menu::new();
        menu.begin(&idle(), &style).button("a");
        assert_eq!(menu.rect(0).expect("a").min.x, left, "{align:?}");
    }
}

/// What `menu` draws: each sprite's rect and color, each text's place,
/// pivot and color, in order.
fn drawn(menu: &Menu) -> Vec<String> {
    let mut ui = UiList::new();
    menu.draw(&mut ui, &style());
    ui.iter()
        .map(|(draw, view)| {
            assert_eq!(view, UiView::new(UiScale::Pixel(1)));
            match draw {
                UiDraw::Sprite(sprite) => {
                    assert_eq!(sprite.pivot, Vec2::ZERO);
                    assert_eq!(sprite.uv, style().white_uv);
                    let Color { r, g, b, a } = sprite.color;
                    let (at, size) = (point(sprite.position), point(sprite.size));
                    format!("box {at} {size} {r},{g},{b},{a}")
                }
                UiDraw::Text(text, string) => {
                    let Color { r, g, b, .. } = text.color;
                    let (at, pivot) = (point(text.position), point(text.pivot));
                    format!("{string:?} {at} {pivot} {r},{g},{b}")
                }
                _ => "other".to_string(),
            }
        })
        .collect()
}

#[test]
fn a_menu_draws_its_backdrop_panel_rows_and_texts_in_order() {
    let style = style();
    let mut menu = Menu::new();
    assert!(drawn(&menu).is_empty(), "no items yet");
    let mut music = 30;
    let mut on = true;
    {
        let mut page = menu.begin(&idle(), &style);
        page.title("Settings");
        page.button("Back");
        page.slider("Music", &mut music, 0, 100, 10);
        page.toggle("Vsync", &mut on);
        page.binding("Pause", "Escape");
    }
    let colors = MenuColors::default();
    let rgba = |color: Color| format!("{},{},{},{}", color.r, color.g, color.b, color.a);
    let rgb = |color: Color| format!("{},{},{}", color.r, color.g, color.b);
    let row = |top: f32, color: Color| format!("box (140, {top}) (200, 14) {}", rgba(color));
    let (left, right) = ("(0, 0.5)", "(1, 0.5)");
    let at = |x: f32, top: f32| format!("({x}, {})", top + 7.0);
    assert_eq!(
        drawn(&menu),
        [
            format!("box (0, 0) (480, 270) {}", rgba(colors.backdrop)),
            format!("box (134, 26) (212, 90) {}", rgba(colors.panel)),
            format!(
                "\"Settings\" {} (0.5, 0.5) {}",
                at(240.0, 32.0),
                rgb(colors.title)
            ),
            row(48.0, colors.focus),
            format!("box (140, 48) (2, 14) {}", rgba(colors.accent)),
            format!(
                "\"Back\" {} (0.5, 0.5) {}",
                at(240.0, 48.0),
                rgb(colors.text)
            ),
            row(64.0, colors.row),
            format!("\"Music\" {} {left} {}", at(146.0, 64.0), rgb(colors.text)),
            format!("box (240, 69) (70, 4) {}", rgba(colors.track)),
            format!("box (240, 69) (21, 4) {}", rgba(colors.accent)),
            format!("\"30\" {} {right} {}", at(334.0, 64.0), rgb(colors.text)),
            row(80.0, colors.row),
            format!("\"Vsync\" {} {left} {}", at(146.0, 80.0), rgb(colors.text)),
            format!("\"On\" {} {right} {}", at(334.0, 80.0), rgb(colors.accent)),
            row(96.0, colors.row),
            format!("\"Pause\" {} {left} {}", at(146.0, 96.0), rgb(colors.text)),
            format!(
                "\"Escape\" {} {right} {}",
                at(334.0, 96.0),
                rgb(colors.accent)
            ),
        ]
    );
}

#[test]
fn a_clear_backdrop_is_not_drawn() {
    let mut style = style();
    style.colors.backdrop = Color::TRANSPARENT;
    let mut menu = Menu::new();
    menu.begin(&idle(), &style).button("a");
    let mut ui = UiList::new();
    menu.draw(&mut ui, &style);
    // The panel, the row, its bar and its label.
    assert_eq!(ui.len(), 4);
}

/// The menu's own actions, for reading them from an app's world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Nav {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
    Click,
}

impl ActionSet for Nav {
    const ALL: &'static [Self] = &[
        Nav::Up,
        Nav::Down,
        Nav::Left,
        Nav::Right,
        Nav::Confirm,
        Nav::Back,
        Nav::Click,
    ];

    fn index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        ["up", "down", "left", "right", "confirm", "back", "click"][self as usize]
    }

    fn kind(self) -> ActionKind {
        ActionKind::Button
    }
}

const NAV: MenuActions<Nav> = MenuActions {
    up: Nav::Up,
    down: Nav::Down,
    left: Nav::Left,
    right: Nav::Right,
    confirm: Nav::Confirm,
    back: Nav::Back,
    click: Nav::Click,
};

fn nav_app() -> App {
    let mut map = ActionMap::<Nav>::new().expect("a valid set");
    for (action, key) in [
        (Nav::Up, Key::Up),
        (Nav::Down, Key::Down),
        (Nav::Left, Key::Left),
        (Nav::Right, Key::Right),
        (Nav::Confirm, Key::Enter),
        (Nav::Back, Key::Escape),
    ] {
        map.bind(action, 0, Binding::Key(key)).expect("fits");
    }
    map.bind(Nav::Click, 0, Binding::MouseButton(MouseButton::Left))
        .expect("fits");
    App::new(Config::default()).add_actions(map)
}

#[test]
fn the_input_is_read_from_the_actions_and_the_pointer_in_ui_units() {
    let mut app = nav_app();
    let tick = std::time::Duration::from_nanos(16_666_667);
    app.update(
        tick,
        &[
            Event::Key {
                key: Key::Down,
                pressed: true,
                repeat: false,
            },
            Event::MouseMoved(Vec2::new(500.0, 300.0)),
            Event::MouseButton {
                button: MouseButton::Left,
                pressed: true,
            },
        ],
    );
    let input = MenuInput::read(app.world(), &NAV, UiScale::Pixel(1));
    assert_eq!((input.down, input.click, input.up), (TAP, TAP, Press::NONE));
    assert_eq!(input.size, SIZE, "the virtual resolution, in pixel UI");
    assert_eq!(input.pointer, None, "no window yet");
    // A window of 1280 × 720 shows the 480 × 270 image twice over, 960 ×
    // 540, with bars 160 pixels wide at its sides and 90 high above and
    // below: the pointer at (500, 300) is at (170, 105).
    *app.world_mut().resource_mut::<WindowSize>() = WindowSize(UVec2::new(1280, 720));
    let input = MenuInput::read(app.world(), &NAV, UiScale::Pixel(1));
    assert_eq!(input.pointer, Some(Vec2::new(170.0, 105.0)));
    assert!(input.keys);
    let doubled = MenuInput::read(app.world(), &NAV, UiScale::Pixel(2));
    assert_eq!(doubled.pointer, Some(Vec2::new(85.0, 52.5)));
    assert_eq!(doubled.size, SIZE / 2.0);
}
