//! Sleeping until an exact time, for the frame limiter.

use std::hint;
use std::thread;
use std::time::{Duration, Instant};

/// How long before the deadline the thread stops sleeping and starts to
/// spin. The operating system wakes a sleeping thread a little late, by an
/// amount that varies, so the last stretch is waited out awake.
const SPIN_MARGIN: Duration = Duration::from_millis(1);

/// Blocks the thread until `until`, and returns as soon after it as it can.
/// It never returns before.
///
/// It sleeps for all but the last millisecond and spins through that, which
/// keeps one processor core busy for the millisecond. If `until` is already
/// past, it returns at once.
pub fn precise_sleep(until: Instant) {
    let sleep = until
        .checked_duration_since(Instant::now())
        .and_then(|left| left.checked_sub(SPIN_MARGIN));
    if let Some(sleep) = sleep {
        thread::sleep(sleep);
    }
    while Instant::now() < until {
        hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_never_returns_before_the_deadline() {
        // Shorter than the spin margin, about equal to it, and longer.
        for micros in [0, 50, 300, 1000, 1500, 5000] {
            let until = Instant::now() + Duration::from_micros(micros);
            precise_sleep(until);
            assert!(Instant::now() >= until, "{micros} µs");
        }
    }

    #[test]
    fn a_deadline_in_the_past_returns_at_once() {
        let start = Instant::now();
        for _ in 0..1000 {
            precise_sleep(start);
        }
        // A thousand calls that had to wait would take far longer. The
        // bound is loose because a busy machine can pause any thread.
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn it_waits_out_a_deadline_that_is_many_sleeps_away() {
        // Deadlines one after another, as a frame limiter sets them, add
        // up: the total is at least the sum.
        let start = Instant::now();
        let mut until = start;
        for _ in 0..10 {
            until += Duration::from_millis(2);
            precise_sleep(until);
        }
        assert!(start.elapsed() >= Duration::from_millis(20));
    }
}
