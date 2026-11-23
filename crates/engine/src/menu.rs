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

//! A kit for a game's menus: a column of buttons, sliders, toggles and
//! rows that name a binding, with a focus that the keyboard, a gamepad or
//! the mouse moves (roadmap M9.2, ADR-0076).
//!
//! # How a menu runs
//!
//! A [`Menu`] holds one page's state: which item has the focus (the one
//! that confirm acts on, drawn highlighted), which the mouse holds, and
//! the items of the last tick. A game keeps one for each page in a
//! resource of its own, and uses it in two systems:
//!
//! 1. **A tick system** reads the tick's input into a [`MenuInput`], with
//!    [`MenuInput::read`], and calls [`Menu::begin`]. On the
//!    [`MenuPage`] it gives, the game lists the page's items from top to
//!    bottom, each call saying what the player did to that item in this
//!    tick: [`MenuPage::button`] returns `true` when it was activated,
//!    [`MenuPage::slider`] and [`MenuPage::toggle`] change the value they
//!    are given. This is an immediate-mode menu: nothing is declared ahead,
//!    and the items are whatever the calls of this tick were.
//! 2. **A frame system** draws the items of the last tick with
//!    [`Menu::draw`], into [`RenderFrame::ui`](nadir_render::RenderFrame::ui).
//!
//! The input is read in ticks, not frames, because a tick sees every
//! press and release since the last ([`Input::latch`]), where a frame
//! sees only the last tick's; and because a game state changed from a tick
//! is changed at the same tick in a replay (ADR-0067). A frame with no tick
//! draws the last tick's items again.
//!
//! # The input
//!
//! The menus read the game's own actions, which [`MenuActions`] names: up,
//! down, left and right, confirm, back, and a click, bound to the left
//! mouse button. Binding them is the game's: the arrows and a gamepad's
//! d-pad and left stick to the directions, say, Enter and the pad's `A`
//! to confirm, Escape and `B` to go back. They live in the game's
//! [`ActionMap`](nadir_input::ActionMap) beside its gameplay actions, as an
//! app has one action set.
//!
//! - **Up and down** move the focus to the item above or below, skipping
//!   titles and text, and wrap around at the ends. Held, a direction
//!   repeats after [`REPEAT_DELAY`] ticks, then every [`REPEAT_EVERY`].
//! - **Left and right** step the focused slider by its step, or flip the
//!   focused toggle.
//! - **Confirm** activates the focused button or binding row, or flips the
//!   focused toggle.
//! - **The mouse:** moving the pointer over an item focuses it; pressing
//!   the click on an item and releasing it there activates it; pressing it
//!   on a slider, and dragging, sets the slider where the pointer is. The
//!   pointer only focuses when it moves, so that a pointer resting on the
//!   menu does not fight the keys.
//! - **Back** is [`MenuPage::back`], for the game to act on: a mouse-only
//!   player needs a button for it on each page too.
//!
//! The pointer is live, not latched, and replays do not hold it: a replay
//! replays a menu used with the keys or a pad, but not one clicked.
//!
//! # Layout and drawing
//!
//! The items are rows of [`MenuStyle::row`] UI units, [`MenuStyle::width`]
//! wide, one under the next from [`MenuStyle::top`], in a column at the
//! left, middle or right of the UI ([`MenuStyle::align`]). The menu draws
//! over the world: a backdrop over the whole UI if its color is not clear,
//! a panel behind the rows, each row's box (the focused one in the focus
//! color, with a bar of the accent color at its left), and its text, in
//! [`MenuStyle::font`]. Every box is a sprite of [`MenuStyle::white`], a
//! white region of a texture, tinted; the text is not measured, so a label
//! too long for its row runs past it.
//!
//! Neither the tick nor the draw allocates once the menu has held as many
//! items and characters before.

use core::fmt::Write as _;

use nadir_assets::Handle;
use nadir_core::{Color, Rect, Vec2};
use nadir_ecs::World;
use nadir_input::{ActionSet, ActionState, Input};
use nadir_render::{Align, Font, SpriteDraw, TextDraw, Texture, UiList, UiScale, UiSpace, UiView};

use crate::{VirtualResolution, WindowSize};

/// How many ticks a direction is held before it repeats: 0.4 s.
pub const REPEAT_DELAY: u32 = 24;

/// How many ticks apart a held direction repeats, after
/// [`REPEAT_DELAY`]: 0.1 s.
pub const REPEAT_EVERY: u32 = 6;

/// Which of the game's actions drive its menus. Each is an
/// [`ActionKind::Button`](nadir_input::ActionKind::Button), or an
/// [`ActionKind::Axis1`](nadir_input::ActionKind::Axis1) that counts as held
/// while it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuActions<A> {
    /// Moves the focus up.
    pub up: A,
    /// Moves the focus down.
    pub down: A,
    /// Steps a slider down, or flips a toggle.
    pub left: A,
    /// Steps a slider up, or flips a toggle.
    pub right: A,
    /// Activates the focused item.
    pub confirm: A,
    /// Goes back: [`MenuPage::back`].
    pub back: A,
    /// The click: bound to the left mouse button, it presses and releases
    /// the item under the pointer.
    pub click: A,
}

/// An action as a tick latched it: held, just pressed, just released
/// ([`ActionState`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Press {
    /// Held during the tick, or tapped in it.
    pub held: bool,
    /// Pressed since the last tick.
    pub just_pressed: bool,
    /// Released since the last tick.
    pub just_released: bool,
}

impl Press {
    /// Not held, nor pressed, nor released.
    pub const NONE: Self = Self {
        held: false,
        just_pressed: false,
        just_released: false,
    };

    /// How `action` was latched in `state`.
    pub fn of<A: ActionSet>(state: &ActionState<A>, action: A) -> Self {
        Self {
            held: state.pressed(action),
            just_pressed: state.just_pressed(action),
            just_released: state.just_released(action),
        }
    }
}

/// What a menu reads in one tick: its actions, the pointer, and the size of
/// the UI. [`MenuInput::read`] makes it from the world; a test can write
/// one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuInput {
    /// Moves the focus up.
    pub up: Press,
    /// Moves the focus down.
    pub down: Press,
    /// Steps a slider down, or flips a toggle.
    pub left: Press,
    /// Steps a slider up, or flips a toggle.
    pub right: Press,
    /// Activates the focused item.
    pub confirm: Press,
    /// Goes back.
    pub back: Press,
    /// The left mouse button.
    pub click: Press,
    /// Where the pointer is, in the menu's UI units, or `None` where that
    /// is not known: no window.
    pub pointer: Option<Vec2>,
    /// The UI's size, in its units: what [`MenuStyle::align`] places the
    /// column in.
    pub size: Vec2,
    /// Whether the keys and buttons drive the menu, so that an item always
    /// has the focus. `false` for a menu of the mouse alone, such as a
    /// pause button over the game ([`MenuInput::pointer_only`]): then the
    /// item under the pointer has the focus, and none while it is over
    /// none.
    pub keys: bool,
}

impl MenuInput {
    /// Nothing pressed, no pointer, and a UI of no size.
    pub const NONE: Self = Self {
        up: Press::NONE,
        down: Press::NONE,
        left: Press::NONE,
        right: Press::NONE,
        confirm: Press::NONE,
        back: Press::NONE,
        click: Press::NONE,
        pointer: None,
        size: Vec2::ZERO,
        keys: true,
    };

    /// The tick's input for a menu of `actions`, drawn at `scale`: each
    /// action as [`ActionState<A>`] latched it, and the pointer
    /// ([`Input::pointer`]) in the UI units of `scale`, mapped with the
    /// [`WindowSize`] and the [`VirtualResolution`]
    /// ([`UiSpace::new`]). The pointer is `None` while the window's size is
    /// zero: in [`App::update`](crate::App::update) alone, until a test sets
    /// it.
    ///
    /// # Panics
    ///
    /// If the world lacks `ActionState<A>`: the game adds its actions with
    /// [`App::add_actions`](crate::App::add_actions).
    pub fn read<A: ActionSet>(world: &World, actions: &MenuActions<A>, scale: UiScale) -> Self {
        let state = world.resource::<ActionState<A>>();
        let press = |action| Press::of(&state, action);
        let window = world.resource::<WindowSize>().0;
        let space = UiSpace::new(scale, world.resource::<VirtualResolution>().0, window);
        let pointer = (window.x > 0 && window.y > 0)
            .then(|| space.to_ui(world.resource::<Input>().pointer()));
        Self {
            up: press(actions.up),
            down: press(actions.down),
            left: press(actions.left),
            right: press(actions.right),
            confirm: press(actions.confirm),
            back: press(actions.back),
            click: press(actions.click),
            pointer,
            size: space.size,
            keys: true,
        }
    }

    /// This input with nothing pressed and no pointer: for a tick in which
    /// the menu must not act, such as one that a rebinding listens through
    /// ([`Input::begin_rebind`]). The focus stays where it is.
    #[must_use]
    pub fn quiet(self) -> Self {
        Self {
            pointer: None,
            size: self.size,
            keys: self.keys,
            ..Self::NONE
        }
    }

    /// This input for a menu of the mouse alone: the pointer and the click,
    /// and no keys ([`MenuInput::keys`]).
    #[must_use]
    pub fn pointer_only(self) -> Self {
        Self {
            click: self.click,
            pointer: self.pointer,
            size: self.size,
            keys: false,
            ..Self::NONE
        }
    }
}

/// The colors a menu is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MenuColors {
    /// Over the whole UI, behind the panel: clear for none.
    pub backdrop: Color,
    /// Behind the rows.
    pub panel: Color,
    /// A row's box.
    pub row: Color,
    /// The focused row's box.
    pub focus: Color,
    /// The focused row's bar, a slider's fill, and a toggle that is on.
    pub accent: Color,
    /// A row's text.
    pub text: Color,
    /// A text row's, and a toggle's that is off.
    pub dim: Color,
    /// A title's.
    pub title: Color,
    /// A slider's track.
    pub track: Color,
}

impl Default for MenuColors {
    /// Light text on dark blue boxes, the focus brighter, and a gold
    /// accent, over a backdrop that darkens the world.
    fn default() -> Self {
        Self {
            backdrop: Color::rgba(0, 0, 0, 144),
            panel: Color::rgba(14, 18, 30, 232),
            row: Color::rgb(36, 44, 68),
            focus: Color::rgb(64, 88, 148),
            accent: Color::rgb(240, 200, 80),
            text: Color::rgb(240, 240, 240),
            dim: Color::rgb(156, 164, 182),
            title: Color::rgb(255, 222, 124),
            track: Color::rgb(14, 18, 30),
        }
    }
}

/// How a menu is laid out and drawn. The sizes are in UI units of
/// [`MenuStyle::scale`], which for pixel UI are the virtual image's
/// pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct MenuStyle {
    /// The font of every text: loaded with
    /// [`Font::load_now_in`](nadir_render::Font::load_now_in), whose UI
    /// unit is a pixel of the font.
    pub font: Handle<Font>,
    /// A texture with a white region, which every box is drawn of, tinted.
    pub white: Handle<Texture>,
    /// The white region of [`MenuStyle::white`], in texture coordinates
    /// from 0 to 1. Keep it inside the white texels, off their edge, so
    /// that a box drawn large samples no neighbor.
    pub white_uv: Rect,
    /// The UI's scale: [`UiScale::Pixel(1)`](UiScale::Pixel) by default.
    pub scale: UiScale,
    /// Where the column is across the UI: at its left edge, its middle or
    /// its right edge, [`MenuStyle::padding`] in from an edge.
    pub align: Align,
    /// The first row's top, from the UI's top.
    pub top: f32,
    /// A row's width.
    pub width: f32,
    /// A row's height.
    pub row: f32,
    /// The space between two rows.
    pub gap: f32,
    /// The space between a row's edge and its text, and around the rows
    /// to the panel's edge.
    pub padding: f32,
    /// The colors.
    pub colors: MenuColors,
}

impl MenuStyle {
    /// A column in the middle, from 32 units down: rows of 200 by 14, 2
    /// apart, in pixel UI at one unit a virtual pixel, in
    /// [`MenuColors::default`].
    pub fn new(font: Handle<Font>, white: Handle<Texture>, white_uv: Rect) -> Self {
        Self {
            font,
            white,
            white_uv,
            scale: UiScale::Pixel(1),
            align: Align::Center,
            top: 32.0,
            width: 200.0,
            row: 14.0,
            gap: 2.0,
            padding: 6.0,
            colors: MenuColors::default(),
        }
    }
}

/// What an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Title,
    Text,
    Button,
    Toggle,
    Slider,
    Binding,
}

impl Kind {
    /// Whether it can have the focus.
    fn focusable(self) -> bool {
        !matches!(self, Kind::Title | Kind::Text)
    }
}

/// An item as the last tick listed it, for drawing it and finding it under
/// the pointer.
#[derive(Clone, Copy, Debug)]
struct Item {
    kind: Kind,
    rect: Rect,
    /// Its label's and its value's bytes in [`Menu::text`].
    label: (usize, usize),
    value: (usize, usize),
    /// A slider's place between its ends, from 0 to 1.
    fraction: f32,
    /// A toggle's value.
    on: bool,
}

/// The directions, as [`MenuPage::fired`] and [`Menu::held`] index them.
const UP: usize = 0;
const DOWN: usize = 1;
const LEFT: usize = 2;
const RIGHT: usize = 3;

/// The room a slider leaves at its right for its value.
const VALUE_WIDTH: f32 = 24.0;
/// A slider's track's height, and the focused row's bar's width.
const TRACK: f32 = 4.0;
const BAR: f32 = 2.0;

/// One page of a menu: its focus, what the mouse holds, and the items of
/// the last tick, which [`Menu::draw`] draws. See the [module](self).
#[derive(Clone, Debug, Default)]
pub struct Menu {
    items: Vec<Item>,
    text: String,
    focus: Option<usize>,
    /// The item the click went down on, until it comes up.
    pressed: Option<usize>,
    /// The slider the click went down on, which follows the pointer.
    dragging: Option<usize>,
    /// How many ticks each direction has been held.
    held: [u32; 4],
    /// A move of the focus down (`true`) or up asked for in a tick that
    /// began with no items to move among: made as the tick ends.
    deferred: Option<bool>,
    /// Where the pointer was in the last tick.
    pointer: Option<Vec2>,
    /// Whether the last tick's input had the keys, so that the focus
    /// shows.
    keys: bool,
    /// The UI's size, and the panel around the rows, in the last tick.
    size: Vec2,
    panel: Rect,
}

impl Menu {
    /// A page with no items yet: the first focusable item gets the focus.
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts this tick's items, read with `input` and laid out by `style`.
    /// The keys' moves of the focus go by the last tick's items. List the
    /// items on the page it gives, from top to bottom; the tick ends when
    /// it drops.
    pub fn begin<'a>(&'a mut self, input: &MenuInput, style: &'a MenuStyle) -> MenuPage<'a> {
        let directions = [input.up, input.down, input.left, input.right];
        let mut fired = [false; 4];
        for (direction, press) in directions.into_iter().enumerate() {
            fired[direction] = repeats(press, &mut self.held[direction]);
        }
        if input.keys {
            for (fired, down) in [(fired[UP], false), (fired[DOWN], true)] {
                if !fired {
                    continue;
                }
                if self.items.is_empty() {
                    self.deferred = Some(down);
                } else {
                    self.focus = self.step(down);
                }
            }
        } else {
            self.focus = None;
        }
        if input.click.just_pressed {
            self.pressed = None;
            self.dragging = None;
        }
        let moved = input.pointer.is_some() && input.pointer != self.pointer;
        self.items.clear();
        self.text.clear();
        self.size = input.size;
        self.keys = input.keys;
        let left = match style.align {
            Align::Left => style.padding,
            Align::Center => (input.size.x - style.width) * 0.5,
            Align::Right => input.size.x - style.width - style.padding,
        };
        MenuPage {
            menu: self,
            input: *input,
            style,
            left,
            top: style.top,
            fired,
            moved,
        }
    }

    /// The focusable item after (`down`) or before the focused one, wrapping
    /// around, among the items listed; from the first focusable one if none
    /// has the focus, as it would once a tick has run.
    fn step(&self, down: bool) -> Option<usize> {
        let focusable = |index: &usize| self.items[*index].kind.focusable();
        let count = self.items.len();
        let from = self
            .focus
            .filter(|index| *index < count && focusable(index))
            .or_else(|| (0..count).find(focusable))?;
        (1..=count)
            .map(|offset| {
                if down {
                    (from + offset) % count
                } else {
                    (from + count - offset) % count
                }
            })
            .find(focusable)
    }

    /// The item that has the focus: its place among the last tick's items,
    /// counting titles and text, from 0.
    pub fn focused(&self) -> Option<usize> {
        self.focus
    }

    /// Gives the focus to the item at `index` among the items, counting
    /// titles and text: the next tick's, if it is focusable there.
    pub fn set_focus(&mut self, index: usize) {
        self.focus = Some(index);
    }

    /// Where the last tick put the item at `index`, counting titles and
    /// text, in UI units: for a test that points at it.
    pub fn rect(&self, index: usize) -> Option<Rect> {
        self.items.get(index).map(|item| item.rect)
    }

    /// How many items the last tick listed, titles and text among them.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the last tick listed no item.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Forgets the focus, what the mouse holds and the held directions, as
    /// when a page is entered afresh: the first focusable item gets the
    /// focus. The items stay until the next tick lists its own.
    pub fn reset(&mut self) {
        self.focus = None;
        self.pressed = None;
        self.dragging = None;
        self.held = [0; 4];
        self.deferred = None;
    }

    /// Draws the last tick's items into `ui` in `style`, in front of what
    /// was pushed before: the backdrop, the panel, then each row's box and
    /// text. It sets the view to `style`'s scale, unclipped. A menu that
    /// has listed no item draws nothing.
    pub fn draw(&self, ui: &mut UiList, style: &MenuStyle) {
        if self.items.is_empty() {
            return;
        }
        ui.set_view(UiView::new(style.scale));
        let colors = &style.colors;
        let quad = |ui: &mut UiList, rect: Rect, color: Color| {
            if color.a == 0 || rect.is_empty() {
                return;
            }
            let mut sprite = SpriteDraw::new(style.white, rect.min, rect.size());
            sprite.uv = style.white_uv;
            sprite.pivot = Vec2::ZERO;
            sprite.color = color;
            ui.sprite(sprite);
        };
        let text = |ui: &mut UiList, at: Vec2, pivot: Vec2, color: Color, text: &str| {
            if text.is_empty() {
                return;
            }
            let mut draw = TextDraw::new(style.font, at);
            draw.pivot = pivot;
            draw.color = color;
            ui.text(draw, text);
        };
        quad(ui, Rect::new(Vec2::ZERO, self.size), colors.backdrop);
        quad(ui, self.panel, colors.panel);
        for (index, item) in self.items.iter().enumerate() {
            let rect = item.rect;
            let middle = rect.center();
            let left = Vec2::new(rect.min.x + style.padding, middle.y);
            let right = Vec2::new(rect.max.x - style.padding, middle.y);
            let (start, end) = item.label;
            let label = &self.text[start..end];
            let (start, end) = item.value;
            let value = &self.text[start..end];
            let centered = Vec2::new(0.5, 0.5);
            let (at_left, at_right) = (Vec2::new(0.0, 0.5), Vec2::new(1.0, 0.5));
            if item.kind.focusable() {
                let focused = self.focus == Some(index);
                let fill = if focused { colors.focus } else { colors.row };
                quad(ui, rect, fill);
                if focused {
                    let bar = Rect::from_min_size(rect.min, Vec2::new(BAR, rect.height()));
                    quad(ui, bar, colors.accent);
                }
            }
            match item.kind {
                Kind::Title => text(ui, middle, centered, colors.title, label),
                Kind::Text => text(ui, middle, centered, colors.dim, label),
                Kind::Button => text(ui, middle, centered, colors.text, label),
                Kind::Toggle => {
                    text(ui, left, at_left, colors.text, label);
                    let color = if item.on { colors.accent } else { colors.dim };
                    text(ui, right, at_right, color, value);
                }
                Kind::Slider => {
                    text(ui, left, at_left, colors.text, label);
                    let track = track(rect, style);
                    quad(ui, track, colors.track);
                    let width = track.width() * item.fraction.clamp(0.0, 1.0);
                    let fill = Rect::from_min_size(track.min, Vec2::new(width, track.height()));
                    quad(ui, fill, colors.accent);
                    text(ui, right, at_right, colors.text, value);
                }
                Kind::Binding => {
                    text(ui, left, at_left, colors.text, label);
                    text(ui, right, at_right, colors.accent, value);
                }
            }
        }
    }
}

/// A slider's track in its row: from the row's middle to [`VALUE_WIDTH`]
/// short of its padding, [`TRACK`] high, in the middle of the row.
fn track(row: Rect, style: &MenuStyle) -> Rect {
    let middle = row.center();
    let min = Vec2::new(middle.x, middle.y - TRACK * 0.5);
    let max = Vec2::new(
        (row.max.x - style.padding - VALUE_WIDTH).max(middle.x),
        middle.y + TRACK * 0.5,
    );
    Rect::new(min, max)
}

/// Whether a direction held as `press` acts in this tick, with `held` the
/// ticks it has been held, which this keeps: as it is pressed, then after
/// [`REPEAT_DELAY`] ticks, then every [`REPEAT_EVERY`].
fn repeats(press: Press, held: &mut u32) -> bool {
    if press.just_pressed {
        *held = 0;
        return true;
    }
    if !press.held {
        *held = 0;
        return false;
    }
    *held = held.saturating_add(1);
    *held >= REPEAT_DELAY && (*held - REPEAT_DELAY).is_multiple_of(REPEAT_EVERY)
}

/// What the player did to an item in this tick.
#[derive(Clone, Copy, Debug, Default)]
struct Acts {
    activated: bool,
    left: bool,
    right: bool,
    /// Where the pointer drags a slider to, across.
    drag: Option<f32>,
}

/// A menu's items in one tick, listed top to bottom: each call adds one,
/// and says what the player did to it. Made by [`Menu::begin`]; dropping it
/// ends the tick.
pub struct MenuPage<'a> {
    menu: &'a mut Menu,
    input: MenuInput,
    style: &'a MenuStyle,
    /// The column's left edge, and the next row's top.
    left: f32,
    top: f32,
    /// Whether each direction acts in this tick, repeats included.
    fired: [bool; 4],
    /// Whether the pointer moved since the last tick.
    moved: bool,
}

impl MenuPage<'_> {
    /// A heading, centered, which cannot have the focus.
    pub fn title(&mut self, text: &str) {
        self.push(Kind::Title, text);
    }

    /// A line of text, centered and dimmer, which cannot have the focus.
    pub fn text(&mut self, text: &str) {
        self.push(Kind::Text, text);
    }

    /// An empty row's height, between two groups of items.
    pub fn space(&mut self) {
        self.top += self.style.row + self.style.gap;
    }

    /// A button, its label centered: `true` in the tick it is activated,
    /// by confirm while it has the focus, or by a click pressed and
    /// released on it.
    pub fn button(&mut self, label: &str) -> bool {
        let index = self.push(Kind::Button, label);
        self.act(index).activated
    }

    /// A row naming a binding, for a controls menu: its label at the left,
    /// `value` at the right, such as the key's name, or "Press a key" while
    /// a rebinding listens. It acts as a button: `true` when activated.
    pub fn binding(&mut self, label: &str, value: &str) -> bool {
        let index = self.push(Kind::Binding, label);
        self.value(index, format_args!("{value}"));
        self.act(index).activated
    }

    /// A toggle: its label at the left, and `On` or `Off`. Confirm, a
    /// click, or left or right while it has the focus, flips `on`. `true`
    /// in the tick it flipped.
    pub fn toggle(&mut self, label: &str, on: &mut bool) -> bool {
        let index = self.push(Kind::Toggle, label);
        let acts = self.act(index);
        let flipped = acts.activated || acts.left || acts.right;
        if flipped {
            *on = !*on;
        }
        let state = if *on { "On" } else { "Off" };
        self.value(index, format_args!("{state}"));
        self.menu.items[index].on = *on;
        flipped
    }

    /// A slider of whole numbers from `min` to `max`: its label at the
    /// left, then its track and `value`. Left and right while it has the
    /// focus step `value` by `step` (at least 1), within the ends; a click
    /// on it, then a drag, sets it where the pointer is across the track,
    /// to the nearest step from `min`. A `value` outside the ends is
    /// brought inside. `true` in the tick it changed.
    pub fn slider(&mut self, label: &str, value: &mut i32, min: i32, max: i32, step: i32) -> bool {
        let (low, high) = (min.min(max), min.max(max));
        let step = step.max(1);
        let index = self.push(Kind::Slider, label);
        let acts = self.act(index);
        let before = *value;
        let mut now = before.clamp(low, high);
        if acts.left {
            now = now.saturating_sub(step).max(low);
        }
        if acts.right {
            now = now.saturating_add(step).min(high);
        }
        let span = i64::from(high) - i64::from(low);
        if let Some(x) = acts.drag {
            let track = track(self.menu.items[index].rect, self.style);
            let across = if track.width() > 0.0 {
                ((x - track.min.x) / track.width()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let steps = (across as f64 * span as f64 / f64::from(step)).round() as i64;
            let at = i64::from(low) + steps * i64::from(step);
            now = i32::try_from(at.clamp(i64::from(low), i64::from(high))).unwrap_or(high);
        }
        *value = now;
        let fraction = if span > 0 {
            ((i64::from(now) - i64::from(low)) as f64 / span as f64) as f32
        } else {
            0.0
        };
        self.menu.items[index].fraction = fraction;
        self.value(index, format_args!("{now}"));
        now != before
    }

    /// Whether back was pressed in this tick: the game goes back a page,
    /// or out of the menu. A menu of the mouse alone has no back.
    pub fn back(&self) -> bool {
        self.input.keys && self.input.back.just_pressed
    }

    /// Adds an item of `kind` with `label`, in the next row: its index.
    fn push(&mut self, kind: Kind, label: &str) -> usize {
        let style = self.style;
        let rect = Rect::from_min_size(
            Vec2::new(self.left, self.top),
            Vec2::new(style.width, style.row),
        );
        self.top += style.row + style.gap;
        let start = self.menu.text.len();
        self.menu.text.push_str(label);
        let end = self.menu.text.len();
        self.menu.items.push(Item {
            kind,
            rect,
            label: (start, end),
            value: (end, end),
            fraction: 0.0,
            on: false,
        });
        self.menu.items.len() - 1
    }

    /// Writes the item's value after its label.
    fn value(&mut self, index: usize, args: core::fmt::Arguments<'_>) {
        let start = self.menu.text.len();
        // Writing to a `String` cannot fail.
        let _ = self.menu.text.write_fmt(args);
        self.menu.items[index].value = (start, self.menu.text.len());
    }

    /// What the player did to the focusable item at `index` in this tick,
    /// with the focus and the mouse's hold brought up to date.
    fn act(&mut self, index: usize) -> Acts {
        let input = self.input;
        let menu = &mut *self.menu;
        let item = menu.items[index];
        let inside = input.pointer.is_some_and(|at| item.rect.contains(at));
        if input.keys {
            if menu.focus.is_none() || (self.moved && inside) {
                menu.focus = Some(index);
            }
        } else if inside {
            menu.focus = Some(index);
        }
        if input.click.just_pressed && inside {
            menu.pressed = Some(index);
            menu.focus = Some(index);
            if item.kind == Kind::Slider {
                menu.dragging = Some(index);
            }
        }
        let focused = input.keys && menu.focus == Some(index);
        let clicked = input.click.just_released && menu.pressed == Some(index) && inside;
        let dragged = input.click.held && menu.dragging == Some(index);
        Acts {
            activated: (focused && input.confirm.just_pressed) || clicked,
            left: focused && self.fired[LEFT],
            right: focused && self.fired[RIGHT],
            drag: input.pointer.filter(|_| dragged).map(|at| at.x),
        }
    }
}

impl Drop for MenuPage<'_> {
    /// Ends the tick: lets go of what the click held once it is up, keeps
    /// the focus on an item that can have it, and puts the panel around the
    /// rows.
    fn drop(&mut self) {
        let input = self.input;
        let menu = &mut *self.menu;
        if !input.click.held || input.click.just_released {
            menu.pressed = None;
            menu.dragging = None;
        }
        let count = menu.items.len();
        let focusable = |index: usize| menu.items[index].kind.focusable();
        if input.keys {
            let fits = menu
                .focus
                .is_some_and(|index| index < count && focusable(index));
            if !fits {
                let from = menu.focus.unwrap_or(0).min(count);
                menu.focus = (from..count)
                    .find(|&index| focusable(index))
                    .or_else(|| (0..count).find(|&index| focusable(index)));
            }
            if let Some(down) = menu.deferred.take() {
                menu.focus = menu.step(down);
            }
        }
        menu.pointer = input.pointer;
        let rows = menu.items.iter().map(|item| item.rect).reduce(Rect::union);
        let padding = Vec2::splat(self.style.padding);
        menu.panel = rows.map_or(Rect::new(Vec2::ZERO, Vec2::ZERO), |rows| {
            Rect::new(rows.min - padding, rows.max + padding)
        });
    }
}

#[cfg(test)]
mod tests;
