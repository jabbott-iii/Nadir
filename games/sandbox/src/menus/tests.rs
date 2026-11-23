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

//! The menus, played through the game's frames as a player would: with the
//! keys alone, a pad alone, and the mouse alone.

use nadir::{Event, GamepadId, Quit, WindowSize};

use super::*;
use crate::COOKED;
use crate::tests::{TICK, player, position};
use crate::{Scene, Start, game};

/// The field from its main menu, as the game starts in its window.
fn menu_app() -> App {
    let config = Config {
        assets: COOKED.into(),
        ..Config::default()
    };
    let mut app = game(App::new(config), Scene::Field, Start::Menu)
        .unwrap_or_else(|error| panic!("the sandbox does not set up: {error}"));
    frame(&mut app, &[]);
    assert_eq!(state(&app), GameState::MainMenu, "Boot goes on at once");
    app
}

/// Runs a frame of a tick with `events`.
fn frame(app: &mut App, events: &[Event]) {
    app.update(TICK, events);
}

fn state(app: &App) -> GameState {
    app.world().resource::<StateMachine>().state().clone()
}

fn page(app: &App) -> Page {
    app.world().resource::<Menus>().page
}

/// Runs frames until the game is in `kind`, at most `frames`.
fn until(app: &mut App, kind: StateKind, frames: u32) {
    for _ in 0..frames {
        if app.world().resource::<StateMachine>().is(kind) {
            return;
        }
        frame(app, &[]);
    }
    assert!(
        app.world().resource::<StateMachine>().is(kind),
        "{kind:?} after {frames} frames: {}",
        state(app)
    );
}

fn key(key: Key, pressed: bool) -> Event {
    Event::Key {
        key,
        pressed,
        repeat: false,
    }
}

/// Presses `key` in a frame and lets it go in the next.
fn tap(app: &mut App, pressed: Key) {
    frame(app, &[key(pressed, true)]);
    frame(app, &[key(pressed, false)]);
}

const PAD: GamepadId = GamepadId::from_raw(1);

fn pad(button: GamepadButton, pressed: bool) -> Event {
    Event::GamepadButton {
        id: PAD,
        button,
        pressed,
    }
}

/// Presses the pad's `button` in a frame and lets it go in the next.
fn push(app: &mut App, button: GamepadButton) {
    frame(app, &[pad(button, true)]);
    frame(app, &[pad(button, false)]);
}

/// The window's size: twice the virtual image, with no bars, so that a UI
/// unit is two of its pixels.
const WINDOW: UVec2 = UVec2::new(960, 540);

/// Moves the pointer to the middle of the item at `index` of the page
/// shown, then clicks there, in four frames.
fn click(app: &mut App, index: usize) {
    // A page opened in the last tick lists its items in the next.
    frame(app, &[]);
    let at = {
        let menus = app.world().resource::<Menus>();
        let menu = &menus.pages[menus.page as usize];
        menu.rect(index)
            .unwrap_or_else(|| panic!("no item {index} on {:?}", menus.page))
            .center()
    };
    click_at(app, at);
}

/// Clicks at the UI point `at`.
fn click_at(app: &mut App, at: Vec2) {
    let pixels = at * 2.0;
    let left = |pressed| Event::MouseButton {
        button: MouseButton::Left,
        pressed,
    };
    frame(app, &[Event::MouseMoved(pixels)]);
    frame(app, &[left(true)]);
    frame(app, &[left(false)]);
}

fn mouse_app() -> App {
    let mut app = menu_app();
    *app.world_mut().resource_mut::<WindowSize>() = WindowSize(WINDOW);
    frame(&mut app, &[]);
    app
}

fn players(app: &App) -> usize {
    app.world().read::<Player>().len()
}

#[test]
fn with_the_keys_play_loads_the_field_and_escape_pauses_and_resumes() {
    let mut app = menu_app();
    assert_eq!(page(&app), Page::Main);
    assert_eq!(players(&app), 0, "nothing of the field yet");
    tap(&mut app, Key::Enter);
    until(&mut app, StateKind::InGame, 3);
    assert_eq!(players(&app), 1);
    tap(&mut app, Key::Escape);
    assert_eq!(state(&app), GameState::Paused);
    assert_eq!(page(&app), Page::Pause);
    // Escape is the pause and back: either resumes.
    tap(&mut app, Key::Escape);
    assert_eq!(state(&app), GameState::InGame);
}

#[test]
fn with_a_pad_alone_every_page_opens_and_the_main_menu_clears_the_field() {
    let mut app = menu_app();
    frame(&mut app, &[Event::GamepadConnected(PAD)]);
    // Settings, then each of its pages and back: back keeps the focus where
    // it was, so each is one down from the last.
    push(&mut app, GamepadButton::DPadDown);
    push(&mut app, GamepadButton::A);
    assert_eq!(page(&app), Page::Settings);
    for (downs, opened) in [(0, Page::Display), (1, Page::Audio), (1, Page::Controls)] {
        for _ in 0..downs {
            push(&mut app, GamepadButton::DPadDown);
        }
        push(&mut app, GamepadButton::A);
        assert_eq!(page(&app), opened);
        push(&mut app, GamepadButton::B);
        assert_eq!(page(&app), Page::Settings, "back from {opened:?}");
    }
    // On Controls, then up to Display.
    push(&mut app, GamepadButton::DPadUp);
    push(&mut app, GamepadButton::DPadUp);
    push(&mut app, GamepadButton::A);
    assert_eq!(page(&app), Page::Display);
    // The display's toggles, by the d-pad's right.
    push(&mut app, GamepadButton::DPadRight);
    assert!(app.world().resource::<DisplaySettings>().fullscreen);
    push(&mut app, GamepadButton::B);
    push(&mut app, GamepadButton::B);
    assert_eq!(page(&app), Page::Main);
    // Play, one up from Settings, where the main page's focus stayed; then
    // Start pauses, and Main menu is two down from Resume.
    push(&mut app, GamepadButton::DPadUp);
    push(&mut app, GamepadButton::A);
    until(&mut app, StateKind::InGame, 3);
    assert_eq!(players(&app), 1);
    push(&mut app, GamepadButton::Start);
    assert_eq!(state(&app), GameState::Paused);
    push(&mut app, GamepadButton::DPadDown);
    push(&mut app, GamepadButton::DPadDown);
    push(&mut app, GamepadButton::A);
    assert_eq!(state(&app), GameState::MainMenu);
    assert_eq!(players(&app), 0, "the field's people are gone");
    assert_eq!(page(&app), Page::Main);
    // And played again, it starts afresh.
    push(&mut app, GamepadButton::A);
    until(&mut app, StateKind::InGame, 3);
    assert_eq!(players(&app), 1);
    assert_eq!(position(&app, player(&app)), crate::PLAYER_START);
}

#[test]
fn a_pad_s_stick_moves_the_focus_and_repeats_while_held() {
    let mut app = menu_app();
    let stick = |value| Event::GamepadAxis {
        id: PAD,
        axis: GamepadAxis::LeftY,
        value,
    };
    frame(&mut app, &[Event::GamepadConnected(PAD), stick(0.9)]);
    let menus = |app: &App| app.world().resource::<Menus>().pages[Page::Main as usize].focused();
    // Play, Settings, Quit are items 2 to 4: pushed down, it is on
    // Settings; held 24 ticks, on Quit; 6 more, round to Play.
    assert_eq!(menus(&app), Some(3));
    for _ in 0..nadir::menu::REPEAT_DELAY {
        frame(&mut app, &[]);
    }
    assert_eq!(menus(&app), Some(4));
    for _ in 0..nadir::menu::REPEAT_EVERY {
        frame(&mut app, &[]);
    }
    assert_eq!(menus(&app), Some(2));
    frame(&mut app, &[stick(0.0)]);
    for _ in 0..30 {
        frame(&mut app, &[]);
    }
    assert_eq!(menus(&app), Some(2), "let go, it stays");
}

#[test]
fn with_the_mouse_alone_every_page_opens_its_values_change_and_quit_quits() {
    let mut app = mouse_app();
    // Main: Settings is item 3.
    click(&mut app, 3);
    assert_eq!(page(&app), Page::Settings);
    // Settings: Display, Audio, Controls, Back are 1 to 4.
    click(&mut app, 2);
    assert_eq!(page(&app), Page::Audio);
    frame(&mut app, &[]);
    // The music's slider, clicked at its track's start, is 0.
    let music = {
        let menus = app.world().resource::<Menus>();
        let row = menus.pages[Page::Audio as usize].rect(1).expect("Music");
        Vec2::new(row.center().x + 1.0, row.center().y)
    };
    click_at(&mut app, music);
    assert_eq!(app.world().resource::<Menus>().volumes.music, 0);
    click(&mut app, 5);
    assert_eq!(page(&app), Page::Settings);
    click(&mut app, 1);
    assert_eq!(page(&app), Page::Display);
    frame(&mut app, &[]);
    click(&mut app, 2);
    assert!(!app.world().resource::<DisplaySettings>().vsync);
    click(&mut app, 3);
    click(&mut app, 3);
    assert_eq!(page(&app), Page::Controls);
    frame(&mut app, &[]);
    // Controls: the rows are 1 to 11, Defaults 12, Back 13.
    click(&mut app, 13);
    click(&mut app, 4);
    assert_eq!(page(&app), Page::Main);
    // Play.
    click(&mut app, 2);
    until(&mut app, StateKind::InGame, 3);
    // The Pause button shows while the pointer moves.
    frame(&mut app, &[Event::MouseMoved(Vec2::new(900.0, 20.0))]);
    let button = {
        let menus = app.world().resource::<Menus>();
        assert!(menus.button_shown);
        menus.button.rect(0).expect("the Pause button").center()
    };
    click_at(&mut app, button);
    assert_eq!(state(&app), GameState::Paused);
    // Pause: Resume, Settings, Main menu, Quit are 1 to 4.
    frame(&mut app, &[]);
    click(&mut app, 1);
    assert_eq!(state(&app), GameState::InGame);
    frame(&mut app, &[Event::MouseMoved(button * 2.0 + Vec2::ONE)]);
    click_at(&mut app, button);
    frame(&mut app, &[]);
    assert!(!app.world().resource::<Quit>().requested());
    click(&mut app, 4);
    assert!(app.world().resource::<Quit>().requested());
}

#[test]
fn the_pause_button_hides_once_the_pointer_rests_and_is_never_shown_without_a_window() {
    let mut app = mouse_app();
    tap(&mut app, Key::Enter);
    until(&mut app, StateKind::InGame, 3);
    frame(&mut app, &[]);
    assert!(
        !app.world().resource::<Menus>().button_shown,
        "it has not moved"
    );
    frame(&mut app, &[Event::MouseMoved(Vec2::new(100.0, 100.0))]);
    assert!(app.world().resource::<Menus>().button_shown);
    for _ in 0..POINTER_REST_TICKS {
        frame(&mut app, &[]);
    }
    assert!(!app.world().resource::<Menus>().button_shown, "rested 3 s");
    // With no window there is no pointer.
    let mut field = crate::tests::sandbox();
    field.update(TICK, &[Event::MouseMoved(Vec2::new(100.0, 100.0))]);
    assert!(!field.world().resource::<Menus>().button_shown);
}

#[test]
fn settings_go_back_to_the_pause_menu_they_were_opened_from() {
    let mut app = menu_app();
    tap(&mut app, Key::Enter);
    until(&mut app, StateKind::InGame, 3);
    tap(&mut app, Key::Escape);
    tap(&mut app, Key::Down);
    tap(&mut app, Key::Enter);
    assert_eq!(page(&app), Page::Settings);
    tap(&mut app, Key::Backspace);
    assert_eq!(page(&app), Page::Pause);
    assert_eq!(state(&app), GameState::Paused);
}

#[test]
fn the_audio_page_sets_each_bus_in_tens() {
    let mut app = menu_app();
    tap(&mut app, Key::Down);
    tap(&mut app, Key::Enter);
    tap(&mut app, Key::Down);
    tap(&mut app, Key::Enter);
    assert_eq!(page(&app), Page::Audio);
    // Music, then Effects.
    tap(&mut app, Key::Left);
    tap(&mut app, Key::Down);
    tap(&mut app, Key::Left);
    tap(&mut app, Key::Left);
    let volumes = app.world().resource::<Menus>().volumes;
    assert_eq!(
        (volumes.music, volumes.sfx, volumes.ui, volumes.ambience),
        (90, 80, 100, 100)
    );
}

/// From the main menu, the controls page, its first row focused.
fn controls(app: &mut App) {
    tap(app, Key::Down);
    tap(app, Key::Enter);
    tap(app, Key::Down);
    tap(app, Key::Down);
    tap(app, Key::Enter);
    assert_eq!(page(app), Page::Controls);
}

fn bindings(app: &App, action: Act) -> [Option<Binding>; 4] {
    *app.world().resource::<ActionMap<Act>>().bindings(action)
}

fn wasd(up: Key, down: Key) -> Option<Binding> {
    Some(Binding::KeyComposite2 {
        up,
        down,
        left: Key::A,
        right: Key::D,
    })
}

#[test]
fn a_row_rebinds_to_the_next_press_and_a_taken_key_swaps() {
    let mut app = menu_app();
    controls(&mut app);
    // Move up: Enter listens, and I is its key.
    tap(&mut app, Key::Enter);
    assert_eq!(app.world().resource::<Menus>().listening, Some(0));
    assert!(app.world().resource::<Input>().is_rebinding());
    tap(&mut app, Key::I);
    assert_eq!(bindings(&app, Act::Move)[0], wasd(Key::I, Key::S));
    assert_eq!(app.world().resource::<Menus>().listening, None);
    // Move down to I, which Move up has: they swap.
    tap(&mut app, Key::Down);
    tap(&mut app, Key::Enter);
    tap(&mut app, Key::I);
    assert_eq!(bindings(&app, Act::Move)[0], wasd(Key::S, Key::I));
    assert_eq!(
        app.world().resource::<Menus>().note,
        "Swapped with the row that had it"
    );
    // Escape cancels, and is not also back: the page stays.
    tap(&mut app, Key::Enter);
    tap(&mut app, Key::Escape);
    assert_eq!(page(&app), Page::Controls);
    assert_eq!(app.world().resource::<Menus>().note, "Canceled");
    assert_eq!(bindings(&app, Act::Move)[0], wasd(Key::S, Key::I));
    // In the game, I walks down.
    tap(&mut app, Key::Escape);
    tap(&mut app, Key::Escape);
    assert_eq!(page(&app), Page::Main);
    // Play, one up from Settings, where the main page's focus stayed.
    tap(&mut app, Key::Up);
    tap(&mut app, Key::Enter);
    until(&mut app, StateKind::InGame, 3);
    let start = position(&app, player(&app));
    frame(&mut app, &[key(Key::I, true)]);
    frame(&mut app, &[]);
    assert!(position(&app, player(&app)).y > start.y, "I walks down");
}

#[test]
fn a_pad_rebinds_the_pause_and_defaults_bring_the_bindings_back() {
    let mut app = menu_app();
    frame(&mut app, &[Event::GamepadConnected(PAD)]);
    controls(&mut app);
    // "Pause, pad" is the eleventh row.
    for _ in 0..10 {
        push(&mut app, GamepadButton::DPadDown);
    }
    push(&mut app, GamepadButton::A);
    assert_eq!(app.world().resource::<Menus>().listening, Some(10));
    push(&mut app, GamepadButton::Y);
    assert_eq!(
        bindings(&app, Act::Pause)[1],
        Some(Binding::PadButton(GamepadButton::Y))
    );
    // A click cancels too, for the mouse.
    push(&mut app, GamepadButton::A);
    let left = |pressed| Event::MouseButton {
        button: MouseButton::Left,
        pressed,
    };
    frame(&mut app, &[left(true)]);
    frame(&mut app, &[left(false)]);
    assert_eq!(app.world().resource::<Menus>().note, "Canceled");
    // Defaults is the row after the eleven.
    push(&mut app, GamepadButton::DPadDown);
    push(&mut app, GamepadButton::A);
    assert_eq!(
        bindings(&app, Act::Pause)[1],
        Some(Binding::PadButton(GamepadButton::Start))
    );
}

#[test]
fn the_loading_screen_shows_while_the_field_loads() {
    let mut app = menu_app();
    // Play: the frame of the press shows the loading, and the next loads
    // the level.
    frame(&mut app, &[key(Key::Enter, true)]);
    let world = app.world();
    if world.resource::<StateMachine>().is(StateKind::Loading) {
        let texts: Vec<String> = world
            .resource::<RenderFrame>()
            .ui
            .iter()
            .filter_map(|(draw, _)| match draw {
                nadir::render::UiDraw::Text(_, text) => Some(text.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["Loading the field..."]);
    } else {
        panic!("not loading: {}", state(&app));
    }
}

/// A steady frame of a menu allocates nothing.
#[cfg(feature = "dev")]
#[test]
fn a_steady_menu_frame_allocates_nothing() {
    let mut app = mouse_app();
    crate::tests::without_reports(&app);
    controls(&mut app);
    for _ in 0..3 {
        frame(&mut app, &[]);
    }
    let start = nadir::thread_alloc_counts();
    for _ in 0..100 {
        frame(&mut app, &[]);
    }
    let allocated = nadir::thread_alloc_counts().since(start);
    assert_eq!(allocated, nadir::AllocCounts::default());
}
