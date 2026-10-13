//! The checks behind `cargo xtask ci`, and the code that runs and reports
//! them.

use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::error::Error;
use crate::{cargo, deps, layers, release_no_dev, unsafe_audit};

/// What a check found.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Report {
    /// Lines to print whether or not the check passed.
    pub(crate) notes: Vec<String>,
    /// One line per violation. The check passed if there are none.
    pub(crate) violations: Vec<String>,
}

/// One check, which is also a subcommand.
pub(crate) struct Check {
    /// The subcommand that runs it.
    pub(crate) name: &'static str,
    /// What it does, for the usage text.
    pub(crate) about: &'static str,
    /// Runs it on the workspace at the given root.
    pub(crate) run: fn(&Path) -> Result<Report, Error>,
}

/// Every check, in the order `ci` runs them (`docs/04-quality.md`).
pub(crate) const CHECKS: &[Check] = &[
    Check {
        name: "fmt",
        about: "cargo fmt --all -- --check",
        run: |root| pass_through(root, FMT),
    },
    Check {
        name: "clippy",
        about: "cargo clippy --workspace --all-targets --all-features -- -D warnings",
        run: |root| pass_through(root, CLIPPY),
    },
    Check {
        name: "test",
        about: "cargo test --workspace --all-features",
        run: |root| pass_through(root, TEST),
    },
    Check {
        name: "deps",
        about: "Every third-party package is approved in xtask/deps-allowlist.txt",
        run: deps::run,
    },
    Check {
        name: "layers",
        about: "Workspace crates depend only on what the dependency matrix allows",
        run: layers::run,
    },
    Check {
        name: "unsafe-audit",
        about: "`unsafe` only in sys, platform and gfx, each use with a SAFETY comment",
        run: unsafe_audit::run,
    },
    Check {
        name: "release-no-dev",
        about: "nadir-debug is not in the sandbox's dependency tree without `dev`",
        run: release_no_dev::run,
    },
];

const FMT: &[&str] = &["fmt", "--all", "--", "--check"];
const CLIPPY: &[&str] = &[
    "clippy",
    "--workspace",
    "--all-targets",
    "--all-features",
    "--",
    "-D",
    "warnings",
];
const TEST: &[&str] = &["test", "--workspace", "--all-features"];

/// A check that is one cargo command, which reports its own findings.
fn pass_through(root: &Path, args: &[&str]) -> Result<Report, Error> {
    cargo::run(root, args)?;
    Ok(Report::default())
}

/// Runs one check and prints what it found. Returns whether it passed.
pub(crate) fn run_one(check: &Check, root: &Path, out: &mut dyn Write) -> io::Result<bool> {
    let name = check.name;
    match (check.run)(root) {
        Ok(report) => {
            for note in &report.notes {
                writeln!(out, "{note}")?;
            }
            for violation in &report.violations {
                writeln!(out, "violation: {violation}")?;
            }
            match report.violations.len() {
                0 => writeln!(out, "{name}: ok")?,
                1 => writeln!(out, "{name}: FAILED (1 violation)")?,
                count => writeln!(out, "{name}: FAILED ({count} violations)")?,
            }
            Ok(report.violations.is_empty())
        }
        Err(error) => {
            writeln!(out, "{name}: FAILED: {error}")?;
            Ok(false)
        }
    }
}

/// How a check ended in a run of several.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Passed,
    Failed,
    /// Not run, because an earlier check failed.
    Skipped,
}

/// Runs `checks` in order, stopping at the first failure, then prints a
/// summary table. Returns whether all of them passed.
pub(crate) fn run_all(checks: &[Check], root: &Path, out: &mut dyn Write) -> io::Result<bool> {
    let mut outcomes = Vec::with_capacity(checks.len());
    let mut failed = false;
    for check in checks {
        if failed {
            outcomes.push((check.name, Status::Skipped, Duration::ZERO));
            continue;
        }
        writeln!(out, "== {}", check.name)?;
        // The cargo commands write to the terminal directly, so this
        // heading has to be there before they start.
        out.flush()?;
        let start = Instant::now();
        let passed = run_one(check, root, out)?;
        failed = !passed;
        let status = if passed {
            Status::Passed
        } else {
            Status::Failed
        };
        outcomes.push((check.name, status, start.elapsed()));
    }
    out.write_all(summary(&outcomes).as_bytes())?;
    Ok(!failed)
}

/// Formats the table that ends a `ci` run.
fn summary(outcomes: &[(&str, Status, Duration)]) -> String {
    let width = outcomes
        .iter()
        .map(|(name, ..)| name.len())
        .max()
        .unwrap_or(0);
    let mut table = String::from("== summary\n");
    let mut failure = None;
    for &(name, status, elapsed) in outcomes {
        // Writing to a `String` cannot fail.
        let _ = match status {
            Status::Passed => writeln!(
                table,
                "{name:<width$}  ok       {:>7.1} s",
                elapsed.as_secs_f64()
            ),
            Status::Failed => writeln!(
                table,
                "{name:<width$}  FAILED   {:>7.1} s",
                elapsed.as_secs_f64()
            ),
            Status::Skipped => writeln!(table, "{name:<width$}  skipped"),
        };
        if status == Status::Failed {
            failure = Some(name);
        }
    }
    let _ = match failure {
        Some(name) => writeln!(table, "ci: FAILED at `{name}`"),
        None => writeln!(table, "ci: ok"),
    };
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passing(_: &Path) -> Result<Report, Error> {
        Ok(Report {
            notes: vec!["looked at 3 things".to_string()],
            violations: Vec::new(),
        })
    }

    fn silent(_: &Path) -> Result<Report, Error> {
        Ok(Report::default())
    }

    fn violating(_: &Path) -> Result<Report, Error> {
        Ok(Report {
            notes: vec!["looked at 3 things".to_string()],
            violations: vec!["a is wrong".to_string(), "b is wrong".to_string()],
        })
    }

    fn one_violation(_: &Path) -> Result<Report, Error> {
        Ok(Report {
            notes: Vec::new(),
            violations: vec!["a is wrong".to_string()],
        })
    }

    fn erroring(_: &Path) -> Result<Report, Error> {
        Err(Error::UnexpectedOutput {
            command: "cargo tree".to_string(),
            detail: "nothing".to_string(),
        })
    }

    fn unreachable(_: &Path) -> Result<Report, Error> {
        panic!("a check ran after an earlier one had failed");
    }

    fn check(name: &'static str, run: fn(&Path) -> Result<Report, Error>) -> Check {
        Check {
            name,
            about: "",
            run,
        }
    }

    fn output_of_one(run: fn(&Path) -> Result<Report, Error>) -> (bool, String) {
        let mut out = Vec::new();
        let passed = run_one(&check("demo", run), Path::new("."), &mut out).unwrap();
        (passed, String::from_utf8(out).unwrap())
    }

    /// The output of `run_all`, with the measured times replaced by `?`.
    fn output_of_all(checks: &[Check]) -> (bool, String) {
        let mut out = Vec::new();
        let passed = run_all(checks, Path::new("."), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let text: Vec<String> = text
            .lines()
            .map(|line| match line.strip_suffix(" s") {
                Some(timed) => format!(
                    "{} ? s",
                    timed.trim_end_matches(|c: char| c != ' ').trim_end()
                ),
                None => line.to_string(),
            })
            .collect();
        (passed, text.join("\n"))
    }

    #[test]
    fn a_passing_check_prints_its_notes_and_ok() {
        assert_eq!(
            output_of_one(passing),
            (true, "looked at 3 things\ndemo: ok\n".to_string())
        );
        assert_eq!(output_of_one(silent), (true, "demo: ok\n".to_string()));
    }

    #[test]
    fn a_check_with_violations_fails_and_lists_them() {
        assert_eq!(
            output_of_one(violating),
            (
                false,
                "looked at 3 things\nviolation: a is wrong\nviolation: b is wrong\n\
                 demo: FAILED (2 violations)\n"
                    .to_string()
            )
        );
        assert_eq!(
            output_of_one(one_violation),
            (
                false,
                "violation: a is wrong\ndemo: FAILED (1 violation)\n".to_string()
            )
        );
    }

    #[test]
    fn a_check_that_cannot_run_fails_with_the_reason() {
        assert_eq!(
            output_of_one(erroring),
            (
                false,
                "demo: FAILED: unexpected output from `cargo tree`: nothing\n".to_string()
            )
        );
    }

    #[test]
    fn all_checks_passing_is_a_pass() {
        let (passed, output) = output_of_all(&[check("one", passing), check("second", silent)]);
        assert!(passed);
        assert_eq!(
            output,
            "== one\nlooked at 3 things\none: ok\n== second\nsecond: ok\n== summary\n\
             one     ok ? s\nsecond  ok ? s\nci: ok"
        );
    }

    #[test]
    fn the_run_stops_at_the_first_failure() {
        let (passed, output) = output_of_all(&[
            check("one", silent),
            check("two", one_violation),
            check("three", unreachable),
            check("four", unreachable),
        ]);
        assert!(!passed);
        assert_eq!(
            output,
            "== one\none: ok\n== two\nviolation: a is wrong\ntwo: FAILED (1 violation)\n\
             == summary\none    ok ? s\ntwo    FAILED ? s\nthree  skipped\nfour   skipped\n\
             ci: FAILED at `two`"
        );
    }

    #[test]
    fn a_check_that_cannot_run_stops_the_run_too() {
        let (passed, output) = output_of_all(&[check("one", erroring), check("two", unreachable)]);
        assert!(!passed);
        assert!(
            output.ends_with("two  skipped\nci: FAILED at `one`"),
            "{output}"
        );
    }

    #[test]
    fn the_summary_lines_up_names_results_and_times() {
        let outcomes = [
            ("fmt", Status::Passed, Duration::from_millis(340)),
            (
                "unsafe-audit",
                Status::Passed,
                Duration::from_millis(12_060),
            ),
            ("test", Status::Failed, Duration::from_secs(125)),
            ("release-no-dev", Status::Skipped, Duration::ZERO),
        ];
        assert_eq!(
            summary(&outcomes),
            "== summary\n\
             fmt             ok           0.3 s\n\
             unsafe-audit    ok          12.1 s\n\
             test            FAILED     125.0 s\n\
             release-no-dev  skipped\n\
             ci: FAILED at `test`\n"
        );
        assert_eq!(summary(&[]), "== summary\nci: ok\n");
    }

    #[test]
    fn ci_runs_the_checks_in_the_documented_order() {
        let names: Vec<&str> = CHECKS.iter().map(|check| check.name).collect();
        assert_eq!(
            names,
            [
                "fmt",
                "clippy",
                "test",
                "deps",
                "layers",
                "unsafe-audit",
                "release-no-dev"
            ]
        );
    }

    #[test]
    fn the_cargo_checks_run_the_documented_commands() {
        // The usage text shows these, so they must be what actually runs.
        for (name, args) in [("fmt", FMT), ("clippy", CLIPPY), ("test", TEST)] {
            let check = CHECKS.iter().find(|check| check.name == name).unwrap();
            assert_eq!(check.about, cargo::display(args));
        }
    }
}
