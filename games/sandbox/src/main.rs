//! The sandbox game. It drives the Nadir engine's requirements.
//!
//! So far it opens a window and logs, once a second, how fast the simulation
//! ticks. Close the window to quit.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use nadir::log::info;
use nadir::prelude::*;

fn main() -> Result<(), nadir::Error> {
    let ticks = Rc::new(Cell::new(0u64));
    let counter = Rc::clone(&ticks);
    let stopwatch = Stopwatch::start();
    let mut rate = TickRate::default();

    App::new(Config {
        title: "Sandbox".into(),
        game: "Sandbox".into(),
        ..Config::default()
    })
    .add_fixed_system("count_ticks", move |_tick| counter.set(counter.get() + 1))
    .add_frame_system("report_tick_rate", move |_frame| {
        if let Some(report) = rate.sample(stopwatch.elapsed(), ticks.get()) {
            info!(
                "tick rate: {:.2} Hz over the last second, {:.2} Hz since the start \
                 ({} ticks in {:.1} s)",
                report.recent_hz,
                report.average_hz,
                report.ticks,
                report.elapsed.as_secs_f64()
            );
        }
    })
    .run()
}

/// How often the tick rate is reported.
const REPORT_INTERVAL: Duration = Duration::from_secs(1);

/// Measures the tick rate against real time.
#[derive(Debug, Default)]
struct TickRate {
    /// When the last report was made, since the start.
    reported_at: Duration,
    /// How many ticks had run by then.
    reported_ticks: u64,
}

/// One measurement of the tick rate.
#[derive(Debug, PartialEq)]
struct Report {
    /// The rate since the report before this one.
    recent_hz: f64,
    /// The rate since the start.
    average_hz: f64,
    /// All the ticks so far.
    ticks: u64,
    /// The time since the start.
    elapsed: Duration,
}

impl TickRate {
    /// Takes the time since the start and the ticks run so far. Returns a
    /// report if [`REPORT_INTERVAL`] has passed since the last one.
    fn sample(&mut self, elapsed: Duration, ticks: u64) -> Option<Report> {
        let since_report = elapsed.checked_sub(self.reported_at)?;
        if since_report < REPORT_INTERVAL {
            return None;
        }
        let recent_ticks = ticks.saturating_sub(self.reported_ticks);
        let report = Report {
            recent_hz: recent_ticks as f64 / since_report.as_secs_f64(),
            average_hz: ticks as f64 / elapsed.as_secs_f64(),
            ticks,
            elapsed,
        };
        self.reported_at = elapsed;
        self.reported_ticks = ticks;
        Some(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn nothing_is_reported_within_the_first_second() {
        let mut rate = TickRate::default();
        assert_eq!(rate.sample(Duration::ZERO, 0), None);
        assert_eq!(rate.sample(500 * MS, 30), None);
        assert_eq!(rate.sample(999 * MS, 59), None);
    }

    #[test]
    fn a_report_comes_once_a_second_with_both_rates() {
        let mut rate = TickRate::default();
        let first = rate.sample(1000 * MS, 60);
        let expected = Report {
            recent_hz: 60.0,
            average_hz: 60.0,
            ticks: 60,
            elapsed: 1000 * MS,
        };
        assert_eq!(first, Some(expected));
        // Not again until a second after that report.
        assert_eq!(rate.sample(1016 * MS, 61), None);
        assert_eq!(rate.sample(1999 * MS, 119), None);

        // A slow second: 30 ticks. The average covers both seconds.
        let second = rate.sample(2000 * MS, 90);
        let expected = Report {
            recent_hz: 30.0,
            average_hz: 45.0,
            ticks: 90,
            elapsed: 2000 * MS,
        };
        assert_eq!(second, Some(expected));
    }

    #[test]
    fn a_late_report_measures_the_time_that_really_passed() {
        let mut rate = TickRate::default();
        // The frame that reports came 250 ms late, and ticks kept running.
        let report = rate.sample(1250 * MS, 75);
        assert_eq!(report.map(|report| report.recent_hz), Some(60.0));
        // The next second is counted from that report.
        assert_eq!(rate.sample(2249 * MS, 134), None);
        let report = rate.sample(2250 * MS, 135);
        assert_eq!(report.map(|report| report.recent_hz), Some(60.0));
    }
}
