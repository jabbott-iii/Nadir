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

//! The debug UI's state across frames, a frame, and its windows.

use std::fmt::{self, Write};

use nadir_core::{Color, FastHashSet, Handle, Rect, UVec2, Vec2};
use nadir_render::{
    Font, SortMode, SpriteDraw, TextDraw, Texture, UiList, UiScale, UiSpace, UiView,
};

use crate::edit::Caret;
use crate::id::{Id, split};
use crate::input::{DebugInput, Taken};
use crate::{Style, TextMetrics, Ui};

/// The layer the debug UI draws in: the top one.
const LAYER: u8 = u8::MAX;

/// Wheel lines are this many rows.
const ROWS_A_LINE: f32 = 3.0;

/// The parts of a window that are not its content, each with an ID
/// outside the labels'.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Chrome {
    Collapse,
    Grip,
    Thumb,
    Bar,
}

/// Where a window's parts are, drawn at `rect`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Geometry {
    rect: Rect,
    bar: Rect,
    collapse: Rect,
    /// The title bar but its collapse box: what drags it.
    drag: Rect,
    /// Under the title bar.
    view: Rect,
    track: Rect,
    grip: Rect,
    /// Whether the content, as high as the last frame measured it, is
    /// taller than the view.
    scrollable: bool,
    /// How far it scrolls.
    range: f32,
    thumb_height: f32,
    /// How far the thumb moves.
    room: f32,
}

impl Geometry {
    /// The thumb at `scroll`.
    fn thumb(&self, scroll: f32) -> Rect {
        let share = if self.range > 0.0 {
            scroll / self.range
        } else {
            0.0
        };
        Rect::from_min_size(
            Vec2::new(self.track.min.x, self.track.min.y + share * self.room),
            Vec2::new(self.track.width(), self.thumb_height),
        )
    }
}

impl Chrome {
    /// A window's parts at `rect`, with rows `row` high, a scrollbar and a
    /// grip `size` wide, and content `content` high.
    fn geometry(rect: Rect, row: f32, size: f32, content: f32) -> Geometry {
        let bar = Rect::new(rect.min, Vec2::new(rect.max.x, rect.min.y + row));
        let collapse = Rect::from_min_size(rect.min, Vec2::splat(row));
        let view = Rect::new(Vec2::new(rect.min.x, bar.max.y), rect.max);
        let track = Rect::new(
            Vec2::new(rect.max.x - size, bar.max.y),
            Vec2::new(rect.max.x, rect.max.y - size),
        );
        let scrollable = content > view.height();
        let thumb_height = if scrollable {
            (view.height() / content * track.height()).max(size)
        } else {
            0.0
        };
        Geometry {
            rect,
            bar,
            collapse,
            drag: Rect::new(Vec2::new(collapse.max.x, bar.min.y), bar.max),
            view,
            track,
            grip: Rect::new(rect.max - Vec2::splat(size), rect.max),
            scrollable,
            range: (content - view.height()).max(0.0),
            thumb_height,
            room: (track.height() - thumb_height).max(0.0),
        }
    }
}

/// How a window's content was laid out in this frame's first call: for a
/// second call, which goes on from it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Part {
    clip: Rect,
    left: f32,
    width: f32,
    /// Where its first row went: its top, scrolled.
    top: f32,
}

/// Where something is drawn and hit: a window, or the popup over them all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Layer {
    Window(Id),
    Popup,
}

/// A window known: one drawn, or one whose place was set.
#[derive(Clone, Debug, PartialEq)]
struct WindowState {
    id: Id,
    /// Its explicit ID, or its title: the name of its place.
    key: String,
    /// Its place, as the user or `set_place` left it.
    rect: Rect,
    collapsed: bool,
    scroll: f32,
    /// The height of its content in the last frame it was drawn.
    content: f32,
    /// Drawn in the last finished frame.
    drawn: bool,
    /// Drawn in this frame.
    shown: bool,
    /// The rect it was drawn at, which hit tests use the next frame: its
    /// title bar alone when collapsed.
    hit: Rect,
    /// Where its next row goes, for a second call in one frame.
    next_y: f32,
    /// This frame's layout of its content.
    part: Option<Part>,
}

/// The widget the pointer holds, from the press until the release.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Active {
    pub(crate) id: Id,
    /// Called this frame.
    pub(crate) seen: bool,
    /// Where the press was.
    pub(crate) from: Vec2,
    /// The pointer as the widget last saw it.
    pub(crate) last: Vec2,
    /// What a drag started from: a window's rect, or a value.
    pub(crate) start: Rect,
    pub(crate) start_value: f32,
}

/// The text field that has the keyboard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Focus {
    pub(crate) id: Id,
    pub(crate) seen: bool,
    pub(crate) caret: Caret,
    /// How far the text is scrolled left, to keep the caret in view.
    pub(crate) scroll: f32,
    /// The text as the field left it: its length and hash, so that a text
    /// the caller changed puts the caret at its end.
    pub(crate) len: usize,
    pub(crate) hash: u64,
}

/// An open combo's list.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Popup {
    pub(crate) id: Id,
    pub(crate) seen: bool,
    /// The combo it opened from.
    pub(crate) anchor: Rect,
    /// Its list.
    pub(crate) rect: Rect,
}

/// Who has the mouse while its button is down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Nobody,
    Ui,
    /// The press was outside every debug window: the game's, until the
    /// release.
    Game,
}

/// What a command draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Shape {
    Rect {
        rect: Rect,
        color: Color,
    },
    /// A window's title bar, in the front window's color or not.
    Title {
        rect: Rect,
    },
    Text {
        at: Vec2,
        start: u32,
        end: u32,
        color: Color,
    },
    Line {
        from: Vec2,
        to: Vec2,
        color: Color,
    },
}

/// A shape to draw, in its layer, cut to its clip.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Cmd {
    layer: Layer,
    clip: Rect,
    shape: Shape,
}

/// What an interaction with a widget did this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Interaction {
    /// The pointer is over it, and nothing else is held.
    pub(crate) hovered: bool,
    /// The button went down on it this frame.
    pub(crate) pressed: bool,
    /// It is held: pressed this frame or before, released or not yet.
    pub(crate) held: bool,
    /// The button went up while it was held.
    pub(crate) released: bool,
    /// Released over it, having been pressed on it.
    pub(crate) clicked: bool,
}

/// An immediate-mode debug UI: each frame, code calls a window and its
/// widgets, and each widget says what was done to it. It keeps what lasts
/// from frame to frame: which widget is held, which text field has the
/// keyboard, the windows' places and which tree nodes are open.
///
/// Each frame: [`DebugUi::frame`] takes the frame's input, its windows are
/// filled with [`Frame::window`], and [`Frame::finish`] pushes what they
/// draw into a [`UiList`] (`RenderFrame::ui`), after the game's UI and in
/// front of it.
pub struct DebugUi {
    pub(crate) style: Style,
    font: Handle<Font>,
    white: Handle<Texture>,
    pub(crate) metrics: Box<dyn TextMetrics>,
    /// From back to front.
    windows: Vec<WindowState>,
    pub(crate) active: Option<Active>,
    pub(crate) focus: Option<Focus>,
    /// The field the keyboard was taken from by another, which says so
    /// when it is next called.
    pub(crate) lost: Option<Id>,
    pub(crate) focus_next: bool,
    pub(crate) open: FastHashSet<Id>,
    pub(crate) popup: Option<Popup>,
    owner: Owner,
    /// Whether a window was drawn in the last finished frame.
    drawn_any: bool,
    wants_mouse: bool,
    wants_keyboard: bool,
    revision: u64,
    // This frame.
    pub(crate) space: UiSpace,
    pub(crate) input: Taken,
    pub(crate) pointer: Vec2,
    pub(crate) press: Option<Vec2>,
    pub(crate) release: Option<(Vec2, bool)>,
    pub(crate) down: bool,
    hover_layer: Option<Layer>,
    press_layer: Option<Layer>,
    cmds: Vec<Cmd>,
    pub(crate) text: String,
    scratch: String,
    order: Vec<(u32, u32)>,
    pub(crate) values: Vec<f32>,
}

impl DebugUi {
    /// A debug UI that draws its text in `font`, measured by `metrics`
    /// (for a font loaded in the game, `FontMetrics` of a copy of it), and
    /// its boxes with `white`, a texture whose texels are white.
    pub fn new(
        font: Handle<Font>,
        metrics: impl TextMetrics + 'static,
        white: Handle<Texture>,
    ) -> Self {
        Self {
            style: Style::default(),
            font,
            white,
            metrics: Box::new(metrics),
            windows: Vec::new(),
            active: None,
            focus: None,
            lost: None,
            focus_next: false,
            open: FastHashSet::default(),
            popup: None,
            owner: Owner::Nobody,
            drawn_any: false,
            wants_mouse: false,
            wants_keyboard: false,
            revision: 0,
            space: UiSpace::new(UiScale::Free(1.0), UVec2::ONE, UVec2::ZERO),
            input: Taken::default(),
            pointer: Vec2::ZERO,
            press: None,
            release: None,
            down: false,
            hover_layer: None,
            press_layer: None,
            cmds: Vec::new(),
            text: String::new(),
            scratch: String::new(),
            order: Vec::new(),
            values: Vec::new(),
        }
    }

    /// Draws with another font from the next frame: after the font was
    /// reloaded, say.
    pub fn set_font(&mut self, font: Handle<Font>, metrics: impl TextMetrics + 'static) {
        self.font = font;
        self.metrics = Box::new(metrics);
    }

    /// Its look.
    pub fn style(&self) -> &Style {
        &self.style
    }

    /// Its look, to change: its scale, the settings' UI scale, among it.
    pub fn style_mut(&mut self) -> &mut Style {
        &mut self.style
    }

    /// Whether the pointer is the debug UI's: over a window it drew in the
    /// last frame, or holding one of its widgets. The game should then
    /// leave the mouse alone. A press on a window is the debug UI's until
    /// its release, wherever the pointer goes, and one that started outside
    /// every window the game's, over a window or not.
    pub fn wants_mouse(&self) -> bool {
        self.wants_mouse
    }

    /// Whether a text field drawn in the last frame has the keyboard: the
    /// game should then leave the keys alone.
    pub fn wants_keyboard(&self) -> bool {
        self.wants_keyboard
    }

    /// Every window known, drawn or not, each with its place: for saving
    /// them. A window's name is its explicit ID or its title.
    pub fn places(&self) -> impl ExactSizeIterator<Item = WindowPlace<'_>> + '_ {
        self.windows.iter().map(|window| WindowPlace {
            title: &window.key,
            rect: window.rect,
            collapsed: window.collapsed,
        })
    }

    /// Places the window named by `title` (its explicit ID after `##`, or
    /// the whole title) at `rect`, collapsed or not, now or when it first
    /// shows: for restoring a saved place. A rect with an edge that is not
    /// finite is ignored, and one smaller than the style's `min_size` is
    /// made that large. A window is kept on screen when it is drawn, not
    /// moved where it is placed.
    pub fn set_place(&mut self, title: &str, rect: Rect, collapsed: bool) {
        let finite = [rect.min.x, rect.min.y, rect.max.x, rect.max.y]
            .iter()
            .all(|v| v.is_finite());
        if !finite {
            return;
        }
        let (_, key) = split(title);
        let rect = Rect::from_min_size(rect.min, rect.size().max(self.style.min_size));
        let index = self.window_index(Id::ROOT.with(key), key, rect);
        let window = &mut self.windows[index];
        window.rect = rect;
        window.collapsed = collapsed;
    }

    /// A number that changes when the user collapses or opens a window, or
    /// ends a move or a resize that left it elsewhere: the places then
    /// differ from those saved.
    pub fn places_revision(&self) -> u64 {
        self.revision
    }

    /// Starts a frame: takes the frame's events from `input` (see
    /// [`DebugInput`]) and lays the UI out at the style's scale for an
    /// output of `output_size` pixels over a virtual image of
    /// `virtual_resolution`, as [`UiSpace::new`] places it. The frame ends
    /// when the [`Frame`] is dropped, finished or not.
    pub fn frame(
        &mut self,
        input: &mut DebugInput,
        virtual_resolution: UVec2,
        output_size: UVec2,
    ) -> Frame<'_> {
        input.take(&mut self.input);
        let space = UiSpace::new(self.style.scale, virtual_resolution, output_size);
        self.space = space;
        self.pointer = space.to_ui(self.input.pointer);
        self.press = self.input.press.map(|at| space.to_ui(at));
        self.release = self
            .input
            .release
            .map(|(at, cancelled)| (space.to_ui(at), cancelled));
        self.down = self.input.down;
        self.cmds.clear();
        self.text.clear();
        for window in &mut self.windows {
            window.shown = false;
        }
        if let Some(active) = &mut self.active {
            active.seen = false;
        }
        if let Some(focus) = &mut self.focus {
            focus.seen = false;
        }
        if let Some(popup) = &mut self.popup {
            popup.seen = false;
        }

        self.hover_layer = self.layer_at(self.pointer);
        self.press_layer = None;
        if let Some(at) = self.press {
            let layer = self.layer_at(at);
            self.press_layer = layer;
            self.owner = if layer.is_some() {
                Owner::Ui
            } else {
                Owner::Game
            };
            // A press outside the list and its combo closes the list.
            if self
                .popup
                .is_some_and(|popup| !popup.rect.contains(at) && !popup.anchor.contains(at))
            {
                self.popup = None;
            }
            if let Some(Layer::Window(id)) = layer {
                self.bring_to_front(id);
            }
        }
        if self.owner == Owner::Game {
            self.hover_layer = None;
            self.press_layer = None;
        }
        Frame { ui: self }
    }

    /// The window or popup at `at`, by the rects drawn in the last frame.
    fn layer_at(&self, at: Vec2) -> Option<Layer> {
        if self.popup.is_some_and(|popup| popup.rect.contains(at)) {
            return Some(Layer::Popup);
        }
        self.windows
            .iter()
            .rev()
            .find(|window| window.drawn && window.hit.contains(at))
            .map(|window| Layer::Window(window.id))
    }

    fn bring_to_front(&mut self, id: Id) {
        if let Some(index) = self.windows.iter().position(|window| window.id == id) {
            let window = self.windows.remove(index);
            self.windows.push(window);
        }
    }

    /// The index of the window `id`, made at `rect` if it is not known.
    fn window_index(&mut self, id: Id, key: &str, rect: Rect) -> usize {
        if let Some(index) = self.windows.iter().position(|window| window.id == id) {
            return index;
        }
        self.windows.push(WindowState {
            id,
            key: key.to_string(),
            rect,
            collapsed: false,
            scroll: 0.0,
            content: 0.0,
            drawn: false,
            shown: false,
            hit: Rect::default(),
            next_y: 0.0,
            part: None,
        });
        self.windows.len() - 1
    }

    /// A row's height: a line and its padding above and below.
    pub(crate) fn row_height(&self) -> f32 {
        self.metrics.line_height() + 2.0 * self.style.padding
    }

    /// How wide `text` is in the font.
    pub(crate) fn width(&self, text: &str) -> f32 {
        self.metrics.caret_x(text, text.len())
    }

    /// Interacts with the widget `id` at `rect`, in `layer`, cut to `clip`.
    pub(crate) fn interact(&mut self, layer: Layer, clip: Rect, id: Id, rect: Rect) -> Interaction {
        let inside = |at: Vec2| rect.contains(at) && clip.contains(at);
        let mut out = Interaction::default();
        if let Some(active) = self.active.as_mut().filter(|active| active.id == id) {
            active.seen = true;
        }
        let free = self.active.is_none_or(|active| active.id == id);
        out.hovered = free && self.hover_layer == Some(layer) && inside(self.pointer);
        if let Some(at) = self.press
            && self.active.is_none()
            && self.press_layer == Some(layer)
            && inside(at)
        {
            self.active = Some(Active {
                id,
                seen: true,
                from: at,
                last: at,
                start: Rect::default(),
                start_value: 0.0,
            });
            out.pressed = true;
        }
        if self.active.is_some_and(|active| active.id == id) {
            out.held = true;
            if let Some((at, cancelled)) = self.release {
                out.released = true;
                out.clicked = !cancelled && inside(at);
            }
        }
        out
    }

    /// The widget held, if it is `id`.
    pub(crate) fn active_mut(&mut self, id: Id) -> Option<&mut Active> {
        self.active.as_mut().filter(|active| active.id == id)
    }

    /// Records a box.
    pub(crate) fn rect(&mut self, layer: Layer, clip: Rect, rect: Rect, color: Color) {
        self.push(layer, clip, Shape::Rect { rect, color });
    }

    /// Records the text in `self.text` from `start` to `end`, its top-left
    /// corner at `at`.
    pub(crate) fn text_at(
        &mut self,
        layer: Layer,
        clip: Rect,
        at: Vec2,
        (start, end): (u32, u32),
        color: Color,
    ) {
        self.push(
            layer,
            clip,
            Shape::Text {
                at,
                start,
                end,
                color,
            },
        );
    }

    /// Records a line one unit thick.
    pub(crate) fn line(&mut self, layer: Layer, clip: Rect, from: Vec2, to: Vec2, color: Color) {
        self.push(layer, clip, Shape::Line { from, to, color });
    }

    pub(crate) fn push(&mut self, layer: Layer, clip: Rect, shape: Shape) {
        if !clip.is_empty() {
            self.cmds.push(Cmd { layer, clip, shape });
        }
    }

    /// Formats `text` into the text buffer: its start and end.
    pub(crate) fn format(&mut self, text: impl fmt::Display) -> (u32, u32) {
        // The buffer holds a frame's texts, far fewer than 4 GiB of them.
        let start = self.text.len() as u32;
        if write!(self.text, "{text}").is_err() {
            self.text.truncate(start as usize);
        }
        (start, self.text.len() as u32)
    }

    /// The text from `start` to `end`.
    pub(crate) fn text(&self, (start, end): (u32, u32)) -> &str {
        &self.text[start as usize..end as usize]
    }

    /// Pushes the frame's shapes into `out`.
    fn emit(&mut self, out: &mut UiList) {
        self.order.clear();
        let z = |layer: Layer, windows: &[WindowState]| match layer {
            Layer::Popup => u32::MAX,
            Layer::Window(id) => windows
                .iter()
                .position(|window| window.id == id)
                .map_or(0, |index| index as u32),
        };
        for (seq, cmd) in (0u32..).zip(&self.cmds) {
            self.order.push((z(cmd.layer, &self.windows), seq));
        }
        self.order.sort_unstable();
        // The front window is the last drawn.
        let front = self
            .windows
            .iter()
            .rev()
            .find(|window| window.shown)
            .map(|window| Layer::Window(window.id));
        let before = out.view();
        let base = UiView::new(self.style.scale);
        let mut clip = None;
        for &(_, seq) in &self.order {
            let cmd = self.cmds[seq as usize];
            if clip != Some(cmd.clip) {
                out.set_view(base.clipped(cmd.clip));
                clip = Some(cmd.clip);
            }
            match cmd.shape {
                Shape::Rect { rect, color } => {
                    out.sprite(self.sprite(rect.min, rect.size(), color));
                }
                Shape::Title { rect } => {
                    let color = if front == Some(cmd.layer) {
                        self.style.title_front
                    } else {
                        self.style.title
                    };
                    out.sprite(self.sprite(rect.min, rect.size(), color));
                }
                Shape::Text {
                    at,
                    start,
                    end,
                    color,
                } => {
                    let mut draw = TextDraw::new(self.font, at);
                    draw.layout.markup = false;
                    draw.color = color;
                    draw.layer = LAYER;
                    draw.sort = SortMode::Fixed(u32::MAX);
                    out.text(draw, &self.text[start as usize..end as usize]);
                }
                Shape::Line { from, to, color } => {
                    let along = to - from;
                    let length = along.length();
                    if length > 0.0 && length.is_finite() {
                        let mut sprite = self.sprite(from, Vec2::new(length, 1.0), color);
                        sprite.pivot = Vec2::new(0.0, 0.5);
                        sprite.rotation = along.y.atan2(along.x);
                        out.sprite(sprite);
                    }
                }
            }
        }
        out.set_view(before);
    }

    /// A box of the white texture, by its top-left corner.
    fn sprite(&self, at: Vec2, size: Vec2, color: Color) -> SpriteDraw {
        let mut sprite = SpriteDraw::new(self.white, at, size);
        sprite.pivot = Vec2::ZERO;
        sprite.color = color;
        sprite.layer = LAYER;
        sprite.sort = SortMode::Fixed(u32::MAX);
        sprite
    }

    /// Ends the frame: what was not called this frame lets go, and what the
    /// game should leave alone is worked out.
    fn end(&mut self) {
        if self.release.is_some() || self.active.is_some_and(|active| !active.seen) {
            self.active = None;
        }
        if self.focus.is_some_and(|focus| !focus.seen) {
            self.focus = None;
        }
        if self.popup.is_some_and(|popup| !popup.seen) {
            self.popup = None;
        }
        for window in &mut self.windows {
            window.drawn = window.shown;
        }
        self.drawn_any = self.windows.iter().any(|window| window.drawn);
        let pointer = self.pointer;
        let over = self.popup.is_some_and(|popup| popup.rect.contains(pointer))
            || self
                .windows
                .iter()
                .any(|window| window.drawn && window.hit.contains(pointer));
        // A press on a window keeps the mouse until its release, wherever
        // the pointer goes; one outside them all leaves it to the game.
        self.wants_mouse = match self.owner {
            Owner::Game => false,
            Owner::Ui => true,
            Owner::Nobody => over || self.active.is_some(),
        };
        self.wants_keyboard = self.focus.is_some();
        if !self.down {
            self.owner = Owner::Nobody;
        }
    }
}

/// A window's place, as [`DebugUi::places`] gives it.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowPlace<'a> {
    /// Its name: its explicit ID, or its title.
    pub title: &'a str,
    /// Where it is, and how large, in UI units.
    pub rect: Rect,
    /// Whether only its title bar shows.
    pub collapsed: bool,
}

/// A frame of the debug UI, from [`DebugUi::frame`]: its windows are
/// filled, then [`Frame::finish`] pushes what they draw. Dropped, it ends
/// the frame: a frame not finished draws nothing, and its input is spent.
pub struct Frame<'a> {
    ui: &'a mut DebugUi,
}

impl Frame<'_> {
    /// A window titled `title`, filled by `add`, which is not called while
    /// the window is collapsed. Returns whether it is open: not collapsed.
    ///
    /// - It shows at `first` the first time, unless a place was set for it
    ///   ([`DebugUi::set_place`]), and keeps its place after.
    /// - It is drawn in front of the windows already open, and comes to
    ///   the front when it is pressed.
    /// - Its title bar drags it, kept at least partly on screen, and its
    ///   box on the left collapses it; the grip at its bottom-right corner
    ///   resizes it; when its content is taller than it, the wheel over it
    ///   or its scrollbar's thumb scrolls it. A drag starts from where it is
    ///   drawn, which is its place moved on screen if it was off it.
    /// - `"Title##key"` shows `Title` and is the window `key`, whose place
    ///   is kept as the title changes. A second call of one window in a
    ///   frame continues its content.
    /// - With no output to draw on (a size of 0), nothing is shown.
    pub fn window(
        &mut self,
        title: impl fmt::Display,
        first: Rect,
        add: impl FnOnce(&mut Ui<'_>),
    ) -> bool {
        let core = &mut *self.ui;
        let screen = core.space.size;
        if !(screen.x > 0.0 && screen.y > 0.0) {
            return false;
        }
        core.scratch.clear();
        let _ = write!(core.scratch, "{title}");
        let scratch = std::mem::take(&mut core.scratch);
        let (shown, key) = split(&scratch);
        let id = Id::ROOT.with(key);
        let first = sanitize(first, core.style.min_size);
        let mut index = core.window_index(id, key, first);
        let title_range = {
            let start = core.text.len() as u32;
            core.text.push_str(shown);
            (start, core.text.len() as u32)
        };
        core.scratch = scratch;
        let style = core.style;
        let row = core.row_height();
        let layer = Layer::Window(id);

        // A second call in the frame goes on from the first, laid out as the
        // first laid it out.
        if core.windows[index].shown {
            let window = &core.windows[index];
            let (collapsed, part, next_y) = (window.collapsed, window.part, window.next_y);
            let Some(part) = part.filter(|_| !collapsed) else {
                return !collapsed;
            };
            let mut ui = Ui::new(core, layer, id, part.clip, part.left, part.width, next_y);
            add(&mut ui);
            let end = ui.y();
            let window = &mut core.windows[index];
            window.next_y = end;
            window.content = end - part.top + 2.0 * style.padding;
            return true;
        }
        // Shown again while others were drawn: in front of them.
        if !core.windows[index].drawn && core.drawn_any {
            core.bring_to_front(id);
            index = core.windows.len() - 1;
        }
        let (stored, mut collapsed, mut scroll, content) = {
            let window = &core.windows[index];
            (window.rect, window.collapsed, window.scroll, window.content)
        };
        let content = if content.is_finite() { content } else { 0.0 };
        let size = style.scrollbar;

        // The chrome, where the last frame drew it, takes its presses before
        // the content: the box, the grip and the thumb before the bar.
        let was = Chrome::geometry(on_screen(stored, screen, row), row, size, content);
        let mut place = stored;
        let pointer = core.pointer;
        let chrome = |part: Chrome| id.with(part);
        if core
            .interact(layer, was.rect, chrome(Chrome::Collapse), was.collapse)
            .clicked
        {
            collapsed = !collapsed;
            core.revision += 1;
        }
        if !collapsed {
            let held = core.interact(layer, was.rect, chrome(Chrome::Grip), was.grip);
            let mut start = None;
            if let Some(active) = core.active_mut(chrome(Chrome::Grip)) {
                // From where it is drawn, which may not be its place.
                if held.pressed {
                    active.start = was.rect;
                }
                let grown = (active.start.size() + pointer - active.from).max(style.min_size);
                place = Rect::from_min_size(active.start.min, grown);
                start = Some(active.start);
            }
            if held.released && start.is_some_and(|start| start != place) {
                core.revision += 1;
            }
            if was.scrollable {
                let thumb = was.thumb(scroll);
                let held = core.interact(layer, was.rect, chrome(Chrome::Thumb), thumb);
                if let Some(active) = core.active_mut(chrome(Chrome::Thumb)) {
                    if held.pressed {
                        active.start_value = scroll;
                    }
                    if was.room > 0.0 {
                        scroll =
                            active.start_value + (pointer.y - active.from.y) * was.range / was.room;
                    }
                }
            }
        }
        let held = core.interact(layer, was.rect, chrome(Chrome::Bar), was.drag);
        let mut start = None;
        if let Some(active) = core.active_mut(chrome(Chrome::Bar)) {
            if held.pressed {
                active.start = was.rect;
            }
            let moved = pointer - active.from;
            place = Rect::new(active.start.min + moved, active.start.max + moved);
            start = Some(active.start);
        }
        if held.released && start.is_some_and(|start| start != place) {
            core.revision += 1;
        }
        if core.hover_layer == Some(layer) && was.view.contains(pointer) {
            scroll -= core.input.wheel.y * ROWS_A_LINE * row;
        }

        // Drawn where it is now, moved or resized this frame.
        let now = Chrome::geometry(on_screen(place, screen, row), row, size, content);
        // Held in 0..=range, a NaN at 0, with no clamp whose bounds could
        // cross.
        scroll = if scroll.is_nan() {
            0.0
        } else {
            scroll.min(now.range).max(0.0)
        };
        if !collapsed {
            core.rect(layer, now.rect, now.rect, style.panel);
        }
        core.push(layer, now.rect, Shape::Title { rect: now.bar });
        let mark = core.format(if collapsed { "+" } else { "-" });
        let mark_at =
            now.collapse.min + Vec2::new((row - core.width(core.text(mark))) * 0.5, style.padding);
        core.text_at(layer, now.collapse, mark_at, mark, style.text);
        let title_at = Vec2::new(now.collapse.max.x, now.bar.min.y + style.padding);
        core.text_at(layer, now.drag, title_at, title_range, style.text);

        let mut part = None;
        let mut next_y = 0.0;
        let mut measured = content;
        if !collapsed {
            let width = if now.scrollable {
                now.view.width() - size
            } else {
                now.view.width()
            };
            let clip =
                Rect::from_min_size(now.view.min, Vec2::new(width.max(0.0), now.view.height()));
            let laid = Part {
                clip,
                left: now.view.min.x + style.padding,
                width: width - 2.0 * style.padding,
                top: now.view.min.y + style.padding - scroll,
            };
            let mut ui = Ui::new(core, layer, id, laid.clip, laid.left, laid.width, laid.top);
            add(&mut ui);
            next_y = ui.y();
            measured = next_y - laid.top + 2.0 * style.padding;
            part = Some(laid);
            if now.scrollable {
                core.rect(layer, now.rect, now.track, style.dim);
                core.rect(layer, now.rect, now.thumb(scroll), style.widget);
            }
            core.rect(layer, now.rect, now.grip, style.dim);
        }

        let window = &mut core.windows[index];
        window.rect = place;
        window.collapsed = collapsed;
        window.scroll = scroll;
        window.hit = if collapsed { now.bar } else { now.rect };
        window.content = measured;
        window.next_y = next_y;
        window.part = part;
        window.shown = true;
        !collapsed
    }

    /// Pushes what the frame's windows draw into `out`
    /// (`RenderFrame::ui`), window by window from back to front, an open
    /// combo's list last: in the top layer at the last depth
    /// (`SortMode::Fixed(u32::MAX)`), at the style's scale, each cut to its
    /// window. Call it after the game's UI is pushed, which then draws
    /// behind it. The list's view is put back as it was.
    pub fn finish(self, out: &mut UiList) {
        self.ui.emit(out);
    }
}

impl Drop for Frame<'_> {
    fn drop(&mut self) {
        self.ui.end();
    }
}

/// `rect` with no NaN or infinite edge and no smaller than `min`.
fn sanitize(rect: Rect, min: Vec2) -> Rect {
    let finite = [rect.min.x, rect.min.y, rect.max.x, rect.max.y]
        .iter()
        .all(|v| v.is_finite());
    let rect = if finite {
        rect
    } else {
        Rect::from_min_size(Vec2::ZERO, min)
    };
    Rect::from_min_size(rect.min, rect.size().max(min))
}

/// Where a window at `rect` is drawn on a screen of `screen`: moved so
/// that its title bar, `bar` high, is on it, at least some of its width.
fn on_screen(rect: Rect, screen: Vec2, bar: f32) -> Rect {
    let size = rect.size();
    let keep = (2.0 * bar).min(size.x);
    let x = rect.min.x.max(keep - size.x).min(screen.x - keep);
    let y = rect.min.y.min(screen.y - bar).max(0.0);
    Rect::from_min_size(Vec2::new(x, y), size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_is_kept_with_its_title_bar_on_screen() {
        let screen = Vec2::new(100.0, 80.0);
        let rect = |x: f32, y: f32| Rect::from_min_size(Vec2::new(x, y), Vec2::new(40.0, 30.0));
        assert_eq!(on_screen(rect(10.0, 10.0), screen, 10.0), rect(10.0, 10.0));
        // Off to the right, the left and the bottom; above the top.
        assert_eq!(on_screen(rect(500.0, 10.0), screen, 10.0).min.x, 80.0);
        assert_eq!(on_screen(rect(-500.0, 10.0), screen, 10.0).min.x, -20.0);
        assert_eq!(on_screen(rect(10.0, 500.0), screen, 10.0).min.y, 70.0);
        assert_eq!(on_screen(rect(10.0, -5.0), screen, 10.0).min.y, 0.0);
        // Its size is kept.
        assert_eq!(
            on_screen(rect(500.0, 500.0), screen, 10.0).size(),
            Vec2::new(40.0, 30.0)
        );
    }

    #[test]
    fn a_rect_that_is_not_finite_or_too_small_is_made_a_window_s() {
        let min = Vec2::new(60.0, 40.0);
        let nan = Rect::new(Vec2::new(f32::NAN, 0.0), Vec2::ONE);
        assert_eq!(sanitize(nan, min), Rect::from_min_size(Vec2::ZERO, min));
        let small = Rect::from_min_size(Vec2::new(5.0, 5.0), Vec2::new(10.0, 100.0));
        assert_eq!(sanitize(small, min).size(), Vec2::new(60.0, 100.0));
    }
}
