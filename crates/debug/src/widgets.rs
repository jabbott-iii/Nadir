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

//! A window's content: its layout and its widgets.

use std::fmt;
use std::hash::Hash;
use std::ops::RangeInclusive;

use nadir_core::hash::fnv1a64;
use nadir_core::{Color, Key, Rect, Vec2};

use crate::edit::{Caret, apply};
use crate::id::{Id, split};
use crate::input::Edit;
use crate::ui::{DebugUi, Focus, Interaction, Layer, Popup};

/// Marks a `with_id` key apart from a widget's label.
const SCOPE: u64 = 0x7363_6f70_655f_6964;

/// A plot's height unless it says another.
const DEFAULT_PLOT_HEIGHT: f32 = 30.0;

/// What a widget did this frame.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Response {
    /// The pointer is over it, and no other widget is held.
    pub hovered: bool,
    /// It is held: the button went down on it and is not up yet, or went
    /// up this frame.
    pub active: bool,
    /// The button went down on it and up over it, the press and the
    /// release each where they happened.
    pub clicked: bool,
    /// Its value changed.
    pub changed: bool,
    /// A text field: Enter was pressed while it had the keyboard, which it
    /// keeps.
    pub submitted: bool,
    /// A text field: it lost the keyboard, to Escape or to a press
    /// elsewhere.
    pub lost_focus: bool,
}

impl From<Interaction> for Response {
    fn from(interaction: Interaction) -> Self {
        Self {
            hovered: interaction.hovered,
            active: interaction.held,
            clicked: interaction.clicked,
            ..Self::default()
        }
    }
}

/// How [`Ui::plot`] draws its values.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct Plot {
    /// The values at its bottom and top: a value outside is drawn at the
    /// edge; a reversed range is swapped, and one with no width, or a NaN
    /// bound, draws every value in the middle.
    pub range: RangeInclusive<f32>,
    /// Its height, in UI units: one that is not finite or not above 0 is
    /// the default, 30.
    pub height: f32,
    /// The value marked with a line across it, if any.
    pub selected: Option<usize>,
}

impl Plot {
    /// A plot of the values in `range`, 30 units high, nothing marked.
    pub fn new(range: RangeInclusive<f32>) -> Self {
        Self {
            range,
            height: DEFAULT_PLOT_HEIGHT,
            selected: None,
        }
    }
}

/// What a plot did this frame.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlotResponse {
    /// As any widget's.
    pub response: Response,
    /// The value under the pointer.
    pub hovered: Option<usize>,
    /// The value clicked.
    pub clicked: Option<usize>,
}

/// A row of widgets side by side, being laid out.
#[derive(Clone, Copy, Debug)]
struct Row {
    x: f32,
    top: f32,
    height: f32,
}

/// A window's content, being filled: each widget takes a row, from the
/// top, or a place in a row of [`Ui::horizontal`].
///
/// A widget's text is formatted with `Display`, into a buffer the frames
/// reuse. Its identity is the hash of its scope's (its window's, a tree
/// node's, a [`Ui::with_id`]'s) and its text, or its explicit ID:
/// `"Pause (3)##pause"` shows `Pause (3)` and is the widget `pause`, which
/// stays the one held, or the field with the keyboard, as its count
/// changes. Two widgets of one text in one scope are one widget: give them
/// IDs, or scopes.
pub struct Ui<'a> {
    core: &'a mut DebugUi,
    layer: Layer,
    scope: Id,
    clip: Rect,
    left: f32,
    width: f32,
    indent: f32,
    y: f32,
    row: Option<Row>,
}

impl<'a> Ui<'a> {
    pub(crate) fn new(
        core: &'a mut DebugUi,
        layer: Layer,
        scope: Id,
        clip: Rect,
        left: f32,
        width: f32,
        y: f32,
    ) -> Self {
        Self {
            core,
            layer,
            scope,
            clip,
            left,
            width,
            indent: 0.0,
            y,
            row: None,
        }
    }

    /// Where the next row goes.
    pub(crate) fn y(&self) -> f32 {
        self.y
    }

    /// Fills `add` with the widgets of `key`'s scope: for the same texts
    /// in a loop, say, `ui.with_id(index, |ui| ...)`.
    pub fn with_id(&mut self, key: impl Hash, add: impl FnOnce(&mut Ui<'_>)) {
        let scope = self.scope;
        self.scope = scope.with((SCOPE, key));
        add(self);
        self.scope = scope;
    }

    /// Lays the widgets of `add` side by side in one row, as high as the
    /// highest. A control with a label takes the style's `control_width`
    /// there, its label after it; a button, a checkbox or a label, its
    /// text's width.
    pub fn horizontal(&mut self, add: impl FnOnce(&mut Ui<'_>)) {
        if self.row.is_some() {
            add(self);
            return;
        }
        self.row = Some(Row {
            x: self.left + self.indent,
            top: self.y,
            height: 0.0,
        });
        add(self);
        if let Some(row) = self.row.take()
            && row.height > 0.0
        {
            self.y += row.height + self.core.style.spacing;
        }
    }

    /// Whether `key` was pressed this frame, or repeated: for keys a text
    /// field does not use, Up, Down, Tab, PageUp and PageDown, while it has
    /// the keyboard, as a console's history and completion want.
    pub fn key_pressed(&self, key: Key) -> bool {
        self.core
            .input
            .edits
            .iter()
            .any(|edit| matches!(*edit, Edit::Key { key: pressed, .. } if pressed == key))
    }

    /// Gives the keyboard to the next text field called, its caret at its
    /// end: a console's command line when it opens, say. The field takes
    /// none of this frame's edits, which were typed before it had the
    /// keyboard: the key that opened the console is not typed into it.
    pub fn focus_next(&mut self) {
        self.core.focus_next = true;
    }

    /// A line of text.
    pub fn label(&mut self, text: impl fmt::Display) {
        let color = self.core.style.text;
        self.label_colored(color, text);
    }

    /// A line of text in `color`. One out of the window's view is not
    /// formatted, in a column of rows, which keeps a long log cheap.
    pub fn label_colored(&mut self, color: Color, text: impl fmt::Display) {
        let height = self.core.row_height();
        if self.row.is_none() {
            let probe = Rect::from_min_size(
                Vec2::new(self.left + self.indent, self.y),
                Vec2::new(self.width - self.indent, height),
            );
            if !probe.intersects(self.clip) {
                self.allocate(None, height);
                return;
            }
        }
        let shown = self.core.format(text);
        let width = self.core.width(self.core.text(shown));
        let rect = self.allocate(Some(width), height);
        let at = rect.min + Vec2::new(0.0, self.core.style.padding);
        self.core.text_at(self.layer, self.clip, at, shown, color);
    }

    /// A button, as wide as its text. Clicked when the button goes down on
    /// it and up over it.
    pub fn button(&mut self, text: impl fmt::Display) -> Response {
        let (id, shown) = self.labeled(text);
        let padding = self.core.style.padding;
        let width = self.core.width(self.core.text(shown)) + 4.0 * padding;
        let height = self.core.row_height();
        let rect = self.allocate(Some(width), height);
        let interaction = self.interact(id, rect);
        let color = self.state_color(interaction);
        self.fill(rect, color);
        let at = rect.min + Vec2::new(2.0 * padding, padding);
        self.write(at, shown, self.core.style.text, rect);
        interaction.into()
    }

    /// A box ticked when `value` is true, and its text: a click on either
    /// flips it.
    pub fn checkbox(&mut self, text: impl fmt::Display, value: &mut bool) -> Response {
        let (id, shown) = self.labeled(text);
        let style = self.core.style;
        let height = self.core.row_height();
        let width = height + style.spacing + self.core.width(self.core.text(shown));
        let rect = self.allocate(Some(width), height);
        let interaction = self.interact(id, rect);
        let mut response = Response::from(interaction);
        if interaction.clicked {
            *value = !*value;
            response.changed = true;
        }
        let side = height - 2.0 * style.padding;
        let mark = Rect::from_min_size(rect.min + Vec2::splat(style.padding), Vec2::splat(side));
        let color = self.state_color(interaction);
        self.fill(mark, color);
        if *value {
            let inner = Rect::new(mark.min + Vec2::splat(2.0), mark.max - Vec2::splat(2.0));
            self.fill(inner, style.accent);
        }
        let at = Vec2::new(
            rect.min.x + height + style.spacing,
            rect.min.y + style.padding,
        );
        self.write(at, shown, style.text, rect);
        response
    }

    /// A slider of `value` in `range`: pressing or dragging on it sets the
    /// value under the pointer, held in the range. A value outside the range
    /// is left as it is until the slider is moved; a reversed range is
    /// swapped; with a NaN bound, it changes nothing.
    pub fn slider_f32(
        &mut self,
        text: impl fmt::Display,
        value: &mut f32,
        range: RangeInclusive<f32>,
    ) -> Response {
        let (id, shown) = self.labeled(text);
        let (control, label) = self.control_row(shown);
        let interaction = self.interact(id, control);
        let mut response = Response::from(interaction);
        let (low, high) = ordered(*range.start(), *range.end());
        let usable = low.is_finite() && high.is_finite();
        if interaction.held && usable {
            let t = fraction(self.core.pointer.x, control);
            // Exact at the ends, and no overflow over a wide range.
            let next = low * (1.0 - t) + high * t;
            if next != *value {
                *value = next;
                response.changed = true;
            }
        }
        let t = if usable && high > low {
            unit((*value - low) / (high - low))
        } else {
            0.0
        };
        let shown_value = self.core.format(format_args!("{:.3}", *value));
        self.draw_slider(interaction, control, t, shown_value);
        self.write_label(label, shown);
        response
    }

    /// A slider of whole numbers, as [`Ui::slider_f32`]: the value under the
    /// pointer is rounded to the nearest, over any range of `i32`.
    pub fn slider_i32(
        &mut self,
        text: impl fmt::Display,
        value: &mut i32,
        range: RangeInclusive<i32>,
    ) -> Response {
        let (id, shown) = self.labeled(text);
        let (control, label) = self.control_row(shown);
        let interaction = self.interact(id, control);
        let mut response = Response::from(interaction);
        let (low, high) = {
            let (a, b) = (i64::from(*range.start()), i64::from(*range.end()));
            (a.min(b), a.max(b))
        };
        let span = (high - low) as f64;
        if interaction.held {
            let t = f64::from(fraction(self.core.pointer.x, control));
            let next = (low + (span * t).round() as i64).clamp(low, high);
            // Inside two i32 bounds.
            let next = next as i32;
            if next != *value {
                *value = next;
                response.changed = true;
            }
        }
        let t = if span > 0.0 {
            ((i64::from(*value) - low) as f64 / span).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let shown_value = self.core.format(*value);
        self.draw_slider(interaction, control, t, shown_value);
        self.write_label(label, shown);
        response
    }

    /// A number changed by dragging across it: `per_unit` for each UI unit
    /// the pointer moves right, less moving left. A step or a result that
    /// is not finite changes nothing.
    pub fn drag_f32(
        &mut self,
        text: impl fmt::Display,
        value: &mut f32,
        per_unit: f32,
    ) -> Response {
        let (id, shown) = self.labeled(text);
        let (control, label) = self.control_row(shown);
        let interaction = self.interact(id, control);
        let mut response = Response::from(interaction);
        let pointer = self.core.pointer;
        if let Some(active) = self.core.active_mut(id) {
            let moved = pointer.x - active.last.x;
            active.last = pointer;
            let next = *value + moved * per_unit;
            if next.is_finite() && next != *value {
                *value = next;
                response.changed = true;
            }
        }
        let color = self.state_color(interaction);
        self.fill(control, color);
        let shown_value = self.core.format(format_args!("{:.3}", *value));
        self.write_centered(control, shown_value);
        self.write_label(label, shown);
        response
    }

    /// A one-line text field editing `value`.
    ///
    /// A press on it gives it the keyboard, the caret where it was pressed;
    /// a press elsewhere, Escape, or another field taking the keyboard,
    /// takes it back (`lost_focus`, when the field is next called). With the
    /// keyboard, typed text goes in at the caret, replacing a selection;
    /// Backspace and Delete remove; Left, Right, Home and End move the
    /// caret, selecting with Shift; Control or Super with A selects all;
    /// Enter is `submitted`, and the field keeps the keyboard. Other keys
    /// are the caller's ([`Ui::key_pressed`]). A `value` the caller changed
    /// puts the caret at its end. Copy and paste are not built.
    pub fn text_input(&mut self, text: impl fmt::Display, value: &mut String) -> Response {
        let (id, shown) = self.labeled(text);
        let (control, label) = self.control_row(shown);
        let interaction = self.interact(id, control);
        let mut response = Response::from(interaction);
        let style = self.core.style;
        let inner = Rect::new(
            control.min + Vec2::new(style.padding, 0.0),
            control.max - Vec2::new(style.padding, 0.0),
        );
        let core = &mut *self.core;
        // The keyboard taken by another field since this one last ran.
        if core.lost == Some(id) {
            core.lost = None;
            response.lost_focus = true;
        }
        let mut focused = core.focus.is_some_and(|focus| focus.id == id);
        // Given the keyboard by `focus_next`, the field takes none of this
        // frame's edits, typed before it had it: the key that opened a
        // console, say.
        let mut fresh = false;
        if interaction.pressed {
            if !focused {
                take_focus(core, id, value);
                focused = true;
            }
            if let (Some(focus), Some(at)) = (core.focus.as_mut(), core.press) {
                let x = at.x - inner.min.x + focus.scroll;
                focus.caret = Caret::at(nearest_caret(&*core.metrics, value, x));
            }
        } else if focused && core.press.is_some() {
            core.focus = None;
            focused = false;
            response.lost_focus = true;
        }
        if core.focus_next {
            core.focus_next = false;
            if !focused {
                take_focus(core, id, value);
                focused = true;
                fresh = true;
            }
        }
        if focused && let Some(focus) = core.focus.as_mut() {
            focus.seen = true;
            if value.len() != focus.len || fnv1a64(value.as_bytes()) != focus.hash {
                focus.caret = Caret::at(value.len());
                focus.scroll = 0.0;
            }
            focus.caret.fit(value);
            let mut escaped = false;
            for &edit in core.input.edits.iter().filter(|_| !fresh) {
                let edited = apply(value, &mut focus.caret, edit);
                response.changed |= edited.changed;
                response.submitted |= edited.submitted;
                if edited.escaped {
                    escaped = true;
                    break;
                }
            }
            focus.len = value.len();
            focus.hash = fnv1a64(value.as_bytes());
            // Scrolled to keep the caret in view, and no further than the
            // text's end needs.
            let caret_x = core.metrics.caret_x(value, focus.caret.caret);
            let end_x = core.metrics.caret_x(value, value.len());
            let room = (inner.width() - 1.0).max(0.0);
            if caret_x - focus.scroll > room {
                focus.scroll = caret_x - room;
            }
            if caret_x < focus.scroll {
                focus.scroll = caret_x;
            }
            focus.scroll = focus.scroll.min(end_x - room).max(0.0);
            if escaped {
                core.focus = None;
                focused = false;
                response.lost_focus = true;
            }
        }

        let color = if focused {
            style.active
        } else {
            self.state_color(interaction)
        };
        self.fill(control, color);
        let field = self.clip.intersection(inner);
        let focus = self.core.focus.filter(|_| focused);
        let scroll = focus.map_or(0.0, |focus| focus.scroll);
        let start = inner.min.x - scroll;
        if let (Some(focus), Some(field)) = (focus, field) {
            let (from, to) = focus.caret.selection();
            if from != to {
                let x0 = start + self.core.metrics.caret_x(value, from);
                let x1 = start + self.core.metrics.caret_x(value, to);
                let selection = Rect::new(
                    Vec2::new(x0, control.min.y + style.padding),
                    Vec2::new(x1, control.max.y - style.padding),
                );
                self.core
                    .rect(self.layer, field, selection, style.selection);
            }
        }
        if let Some(field) = field {
            let range = self.core.format(&*value);
            let at = Vec2::new(start, control.min.y + style.padding);
            self.core.text_at(self.layer, field, at, range, style.text);
        }
        if let (Some(focus), Some(field)) = (focus, field) {
            let x = start + self.core.metrics.caret_x(value, focus.caret.caret);
            let caret = Rect::from_min_size(
                Vec2::new(x, control.min.y + style.padding),
                Vec2::new(1.0, self.core.metrics.line_height()),
            );
            self.core.rect(self.layer, field, caret, style.text);
        }
        self.write_label(label, shown);
        response
    }

    /// A choice of `items`, `selected` the one shown: a click opens their
    /// list over everything, and a click on one picks it; a press elsewhere
    /// closes the list. With no items, or `selected` past them, it shows
    /// nothing and changes nothing until one is picked.
    pub fn combo(
        &mut self,
        text: impl fmt::Display,
        selected: &mut usize,
        items: &[&str],
    ) -> Response {
        let (id, shown) = self.labeled(text);
        let (control, label) = self.control_row(shown);
        let interaction = self.interact(id, control);
        let mut response = Response::from(interaction);
        let style = self.core.style;
        let row = self.core.row_height();
        let open = self.core.popup.is_some_and(|popup| popup.id == id);
        if interaction.clicked {
            self.core.popup = (!open).then_some(Popup {
                id,
                seen: true,
                anchor: control,
                rect: control,
            });
        }
        let color = self.state_color(interaction);
        self.fill(control, color);
        if let Some(item) = items.get(*selected) {
            let range = self.core.format(item);
            let at = control.min + Vec2::splat(style.padding);
            self.write(at, range, style.text, control);
        }
        self.write_label(label, shown);

        if self.core.popup.is_some_and(|popup| popup.id == id) {
            let screen = self.core.space.size;
            let height = row * items.len() as f32;
            let mut top = control.max.y;
            if top + height > screen.y && control.min.y - height >= 0.0 {
                top = control.min.y - height;
            }
            let rect = Rect::from_min_size(
                Vec2::new(control.min.x, top),
                Vec2::new(control.width(), height),
            );
            if let Some(popup) = self.core.popup.as_mut() {
                popup.seen = true;
                popup.anchor = control;
                popup.rect = rect;
            }
            let (layer, clip) = (self.layer, self.clip);
            self.layer = Layer::Popup;
            self.clip = rect;
            self.fill(rect, style.panel);
            for (index, item) in items.iter().enumerate() {
                let at = Rect::from_min_size(
                    Vec2::new(rect.min.x, rect.min.y + row * index as f32),
                    Vec2::new(rect.width(), row),
                );
                let picked = self.interact(id.with(index), at);
                if picked.hovered || picked.held {
                    self.fill(at, style.hot);
                }
                let range = self.core.format(item);
                self.write(at.min + Vec2::splat(style.padding), range, style.text, at);
                if picked.clicked {
                    *selected = index;
                    response.changed = true;
                    self.core.popup = None;
                }
            }
            self.layer = layer;
            self.clip = clip;
        }
        response
    }

    /// A node of a tree, open or closed by a click, `add` filling its
    /// children, moved in, while it is open. It keeps its state by its
    /// identity, and is the scope of its children's.
    pub fn tree_node(
        &mut self,
        text: impl fmt::Display,
        add: impl FnOnce(&mut Ui<'_>),
    ) -> Response {
        let (id, shown) = self.labeled(text);
        let style = self.core.style;
        let height = self.core.row_height();
        let rect = self.allocate(None, height);
        let interaction = self.interact(id, rect);
        let mut response = Response::from(interaction);
        let mut open = self.core.open.contains(&id);
        if interaction.clicked {
            open = !open;
            if open {
                self.core.open.insert(id);
            } else {
                self.core.open.remove(&id);
            }
            response.changed = true;
        }
        if interaction.hovered || interaction.held {
            self.fill(rect, style.hot);
        }
        let mark = self.core.format(if open { "-" } else { "+" });
        let at = rect.min + Vec2::splat(style.padding);
        self.write(at, mark, style.text, rect);
        let at = Vec2::new(rect.min.x + height, rect.min.y + style.padding);
        self.write(at, shown, style.text, rect);
        if open {
            let (scope, indent) = (self.scope, self.indent);
            self.scope = id;
            self.indent += style.indent;
            add(self);
            self.scope = scope;
            self.indent = indent;
        }
        response
    }

    /// A line across the window.
    pub fn separator(&mut self) {
        let style = self.core.style;
        let rect = self.allocate(None, style.spacing + 1.0);
        let line = Rect::from_min_size(
            Vec2::new(rect.min.x, rect.min.y + (style.spacing * 0.5).floor()),
            Vec2::new(rect.width(), 1.0),
        );
        self.fill(line, style.dim);
    }

    /// A line graph of `values`, from the left edge to the right, each
    /// value `i` of `n` at `i / (n − 1)` of the width, as `plot` says;
    /// values that are not finite break the line. It says which value is
    /// under the pointer, and which was clicked.
    pub fn plot(
        &mut self,
        text: impl fmt::Display,
        values: impl IntoIterator<Item = f32>,
        plot: Plot,
    ) -> PlotResponse {
        let (id, shown) = self.labeled(text);
        let style = self.core.style;
        let text_width = self.core.width(self.core.text(shown));
        let height = if plot.height.is_finite() && plot.height > 0.0 {
            plot.height.max(1.0)
        } else {
            DEFAULT_PLOT_HEIGHT
        };
        let (graph, label) = self.control_rects(text_width, height);
        let interaction = self.interact(id, graph);
        self.core.values.clear();
        self.core.values.extend(values);
        let count = self.core.values.len();
        let x_of = |index: usize| {
            if count > 1 {
                graph.min.x + graph.width() * index as f32 / (count - 1) as f32
            } else {
                graph.center().x
            }
        };
        let index_at = |x: f32| {
            if count == 0 {
                return None;
            }
            let share = fraction(x, graph);
            Some(((share * (count - 1) as f32).round() as usize).min(count - 1))
        };
        let (low, high) = ordered(*plot.range.start(), *plot.range.end());
        let usable = low.is_finite() && high.is_finite() && high > low;
        let y_of = |value: f32| {
            let share = if usable {
                unit((value - low) / (high - low))
            } else {
                0.5
            };
            graph.max.y - 0.5 - share * (graph.height() - 1.0)
        };
        self.fill(graph, style.widget);
        let clip = self.clip.intersection(graph);
        if let Some(clip) = clip {
            if let Some(selected) = plot.selected.filter(|&index| index < count) {
                // Inside the graph, the last value's mark too.
                let x = x_of(selected).min(graph.max.x - 1.0);
                let line =
                    Rect::from_min_size(Vec2::new(x, graph.min.y), Vec2::new(1.0, graph.height()));
                self.core.rect(self.layer, clip, line, style.hot);
            }
            let mut previous: Option<Vec2> = None;
            for index in 0..count {
                let value = self.core.values[index];
                let point = value
                    .is_finite()
                    .then(|| Vec2::new(x_of(index), y_of(value)));
                match (previous, point) {
                    (Some(from), Some(to)) => {
                        self.core.line(self.layer, clip, from, to, style.accent);
                    }
                    (None, Some(at)) if count == 1 => {
                        let dot = Rect::from_min_size(at - Vec2::splat(0.5), Vec2::ONE);
                        self.core.rect(self.layer, clip, dot, style.accent);
                    }
                    _ => {}
                }
                previous = point;
            }
        }
        self.write_label(label, shown);
        let pointer = self.core.pointer;
        let release = self.core.release.map(|(at, _)| at);
        PlotResponse {
            response: interaction.into(),
            hovered: if interaction.hovered {
                index_at(pointer.x)
            } else {
                None
            },
            clicked: if interaction.clicked {
                release.and_then(|at| index_at(at.x))
            } else {
                None
            },
        }
    }

    /// A color: a swatch and its text, then a slider of 0 to 255 for each
    /// of red, green, blue and alpha, in its own scope.
    pub fn color_edit(&mut self, text: impl fmt::Display, color: &mut Color) -> Response {
        let (id, shown) = self.labeled(text);
        let style = self.core.style;
        let height = self.core.row_height();
        let width = height + style.spacing + self.core.width(self.core.text(shown));
        let rect = self.allocate(Some(width), height);
        let swatch = Rect::from_min_size(
            rect.min + Vec2::splat(style.padding),
            Vec2::splat(height - 2.0 * style.padding),
        );
        self.fill(swatch, *color);
        let at = Vec2::new(
            rect.min.x + height + style.spacing,
            rect.min.y + style.padding,
        );
        self.write(at, shown, style.text, rect);
        let mut response = Response::default();
        let scope = self.scope;
        self.scope = id;
        let mut channels = [color.r, color.g, color.b, color.a].map(i32::from);
        for (name, channel) in ["R", "G", "B", "A"].into_iter().zip(&mut channels) {
            let moved = self.slider_i32(name, channel, 0..=255);
            response.changed |= moved.changed;
            response.active |= moved.active;
            response.hovered |= moved.hovered;
        }
        self.scope = scope;
        if response.changed {
            // Held in 0..=255 by the sliders.
            let [r, g, b, a] = channels.map(|channel| channel.clamp(0, 255) as u8);
            *color = Color::rgba(r, g, b, a);
        }
        response
    }

    /// Formats a widget's text: its identity, and what it shows.
    fn labeled(&mut self, text: impl fmt::Display) -> (Id, (u32, u32)) {
        let (start, end) = self.core.format(text);
        let (shown, key) = split(self.core.text((start, end)));
        let id = self.scope.with(key);
        (id, (start, start + shown.len() as u32))
    }

    /// The rect of the next widget, `width` wide or the row's width, and
    /// `height` high, and moves the layout past it.
    fn allocate(&mut self, width: Option<f32>, height: f32) -> Rect {
        let spacing = self.core.style.spacing;
        if let Some(row) = self.row.as_mut() {
            let width = width.unwrap_or(self.core.style.control_width);
            let rect = Rect::from_min_size(Vec2::new(row.x, row.top), Vec2::new(width, height));
            row.x += width + spacing;
            row.height = row.height.max(height);
            return rect;
        }
        let x = self.left + self.indent;
        let width = width.unwrap_or(self.width - self.indent);
        let rect = Rect::from_min_size(Vec2::new(x, self.y), Vec2::new(width, height));
        self.y += height + spacing;
        rect
    }

    /// A control with a label after it, a row high: the control's rect and
    /// where the label goes.
    fn control_row(&mut self, shown: (u32, u32)) -> (Rect, Vec2) {
        let width = self.core.width(self.core.text(shown));
        let height = self.core.row_height();
        self.control_rects(width, height)
    }

    /// A control `height` high with a label `text_width` wide after it.
    fn control_rects(&mut self, text_width: f32, height: f32) -> (Rect, Vec2) {
        let style = self.core.style;
        let (rect, control) = if self.row.is_some() {
            let control = style.control_width;
            (
                self.allocate(Some(control + style.spacing + text_width), height),
                control,
            )
        } else {
            let rect = self.allocate(None, height);
            (rect, (rect.width() * style.control_share).floor().max(0.0))
        };
        let control = Rect::from_min_size(rect.min, Vec2::new(control, height));
        let label = Vec2::new(control.max.x + style.spacing, rect.min.y + style.padding);
        (control, label)
    }

    fn interact(&mut self, id: Id, rect: Rect) -> Interaction {
        self.core.interact(self.layer, self.clip, id, rect)
    }

    /// The color of a control in `interaction`'s state.
    fn state_color(&self, interaction: Interaction) -> Color {
        let style = &self.core.style;
        if interaction.held {
            style.active
        } else if interaction.hovered {
            style.hot
        } else {
            style.widget
        }
    }

    fn fill(&mut self, rect: Rect, color: Color) {
        self.core.rect(self.layer, self.clip, rect, color);
    }

    /// Writes the text `range` at `at`, cut to `within` and the clip.
    fn write(&mut self, at: Vec2, range: (u32, u32), color: Color, within: Rect) {
        if let Some(clip) = self.clip.intersection(within) {
            self.core.text_at(self.layer, clip, at, range, color);
        }
    }

    /// Writes the text `range` in the middle of `control`.
    fn write_centered(&mut self, control: Rect, range: (u32, u32)) {
        let width = self.core.width(self.core.text(range));
        let at = Vec2::new(
            (control.center().x - width * 0.5).floor(),
            control.min.y + self.core.style.padding,
        );
        let color = self.core.style.text;
        self.write(at, range, color, control);
    }

    /// Writes a control's label at `at`.
    fn write_label(&mut self, at: Vec2, range: (u32, u32)) {
        let color = self.core.style.text;
        self.core.text_at(self.layer, self.clip, at, range, color);
    }

    fn draw_slider(&mut self, interaction: Interaction, control: Rect, t: f32, value: (u32, u32)) {
        let style = self.core.style;
        let color = self.state_color(interaction);
        self.fill(control, color);
        let handle_width = (control.height() * 0.5).floor().max(1.0);
        let x = control.min.x + t * (control.width() - handle_width).max(0.0);
        let handle = Rect::from_min_size(
            Vec2::new(x, control.min.y),
            Vec2::new(handle_width, control.height()),
        );
        self.fill(handle, style.accent);
        self.write_centered(control, value);
    }
}

/// `a` and `b`, the smaller first; a NaN stays where it is.
fn ordered(a: f32, b: f32) -> (f32, f32) {
    if b < a { (b, a) } else { (a, b) }
}

/// How far across `rect` `x` is, held in 0 to 1; 0 for a rect with no
/// width.
fn fraction(x: f32, rect: Rect) -> f32 {
    unit((x - rect.min.x) / rect.width())
}

/// `share` held in 0 to 1, a NaN at 0.
fn unit(share: f32) -> f32 {
    if share.is_nan() {
        0.0
    } else {
        share.clamp(0.0, 1.0)
    }
}

/// Gives the keyboard to the field `id`, editing `value`, its caret at the
/// end; a field that had it says so when it is next called.
fn take_focus(core: &mut DebugUi, id: Id, value: &str) {
    if let Some(old) = core.focus.filter(|old| old.id != id) {
        core.lost = Some(old.id);
    }
    if core.lost == Some(id) {
        core.lost = None;
    }
    core.focus = Some(Focus {
        id,
        seen: true,
        caret: Caret::at(value.len()),
        scroll: 0.0,
        len: value.len(),
        hash: fnv1a64(value.as_bytes()),
    });
}

/// The character boundary of `text` nearest `x`, the pen's distance from
/// its start.
fn nearest_caret(metrics: &dyn crate::TextMetrics, text: &str, x: f32) -> usize {
    let mut best = (f32::INFINITY, 0);
    for byte in text
        .char_indices()
        .map(|(byte, _)| byte)
        .chain([text.len()])
    {
        let distance = (metrics.caret_x(text, byte) - x).abs();
        if distance < best.0 {
            best = (distance, byte);
        }
    }
    best.1
}
