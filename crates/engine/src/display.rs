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

//! What a game asks of its window while it runs, for its menus:
//! [`DisplaySettings`], which [`App::run`](crate::App::run) applies, and
//! [`Quit`], which ends it.

/// How the window shows the game: a resource that a settings menu changes,
/// and [`App::run`](crate::App::run) applies after the frame that changed
/// it.
///
/// [`App::new`](crate::App::new) sets it from [`Config::fullscreen`] and
/// [`Config::vsync`], which the window opens with.
/// [`App::update`](crate::App::update) and
/// [`App::offscreen`](crate::App::offscreen), which have no window, leave
/// it as it is.
///
/// [`Config::fullscreen`]: crate::Config::fullscreen
/// [`Config::vsync`]: crate::Config::vsync
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplaySettings {
    /// Whether the window fills the screen, in borderless fullscreen.
    pub fullscreen: bool,
    /// Whether each frame waits for the display's refresh: a swap interval
    /// of 1. A driver may refuse a change, as llvmpipe refuses 0 (it offers
    /// 1 alone): then the log says so, and [`App::run`](crate::App::run)
    /// puts the value that holds back here. Windows and macOS have no
    /// OpenGL context yet, and refuse every change (roadmap M2.6 and M2.7).
    pub vsync: bool,
}

/// Whether the game asked to end: a resource. A system calls
/// [`Quit::request`], from a main menu's Quit say, and
/// [`App::run`](crate::App::run) ends after that frame, as if the window
/// were closed. [`App::update`](crate::App::update) and
/// [`App::offscreen`](crate::App::offscreen) do not end: a test or a
/// recording that runs them reads [`Quit::requested`] itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Quit {
    requested: bool,
}

impl Quit {
    /// Asks the game to end after this frame. A request cannot be taken
    /// back.
    pub fn request(&mut self) {
        self.requested = true;
    }

    /// Whether the game was asked to end.
    pub fn requested(&self) -> bool {
        self.requested
    }
}
