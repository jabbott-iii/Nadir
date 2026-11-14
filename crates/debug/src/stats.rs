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

//! The stats overlay: the numbers of the last frame, as plain values the
//! engine copies in (`debug-tools.md`, "Tools").

use std::time::Duration;

use nadir_core::Color;

use crate::Ui;

/// The color of a count that should stay 0, once it is not.
const WARNING: Color = Color::rgb(255, 176, 64);

/// What the stats overlay shows: plain numbers, which the engine copies in
/// each frame from its own resources.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    /// Frames a second, averaged over the last frames.
    pub frame_rate: f32,
    /// How long the processor worked on the last frame.
    pub frame_time: Duration,
    /// How long the frame's ticks took, together.
    pub tick_time: Duration,
    /// How many ticks the frame ran.
    pub ticks: u32,
    /// The draw calls of the last frame drawn.
    pub draw_calls: u32,
    /// The world's sprites it drew.
    pub sprites: u32,
    /// The entities alive.
    pub entities: usize,
    /// The heap allocations of the last frame, on the main thread.
    pub allocations: u64,
    /// The bytes those allocations asked for.
    pub allocated_bytes: u64,
    /// The voices sounding.
    pub voices: u32,
    /// The audio device's underruns so far.
    pub underruns: u64,
}

impl Stats {
    /// Fills a window with the numbers, a line each; underruns, once there
    /// are some, in amber.
    pub fn show(&self, ui: &mut Ui<'_>) {
        ui.label(format_args!("{:.0} fps", self.frame_rate));
        ui.label(format_args!("frame {:.2} ms CPU", millis(self.frame_time)));
        ui.label(format_args!(
            "ticks {} in {:.2} ms",
            self.ticks,
            millis(self.tick_time)
        ));
        ui.label(format_args!("draw calls {}", self.draw_calls));
        ui.label(format_args!("sprites {}", self.sprites));
        ui.label(format_args!("entities {}", self.entities));
        ui.label(format_args!(
            "allocs {} ({} bytes)",
            self.allocations, self.allocated_bytes
        ));
        ui.label(format_args!("voices {}", self.voices));
        if self.underruns > 0 {
            ui.label_colored(WARNING, format_args!("underruns {}", self.underruns));
        } else {
            ui.label(format_args!("underruns {}", self.underruns));
        }
    }
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}
