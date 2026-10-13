//! Results: the report file, the comparison with the baseline, and the
//! line printed for each benchmark.
//!
//! The report is JSON:
//!
//! ```json
//! {
//!   "benchmarks": [
//!     { "name": "json_parse", "median_ns": 812345.5, "p95_ns": 845100.0,
//!       "samples": 50, "iterations": 12 }
//!   ]
//! }
//! ```
//!
//! The baseline, `bench/baseline.json`, is a report that was copied there.

use nadir_codec::Limits;
use nadir_codec::json::{self, Value};

use crate::harness::Measurement;

/// A baseline is exceeded when a benchmark's median is more than this many
/// times its baseline median: 10% slower (`docs/04-quality.md`).
const SLOWDOWN: f64 = 1.10;

/// The result of one benchmark.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    /// The benchmark's name.
    pub(crate) name: String,
    /// The median time of one iteration, in nanoseconds.
    pub(crate) median_ns: f64,
    /// The 95th percentile of the time of one iteration, in nanoseconds.
    pub(crate) p95_ns: f64,
    /// How many samples were taken.
    pub(crate) samples: u64,
    /// How many iterations each sample ran.
    pub(crate) iterations: u64,
}

impl Entry {
    /// Sums up the measurement of the benchmark called `name`.
    pub(crate) fn new(name: &str, measurement: &Measurement) -> Self {
        Self {
            name: name.to_string(),
            median_ns: measurement.median_ns(),
            p95_ns: measurement.p95_ns(),
            samples: measurement.samples_ns.len() as u64,
            iterations: measurement.iterations,
        }
    }
}

/// Writes `entries` as a report. Counts beyond `i64::MAX`, which do not
/// come up, are written as that.
pub(crate) fn to_json(entries: &[Entry]) -> String {
    let count = |count: u64| Value::Int(i64::try_from(count).unwrap_or(i64::MAX));
    let benchmarks = entries
        .iter()
        .map(|entry| {
            Value::Object(vec![
                ("name".to_string(), Value::String(entry.name.clone())),
                ("median_ns".to_string(), Value::Float(entry.median_ns)),
                ("p95_ns".to_string(), Value::Float(entry.p95_ns)),
                ("samples".to_string(), count(entry.samples)),
                ("iterations".to_string(), count(entry.iterations)),
            ])
        })
        .collect();
    let report = Value::Object(vec![("benchmarks".to_string(), Value::Array(benchmarks))]);
    let mut text = json::to_string_pretty(&report);
    text.push('\n');
    text
}

/// The medians recorded in a baseline: benchmark name and nanoseconds.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Baseline {
    medians: Vec<(String, f64)>,
}

impl Baseline {
    /// Reads a report. Only the name and the median of each benchmark are
    /// used; a report without them is an error, described in words.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let report = json::parse(text, Limits::default()).map_err(|error| error.to_string())?;
        let benchmarks = report
            .get("benchmarks")
            .ok_or("`benchmarks` is missing")?
            .as_array()
            .ok_or("`benchmarks` is not an array")?;
        let mut medians = Vec::with_capacity(benchmarks.len());
        for benchmark in benchmarks {
            let name = benchmark
                .get("name")
                .and_then(Value::as_str)
                .ok_or("a benchmark has no `name` string")?;
            let median = benchmark
                .get("median_ns")
                .and_then(Value::as_f64)
                .filter(|median| *median > 0.0)
                .ok_or_else(|| format!("`{name}` has no positive `median_ns` number"))?;
            medians.push((name.to_string(), median));
        }
        Ok(Self { medians })
    }

    /// Compares a new median with the one recorded for `name`.
    pub(crate) fn compare(&self, name: &str, median_ns: f64) -> Change {
        let recorded = self.medians.iter().find(|(known, _)| known == name);
        match recorded {
            None => Change::NoBaseline,
            Some(&(_, baseline)) => {
                let ratio = median_ns / baseline;
                if ratio > SLOWDOWN {
                    Change::Slower(ratio)
                } else {
                    Change::Within(ratio)
                }
            }
        }
    }
}

/// How a benchmark did against its baseline. The number is the new median
/// divided by the baseline median.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Change {
    /// The baseline has no entry for the benchmark.
    NoBaseline,
    /// Faster, the same, or slower by no more than 10%.
    Within(f64),
    /// More than 10% slower. This is flagged, and is not a failure.
    Slower(f64),
}

/// The line printed for one benchmark. Names are padded to `name_width`.
pub(crate) fn line(entry: &Entry, change: Change, name_width: usize) -> String {
    let percent = |ratio: f64| format!("{:+.1}% vs baseline", (ratio - 1.0) * 100.0);
    let change = match change {
        Change::NoBaseline => "no baseline".to_string(),
        Change::Within(ratio) => percent(ratio),
        Change::Slower(ratio) => format!("{}  SLOWER", percent(ratio)),
    };
    format!(
        "{:<name_width$}  median {:>9}  p95 {:>9}  {} x {}  {change}",
        entry.name,
        duration(entry.median_ns),
        duration(entry.p95_ns),
        entry.samples,
        entry.iterations,
    )
}

/// A time given in nanoseconds, in the unit that suits its size.
pub(crate) fn duration(ns: f64) -> String {
    if ns < 1e3 {
        format!("{ns:.1} ns")
    } else if ns < 1e6 {
        format!("{:.1} µs", ns / 1e3)
    } else if ns < 1e9 {
        format!("{:.1} ms", ns / 1e6)
    } else {
        format!("{:.2} s", ns / 1e9)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, median_ns: f64, p95_ns: f64) -> Entry {
        Entry {
            name: name.to_string(),
            median_ns,
            p95_ns,
            samples: 50,
            iterations: 12,
        }
    }

    #[test]
    fn an_entry_sums_up_a_measurement() {
        let measurement = Measurement {
            iterations: 12,
            samples_ns: (1..=50).rev().map(f64::from).collect(),
        };
        assert_eq!(
            Entry::new("json_parse", &measurement),
            Entry {
                name: "json_parse".to_string(),
                median_ns: 25.5,
                p95_ns: 48.0,
                samples: 50,
                iterations: 12,
            }
        );
    }

    #[test]
    fn the_report_lists_each_benchmark_with_its_numbers() {
        let text = to_json(&[
            entry("json_parse", 812_345.5, 845_100.0),
            entry("sprite_sort", 1_250.25, 1_300.0),
        ]);
        assert_eq!(
            text,
            r#"{
  "benchmarks": [
    {
      "name": "json_parse",
      "median_ns": 812345.5,
      "p95_ns": 845100.0,
      "samples": 50,
      "iterations": 12
    },
    {
      "name": "sprite_sort",
      "median_ns": 1250.25,
      "p95_ns": 1300.0,
      "samples": 50,
      "iterations": 12
    }
  ]
}
"#
        );
        assert_eq!(to_json(&[]), "{\n  \"benchmarks\": []\n}\n");
    }

    #[test]
    fn a_report_can_be_read_back_as_a_baseline() {
        let entries = [
            entry("json_parse", 812_345.5, 845_100.0),
            entry("sprite_sort", 1_250.25, 1_300.0),
        ];
        let baseline = Baseline::parse(&to_json(&entries)).unwrap();
        assert_eq!(
            baseline.medians,
            [
                ("json_parse".to_string(), 812_345.5),
                ("sprite_sort".to_string(), 1_250.25)
            ]
        );
        assert_eq!(Baseline::parse(&to_json(&[])).unwrap(), Baseline::default());
    }

    #[test]
    fn a_baseline_needs_only_names_and_medians() {
        // Written by hand, with a whole number and a comment.
        let text = r#"{
            // recorded on the build machine
            "benchmarks": [{ "name": "a", "median_ns": 1000 }],
            "note": "extra members are ignored"
        }"#;
        assert_eq!(
            Baseline::parse(text).unwrap().medians,
            [("a".to_string(), 1000.0)]
        );
    }

    #[test]
    fn a_baseline_that_is_not_a_report_is_an_error() {
        let error = |text: &str| Baseline::parse(text).unwrap_err();
        assert!(error("not json").starts_with("JSON error at line 1, column 1"));
        assert_eq!(error("{}"), "`benchmarks` is missing");
        assert_eq!(error("[]"), "`benchmarks` is missing");
        assert_eq!(
            error(r#"{"benchmarks": {}}"#),
            "`benchmarks` is not an array"
        );
        assert_eq!(
            error(r#"{"benchmarks": [{"median_ns": 1.0}]}"#),
            "a benchmark has no `name` string"
        );
        assert_eq!(
            error(r#"{"benchmarks": [{"name": 7, "median_ns": 1.0}]}"#),
            "a benchmark has no `name` string"
        );
        assert_eq!(
            error(r#"{"benchmarks": ["a"]}"#),
            "a benchmark has no `name` string"
        );
        for median in ["\"fast\"", "0", "0.0", "-5.0", "null"] {
            let text = format!(r#"{{"benchmarks": [{{"name": "a", "median_ns": {median}}}]}}"#);
            assert_eq!(
                error(&text),
                "`a` has no positive `median_ns` number",
                "{median}"
            );
        }
        assert_eq!(
            error(r#"{"benchmarks": [{"name": "a"}]}"#),
            "`a` has no positive `median_ns` number"
        );
    }

    #[test]
    fn a_median_is_compared_with_the_baseline_of_the_same_name() {
        let baseline = Baseline {
            medians: vec![("a".to_string(), 1000.0), ("ab".to_string(), 50.0)],
        };
        assert_eq!(baseline.compare("a", 1000.0), Change::Within(1.0));
        assert_eq!(baseline.compare("a", 500.0), Change::Within(0.5));
        assert_eq!(baseline.compare("a", 1050.0), Change::Within(1.05));
        assert_eq!(baseline.compare("a", 1250.0), Change::Slower(1.25));
        assert_eq!(baseline.compare("ab", 100.0), Change::Slower(2.0));
        assert_eq!(baseline.compare("b", 1.0), Change::NoBaseline);
        assert_eq!(baseline.compare("", 1.0), Change::NoBaseline);
        assert_eq!(Baseline::default().compare("a", 1.0), Change::NoBaseline);
    }

    #[test]
    fn only_more_than_ten_percent_slower_is_flagged() {
        let baseline = Baseline {
            medians: vec![("a".to_string(), 1000.0)],
        };
        assert!(matches!(baseline.compare("a", 1099.0), Change::Within(_)));
        assert!(matches!(baseline.compare("a", 1100.0), Change::Within(_)));
        assert!(matches!(baseline.compare("a", 1101.0), Change::Slower(_)));
        // Much faster is not flagged either.
        assert!(matches!(baseline.compare("a", 1.0), Change::Within(_)));
    }

    #[test]
    fn times_are_printed_in_a_fitting_unit() {
        assert_eq!(duration(0.0), "0.0 ns");
        assert_eq!(duration(0.35), "0.3 ns");
        assert_eq!(duration(999.94), "999.9 ns");
        assert_eq!(duration(1000.0), "1.0 µs");
        assert_eq!(duration(212_449.0), "212.4 µs");
        assert_eq!(duration(1e6), "1.0 ms");
        assert_eq!(duration(15_500_000.0), "15.5 ms");
        assert_eq!(duration(1e9), "1.00 s");
        assert_eq!(duration(2.345e9), "2.35 s");
    }

    #[test]
    fn a_line_shows_the_times_the_counts_and_the_change() {
        let entry = entry("json_parse", 812_345.5, 845_100.0);
        assert_eq!(
            line(&entry, Change::NoBaseline, 10),
            "json_parse  median  812.3 µs  p95  845.1 µs  50 x 12  no baseline"
        );
        assert_eq!(
            line(&entry, Change::Within(1.0), 12),
            "json_parse    median  812.3 µs  p95  845.1 µs  50 x 12  +0.0% vs baseline"
        );
        assert_eq!(
            line(&entry, Change::Within(0.875), 4),
            "json_parse  median  812.3 µs  p95  845.1 µs  50 x 12  -12.5% vs baseline"
        );
        assert_eq!(
            line(&entry, Change::Slower(1.234), 10),
            "json_parse  median  812.3 µs  p95  845.1 µs  50 x 12  +23.4% vs baseline  SLOWER"
        );
    }
}
