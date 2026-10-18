//! Time for the main loop: a clock that turns frames of any length into
//! simulation ticks of one fixed length, and a stopwatch to time the frames.
//!
//! ```
//! use std::time::Duration;
//!
//! use nadir_core::time::FixedClock;
//!
//! // 60 ticks per second, and no frame counts for more than a quarter second.
//! let mut clock = FixedClock::new(60, Duration::from_millis(250));
//!
//! // A frame of 40 ms holds two ticks of 16.67 ms, and 6.67 ms are left.
//! let frame = clock.begin_frame(Duration::from_millis(40));
//! let mut ticks = 0;
//! while let Some(_tick) = clock.consume_tick() {
//!     ticks += 1;
//! }
//! assert_eq!(ticks, 2);
//! assert!((frame.alpha - 0.4).abs() < 1e-6);
//! ```

use std::time::{Duration, Instant};

/// One step of the simulation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tick {
    /// The number of this tick: 0 for the first, and one more for each
    /// after it.
    pub index: u64,
    /// The length of a tick, in seconds. Every tick has the same.
    pub dt: f32,
}

/// One pass of the main loop: what was drawn, or would be.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// The time since the previous frame, in seconds. A hitch is clamped,
    /// so this is never more than the clock's longest frame.
    pub dt: f32,
    /// Where this frame lies between the last two ticks: 0 at the earlier
    /// one, and toward 1 as the later one comes due. Rendering blends the
    /// two states by this. It is from 0 to 1.
    pub alpha: f32,
}

/// What the accumulator of a [`FixedClock`] counts in one tick. The unit is
/// a nanosecond times the tick rate, which makes one tick this many for any
/// whole rate, with nothing to round.
const UNITS_PER_TICK: u64 = 1_000_000_000;

/// Turns the time that frames take into ticks of one fixed length.
///
/// Each frame adds its length to an accumulator, and each tick takes one
/// tick's length out of it. A frame may therefore hold no tick, or one, or
/// several, and over time the ticks keep exactly to their rate.
///
/// The clock does not read the time itself. The caller says how long each
/// frame took, so the same frame lengths give the same ticks on every
/// machine.
#[derive(Clone, Debug)]
pub struct FixedClock {
    tick_hz: u32,
    max_frame: Duration,
    /// Time that no tick has used yet, in nanoseconds times `tick_hz`.
    accumulator: u64,
    /// The index of the next tick.
    next_index: u64,
}

impl FixedClock {
    /// A clock of `tick_hz` ticks per second. A rate of 0 counts as 1.
    ///
    /// A frame longer than `max_frame` counts as that long: the hitch
    /// clamp. After a stall the simulation then falls behind real time,
    /// which is better than running a long burst of ticks to catch up and
    /// stalling again because of it.
    pub fn new(tick_hz: u32, max_frame: Duration) -> Self {
        Self {
            tick_hz: tick_hz.max(1),
            max_frame,
            accumulator: 0,
            next_index: 0,
        }
    }

    /// Starts a frame that comes `elapsed` after the previous one.
    ///
    /// Then call [`FixedClock::consume_tick`] until it returns `None`. The
    /// `alpha` of the frame returned is already the one that holds once
    /// those ticks have run.
    pub fn begin_frame(&mut self, elapsed: Duration) -> Frame {
        let elapsed = elapsed.min(self.max_frame);
        let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        let units = nanos.saturating_mul(u64::from(self.tick_hz));
        self.accumulator = self.accumulator.saturating_add(units);
        Frame {
            dt: elapsed.as_secs_f32(),
            alpha: self.alpha(),
        }
    }

    /// Returns the next tick that is due, or `None` when the simulation has
    /// caught up with the frame.
    pub fn consume_tick(&mut self) -> Option<Tick> {
        if self.accumulator < UNITS_PER_TICK {
            return None;
        }
        self.accumulator -= UNITS_PER_TICK;
        let tick = Tick {
            index: self.next_index,
            dt: self.tick_dt(),
        };
        self.next_index += 1;
        Some(tick)
    }

    /// Where the current frame lies between the last two ticks, from 0 to
    /// 1, once every tick that is due has run. See [`Frame::alpha`].
    pub fn alpha(&self) -> f32 {
        let remainder = self.accumulator % UNITS_PER_TICK;
        (remainder as f64 / UNITS_PER_TICK as f64) as f32
    }

    /// The length of a tick, in seconds.
    pub fn tick_dt(&self) -> f32 {
        1.0 / self.tick_hz as f32
    }

    /// How many ticks have been consumed so far.
    pub fn ticks(&self) -> u64 {
        self.next_index
    }
}

/// Measures real time.
#[derive(Clone, Copy, Debug)]
pub struct Stopwatch {
    start: Instant,
    lap: Instant,
}

impl Stopwatch {
    /// A stopwatch that starts now.
    pub fn start() -> Self {
        let now = Instant::now();
        Self {
            start: now,
            lap: now,
        }
    }

    /// The time since the stopwatch started.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// The time since the last call of this method, or since the start if
    /// this is the first. The laps add up to the elapsed time: no time
    /// falls between two of them.
    pub fn lap(&mut self) -> Duration {
        let now = Instant::now();
        let lap = now.duration_since(self.lap);
        self.lap = now;
        lap
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::check;

    const MAX_FRAME: Duration = Duration::from_millis(250);

    fn clock() -> FixedClock {
        FixedClock::new(60, MAX_FRAME)
    }

    /// Runs one frame and returns it with the ticks it held.
    fn frame(clock: &mut FixedClock, elapsed: Duration) -> (Frame, Vec<Tick>) {
        let frame = clock.begin_frame(elapsed);
        let mut ticks = Vec::new();
        while let Some(tick) = clock.consume_tick() {
            ticks.push(tick);
        }
        (frame, ticks)
    }

    /// Runs frames of one length until `total` has passed, and returns how
    /// many ticks they held.
    fn ticks_in(clock: &mut FixedClock, frame_length: Duration, total: Duration) -> u64 {
        let before = clock.ticks();
        let mut passed = Duration::ZERO;
        while passed < total {
            let step = frame_length.min(total - passed);
            frame(clock, step);
            passed += step;
        }
        clock.ticks() - before
    }

    #[test]
    fn a_new_clock_has_no_tick_due() {
        let mut clock = clock();
        assert_eq!(clock.consume_tick(), None);
        assert_eq!(clock.ticks(), 0);
        assert_eq!(clock.alpha(), 0.0);
        let (first, ticks) = frame(&mut clock, Duration::ZERO);
        assert_eq!(
            first,
            Frame {
                dt: 0.0,
                alpha: 0.0
            }
        );
        assert_eq!(ticks, []);
    }

    #[test]
    fn a_frame_holds_the_ticks_that_fit_in_it() {
        let mut clock = clock();
        // 10 ms: less than one tick of 16.67 ms.
        let (first, ticks) = frame(&mut clock, Duration::from_millis(10));
        assert_eq!(ticks, []);
        assert!((first.alpha - 0.6).abs() < 1e-6, "{first:?}");
        // 20 ms by now: one tick, and 3.33 ms left.
        let (second, ticks) = frame(&mut clock, Duration::from_millis(10));
        assert_eq!(ticks.len(), 1);
        assert!((second.alpha - 0.2).abs() < 1e-6, "{second:?}");
        // 70 ms by now: four ticks in all, so three more, and 3.33 ms left.
        let (third, ticks) = frame(&mut clock, Duration::from_millis(50));
        assert_eq!(ticks.len(), 3);
        assert!((third.alpha - 0.2).abs() < 1e-6, "{third:?}");
        assert!((third.dt - 0.05).abs() < 1e-9, "{third:?}");
    }

    #[test]
    fn ticks_are_numbered_from_zero_and_all_the_same_length() {
        let mut clock = clock();
        let (_, ticks) = frame(&mut clock, Duration::from_millis(100));
        let (_, more) = frame(&mut clock, Duration::from_millis(100));
        let indices: Vec<u64> = ticks.iter().chain(&more).map(|tick| tick.index).collect();
        assert_eq!(indices, (0..12).collect::<Vec<u64>>());
        assert!(ticks.iter().chain(&more).all(|tick| tick.dt == 1.0 / 60.0));
        assert_eq!(clock.tick_dt(), 1.0 / 60.0);
        assert_eq!(clock.ticks(), 12);
    }

    #[test]
    fn ten_seconds_hold_600_ticks_at_any_frame_rate() {
        let ten_seconds = Duration::from_secs(10);
        let frame_lengths = [
            Duration::from_nanos(16_666_667), // 60 Hz
            Duration::from_nanos(16_683_350), // 59.94 Hz
            Duration::from_nanos(6_944_444),  // 144 Hz
            Duration::from_nanos(4_166_667),  // 240 Hz
            Duration::from_micros(33_333),    // 30 Hz
            Duration::from_millis(1),
            Duration::from_millis(7),
            Duration::from_millis(100),
            Duration::from_millis(250),
        ];
        for frame_length in frame_lengths {
            let ticks = ticks_in(&mut clock(), frame_length, ten_seconds);
            assert_eq!(ticks, 600, "frames of {frame_length:?}");
        }
    }

    #[test]
    fn the_rate_is_exact_over_a_long_time() {
        // An hour of frames at 144 Hz: 3600 s * 60 = 216,000 ticks. A clock
        // that rounded the tick length to whole nanoseconds would drift.
        let mut clock = clock();
        let ticks = ticks_in(
            &mut clock,
            Duration::from_nanos(6_944_444),
            Duration::from_secs(3600),
        );
        assert_eq!(ticks, 216_000);
        assert_eq!(clock.alpha(), 0.0);
    }

    #[test]
    fn a_tick_is_not_rounded_to_whole_nanoseconds() {
        // At 60 Hz a tick is 16,666,666.67 ns. A frame of 16,666,666 ns is
        // short of one, and a clock that rounded the tick down to that
        // would run 60.0000024 times a second.
        let mut clock = clock();
        let (short, ticks) = frame(&mut clock, Duration::from_nanos(16_666_666));
        assert_eq!(ticks, []);
        assert!(short.alpha > 0.9999, "{short:?}");
        // One more nanosecond completes the tick.
        let (_, ticks) = frame(&mut clock, Duration::from_nanos(1));
        assert_eq!(ticks.len(), 1);

        // Three such frames, and then the two nanoseconds they lacked.
        let mut clock = FixedClock::new(60, MAX_FRAME);
        let (_, ticks) = frame(&mut clock, Duration::from_nanos(3 * 16_666_666));
        assert_eq!(ticks.len(), 2);
        let (_, ticks) = frame(&mut clock, Duration::from_nanos(2));
        assert_eq!(ticks.len(), 1);
        assert_eq!(clock.alpha(), 0.0);
    }

    #[test]
    fn other_rates_are_exact_too() {
        for (hz, expected) in [(1, 10), (30, 300), (50, 500), (120, 1200), (1000, 10_000)] {
            let mut clock = FixedClock::new(hz, MAX_FRAME);
            let ticks = ticks_in(
                &mut clock,
                Duration::from_millis(7),
                Duration::from_secs(10),
            );
            assert_eq!(ticks, expected, "{hz} Hz");
            assert_eq!(clock.tick_dt(), 1.0 / hz as f32);
        }
    }

    #[test]
    fn a_rate_of_zero_counts_as_one() {
        let mut clock = FixedClock::new(0, Duration::from_secs(10));
        assert_eq!(clock.tick_dt(), 1.0);
        let (_, ticks) = frame(&mut clock, Duration::from_millis(2500));
        assert_eq!(ticks.len(), 2);
    }

    #[test]
    fn a_hitch_is_clamped() {
        let mut clock = clock();
        // One second without a frame is 60 ticks of real time. The clock
        // counts a quarter second: 15 ticks.
        let (hitch, ticks) = frame(&mut clock, Duration::from_secs(1));
        assert_eq!(ticks.len(), 15);
        assert_eq!(hitch.dt, 0.25);
        // Exactly the longest frame is not clamped, and nothing is owed
        // afterwards.
        let (longest, ticks) = frame(&mut clock, MAX_FRAME);
        assert_eq!(ticks.len(), 15);
        assert_eq!(longest.dt, 0.25);
        let (normal, ticks) = frame(&mut clock, Duration::from_nanos(16_666_667));
        assert_eq!(ticks.len(), 1);
        assert!(normal.dt < 0.017);
    }

    #[test]
    fn even_an_endless_frame_ends() {
        let mut clock = clock();
        let (frame, ticks) = frame(&mut clock, Duration::MAX);
        assert_eq!(ticks.len(), 15);
        assert_eq!(frame.dt, 0.25);
    }

    #[test]
    fn any_frames_give_the_ticks_their_clamped_time_holds() {
        check(
            "ticks follow the clamped time",
            300,
            |rng| {
                let hz = rng.range(1..241u32);
                let frames: Vec<u64> = (0..rng.range(1..60u32))
                    .map(|_| match rng.range(0..10u32) {
                        // A hitch, now and then.
                        0 => u64::from(rng.range(250_000_000..2_000_000_000u32)),
                        _ => u64::from(rng.range(0..40_000_000u32)),
                    })
                    .collect();
                (hz, frames)
            },
            |(hz, frames)| {
                let mut clock = FixedClock::new(*hz, MAX_FRAME);
                let mut clamped_nanos: u128 = 0;
                let mut next_index = 0;
                for &nanos in frames {
                    let frame = clock.begin_frame(Duration::from_nanos(nanos));
                    clamped_nanos += u128::from(nanos.min(250_000_000));
                    while let Some(tick) = clock.consume_tick() {
                        if tick.index != next_index {
                            return Err(format!("tick {} came as {next_index}", tick.index));
                        }
                        next_index += 1;
                    }
                    // Exactly the whole ticks of the time so far.
                    let due = clamped_nanos * u128::from(*hz) / 1_000_000_000;
                    if u128::from(clock.ticks()) != due {
                        return Err(format!("{} ticks, but {due} are due", clock.ticks()));
                    }
                    if !(0.0..=1.0).contains(&frame.alpha) || frame.alpha != clock.alpha() {
                        return Err(format!("alpha {} and {}", frame.alpha, clock.alpha()));
                    }
                    if !(0.0..=0.25).contains(&frame.dt) {
                        return Err(format!("dt {}", frame.dt));
                    }
                }
                Ok(())
            },
        );
    }

    #[test]
    fn alpha_stays_within_one_just_below_a_tick() {
        // One nanosecond short of a tick at 1 Hz: the nearest `f32` is 1.
        let mut clock = FixedClock::new(1, Duration::from_secs(10));
        let frame = clock.begin_frame(Duration::from_nanos(999_999_999));
        assert_eq!(clock.consume_tick(), None);
        assert!((0.0..=1.0).contains(&frame.alpha), "{frame:?}");
        assert!(frame.alpha > 0.999, "{frame:?}");
    }

    #[test]
    fn laps_add_up_to_the_elapsed_time() {
        let mut stopwatch = Stopwatch::start();
        let mut laps = Duration::ZERO;
        for _ in 0..50 {
            std::hint::black_box((0..1000u64).sum::<u64>());
            laps += stopwatch.lap();
        }
        let elapsed = stopwatch.elapsed();
        // The laps ended at the last call of `lap`, shortly before
        // `elapsed` looked at the clock.
        assert!(laps <= elapsed, "{laps:?} and {elapsed:?}");
        assert!(
            elapsed - laps < Duration::from_secs(1),
            "{laps:?} and {elapsed:?}"
        );
        // A copy keeps its own lap.
        let mut copy = stopwatch;
        assert!(copy.lap() <= stopwatch.elapsed());
    }
}
