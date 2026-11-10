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

//! The debug UI's look: its scale, sizes and colors.

use nadir_core::{Color, Vec2};
use nadir_render::UiScale;

/// The debug UI's look. Sizes are in UI units: a UI unit is a pixel of
/// the font.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    /// The UI's scale: the settings' UI scale, a free one, by default the
    /// output's own pixels (`UiScale::Free(1.0)`).
    pub scale: UiScale,
    /// Around a row's text, above and below, and inside a window's edges.
    pub padding: f32,
    /// Between two rows, or two widgets of a row.
    pub spacing: f32,
    /// How far a tree node's children are moved in.
    pub indent: f32,
    /// How much of a row a control takes, its label after it.
    pub control_share: f32,
    /// How wide a labelled control is in a row of several
    /// ([`Ui::horizontal`](crate::Ui::horizontal)).
    pub control_width: f32,
    /// How wide the scrollbar is, and how large the resize grip.
    pub scrollbar: f32,
    /// The smallest a window may be made.
    pub min_size: Vec2,
    /// A window's body.
    pub panel: Color,
    /// A window's title bar.
    pub title: Color,
    /// The title bar of the window in front.
    pub title_front: Color,
    /// A control at rest.
    pub widget: Color,
    /// A control under the pointer.
    pub hot: Color,
    /// A control held.
    pub active: Color,
    /// A checkbox's mark, a slider's handle, a plot's line.
    pub accent: Color,
    /// Text.
    pub text: Color,
    /// A line, the scrollbar's track.
    pub dim: Color,
    /// Selected text.
    pub selection: Color,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            scale: UiScale::Free(1.0),
            padding: 2.0,
            spacing: 2.0,
            indent: 10.0,
            control_share: 0.6,
            control_width: 80.0,
            scrollbar: 6.0,
            min_size: Vec2::new(60.0, 40.0),
            panel: Color::rgba(24, 24, 32, 230),
            title: Color::rgb(48, 48, 72),
            title_front: Color::rgb(64, 72, 128),
            widget: Color::rgb(56, 56, 72),
            hot: Color::rgb(80, 80, 104),
            active: Color::rgb(104, 104, 144),
            accent: Color::rgb(160, 200, 255),
            text: Color::rgb(230, 230, 230),
            dim: Color::rgb(96, 96, 112),
            selection: Color::rgb(64, 96, 160),
        }
    }
}
