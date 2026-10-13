//! Build automation for the Nadir workspace: CI checks, goldens and
//! packaging.
//!
//! Run it as `cargo xtask <command>`; `cargo xtask --help` lists the
//! commands. The checks, the fuzzer and the benchmarks are specified in
//! `docs/04-quality.md`.
//!
//! This tool never uploads, publishes or signs anything, and makes no
//! network calls of its own.

mod cargo;
mod check;
mod deps;
mod error;
mod fuzz;
mod layers;
mod metadata;
mod release_no_dev;
mod unsafe_audit;

use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nadir_core::CoreError;
use nadir_core::cli::Args;

use crate::check::CHECKS;
use crate::error::Error;

/// The exit code for a command line that was not understood.
const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    match run(Args::from_env(), &workspace_root(), &mut io::stdout()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        // If standard error is closed too, there is nobody left to tell.
        Err(error @ Error::Usage(_)) => {
            let _ = writeln!(io::stderr(), "xtask: {error}\n\n{}", usage());
            ExitCode::from(EXIT_USAGE)
        }
        Err(error) => {
            let _ = writeln!(io::stderr(), "xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs the command that `args` names on the workspace at `root`. Returns
/// whether it passed.
fn run(mut args: Args, root: &Path, out: &mut dyn Write) -> Result<bool, Error> {
    let command = args.subcommand();
    if args.flag("--help") {
        out.write_all(usage().as_bytes()).map_err(Error::Output)?;
        return Ok(true);
    }
    let usage = |error: CoreError| Error::Usage(error.to_string());
    let Some(command) = command else {
        args.finish().map_err(usage)?;
        return Err(Error::Usage("no command given".to_string()));
    };
    match command.as_str() {
        "fuzz-smoke" => return fuzz::command(args, root, out),
        "bench" => {
            let filter = args.finish().map_err(usage)?;
            cargo::run(root, &bench_command(&filter))?;
            return Ok(true);
        }
        _ => {}
    }

    let extra = args.finish().map_err(usage)?;
    if let Some(argument) = extra.first() {
        return Err(Error::Usage(format!(
            "`{command}` takes no arguments, but got `{argument}`"
        )));
    }
    let passed = if command == "ci" {
        check::run_all(CHECKS, root, out)
    } else if let Some(check) = CHECKS.iter().find(|check| check.name == command) {
        check::run_one(check, root, out)
    } else {
        return Err(Error::Usage(format!("unknown command `{command}`")));
    };
    passed.map_err(Error::Output)
}

/// The cargo arguments behind `cargo xtask bench [FILTER]`: the benchmarks
/// run in release mode. The harness is the `nadir-bench` binary, which
/// checks the filter and reports for itself.
fn bench_command(filter: &[String]) -> Vec<&str> {
    let mut command = vec!["run", "--release", "--package", "nadir-bench", "--"];
    command.extend(filter.iter().map(String::as_str));
    command
}

/// The workspace root: the directory above this crate's manifest.
fn workspace_root() -> PathBuf {
    let xtask = Path::new(env!("CARGO_MANIFEST_DIR"));
    xtask.parent().unwrap_or(xtask).to_path_buf()
}

fn usage() -> String {
    let mut text = String::from("Usage: cargo xtask <command>\n\nCommands:\n");
    let commands = CHECKS.iter().map(|check| (check.name, check.about)).chain([
        (
            "ci",
            "All of the above in order, stopping at the first failure",
        ),
        (
            "fuzz-smoke",
            "Fuzz the parsers: [--secs N] for each target, [--target NAME] for one",
        ),
        (
            "bench",
            "Run the benchmarks in release mode: [FILTER] for those whose name contains it",
        ),
    ]);
    for (name, about) in commands {
        // Writing to a `String` cannot fail.
        let _ = writeln!(text, "  {name:<14}  {about}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a command line that must be rejected, and returns the message.
    fn usage_error(args: &[&str]) -> String {
        let mut out = Vec::new();
        match run(Args::new(args.iter().copied()), Path::new("."), &mut out) {
            Err(Error::Usage(message)) => {
                assert_eq!(out, b"", "a rejected command line printed something");
                message
            }
            other => panic!("expected a usage error for {args:?}, got {other:?}"),
        }
    }

    #[test]
    fn help_lists_every_command() {
        let mut out = Vec::new();
        let passed = run(Args::new(["--help"]), Path::new("."), &mut out).unwrap();
        assert!(passed);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text, usage());
        for name in [
            "fmt",
            "clippy",
            "test",
            "deps",
            "layers",
            "unsafe-audit",
            "release-no-dev",
            "ci",
            "fuzz-smoke",
            "bench",
        ] {
            assert!(
                text.lines()
                    .any(|line| line.starts_with(&format!("  {name} "))),
                "`{name}` is missing from:\n{text}"
            );
        }
        // Asking for help on a command gives the same text, and does not
        // run the command. (Not `ci`: if this ever did run it, `ci` would
        // run these tests again, without end.)
        let mut out = Vec::new();
        assert!(
            run(
                Args::new(["unsafe-audit", "--help"]),
                Path::new("."),
                &mut out
            )
            .unwrap()
        );
        assert_eq!(String::from_utf8(out).unwrap(), usage());
    }

    #[test]
    fn usage_columns_fit_the_longest_command_name() {
        let longest = CHECKS.iter().map(|check| check.name.len()).max().unwrap();
        assert_eq!(longest, 14);
    }

    #[test]
    fn a_missing_or_unknown_command_is_a_usage_error() {
        assert_eq!(usage_error(&[]), "no command given");
        assert_eq!(usage_error(&["frobnicate"]), "unknown command `frobnicate`");
        // Names are matched whole and case-sensitively.
        assert_eq!(usage_error(&["dep"]), "unknown command `dep`");
        assert_eq!(usage_error(&["CI"]), "unknown command `CI`");
    }

    #[test]
    fn stray_arguments_are_usage_errors() {
        assert_eq!(
            usage_error(&["deps", "extra"]),
            "`deps` takes no arguments, but got `extra`"
        );
        assert_eq!(
            usage_error(&["ci", "--fast"]),
            "unexpected argument `--fast`"
        );
        assert_eq!(
            usage_error(&["--verbose"]),
            "unexpected argument `--verbose`"
        );
        assert_eq!(
            usage_error(&["ci", "--", "--help"]),
            "`ci` takes no arguments, but got `--help`"
        );
    }

    #[test]
    fn a_command_runs_its_check_on_the_given_root() {
        // A directory that is not a workspace: the audit runs, and fails
        // because the three FFI crate roots are not there.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut out = Vec::new();
        let passed = run(Args::new(["unsafe-audit"]), &root, &mut out).unwrap();
        assert!(!passed);
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("violation: crates/sys/src/lib.rs was not found"),
            "{text}"
        );
        assert!(
            text.ends_with("unsafe-audit: FAILED (3 violations)\n"),
            "{text}"
        );
    }

    #[test]
    fn fuzz_smoke_takes_its_own_options() {
        assert_eq!(
            usage_error(&["fuzz-smoke", "--target", "nope"]),
            "unknown fuzz target `nope`; the targets are: json"
        );
        assert_eq!(
            usage_error(&["fuzz-smoke", "--secs"]),
            "option `--secs` needs a value"
        );

        // With no time, only the seeds in `fuzz/corpus/json/` run.
        let mut out = Vec::new();
        let args = Args::new(["fuzz-smoke", "--secs", "0", "--target", "json"]);
        let passed = run(args, &workspace_root(), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(passed, "{text}");
        assert!(
            text.starts_with("fuzz-smoke: 1 target, 0 s each, seed 0x"),
            "{text}"
        );
        assert!(text.ends_with("fuzz-smoke: ok\n"), "{text}");
    }

    #[test]
    fn bench_passes_its_filter_to_the_harness_in_release_mode() {
        assert_eq!(
            bench_command(&["json".to_string()]),
            ["run", "--release", "--package", "nadir-bench", "--", "json"]
        );
        assert_eq!(
            bench_command(&[]),
            ["run", "--release", "--package", "nadir-bench", "--"]
        );
        // Options are not passed on.
        assert_eq!(
            usage_error(&["bench", "--fast"]),
            "unexpected argument `--fast`"
        );
    }

    #[test]
    fn the_workspace_root_holds_the_workspace_manifest() {
        let manifest = std::fs::read_to_string(workspace_root().join("Cargo.toml")).unwrap();
        assert!(manifest.contains("[workspace]"), "{manifest}");
    }
}
