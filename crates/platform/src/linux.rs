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

//! The Linux backend: X11 through Xlib, which is loaded when the window is
//! opened. A Wayland session runs it through XWayland.

pub(crate) mod alsa;
mod egl;
mod keymap;
mod library;
mod x11;
mod xdg;
mod xresources;

pub(crate) use egl::Headless;
pub(crate) use x11::Backend;
pub(crate) use xdg::user_dirs;
