//! The frame limiter: when each frame may end.

use std::time::{Duration, Instant};

/// Paces frames to a rate, for when nothing else does. A display's vertical
/// sync would, but there is no rendering yet to wait for it.
///
/// The limiter keeps a schedule: each frame is due one interval after the
/// one before, not one interval after that one happened to end. A frame
/// that ends a little late therefore does not make the frames after it
/// late too, and the rate holds on average.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameLimiter {
    /// The time from one frame to the next, or `None` for no limit.
    interval: Option<Duration>,
    /// When the current frame is due to end.
    due: Instant,
}

impl FrameLimiter {
    /// A limiter of `frames_per_second`, whose first frame starts at `now`.
    /// `None` and `Some(0)` mean no limit.
    pub(crate) fn new(frames_per_second: Option<u32>, now: Instant) -> Self {
        let interval = frames_per_second
            .filter(|&rate| rate > 0)
            .map(|rate| Duration::from_secs(1) / rate);
        Self { interval, due: now }
    }

    /// Call this at the end of each frame, with the time then. Returns when
    /// the frame is due to end, if that is still ahead: sleep until then.
    ///
    /// If the frame overran, there is nothing to wait for, and the schedule
    /// starts again from `now`. Frames do not hurry afterwards to make up
    /// the time that was lost.
    pub(crate) fn end_of_frame(&mut self, now: Instant) -> Option<Instant> {
        let interval = self.interval?;
        self.due += interval;
        if self.due > now {
            Some(self.due)
        } else {
            self.due = now;
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// 60 frames per second in whole nanoseconds.
    const INTERVAL: Duration = Duration::from_nanos(16_666_666);

    #[test]
    fn a_frame_that_ends_early_waits_for_its_slot() {
        let start = Instant::now();
        let mut limiter = FrameLimiter::new(Some(60), start);
        assert_eq!(limiter.end_of_frame(start + 2 * MS), Some(start + INTERVAL));
        // The next slot follows the first one, wherever in its own slot
        // the second frame ended.
        let now = start + INTERVAL + 9 * MS;
        assert_eq!(limiter.end_of_frame(now), Some(start + 2 * INTERVAL));
        let now = start + 2 * INTERVAL;
        assert_eq!(limiter.end_of_frame(now), Some(start + 3 * INTERVAL));
    }

    #[test]
    fn sixty_frames_take_one_second() {
        let start = Instant::now();
        let mut limiter = FrameLimiter::new(Some(60), start);
        let mut now = start;
        for _ in 0..60 {
            // Each frame does a little work, then sleeps until it is due.
            now += 3 * MS;
            now = limiter.end_of_frame(now).unwrap();
        }
        let taken = now - start;
        let error = Duration::from_secs(1).abs_diff(taken);
        assert!(error < Duration::from_micros(1), "{taken:?}");
    }

    #[test]
    fn a_frame_that_overran_does_not_wait_and_is_not_made_up_for() {
        let start = Instant::now();
        let mut limiter = FrameLimiter::new(Some(60), start);
        // The frame took 50 ms: three slots.
        let late = start + 50 * MS;
        assert_eq!(limiter.end_of_frame(late), None);
        // The schedule starts again from there: the next frame gets a
        // whole slot, not the 0.33 ms left of the old one.
        assert_eq!(limiter.end_of_frame(late + 2 * MS), Some(late + INTERVAL));
    }

    #[test]
    fn a_frame_that_ends_exactly_when_due_does_not_wait() {
        let start = Instant::now();
        let mut limiter = FrameLimiter::new(Some(60), start);
        assert_eq!(limiter.end_of_frame(start + INTERVAL), None);
        assert_eq!(
            limiter.end_of_frame(start + INTERVAL + MS),
            Some(start + 2 * INTERVAL)
        );
    }

    #[test]
    fn other_rates_have_other_intervals() {
        let start = Instant::now();
        for (rate, interval) in [
            (1, 1000 * MS),
            (30, Duration::from_nanos(33_333_333)),
            (1000, MS),
        ] {
            let mut limiter = FrameLimiter::new(Some(rate), start);
            assert_eq!(
                limiter.end_of_frame(start),
                Some(start + interval),
                "{rate}"
            );
        }
    }

    #[test]
    fn without_a_limit_no_frame_waits() {
        let start = Instant::now();
        for limit in [None, Some(0)] {
            let mut limiter = FrameLimiter::new(limit, start);
            assert_eq!(limiter.end_of_frame(start), None);
            assert_eq!(limiter.end_of_frame(start + MS), None);
        }
    }
}
