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

//! The time controls (roadmap M7.6, ADR-0064): F5 pauses the game's time
//! and lets it run again, F6 steps it a tick, F7 and Shift+F7 step its
//! scale; the console's `timescale` sets the scale too. They change the time
//! the game's clock is given, so ticks, animated tiles and clips pause,
//! step and scale together, and a step goes through `App::update`, which
//! takes a replay's next tick.

use std::fmt;
use std::time::Duration;

use nadir_core::Key;
use nadir_core::log::{debug, info};

use crate::TICK_HZ;

/// Pauses the game's time, and lets it run again.
pub(crate) const PAUSE: Key = Key::F5;

/// Steps it a tick, pausing it first if it runs; held, its repeats step on.
pub(crate) const STEP: Key = Key::F6;

/// Steps its scale up through [`SCALES`], and with Shift down.
pub(crate) const SCALE: Key = Key::F7;

/// The scales F7 steps through, wrapping at either end.
pub(crate) const SCALES: [f32; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

/// A tick's length, rounded up to a whole nanosecond: a step's frame. It
/// runs one tick, keeping where the frame lies between two (`alpha`); the
/// third of a nanosecond it holds past a tick adds up to another tick only
/// after 50 million steps, as `App::update` says of such frames.
pub(crate) const TICK: Duration = Duration::from_nanos(1_000_000_000_u64.div_ceil(TICK_HZ as u64));

/// The game's time, as the controls leave it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Time {
    /// The game's time as a share of real time, while it runs.
    pub(crate) scale: f32,
    paused: bool,
    /// The steps asked for since the last frame's clock.
    steps: u32,
    /// Whether each Shift key is down.
    shift: [bool; 2],
}

impl Time {
    /// Running, at 1.
    pub(crate) fn new() -> Self {
        Self {
            scale: 1.0,
            paused: false,
            steps: 0,
            shift: [false; 2],
        }
    }

    /// Whether it runs as real time does: not paused, at 1.
    pub(crate) fn is_real(&self) -> bool {
        !self.paused && self.scale == 1.0
    }

    /// Takes in a key's press, release or repeat, as the window's events
    /// come, before the frame's clock: the controls act in the frame they
    /// were pressed in.
    pub(crate) fn key(&mut self, key: Key, pressed: bool, repeat: bool) {
        match key {
            Key::LeftShift => self.shift[0] = pressed,
            Key::RightShift => self.shift[1] = pressed,
            PAUSE if pressed && !repeat => {
                self.paused = !self.paused;
                self.steps = 0;
                if self.paused {
                    info!("the game's time is paused: F5 runs it again, F6 steps it a tick");
                } else {
                    info!(
                        "the game's time runs again, at {} times real time",
                        self.scale
                    );
                }
            }
            STEP if pressed => {
                if !self.paused {
                    self.paused = true;
                    info!("the game's time is paused: F5 runs it again, F6 steps it a tick");
                }
                self.steps = self.steps.saturating_add(1);
                debug!("the game's time steps a tick");
            }
            SCALE if pressed && !repeat => {
                let down = self.shift[0] || self.shift[1];
                self.scale = next_scale(self.scale, down);
                info!("{}", self.scale_note());
            }
            _ => {}
        }
    }

    /// What the game's time does at its scale, for the log and the
    /// console: `the game's time runs at 2 times real time`, or, paused,
    /// that it will once F5 runs it again.
    pub(crate) fn scale_note(&self) -> ScaleNote {
        ScaleNote {
            paused: self.paused,
            scale: self.scale,
        }
    }

    /// The window lost the focus: the Shift keys are up.
    pub(crate) fn focus_lost(&mut self) {
        self.shift = [false; 2];
    }

    /// The time the game's clock is given for a frame `elapsed` long: none
    /// while paused, but a tick for each step asked for since the last
    /// frame; otherwise `elapsed` at the scale.
    pub(crate) fn game_time(&mut self, elapsed: Duration) -> Duration {
        if self.paused {
            let steps = std::mem::take(&mut self.steps);
            return TICK.saturating_mul(steps);
        }
        if self.scale == 1.0 {
            return elapsed;
        }
        let scaled = elapsed.as_secs_f64() * f64::from(self.scale);
        Duration::try_from_secs_f64(scaled).unwrap_or(Duration::MAX)
    }
}

/// What [`Time::scale_note`] says.
pub(crate) struct ScaleNote {
    paused: bool,
    scale: f32,
}

impl fmt::Display for ScaleNote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.paused {
            write!(
                f,
                "the game's time will run at {} times real time once F5 runs it again",
                self.scale
            )
        } else {
            write!(f, "the game's time runs at {} times real time", self.scale)
        }
    }
}

/// What the corner label says: `paused`, `paused, x0.5`, `time x2`; nothing
/// while the time is real.
impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.paused, self.scale == 1.0) {
            (true, true) => f.write_str("paused"),
            (true, false) => write!(f, "paused, x{}", self.scale),
            (false, false) => write!(f, "time x{}", self.scale),
            (false, true) => Ok(()),
        }
    }
}

/// The scale after `scale` among [`SCALES`], or before it if `down`,
/// wrapping at either end. A scale between two (the console's 0.3) goes
/// to the next one past it.
fn next_scale(scale: f32, down: bool) -> f32 {
    let first = SCALES[0];
    let last = SCALES[SCALES.len() - 1];
    if down {
        SCALES
            .iter()
            .rev()
            .copied()
            .find(|&candidate| candidate < scale)
            .unwrap_or(last)
    } else {
        SCALES
            .iter()
            .copied()
            .find(|&candidate| candidate > scale)
            .unwrap_or(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(time: &mut Time, key: Key) {
        time.key(key, true, false);
        time.key(key, false, false);
    }

    #[test]
    fn a_tick_s_length_runs_one_tick() {
        assert_eq!(TICK, Duration::from_nanos(16_666_667));
        let mut clock = nadir_core::time::FixedClock::new(TICK_HZ, Duration::from_millis(250));
        // From any place between two ticks but the last 20 billionths of
        // one, it runs one tick, and keeps the place.
        for before in [0, 1, 5_000_000, 16_666_000] {
            clock.begin_frame(Duration::from_nanos(before));
            while clock.consume_tick().is_some() {}
            let ticks = clock.ticks();
            let alpha = clock.alpha();
            clock.begin_frame(TICK);
            while clock.consume_tick().is_some() {}
            assert_eq!(clock.ticks(), ticks + 1, "{before}");
            assert!((clock.alpha() - alpha).abs() < 1e-6, "{before}");
        }
    }

    #[test]
    fn f5_pauses_and_runs_again_and_f6_steps_a_tick_each() {
        let mut time = Time::new();
        let frame = Duration::from_millis(16);
        assert!(time.is_real());
        assert_eq!(time.game_time(frame), frame);
        press(&mut time, PAUSE);
        assert!(time.paused && !time.is_real());
        assert_eq!(time.game_time(frame), Duration::ZERO);
        // Two steps in a frame are two ticks; a repeat steps too.
        press(&mut time, STEP);
        time.key(STEP, true, true);
        assert_eq!(time.game_time(frame), TICK * 2);
        assert_eq!(time.game_time(frame), Duration::ZERO);
        // A repeat of F5 does nothing; a press runs the time again, and
        // drops the steps not taken.
        press(&mut time, STEP);
        time.key(PAUSE, true, true);
        assert!(time.paused);
        press(&mut time, PAUSE);
        assert!(!time.paused);
        assert_eq!(time.game_time(frame), frame);
        // F6 while the time runs pauses it, and steps.
        press(&mut time, STEP);
        assert!(time.paused);
        assert_eq!(time.game_time(frame), TICK);
    }

    #[test]
    fn f7_steps_the_scale_up_and_shift_f7_down_wrapping_at_the_ends() {
        let mut time = Time::new();
        let mut seen = Vec::new();
        for _ in 0..5 {
            press(&mut time, SCALE);
            seen.push(time.scale);
        }
        assert_eq!(seen, [2.0, 4.0, 0.25, 0.5, 1.0]);
        time.key(Key::RightShift, true, false);
        press(&mut time, SCALE);
        assert_eq!(time.scale, 0.5);
        // A repeat steps nothing.
        time.key(SCALE, true, true);
        assert_eq!(time.scale, 0.5);
        time.focus_lost();
        press(&mut time, SCALE);
        assert_eq!(time.scale, 1.0, "Shift let go with the focus");
        // From a scale between two, the next one past it.
        assert_eq!(next_scale(0.3, false), 0.5);
        assert_eq!(next_scale(0.3, true), 0.25);
        assert_eq!(next_scale(0.2, true), 4.0);
        assert_eq!(next_scale(9.0, false), 0.25);
        // The scale scales a frame's length; paused, it scales no step.
        time.scale = 0.5;
        assert_eq!(
            time.game_time(Duration::from_millis(20)),
            Duration::from_millis(10)
        );
        press(&mut time, STEP);
        assert_eq!(time.game_time(Duration::from_millis(20)), TICK);
        time.scale = 4.0;
        press(&mut time, PAUSE);
        assert_eq!(time.game_time(Duration::MAX), Duration::MAX);
    }

    #[test]
    fn a_scale_s_note_says_when_the_time_will_run_at_it() {
        let mut time = Time::new();
        time.scale = 2.0;
        assert_eq!(
            time.scale_note().to_string(),
            "the game's time runs at 2 times real time"
        );
        press(&mut time, PAUSE);
        assert_eq!(
            time.scale_note().to_string(),
            "the game's time will run at 2 times real time once F5 runs it again"
        );
    }

    #[test]
    fn the_label_says_what_is_not_real() {
        let mut time = Time::new();
        assert_eq!(time.to_string(), "");
        time.scale = 0.25;
        assert_eq!(time.to_string(), "time x0.25");
        press(&mut time, PAUSE);
        assert_eq!(time.to_string(), "paused, x0.25");
        time.scale = 1.0;
        assert_eq!(time.to_string(), "paused");
    }
}
