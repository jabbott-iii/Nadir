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

//! The stand-in for gamepads on a system that has none yet: Windows until
//! roadmap task M8.2, macOS until M8.3.

use crate::{Event, GamepadId, GamepadState};

/// No gamepads: none is ever reported.
pub(crate) struct Gamepads;

impl Gamepads {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn poll(&mut self, _out: &mut Vec<Event>) {}

    pub(crate) fn state(&self, _id: GamepadId) -> Option<&GamepadState> {
        None
    }
}
