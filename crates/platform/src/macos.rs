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

//! The macOS backend: an AppKit window, the user's folders, and alerts.

mod alert;
mod input;
mod keymap;
mod paths;
mod runtime;
mod window;

pub(crate) use alert::alert;
pub(crate) use paths::user_dirs;
pub(crate) use window::Backend;
