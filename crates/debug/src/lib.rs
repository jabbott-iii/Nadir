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

//! Developer tools for the Nadir engine: an immediate-mode debug UI, a
//! stats overlay ([`Stats`]) and a profiler with Chrome trace export
//! ([`Profiler`]), and, to come, an inspector, a console and debug-draw
//! toggles (roadmap M7.4 to M7.6). They are for development builds only: a
//! game depends on this crate behind its `dev` feature, and
//! `cargo xtask release-no-dev` checks that a release does not.
//!
//! # The debug UI
//!
//! A [`DebugUi`] keeps what lasts from frame to frame. Each frame,
//! [`DebugUi::frame`] takes that frame's events from a [`DebugInput`], code
//! fills windows with [`Frame::window`] and their widgets through a
//! [`Ui`], each widget answering what was done to it (a [`Response`]), and
//! [`Frame::finish`] pushes what they draw into the frame's
//! `RenderFrame::ui`, after the game's UI and in front of it.
//!
//! - **Input:** the engine feeds a [`DebugInput`] with the window's events
//!   (nadir_engine's `dev`); a test feeds it its own. A frame takes at most one
//!   press and one release of the left button, so that a click is judged
//!   where it was pressed and where it was released.
//! - **Identity:** a widget is the hash of its scope's identity and its
//!   text, or its explicit ID after `##`: `"Pause (3)##pause"`.
//! - **Hot and active:** the widget under the pointer is hot; the one the
//!   button went down on is active until it goes up, and a click is a
//!   release over the widget that was pressed.
//! - **Capture:** [`DebugUi::wants_mouse`] and [`DebugUi::wants_keyboard`]
//!   tell the game to leave the mouse or the keys alone, and
//!   [`DebugUi::is_over`] whether a point is over a window.
//! - **Places:** each window's place is kept as data
//!   ([`DebugUi::places`], [`DebugUi::set_place`]); the engine saves it.
//! - **Text:** measured by [`TextMetrics`]: [`FontMetrics`] for a `Font`.
//!
//! # The tools
//!
//! Each fills a window it is given. [`Stats::show`] shows the last frame's
//! numbers, which the engine copies in. A [`Profiler`] keeps the last
//! [`PROFILER_FRAMES`] frames, each with the scopes
//! `nadir_core::profile::take_frame` collected: [`Profiler::show`] draws
//! their processor times as a graph and the scopes of the frame clicked as
//! a tree, and [`Profiler::trace_json`] writes them as Chrome trace event
//! JSON, which Perfetto UI opens.
//!
//! ```
//! use nadir_core::{GenIndex, Handle, Key, MouseButton, Rect, UVec2, Vec2};
//! use nadir_debug::{DebugInput, DebugUi, TextMetrics};
//! use nadir_render::UiList;
//!
//! /// A font of 8 units a line, each character 6 wide.
//! struct Mono;
//! impl TextMetrics for Mono {
//!     fn line_height(&self) -> f32 {
//!         8.0
//!     }
//!     fn caret_x(&self, text: &str, byte: usize) -> f32 {
//!         6.0 * text[..byte].chars().count() as f32
//!     }
//! }
//!
//! fn handle<T>(index: u32) -> Handle<T> {
//!     Handle::new(GenIndex::new(index, std::num::NonZeroU32::MIN))
//! }
//! let mut ui = DebugUi::new(handle(0), Mono, handle(1));
//! let mut input = DebugInput::new();
//! let mut paused = false;
//! let mut name = String::from("hero");
//! let mut renamed = false;
//! let mut out = UiList::new();
//! let output = UVec2::new(640, 360);
//! for _ in 0..2 {
//!     // A click on the checkbox, in the second frame, where the first drew it.
//!     let mut frame = ui.frame(&mut input, output, output);
//!     frame.window("Tools", Rect::new(Vec2::ZERO, Vec2::new(200.0, 120.0)), |ui| {
//!         ui.checkbox("Paused", &mut paused);
//!         // Enter in the field, once it has the keyboard.
//!         renamed |= ui.text_input("Name", &mut name).submitted;
//!     });
//!     frame.finish(&mut out);
//!     input.pointer_moved(Vec2::new(8.0, 18.0));
//!     input.button(MouseButton::Left, true);
//!     input.button(MouseButton::Left, false);
//! }
//! assert!(paused && !renamed);
//! assert!(!ui.wants_keyboard());
//! # let _ = Key::Enter;
//! ```

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

mod edit;
mod id;
mod input;
mod metrics;
mod profiler;
mod stats;
mod style;
#[cfg(test)]
mod tests;
mod ui;
mod widgets;

pub use input::DebugInput;
pub use metrics::{FontMetrics, TextMetrics};
pub use profiler::{
    FrameTiming, PROFILER_FRAMES, PROFILER_RECORDS, ProfiledFrame, Profiler, ProfilerResponse,
};
pub use stats::Stats;
pub use style::Style;
pub use ui::{DebugUi, Frame, WindowPlace};
pub use widgets::{Plot, PlotResponse, Response, Ui};
