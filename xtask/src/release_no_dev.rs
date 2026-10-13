//! `cargo xtask release-no-dev`: a release build of the sandbox contains no
//! dev tooling.

use std::path::Path;

use crate::cargo;
use crate::check::Report;
use crate::error::Error;

/// The package whose release build is checked.
const GAME: &str = "sandbox";

/// The crate that may only be reached through the `dev` feature.
const DEV_ONLY: &str = "nadir-debug";

/// Lists the packages a build of the sandbox links, one per line. The
/// `dev` feature is not asked for.
const ARGS: &[&str] = &["tree", "-p", GAME, "-e", "normal", "--prefix", "none"];

/// Runs the check on the workspace at `root`.
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    check(&cargo::capture(root, ARGS)?)
}

/// Looks for the dev-only crate in the output of `cargo tree`.
fn check(tree: &str) -> Result<Report, Error> {
    let mut packages = tree
        .lines()
        .filter_map(|line| line.split_whitespace().next());
    // The tree starts with the package it was asked about. Anything else
    // means the output is not what this check thinks it is.
    if packages.next() != Some(GAME) {
        return Err(Error::UnexpectedOutput {
            command: cargo::display(ARGS),
            detail: format!("the first line does not name `{GAME}`"),
        });
    }

    let mut report = Report::default();
    if packages.any(|package| package == DEV_ONLY) {
        report.violations.push(format!(
            "`{DEV_ONLY}` is in the dependency tree of `{GAME}` without the `dev` feature"
        ));
        report.notes.push(format!(
            "Make `{DEV_ONLY}` an optional dependency that only `dev` enables."
        ));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_without_the_debug_crate_passes() {
        let tree = "sandbox v0.1.0 (/work/nadir/games/sandbox)\n\
                    nadir v0.1.0 (/work/nadir/crates/engine)\n\
                    nadir-core v0.1.0 (/work/nadir/crates/core)\n\
                    nadir-render v0.1.0 (/work/nadir/crates/render)\n\
                    nadir-core v0.1.0 (/work/nadir/crates/core) (*)\n";
        assert_eq!(check(tree).unwrap(), Report::default());
        assert_eq!(
            check("sandbox v0.1.0 (/work/nadir/games/sandbox)\n").unwrap(),
            Report::default()
        );
    }

    #[test]
    fn the_debug_crate_anywhere_in_the_tree_fails() {
        let tree = "sandbox v0.1.0 (/work/nadir/games/sandbox)\n\
                    nadir v0.1.0 (/work/nadir/crates/engine)\n\
                    nadir-debug v0.1.0 (/work/nadir/crates/debug)\n\
                    nadir-core v0.1.0 (/work/nadir/crates/core)\n";
        let report = check(tree).unwrap();
        assert_eq!(
            report.violations,
            ["`nadir-debug` is in the dependency tree of `sandbox` without the `dev` feature"]
        );
        assert_eq!(report.notes.len(), 1);
    }

    #[test]
    fn only_a_whole_package_name_counts() {
        // A path or a longer name that contains `nadir-debug` is not it.
        let tree = "sandbox v0.1.0 (/work/nadir-debug/games/sandbox)\n\
                    nadir-debug-draw v0.1.0 (/work/nadir-debug/crates/debug-draw)\n";
        assert_eq!(check(tree).unwrap(), Report::default());
    }

    #[test]
    fn output_that_is_not_the_sandbox_tree_is_an_error() {
        for tree in ["", "\n", "nadir v0.1.0 (/work/nadir/crates/engine)\n"] {
            assert!(
                matches!(check(tree), Err(Error::UnexpectedOutput { .. })),
                "{tree:?}"
            );
        }
    }
}
