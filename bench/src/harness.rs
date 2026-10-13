//! Timing: warm the work up, pick an iteration count, take samples.
//!
//! A benchmark does its setup and then hands the work to
//! [`Bencher::iter`]:
//!
//! ```ignore
//! fn sort(bencher: &mut Bencher) {
//!     let numbers = make_numbers();
//!     bencher.iter(|| {
//!         let mut copy = numbers.clone();
//!         copy.sort();
//!         copy
//!     });
//! }
//! ```
//!
//! Whatever the work returns is passed through [`black_box`], so the
//! compiler cannot drop the work as unused. Inputs that the compiler could
//! see through, such as constants, should go through `black_box` as well.

use std::hint::black_box;
use std::time::{Duration, Instant};

/// How a benchmark is timed. The defaults are those of
/// `docs/04-quality.md`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Config {
    /// How long the work runs before anything is recorded.
    pub(crate) warm_up: Duration,
    /// How many samples are recorded.
    pub(crate) samples: usize,
    /// How long one sample should take. The iteration count is scaled to
    /// come close to this.
    pub(crate) sample_time: Duration,
}

impl Default for Config {
    /// Warm up for 0.5 s, then 50 samples of about 10 ms each.
    fn default() -> Self {
        Self {
            warm_up: Duration::from_millis(500),
            samples: 50,
            sample_time: Duration::from_millis(10),
        }
    }
}

#[cfg(test)]
impl Config {
    /// A configuration for tests: a benchmark takes a few milliseconds.
    pub(crate) fn fast() -> Self {
        Self {
            warm_up: Duration::from_millis(2),
            samples: 7,
            sample_time: Duration::from_micros(200),
        }
    }
}

/// What timing one benchmark produced.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Measurement {
    /// How many times the work ran in each sample.
    pub(crate) iterations: u64,
    /// The time one iteration took in each sample, in nanoseconds: the
    /// sample's time divided by `iterations`.
    pub(crate) samples_ns: Vec<f64>,
}

impl Measurement {
    /// The median time of one iteration, in nanoseconds.
    pub(crate) fn median_ns(&self) -> f64 {
        median(&sorted(&self.samples_ns))
    }

    /// The 95th percentile of the time of one iteration, in nanoseconds.
    pub(crate) fn p95_ns(&self) -> f64 {
        percentile(&sorted(&self.samples_ns), 95)
    }
}

/// Handed to a benchmark so that it can time its work.
pub(crate) struct Bencher {
    config: Config,
    measurement: Option<Measurement>,
}

impl Bencher {
    /// Creates a bencher that times as `config` says.
    pub(crate) fn new(config: Config) -> Self {
        Self {
            config,
            measurement: None,
        }
    }

    /// Times `work`. A benchmark calls this once, after its setup.
    pub(crate) fn iter<T>(&mut self, mut work: impl FnMut() -> T) {
        let iterations = self.warm_up(&mut work);
        let mut samples_ns = Vec::with_capacity(self.config.samples);
        for _ in 0..self.config.samples.max(1) {
            let start = Instant::now();
            for _ in 0..iterations {
                black_box(work());
            }
            samples_ns.push(start.elapsed().as_secs_f64() * 1e9 / iterations as f64);
        }
        self.measurement = Some(Measurement {
            iterations,
            samples_ns,
        });
    }

    /// The measurement, if the benchmark called [`Bencher::iter`].
    pub(crate) fn finish(self) -> Option<Measurement> {
        self.measurement
    }

    /// Runs `work` for the warm-up time, in batches that double, and
    /// returns how many iterations fit in one sample.
    fn warm_up<T>(&self, work: &mut impl FnMut() -> T) -> u64 {
        let start = Instant::now();
        let mut done = 0_u64;
        let mut batch = 1_u64;
        let elapsed = loop {
            for _ in 0..batch {
                black_box(work());
            }
            done = done.saturating_add(batch);
            let elapsed = start.elapsed();
            if elapsed >= self.config.warm_up {
                break elapsed;
            }
            batch = batch.saturating_mul(2);
        };
        iterations_per_sample(elapsed, done, self.config.sample_time)
    }
}

/// How many iterations make a sample of about `sample_time`, given that
/// `done` iterations took `elapsed`. At least 1.
fn iterations_per_sample(elapsed: Duration, done: u64, sample_time: Duration) -> u64 {
    let one = elapsed.as_secs_f64() / done.max(1) as f64;
    if one <= 0.0 {
        // The clock did not move. Any count is a guess; keep it small.
        return 1;
    }
    let count = (sample_time.as_secs_f64() / one).round();
    // The cast saturates, and a count this large never comes up: `one` is
    // at least a fraction of a nanosecond.
    (count as u64).max(1)
}

/// A sorted copy of `values`.
fn sorted(values: &[f64]) -> Vec<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted
}

/// The median of `sorted`: the middle value, or the mean of the two middle
/// values. 0 if there are none.
pub(crate) fn median(sorted: &[f64]) -> f64 {
    let middle = sorted.len() / 2;
    match (sorted.get(middle), sorted.len() % 2) {
        (Some(&upper), 1) => upper,
        (Some(&upper), _) => {
            let lower = sorted.get(middle - 1).copied().unwrap_or(upper);
            (lower + upper) / 2.0
        }
        (None, _) => 0.0,
    }
}

/// The `percent`th percentile of `sorted` by the nearest-rank method: the
/// smallest value that at least `percent` in 100 of the values do not
/// exceed. 0 if there are none.
pub(crate) fn percentile(sorted: &[f64], percent: usize) -> f64 {
    let rank = (sorted.len() * percent).div_ceil(100).max(1);
    sorted
        .get(rank - 1)
        .or(sorted.last())
        .copied()
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn ascending(count: usize) -> Vec<f64> {
        (1..=count).map(|value| value as f64).collect()
    }

    #[test]
    fn the_defaults_are_the_documented_ones() {
        let config = Config::default();
        assert_eq!(config.warm_up, Duration::from_millis(500));
        assert_eq!(config.samples, 50);
    }

    #[test]
    fn median_of_odd_and_even_counts() {
        assert_eq!(median(&[]), 0.0);
        assert_eq!(median(&[7.0]), 7.0);
        assert_eq!(median(&[1.0, 3.0]), 2.0);
        assert_eq!(median(&[1.0, 2.0, 10.0]), 2.0);
        assert_eq!(median(&[1.0, 2.0, 4.0, 10.0]), 3.0);
        assert_eq!(median(&ascending(50)), 25.5);
        assert_eq!(median(&ascending(51)), 26.0);
    }

    #[test]
    fn percentile_by_nearest_rank() {
        assert_eq!(percentile(&[], 95), 0.0);
        assert_eq!(percentile(&[7.0], 95), 7.0);
        // 95% of 50 is 47.5, so the 48th value.
        assert_eq!(percentile(&ascending(50), 95), 48.0);
        // 95% of 20 is exactly 19.
        assert_eq!(percentile(&ascending(20), 95), 19.0);
        assert_eq!(percentile(&ascending(21), 95), 20.0);
        assert_eq!(percentile(&ascending(100), 95), 95.0);
        assert_eq!(percentile(&ascending(10), 50), 5.0);
        assert_eq!(percentile(&ascending(10), 100), 10.0);
        assert_eq!(percentile(&ascending(10), 0), 1.0);
        assert_eq!(percentile(&ascending(3), 200), 3.0);
    }

    #[test]
    fn a_measurement_reports_the_median_and_p95_of_unsorted_samples() {
        let mut samples_ns = ascending(50);
        samples_ns.reverse();
        samples_ns.swap(3, 30);
        let measurement = Measurement {
            iterations: 4,
            samples_ns,
        };
        assert_eq!(measurement.median_ns(), 25.5);
        assert_eq!(measurement.p95_ns(), 48.0);
    }

    #[test]
    fn the_iteration_count_fills_one_sample() {
        let ms = Duration::from_millis;
        // 1000 iterations in 500 ms is 0.5 ms each: 20 fit in 10 ms.
        assert_eq!(iterations_per_sample(ms(500), 1000, ms(10)), 20);
        // 4 ns each.
        assert_eq!(
            iterations_per_sample(ms(500), 125_000_000, ms(10)),
            2_500_000
        );
        // Rounded to the nearest count.
        assert_eq!(iterations_per_sample(ms(600), 100, ms(10)), 2);
        assert_eq!(iterations_per_sample(ms(700), 100, ms(10)), 1);
        // Work that takes longer than a sample still runs once.
        assert_eq!(iterations_per_sample(ms(900), 3, ms(10)), 1);
        // Degenerate measurements do not divide by zero.
        assert_eq!(iterations_per_sample(Duration::ZERO, 1000, ms(10)), 1);
        assert_eq!(iterations_per_sample(ms(500), 0, ms(10)), 1);
        assert_eq!(iterations_per_sample(ms(500), 1000, Duration::ZERO), 1);
    }

    #[test]
    fn iter_takes_the_configured_number_of_samples() {
        let calls = Cell::new(0_u64);
        let mut bencher = Bencher::new(Config::fast());
        bencher.iter(|| calls.set(calls.get() + 1));
        let measurement = bencher.finish().unwrap();
        assert_eq!(measurement.samples_ns.len(), 7);
        assert!(measurement.iterations >= 1);
        assert!(measurement.samples_ns.iter().all(|&ns| ns > 0.0));
        assert!(measurement.median_ns() <= measurement.p95_ns());
        // Every sample ran the work `iterations` times, after the warm-up.
        assert!(calls.get() > 7 * measurement.iterations);
    }

    #[test]
    fn the_warm_up_lasts_at_least_the_configured_time() {
        let config = Config {
            warm_up: Duration::from_millis(40),
            samples: 1,
            sample_time: Duration::from_micros(10),
        };
        let start = Instant::now();
        let mut bencher = Bencher::new(config);
        bencher.iter(|| black_box(1_u64) + 1);
        assert!(start.elapsed() >= Duration::from_millis(40));
    }

    #[test]
    fn slow_work_runs_once_per_sample() {
        let config = Config {
            warm_up: Duration::from_millis(1),
            samples: 3,
            sample_time: Duration::from_millis(1),
        };
        let calls = Cell::new(0_u64);
        let mut bencher = Bencher::new(config);
        bencher.iter(|| {
            calls.set(calls.get() + 1);
            std::thread::sleep(Duration::from_millis(5));
        });
        let measurement = bencher.finish().unwrap();
        assert_eq!(measurement.iterations, 1);
        // One warm-up call, then one call for each sample.
        assert_eq!(calls.get(), 4);
        // Each sample is one sleep of at least 5 ms.
        assert!(measurement.samples_ns.iter().all(|&ns| ns >= 5e6));
        assert!(measurement.median_ns() < 5e9);
    }

    #[test]
    fn faster_work_gets_more_iterations() {
        let config = Config {
            warm_up: Duration::from_millis(20),
            samples: 2,
            sample_time: Duration::from_millis(20),
        };
        let mut slow = Bencher::new(config);
        slow.iter(|| std::thread::sleep(Duration::from_millis(4)));
        let mut quick = Bencher::new(config);
        quick.iter(|| black_box(1_u64) + 1);
        let (slow, quick) = (slow.finish().unwrap(), quick.finish().unwrap());
        // At most 5 sleeps of 4 ms fit in a 20 ms sample; additions fit by
        // the thousand.
        assert!(slow.iterations <= 5, "{}", slow.iterations);
        assert!(quick.iterations > 1000, "{}", quick.iterations);
        // The times are for one iteration, not for a whole sample.
        assert!(slow.median_ns() >= 4e6, "{}", slow.median_ns());
        assert!(quick.median_ns() < 1e5, "{}", quick.median_ns());
        assert!(quick.median_ns() < slow.median_ns());
    }

    #[test]
    fn a_configuration_with_no_samples_still_takes_one() {
        let mut bencher = Bencher::new(Config {
            samples: 0,
            ..Config::fast()
        });
        bencher.iter(|| 1 + 1);
        assert_eq!(bencher.finish().unwrap().samples_ns.len(), 1);
    }

    #[test]
    fn a_benchmark_that_never_times_anything_has_no_measurement() {
        assert_eq!(Bencher::new(Config::fast()).finish(), None);
    }
}
