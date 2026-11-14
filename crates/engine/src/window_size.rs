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

//! [`WindowSize`]: the size the frame is drawn at, in development builds.

use nadir_core::UVec2;

/// The size of the inside of the window, in physical pixels: what the
/// frame is drawn into, and what the UI's views
/// ([`RenderFrame::ui`](nadir_render::RenderFrame::ui)) cover. A resource
/// of development builds (feature `dev`).
///
/// [`App::run`](crate::App::run) sets it before each frame, and
/// [`Offscreen::frame`](crate::Offscreen::frame) to the size of its frames.
/// [`App::update`](crate::App::update) alone leaves it as it was: zero,
/// until a test sets it. With [`nadir_input::Input::pointer`] and the
/// virtual resolution, it maps the pointer to a UI's units
/// ([`nadir_render::UiSpace::new`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowSize(pub UVec2);
