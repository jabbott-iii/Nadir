//! Micro-benchmark harness and benchmarks for the Nadir engine.
//!
//! Run it as `cargo xtask bench [FILTER]`, which builds it in release
//! mode. Each benchmark is warmed up and then sampled; the median and the
//! 95th percentile of the time per iteration are printed and written to
//! `target/bench/latest.json`. If `bench/baseline.json` exists, each median
//! is compared with it, and a slowdown of more than 10% is flagged. To
//! record a baseline, copy `latest.json` there after a run of every
//! benchmark: a filtered run writes only what it ran.
//!
//! The method is specified in `docs/04-quality.md`.

mod benches;
mod error;
mod harness;
mod report;

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nadir_core::cli::Args;

use crate::benches::Benchmark;
use crate::error::BenchError;
use crate::harness::{Bencher, Config};
use crate::report::{Baseline, Change, Entry};

/// The exit code for a command line that was not understood.
const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    let result = run(
        Args::from_env(),
        benches::ALL,
        Config::default(),
        &Files::in_workspace(),
        &mut io::stdout(),
    );
    let Err(error) = result else {
        return ExitCode::SUCCESS;
    };
    // If standard error is closed too, there is nobody left to tell.
    let _ = writeln!(io::stderr(), "nadir-bench: {error}");
    match error {
        BenchError::Usage(_) => ExitCode::from(EXIT_USAGE),
        _ => ExitCode::FAILURE,
    }
}

/// The files the harness reads and writes.
struct Files {
    /// The directory that messages show the other paths relative to.
    root: PathBuf,
    /// Where the results of this run go.
    latest: PathBuf,
    /// The results to compare with. It does not have to exist.
    baseline: PathBuf,
}

impl Files {
    /// `target/bench/latest.json` and `bench/baseline.json` in the
    /// workspace this crate was built in.
    fn in_workspace() -> Self {
        let bench = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = bench.parent().unwrap_or(bench);
        Self {
            root: root.to_path_buf(),
            latest: root.join("target").join("bench").join("latest.json"),
            baseline: bench.join("baseline.json"),
        }
    }

    /// `path` as messages show it: relative to the root, if it is inside.
    fn shown<'a>(&self, path: &'a Path) -> std::path::Display<'a> {
        path.strip_prefix(&self.root).unwrap_or(path).display()
    }
}

/// Runs the benchmarks whose name contains the filter in `args` (all of
/// them without one), prints a line for each, and writes the report.
fn run(
    args: Args,
    benchmarks: &[Benchmark],
    config: Config,
    files: &Files,
    out: &mut dyn Write,
) -> Result<(), BenchError> {
    let output = BenchError::Output;
    let chosen = choose(args, benchmarks)?;
    let baseline = read_baseline(&files.baseline)?;
    if baseline.is_none() {
        let path = files.shown(&files.baseline);
        writeln!(out, "no baseline: `{path}` does not exist").map_err(output)?;
    }
    let baseline = baseline.unwrap_or_default();

    let name_width = chosen
        .iter()
        .map(|benchmark| benchmark.name.len())
        .max()
        .unwrap_or(0);
    let mut entries = Vec::with_capacity(chosen.len());
    let mut slower = 0;
    for benchmark in chosen {
        let mut bencher = Bencher::new(config);
        (benchmark.run)(&mut bencher);
        let Some(measurement) = bencher.finish() else {
            return Err(BenchError::Untimed(benchmark.name.to_string()));
        };
        let entry = Entry::new(benchmark.name, &measurement);
        let change = baseline.compare(&entry.name, entry.median_ns);
        slower += usize::from(matches!(change, Change::Slower(_)));
        writeln!(out, "{}", report::line(&entry, change, name_width)).map_err(output)?;
        // A slow benchmark's line should not wait for the next one.
        out.flush().map_err(output)?;
        entries.push(entry);
    }

    write_report(&files.latest, &report::to_json(&entries))?;
    writeln!(out, "wrote `{}`", files.shown(&files.latest)).map_err(output)?;
    if slower > 0 {
        writeln!(
            out,
            "{slower} of {} more than 10% slower than the baseline",
            entries.len()
        )
        .map_err(output)?;
    }
    Ok(())
}

/// Picks the benchmarks to run: those whose name contains the one optional
/// argument.
fn choose(args: Args, benchmarks: &[Benchmark]) -> Result<Vec<&Benchmark>, BenchError> {
    let arguments = args
        .finish()
        .map_err(|error| BenchError::Usage(error.to_string()))?;
    let filter = match arguments.as_slice() {
        [] => "",
        [filter] => filter.as_str(),
        [_, extra, ..] => {
            return Err(BenchError::Usage(format!(
                "expected at most one filter, but also got `{extra}`"
            )));
        }
    };
    let chosen: Vec<&Benchmark> = benchmarks
        .iter()
        .filter(|benchmark| benchmark.name.contains(filter))
        .collect();
    if chosen.is_empty() {
        let names: Vec<&str> = benchmarks.iter().map(|benchmark| benchmark.name).collect();
        return Err(BenchError::Usage(format!(
            "no benchmark name contains `{filter}`; the benchmarks are: {}",
            names.join(", ")
        )));
    }
    Ok(chosen)
}

/// Reads the baseline. `None` if there is no such file.
fn read_baseline(path: &Path) -> Result<Option<Baseline>, BenchError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(BenchError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    match Baseline::parse(&text) {
        Ok(baseline) => Ok(Some(baseline)),
        Err(detail) => Err(BenchError::Baseline {
            path: path.to_path_buf(),
            detail,
        }),
    }
}

/// Writes the report, creating its directory if need be.
fn write_report(path: &Path, text: &str) -> Result<(), BenchError> {
    let written = match path.parent() {
        Some(directory) => fs::create_dir_all(directory),
        None => Ok(()),
    }
    .and_then(|()| fs::write(path, text));
    written.map_err(|source| BenchError::Write {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn add(bencher: &mut Bencher) {
        bencher.iter(|| std::hint::black_box(2_u64) + 2);
    }

    fn sleep(bencher: &mut Bencher) {
        bencher.iter(|| std::thread::sleep(Duration::from_millis(2)));
    }

    fn forgetful(_: &mut Bencher) {}

    const FAKES: &[Benchmark] = &[
        Benchmark {
            name: "add",
            run: add,
        },
        Benchmark {
            name: "sleep_2ms",
            run: sleep,
        },
        Benchmark {
            name: "add_again",
            run: add,
        },
    ];

    /// Fresh file locations for one test, under `target/tmp`.
    fn files(test: &str) -> Files {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/tmp")
            .join(format!("nadir-bench-{test}"));
        // It may be left over from an earlier run.
        let _ = fs::remove_dir_all(&directory);
        Files {
            latest: directory.join("out").join("latest.json"),
            baseline: directory.join("baseline.json"),
            root: directory,
        }
    }

    fn run_fakes(args: &[&str], files: &Files) -> Result<String, BenchError> {
        let mut out = Vec::new();
        run(
            Args::new(args.iter().copied()),
            FAKES,
            Config::fast(),
            files,
            &mut out,
        )?;
        Ok(String::from_utf8(out).unwrap())
    }

    fn usage_error(args: &[&str]) -> String {
        match choose(Args::new(args.iter().copied()), FAKES) {
            Err(BenchError::Usage(message)) => message,
            Err(other) => panic!("expected a usage error, got {other:?}"),
            Ok(_) => panic!("expected a usage error for {args:?}"),
        }
    }

    fn chosen(args: &[&str]) -> Vec<&'static str> {
        choose(Args::new(args.iter().copied()), FAKES)
            .unwrap()
            .iter()
            .map(|benchmark| benchmark.name)
            .collect()
    }

    #[test]
    fn the_filter_picks_benchmarks_by_part_of_their_name() {
        assert_eq!(chosen(&[]), ["add", "sleep_2ms", "add_again"]);
        assert_eq!(chosen(&["add"]), ["add", "add_again"]);
        assert_eq!(chosen(&["sleep"]), ["sleep_2ms"]);
        assert_eq!(chosen(&["_"]), ["sleep_2ms", "add_again"]);
        assert_eq!(chosen(&["add_again"]), ["add_again"]);
    }

    #[test]
    fn a_filter_that_matches_nothing_is_a_usage_error() {
        assert_eq!(
            usage_error(&["sprite"]),
            "no benchmark name contains `sprite`; the benchmarks are: add, sleep_2ms, add_again"
        );
        // Matching is case-sensitive.
        assert!(usage_error(&["ADD"]).starts_with("no benchmark name contains `ADD`"));
        assert_eq!(
            usage_error(&["add", "sleep"]),
            "expected at most one filter, but also got `sleep`"
        );
        assert_eq!(usage_error(&["--fast"]), "unexpected argument `--fast`");
    }

    #[test]
    fn a_run_prints_a_line_for_each_benchmark_and_writes_the_report() {
        let files = files("run");
        let output = run_fakes(&[], &files).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 5, "{output}");
        // Paths are shown relative to the root.
        assert_eq!(lines[0], "no baseline: `baseline.json` does not exist");
        // Names are padded to the longest one.
        assert!(lines[1].starts_with("add        median "), "{output}");
        assert!(lines[2].starts_with("sleep_2ms  median "), "{output}");
        assert!(lines[3].starts_with("add_again  median "), "{output}");
        for line in &lines[1..4] {
            assert!(line.contains("  p95 "), "{output}");
            assert!(line.contains("  7 x "), "{output}");
            assert!(line.ends_with("  no baseline"), "{output}");
        }
        // 2 ms of sleep is printed in milliseconds.
        assert!(lines[2].contains(" ms  p95 "), "{output}");
        let latest = Path::new("out").join("latest.json");
        assert_eq!(lines[4], format!("wrote `{}`", latest.display()));

        // The report holds the same three benchmarks, and is a baseline.
        let report = fs::read_to_string(&files.latest).unwrap();
        let value = nadir_codec::json::parse(&report, nadir_codec::Limits::default()).unwrap();
        let names: Vec<&str> = value
            .get("benchmarks")
            .and_then(|benchmarks| benchmarks.as_array())
            .unwrap()
            .iter()
            .map(|benchmark| {
                benchmark
                    .get("name")
                    .and_then(|name| name.as_str())
                    .unwrap()
            })
            .collect();
        assert_eq!(names, ["add", "sleep_2ms", "add_again"]);
        assert!(report.contains("\"samples\": 7"), "{report}");
        let baseline = Baseline::parse(&report).unwrap();
        assert!(matches!(
            baseline.compare("sleep_2ms", 2e6),
            Change::Within(_)
        ));
    }

    #[test]
    fn a_filtered_run_reports_only_what_ran() {
        let files = files("filtered");
        let output = run_fakes(&["sleep"], &files).unwrap();
        assert_eq!(output.lines().count(), 3, "{output}");
        let report = fs::read_to_string(&files.latest).unwrap();
        assert!(
            report.contains("sleep_2ms") && !report.contains("add"),
            "{report}"
        );
    }

    #[test]
    fn results_are_compared_with_the_baseline() {
        let files = files("baseline");
        fs::create_dir_all(files.baseline.parent().unwrap()).unwrap();
        // The sleep takes at least 2 ms: far slower than the first
        // baseline below, far faster than the second.
        let baseline = |median_ns: f64| {
            format!(r#"{{"benchmarks": [{{"name": "sleep_2ms", "median_ns": {median_ns:?}}}]}}"#)
        };

        fs::write(&files.baseline, baseline(1000.0)).unwrap();
        let output = run_fakes(&[], &files).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 5, "{output}");
        assert!(lines[0].ends_with("  no baseline"), "{output}");
        assert!(lines[1].ends_with("% vs baseline  SLOWER"), "{output}");
        assert!(lines[2].ends_with("  no baseline"), "{output}");
        assert!(lines[3].starts_with("wrote "), "{output}");
        assert_eq!(lines[4], "1 of 3 more than 10% slower than the baseline");

        fs::write(&files.baseline, baseline(60e9)).unwrap();
        let output = run_fakes(&["sleep"], &files).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 2, "{output}");
        assert!(lines[0].ends_with("-100.0% vs baseline"), "{output}");
    }

    #[test]
    fn a_broken_baseline_stops_the_run_before_it_starts() {
        let files = files("broken-baseline");
        fs::create_dir_all(files.baseline.parent().unwrap()).unwrap();
        fs::write(&files.baseline, "{}").unwrap();
        let error = run_fakes(&[], &files).unwrap_err();
        assert!(
            matches!(&error, BenchError::Baseline { path, detail }
                if *path == files.baseline && detail == "`benchmarks` is missing"),
            "{error:?}"
        );
        assert!(!files.latest.exists());
    }

    #[test]
    fn a_report_that_cannot_be_written_is_an_error() {
        let mut files = files("unwritable");
        // A file where the report's directory should be.
        fs::create_dir_all(files.baseline.parent().unwrap()).unwrap();
        let blocker = files.baseline.with_file_name("blocker");
        fs::write(&blocker, "in the way").unwrap();
        files.latest = blocker.join("latest.json");
        let error = run_fakes(&["add_again"], &files).unwrap_err();
        assert!(
            matches!(&error, BenchError::Write { path, .. } if *path == files.latest),
            "{error:?}"
        );
    }

    #[test]
    fn a_benchmark_that_times_nothing_is_reported() {
        let benchmarks = [Benchmark {
            name: "forgetful",
            run: forgetful,
        }];
        let mut out = Vec::new();
        let error = run(
            Args::new(Vec::<String>::new()),
            &benchmarks,
            Config::fast(),
            &files("forgetful"),
            &mut out,
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "benchmark `forgetful` did not time anything"
        );
    }

    #[test]
    fn the_real_files_are_in_the_workspace() {
        let files = Files::in_workspace();
        assert!(files.latest.ends_with("target/bench/latest.json"));
        assert!(files.baseline.ends_with("bench/baseline.json"));
        assert_eq!(files.latest.ancestors().nth(3), Some(files.root.as_path()));
        assert!(files.root.join("Cargo.lock").exists());
        assert_eq!(
            files.shown(&files.baseline).to_string(),
            Path::new("bench")
                .join("baseline.json")
                .display()
                .to_string()
        );
        // A path outside the root is shown whole.
        let outside = Path::new("elsewhere").join("x.json");
        assert_eq!(
            files.shown(&outside).to_string(),
            outside.display().to_string()
        );
    }
}
