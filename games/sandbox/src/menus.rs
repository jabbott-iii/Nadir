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

//! The field's menus (roadmap M9.2, ADR-0076), made with the engine's menu
//! kit, `nadir::menu`, on the game states:
//!
//! - `Boot` goes on to `MainMenu`, whose page has Play, which loads the
//!   field (`Loading("field")`, then `InGame`), Settings and Quit.
//! - In the game, the pause action (Escape, or the pad's Start) pauses it;
//!   so does the Pause button at the top-right, for the mouse, which shows
//!   while the pointer moves and hides once it has rested
//!   [`POINTER_REST_TICKS`]. The pause page has Resume, which the pause
//!   action and back do too, Settings, Main menu and Quit.
//! - Settings leads to Display (fullscreen and vsync, which the engine
//!   applies), Audio (each bus's volume) and Controls (each gameplay
//!   binding, rebound by the next press; `controls.rs`).
//!
//! Every page is used with the keys, a pad or the mouse alone: each has a
//! Back button besides the back action, and its last line says which keys
//! or buttons choose and go back, as the pad last used prints them
//! (`ButtonPrompts`). The settings last until the game ends: the settings
//! file is roadmap M9.3's.
//!
//! The menus read their actions in ticks, so a recording replays a menu
//! used with the keys or a pad; the pointer is not recorded, so a click
//! does not replay.

use std::error::Error;
use std::fmt::Write as _;

use nadir::assets::Scope;
use nadir::input::{Device, Rebound};
use nadir::log::warn;
use nadir::menu::{Menu, MenuActions, MenuInput, MenuPage, MenuStyle};
use nadir::prelude::*;
use nadir::render::{Align, Font, TextDraw, UiScale, UiView};
use nadir::{DisplaySettings, Quit};

use crate::controls::{self, Placed, ROWS};
use crate::{Act, FIELD_LEVEL};

/// The menus' actions, among the sandbox's.
pub(crate) const MENU_ACTIONS: MenuActions<Act> = MenuActions {
    up: Act::Up,
    down: Act::Down,
    left: Act::Left,
    right: Act::Right,
    confirm: Act::Confirm,
    back: Act::Back,
    click: Act::Click,
};

/// The menus' font, `assets/fonts/menu.fnt` cooked (`make_font.py`), and
/// its atlas, where the white image the boxes are drawn of is too.
const FONT: &str = "atlas/fonts/menu.font.kbj";
const FONTS: &str = "atlas/fonts.kbj";
const WHITE: &str = "fonts/white";

/// How long the pointer rests before the Pause button over the game hides:
/// 3 s of ticks.
pub(crate) const POINTER_REST_TICKS: u32 = 180;

/// The rows' top, on most pages and on the controls page, which has the
/// most rows.
const TOP: f32 = 48.0;
const CONTROLS_TOP: f32 = 12.0;

/// A page of the menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Main,
    Pause,
    Settings,
    Display,
    Audio,
    Controls,
}

impl Page {
    const COUNT: usize = 6;
}

/// What a page asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Go {
    Stay,
    /// A page further in, its focus on its first item.
    Open(Page),
    /// The page before, its focus where it was.
    Back(Page),
    Play,
    Resume,
    MainMenu,
    Quit,
    /// A rebinding of the row of [`ROWS`] at this index.
    Listen(usize),
    Defaults,
}

/// The menus: their styles, each page's state, the session's volumes, and
/// the controls page's rebinding.
pub(crate) struct Menus {
    /// The main menu's style, whose backdrop hides what is behind; the
    /// pause menu's, which lets the paused world show through; and the
    /// Pause button's, at the top-right.
    main_style: MenuStyle,
    pause_style: MenuStyle,
    button_style: MenuStyle,
    /// The page shown, and the page Settings goes back to.
    pub(crate) page: Page,
    settings_from: Page,
    pub(crate) pages: [Menu; Page::COUNT],
    /// The Pause button over the game, and whether it shows.
    pub(crate) button: Menu,
    pub(crate) button_shown: bool,
    /// Where the pointer was, and the ticks it has rested there.
    pointer: Vec2,
    rest: u32,
    /// The buses' volumes, as the game started them: full.
    pub(crate) volumes: Volumes,
    /// The row a rebinding listens for, and what the last one did.
    pub(crate) listening: Option<usize>,
    pub(crate) note: &'static str,
    /// The pads' labels, unless they did not load.
    prompts: Option<Handle<ButtonPrompts>>,
    /// A row's value, written each tick.
    value: String,
}

/// Adds the menus to the field: their font and white image, now, the
/// pads' labels, and their systems.
///
/// # Errors
///
/// If the font or its atlas cannot be loaded: it says to cook the assets.
pub(crate) fn add(app: App) -> Result<App, Box<dyn Error>> {
    let menus = Menus::load(&app)?;
    Ok(app
        .insert_resource(menus)
        .on_enter(StateKind::Boot, "to_main_menu", to_main_menu)
        .on_enter(StateKind::MainMenu, "open_main_menu", open_main_menu)
        .on_enter(StateKind::Paused, "open_pause_menu", open_pause_menu)
        .add_fixed_system_in(
            StateKind::MainMenu,
            FixedStage::Gameplay,
            "main_menu",
            run_menus,
        )
        .add_fixed_system_in(
            StateKind::Paused,
            FixedStage::Gameplay,
            "pause_menu",
            run_menus,
        )
        .add_fixed_system_in(StateKind::InGame, FixedStage::Gameplay, "pause", pause)
        .add_frame_system_in(
            StateKind::MainMenu,
            FrameStage::Ui,
            "draw_main_menu",
            draw_menus,
        )
        .add_frame_system_in(
            StateKind::Paused,
            FrameStage::Ui,
            "draw_pause_menu",
            draw_menus,
        )
        .add_frame_system_in(
            StateKind::InGame,
            FrameStage::Ui,
            "draw_pause_button",
            draw_pause_button,
        )
        .add_frame_system_in(
            StateKind::Loading,
            FrameStage::Ui,
            "loading_screen",
            loading_screen,
        ))
}

impl Menus {
    /// Loads the font, its atlas and its page, and the pads' labels, which
    /// may fail with a warning.
    fn load(app: &App) -> Result<Self, Box<dyn Error>> {
        let cook = "run `cargo xtask cook` to cook `assets/` into `target/cooked`";
        let mut server = app.world().resource_mut::<AssetServer>();
        let mut ctx = FinalizeCtx::new();
        let font =
            Font::load_now_in(&mut server, FONT, Scope::Global, &mut ctx).map_err(|error| {
                format!("the menus' font, `{FONT}`, cannot be loaded: {error}; {cook}")
            })?;
        let atlas = server
            .load_now::<Atlas>(FONTS, &mut ctx)
            .map_err(|error| format!("`{FONTS}` cannot be loaded: {error}; {cook}"))?;
        let atlas = server
            .get(atlas)
            .cloned()
            .ok_or("the fonts' atlas did not load")?;
        let pages = atlas.load_pages(&mut server, Scope::Global);
        let missing = || format!("`{FONTS}` has no `{WHITE}`; {cook}");
        let white = atlas
            .sprite(WHITE)
            .and_then(|sprite| sprite.frame(0))
            .ok_or_else(missing)?;
        let texture = *pages.get(white.page()).ok_or_else(missing)?;
        // The middle of the white texels, off their edges.
        let uv = white.uv();
        let uv = Rect::from_center_size(uv.center(), uv.size() * 0.5);
        let prompts = match server.load_now::<ButtonPrompts>(ButtonPrompts::PATH, &mut ctx) {
            Ok(prompts) => Some(prompts),
            Err(error) => {
                warn!(
                    "the pads' labels, `{}`, cannot be loaded: {error}; the menus name the pads' \
                     buttons as the engine does",
                    ButtonPrompts::PATH
                );
                None
            }
        };
        let mut main_style = MenuStyle::new(font, texture, uv);
        main_style.top = TOP;
        main_style.colors.backdrop = Color::rgb(12, 16, 26);
        let mut pause_style = main_style;
        pause_style.colors.backdrop = MenuStyle::new(font, texture, uv).colors.backdrop;
        let mut button_style = main_style;
        button_style.align = Align::Right;
        button_style.top = 4.0;
        button_style.width = 44.0;
        button_style.padding = 4.0;
        button_style.colors.backdrop = Color::TRANSPARENT;
        button_style.colors.panel = Color::TRANSPARENT;
        Ok(Self {
            main_style,
            pause_style,
            button_style,
            page: Page::Main,
            settings_from: Page::Main,
            pages: Default::default(),
            button: Menu::new(),
            button_shown: false,
            pointer: Vec2::ZERO,
            rest: POINTER_REST_TICKS,
            volumes: Volumes::default(),
            listening: None,
            note: "",
            prompts,
            value: String::new(),
        })
    }

    /// The style a state's page is drawn in, its rows' top that of `page`.
    fn style(&self, main: bool, page: Page) -> MenuStyle {
        let mut style = if main {
            self.main_style
        } else {
            self.pause_style
        };
        if page == Page::Controls {
            style.top = CONTROLS_TOP;
        }
        style
    }
}

/// `Boot` goes on to the main menu: the game's assets were loaded at
/// setup.
fn to_main_menu(world: &World, _transition: &Transition) {
    world
        .resource_mut::<StateMachine>()
        .set(GameState::MainMenu);
}

/// The main menu opens at its first page, its first item focused, with no
/// rebinding.
fn open_main_menu(world: &World, _transition: &Transition) {
    let mut menus = world.resource_mut::<Menus>();
    menus.page = Page::Main;
    menus.pages[Page::Main as usize].reset();
    stop_listening(world, &mut menus);
}

/// The pause menu opens at its first page, Resume focused.
fn open_pause_menu(world: &World, _transition: &Transition) {
    let mut menus = world.resource_mut::<Menus>();
    menus.page = Page::Pause;
    menus.pages[Page::Pause as usize].reset();
    stop_listening(world, &mut menus);
}

fn stop_listening(world: &World, menus: &mut Menus) {
    if menus.listening.take().is_some() {
        world.resource_mut::<Input>().cancel_rebind();
    }
    menus.note = "";
}

/// In the game: pauses on the pause action, or a click of the Pause
/// button, which shows while the pointer moves.
fn pause(world: &World, _tick: &Tick) {
    let mut menus = world.resource_mut::<Menus>();
    let menus = &mut *menus;
    let pointer = world.resource::<Input>().pointer();
    if pointer == menus.pointer {
        menus.rest = menus.rest.saturating_add(1);
    } else {
        menus.pointer = pointer;
        menus.rest = 0;
    }
    let style = menus.button_style;
    let input = MenuInput::read(world, &MENU_ACTIONS, style.scale).pointer_only();
    menus.button_shown = input.pointer.is_some() && menus.rest < POINTER_REST_TICKS;
    let clicked = menus.button_shown && menus.button.begin(&input, &style).button("Pause");
    if clicked
        || world
            .resource::<ActionState<Act>>()
            .just_pressed(Act::Pause)
    {
        world.resource_mut::<StateMachine>().set(GameState::Paused);
    }
}

/// The main menu's or the pause menu's page, for this tick: reads the
/// menus' actions, takes what a rebinding heard, runs the page shown, and
/// does what it asks.
fn run_menus(world: &World, _tick: &Tick) {
    let main = world.resource::<StateMachine>().is(StateKind::MainMenu);
    let mut menus = world.resource_mut::<Menus>();
    let menus = &mut *menus;
    let style = menus.style(main, menus.page);
    let mut input = MenuInput::read(world, &MENU_ACTIONS, style.scale);
    // While a rebinding listens, and in the tick it hears, the page acts on
    // no press: the press is the binding's.
    if let Some(row) = menus.listening {
        input = input.quiet();
        heard(world, menus, row);
    }
    let pause_pressed = world
        .resource::<ActionState<Act>>()
        .just_pressed(Act::Pause);
    let Menus {
        page,
        settings_from,
        pages,
        volumes,
        listening,
        note,
        value,
        prompts,
        ..
    } = menus;
    let mut menu = pages[*page as usize].begin(&input, &style);
    let go = match *page {
        Page::Main => main_page(&mut menu),
        Page::Pause => pause_page(&mut menu, pause_pressed && listening.is_none()),
        Page::Settings => settings_page(&mut menu, *settings_from),
        Page::Display => display_page(&mut menu, &mut world.resource_mut::<DisplaySettings>()),
        Page::Audio => audio_page(&mut menu, volumes, &world.resource::<Audio>()),
        Page::Controls => {
            let server = world.resource::<AssetServer>();
            let prompts = prompts.and_then(|prompts| server.get(prompts));
            let family = pad_family(world);
            let map = world.resource::<ActionMap<Act>>();
            controls_page(&mut menu, &map, *listening, (prompts, family), value)
        }
    };
    // The last line: what the last rebinding did, if it said, or else the
    // hint. The controls page has no room for a row of space before it.
    if note.is_empty() {
        hint(world, *prompts, value);
    } else {
        value.clear();
        value.push_str(note);
    }
    if *page != Page::Controls {
        menu.space();
    }
    menu.text(value);
    drop(menu);
    match go {
        Go::Stay => {}
        Go::Open(next) => {
            if next == Page::Settings {
                *settings_from = *page;
            }
            *note = "";
            *page = next;
            pages[next as usize].reset();
        }
        Go::Back(to) => {
            *note = "";
            *page = to;
        }
        Go::Play => world
            .resource_mut::<StateMachine>()
            .set(GameState::Loading(LevelId::new(FIELD_LEVEL))),
        Go::Resume => world.resource_mut::<StateMachine>().set(GameState::InGame),
        Go::MainMenu => world
            .resource_mut::<StateMachine>()
            .set(GameState::MainMenu),
        Go::Quit => world.resource_mut::<Quit>().request(),
        Go::Listen(row) => {
            let kind = ROWS[row].1.listens_for();
            world.resource_mut::<Input>().begin_rebind(kind);
            *listening = Some(row);
            *note = "Press a key or a button: Escape or a click cancels";
        }
        Go::Defaults => {
            *world.resource_mut::<ActionMap<Act>>() =
                crate::actions().expect("the sandbox's bindings fit");
            *note = "The controls are back to the defaults";
        }
    }
}

/// Takes what the rebinding of `row` heard, if it heard: places the
/// binding, and says what it did. A click of the left mouse button, the
/// menus' click, cancels, as Escape does.
fn heard(world: &World, menus: &mut Menus, row: usize) {
    let mut input = world.resource_mut::<Input>();
    let Some(rebound) = input.rebound() else {
        if !input.is_rebinding() {
            menus.listening = None;
        }
        return;
    };
    menus.listening = None;
    let binding = match rebound {
        Rebound::Canceled | Rebound::Bound(Binding::MouseButton(MouseButton::Left)) => {
            menus.note = "Canceled";
            return;
        }
        Rebound::Bound(binding) => binding,
    };
    let mut map = world.resource_mut::<ActionMap<Act>>();
    menus.note = match controls::rebind(&mut map, ROWS[row].1, binding) {
        Placed::Bound | Placed::Same => "",
        Placed::Swapped(_) => "Swapped with the row that had it",
        Placed::Taken(_) => "Taken by a row that cannot swap: unchanged",
        Placed::KeysOnly => "A key moves: unchanged",
    };
}

/// The family of the pad last used, or of the first pad connected, for
/// its labels: an Xbox pad's where there is none.
fn pad_family(world: &World) -> GamepadFamily {
    let gamepads = world.resource::<Gamepads>();
    let active = match world.resource::<Input>().active_device() {
        Device::Gamepad(id) => gamepads.family(id),
        _ => None,
    };
    active
        .or_else(|| gamepads.iter().next().map(|(_, family)| family))
        .unwrap_or(GamepadFamily::Xbox)
}

/// Writes the last line of each page into `out`: which keys or buttons
/// choose and go back, as the device last used names them.
fn hint(world: &World, prompts: Option<Handle<ButtonPrompts>>, out: &mut String) {
    out.clear();
    // Writing to a `String` cannot fail.
    let Device::Gamepad(id) = world.resource::<Input>().active_device() else {
        let _ = write!(out, "Enter: choose   Escape: back");
        return;
    };
    let family = world
        .resource::<Gamepads>()
        .family(id)
        .unwrap_or(GamepadFamily::Other);
    let server = world.resource::<AssetServer>();
    let _ = match prompts.and_then(|prompts| server.get(prompts)) {
        Some(prompts) => write!(
            out,
            "{}: choose   {}: back",
            prompts.button(family, GamepadButton::A),
            prompts.button(family, GamepadButton::B)
        ),
        None => write!(out, "A: choose   B: back"),
    };
}

fn main_page(menu: &mut MenuPage<'_>) -> Go {
    menu.title("Sandbox");
    menu.text("A walk in a field");
    menu.space();
    let mut go = Go::Stay;
    if menu.button("Play") {
        go = Go::Play;
    }
    if menu.button("Settings") {
        go = Go::Open(Page::Settings);
    }
    if menu.button("Quit") {
        go = Go::Quit;
    }
    go
}

/// The pause page: `resume` when the pause action was pressed again.
fn pause_page(menu: &mut MenuPage<'_>, resume: bool) -> Go {
    menu.title("Paused");
    menu.space();
    let mut go = if resume || menu.back() {
        Go::Resume
    } else {
        Go::Stay
    };
    if menu.button("Resume") {
        go = Go::Resume;
    }
    if menu.button("Settings") {
        go = Go::Open(Page::Settings);
    }
    if menu.button("Main menu") {
        go = Go::MainMenu;
    }
    if menu.button("Quit") {
        go = Go::Quit;
    }
    go
}

fn settings_page(menu: &mut MenuPage<'_>, from: Page) -> Go {
    menu.title("Settings");
    menu.space();
    let mut go = if menu.back() {
        Go::Back(from)
    } else {
        Go::Stay
    };
    for (label, page) in [
        ("Display", Page::Display),
        ("Audio", Page::Audio),
        ("Controls", Page::Controls),
    ] {
        if menu.button(label) {
            go = Go::Open(page);
        }
    }
    if menu.button("Back") {
        go = Go::Back(from);
    }
    go
}

/// Fullscreen and vsync, which the engine applies after the frame.
fn display_page(menu: &mut MenuPage<'_>, display: &mut DisplaySettings) -> Go {
    menu.title("Display");
    menu.space();
    let mut go = if menu.back() {
        Go::Back(Page::Settings)
    } else {
        Go::Stay
    };
    menu.toggle("Fullscreen", &mut display.fullscreen);
    menu.toggle("Vsync", &mut display.vsync);
    if menu.button("Back") {
        go = Go::Back(Page::Settings);
    }
    go
}

/// Each bus's volume, in tens of percent: a change goes to the mixer at
/// once, over 50 ms.
fn audio_page(menu: &mut MenuPage<'_>, volumes: &mut Volumes, audio: &Audio) -> Go {
    menu.title("Audio");
    menu.space();
    let mut go = if menu.back() {
        Go::Back(Page::Settings)
    } else {
        Go::Stay
    };
    for (label, bus) in [
        ("Music", Bus::Music),
        ("Effects", Bus::Sfx),
        ("Interface", Bus::Ui),
        ("Ambience", Bus::Ambience),
    ] {
        let mut percent = i32::from(volumes.get(bus));
        if menu.slider(label, &mut percent, 0, 100, 10) {
            volumes.set(bus, u8::try_from(percent).unwrap_or(100));
            audio.set_bus_volume(bus, volumes.gain(bus), 50);
        }
    }
    if menu.button("Back") {
        go = Go::Back(Page::Settings);
    }
    go
}

/// Each row of [`ROWS`] with its binding, or "Press..." while it listens;
/// Defaults; and Back. Its last line says what the last rebinding did.
fn controls_page(
    menu: &mut MenuPage<'_>,
    map: &ActionMap<Act>,
    listening: Option<usize>,
    (prompts, family): (Option<&ButtonPrompts>, GamepadFamily),
    value: &mut String,
) -> Go {
    menu.title("Controls");
    let mut go = if menu.back() {
        Go::Back(Page::Settings)
    } else {
        Go::Stay
    };
    for (row, &(label, place)) in ROWS.iter().enumerate() {
        value.clear();
        if listening == Some(row) {
            value.push_str("Press...");
        } else {
            controls::name(place.get(map), prompts, family, value);
        }
        if menu.binding(label, value) {
            go = Go::Listen(row);
        }
    }
    if menu.button("Defaults") {
        go = Go::Defaults;
    }
    if menu.button("Back") {
        go = Go::Back(Page::Settings);
    }
    go
}

/// Draws the page shown, over the world: the main menu hides it, the pause
/// menu darkens it.
fn draw_menus(world: &World, _frame: &Frame) {
    let main = world.resource::<StateMachine>().is(StateKind::MainMenu);
    let menus = world.resource::<Menus>();
    let style = menus.style(main, menus.page);
    let mut out = world.resource_mut::<RenderFrame>();
    menus.pages[menus.page as usize].draw(&mut out.ui, &style);
}

/// Draws the Pause button over the game while it shows.
fn draw_pause_button(world: &World, _frame: &Frame) {
    let menus = world.resource::<Menus>();
    if menus.button_shown {
        let mut out = world.resource_mut::<RenderFrame>();
        menus.button.draw(&mut out.ui, &menus.button_style);
    }
}

/// The loading screen: a line in the middle of a dark screen.
fn loading_screen(world: &World, _frame: &Frame) {
    let menus = world.resource::<Menus>();
    let style = menus.main_style;
    let mut out = world.resource_mut::<RenderFrame>();
    out.clear = style.colors.backdrop;
    out.ui.set_view(UiView::new(UiScale::Pixel(1)));
    let size = world.resource::<nadir::VirtualResolution>().0.as_vec2();
    let mut text = TextDraw::new(style.font, size * 0.5);
    text.pivot = Vec2::splat(0.5);
    text.color = style.colors.dim;
    out.ui.text(text, "Loading the field...");
}

#[cfg(test)]
mod tests;
