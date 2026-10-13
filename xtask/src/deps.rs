//! `cargo xtask deps`: every third-party package is an approved pin.
//!
//! The policy is `docs/02-dependency-policy.md`. A package is third-party
//! if it comes from a registry or from git, or if it is a directory outside
//! the workspace (vendored code counts).

use std::fs;
use std::path::Path;

use crate::check::Report;
use crate::error::Error;
use crate::metadata::{self, Metadata, Package};

/// The allowlist, relative to the workspace root.
const ALLOWLIST: &str = "xtask/deps-allowlist.txt";

/// One approved package: a line of the allowlist.
#[derive(Debug, PartialEq, Eq)]
struct Pin {
    name: String,
    version: String,
}

/// Runs the check on the workspace at `root`.
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    let path = root.join(ALLOWLIST);
    let allowlist = fs::read_to_string(&path).map_err(|source| Error::Read { path, source })?;
    Ok(check(&metadata::load(root)?, &allowlist))
}

/// Compares the package graph with the text of the allowlist.
fn check(metadata: &Metadata, allowlist: &str) -> Report {
    let mut report = Report::default();
    let pins = parse_allowlist(allowlist, &mut report.violations);

    let mut members = 0;
    let mut third_party = 0;
    for package in &metadata.packages {
        if package.is_member {
            members += 1;
            continue;
        }
        third_party += 1;
        if let Some(problem) = problem(package, &pins) {
            let users = users(metadata, package);
            report.violations.push(format!(
                "`{} {}` {problem}{users}",
                package.name, package.version
            ));
        }
    }

    report.notes.push(format!(
        "workspace packages: {members}, third-party packages: {third_party}"
    ));
    if !report.violations.is_empty() {
        report.notes.push(
            "A third-party crate needs the user's approval first: see \
             docs/02-dependency-policy.md."
                .to_string(),
        );
    }
    report
}

/// Says what is wrong with a package from outside the workspace, if
/// anything.
fn problem(package: &Package, pins: &[Pin]) -> Option<String> {
    let Some(source) = &package.source else {
        return Some(
            "is a directory outside the workspace, which counts as third-party code".to_string(),
        );
    };
    let mut approved_versions = pins
        .iter()
        .filter(|pin| pin.name == package.name)
        .map(|pin| pin.version.as_str());
    if approved_versions
        .clone()
        .any(|version| version == package.version)
    {
        return None;
    }
    Some(match approved_versions.next() {
        Some(version) => format!("({source}) is not the approved version, {version}"),
        None => format!("({source}) is not in {ALLOWLIST}"),
    })
}

/// Names the workspace members that ask for `package` directly, as a
/// suffix for a message. A package nobody asks for directly came in through
/// another third-party package.
fn users(metadata: &Metadata, package: &Package) -> String {
    let users: Vec<&str> = metadata
        .packages
        .iter()
        .filter(|member| member.is_member)
        .filter(|member| {
            member
                .dependencies
                .iter()
                .any(|dependency| dependency.name == package.name)
        })
        .map(|member| member.name.as_str())
        .collect();
    if users.is_empty() {
        String::new()
    } else {
        format!("; required by {}", users.join(", "))
    }
}

/// Reads the allowlist: one `name version ADR-NNNN` per line, with blank
/// lines and `#` comment lines ignored. A line of any other shape becomes a
/// violation.
fn parse_allowlist(text: &str, violations: &mut Vec<String>) -> Vec<Pin> {
    let mut pins = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields.as_slice() {
            [name, version, adr] if is_adr(adr) => pins.push(Pin {
                name: name.to_string(),
                version: version.to_string(),
            }),
            _ => violations.push(format!(
                "{ALLOWLIST}:{}: expected `name version ADR-NNNN`, found `{line}`",
                index + 1
            )),
        }
    }
    pins
}

/// Whether `text` is an ADR reference such as `ADR-0004`.
fn is_adr(text: &str) -> bool {
    text.strip_prefix("ADR-")
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::Kind;
    use crate::metadata::testing::{CRATES_IO, member, third_party};

    fn graph(packages: Vec<Package>) -> Metadata {
        Metadata { packages }
    }

    #[test]
    fn a_workspace_of_only_members_passes() {
        let metadata = graph(vec![
            member("nadir-core"),
            member("nadir-codec").path_dep("nadir-core", Kind::Dev),
        ]);
        let report = check(&metadata, "# nothing approved\n");
        assert_eq!(report.violations, Vec::<String>::new());
        assert_eq!(
            report.notes,
            ["workspace packages: 2, third-party packages: 0"]
        );
    }

    #[test]
    fn a_registry_package_that_is_not_listed_fails() {
        let metadata = graph(vec![
            member("nadir-core"),
            member("nadir-ecs").registry_dep("left-pad"),
            third_party("left-pad", "1.0.3"),
        ]);
        let report = check(&metadata, "");
        assert_eq!(
            report.violations,
            [format!(
                "`left-pad 1.0.3` ({CRATES_IO}) is not in xtask/deps-allowlist.txt; \
                 required by nadir-ecs"
            )]
        );
        assert_eq!(
            report.notes,
            [
                "workspace packages: 2, third-party packages: 1",
                "A third-party crate needs the user's approval first: see \
                 docs/02-dependency-policy.md.",
            ]
        );
    }

    #[test]
    fn a_listed_package_passes_only_at_the_listed_version() {
        let metadata = graph(vec![
            member("nadir-ecs").registry_dep("left-pad"),
            third_party("left-pad", "1.0.3"),
        ]);
        let approved = check(&metadata, "left-pad 1.0.3 ADR-0012\n");
        assert_eq!(approved.violations, Vec::<String>::new());

        let other_version = check(&metadata, "left-pad 1.0.2 ADR-0012\n");
        assert_eq!(
            other_version.violations,
            [format!(
                "`left-pad 1.0.3` ({CRATES_IO}) is not the approved version, 1.0.2; \
                 required by nadir-ecs"
            )]
        );

        // A longer name or version must not match by prefix.
        let similar = check(
            &metadata,
            "left-pad 1.0.30 ADR-0012\nleft-padding 1.0.3 ADR-0012\n",
        );
        assert_eq!(similar.violations.len(), 1, "{:?}", similar.violations);
    }

    #[test]
    fn two_versions_of_one_package_can_both_be_approved() {
        let metadata = graph(vec![
            member("nadir-ecs"),
            third_party("left-pad", "1.0.3"),
            third_party("left-pad", "2.0.0"),
        ]);
        let both = check(
            &metadata,
            "left-pad 1.0.3 ADR-0012\nleft-pad 2.0.0 ADR-0013\n",
        );
        assert_eq!(both.violations, Vec::<String>::new());

        let one = check(&metadata, "left-pad 1.0.3 ADR-0012\n");
        assert_eq!(
            one.violations,
            [format!(
                "`left-pad 2.0.0` ({CRATES_IO}) is not the approved version, 1.0.3"
            )]
        );
    }

    #[test]
    fn a_transitive_package_fails_without_naming_a_member() {
        let metadata = graph(vec![
            member("nadir-ecs").registry_dep("left-pad"),
            third_party("left-pad", "1.0.3").registry_dep("pad-core"),
            third_party("pad-core", "0.4.1"),
        ]);
        let report = check(&metadata, "left-pad 1.0.3 ADR-0012\n");
        assert_eq!(
            report.violations,
            [format!(
                "`pad-core 0.4.1` ({CRATES_IO}) is not in xtask/deps-allowlist.txt"
            )]
        );
    }

    #[test]
    fn a_git_package_fails() {
        let mut package = third_party("left-pad", "1.0.3");
        package.source = Some("git+https://example.com/left-pad#0123abc".to_string());
        let report = check(&graph(vec![member("nadir-ecs"), package]), "");
        assert_eq!(
            report.violations,
            [
                "`left-pad 1.0.3` (git+https://example.com/left-pad#0123abc) is not in \
              xtask/deps-allowlist.txt"
            ]
        );
    }

    #[test]
    fn a_directory_outside_the_workspace_fails_even_when_listed() {
        let mut vendored = third_party("left-pad", "1.0.3");
        vendored.source = None;
        let metadata = graph(vec![
            member("nadir-ecs").path_dep("left-pad", Kind::Normal),
            vendored,
        ]);
        let report = check(&metadata, "left-pad 1.0.3 ADR-0012\n");
        assert_eq!(
            report.violations,
            [
                "`left-pad 1.0.3` is a directory outside the workspace, which counts as \
              third-party code; required by nadir-ecs"
            ]
        );
    }

    #[test]
    fn every_member_that_asks_for_the_package_is_named() {
        let metadata = graph(vec![
            member("nadir-core"),
            member("nadir-ecs").registry_dep("left-pad"),
            member("nadir-scene").registry_dep("left-pad"),
            third_party("left-pad", "1.0.3"),
        ]);
        let report = check(&metadata, "");
        assert_eq!(report.violations.len(), 1);
        assert!(
            report.violations[0].ends_with("; required by nadir-ecs, nadir-scene"),
            "{}",
            report.violations[0]
        );
    }

    #[test]
    fn allowlist_ignores_comments_and_blank_lines() {
        let mut violations = Vec::new();
        let pins = parse_allowlist(
            "# Approved packages.\n\n  # indented comment\nleft-pad 1.0.3 ADR-0012\n\
             \tpad-core   0.4.1\tADR-7 \r\n",
            &mut violations,
        );
        assert_eq!(violations, Vec::<String>::new());
        assert_eq!(
            pins,
            [
                Pin {
                    name: "left-pad".to_string(),
                    version: "1.0.3".to_string()
                },
                Pin {
                    name: "pad-core".to_string(),
                    version: "0.4.1".to_string()
                },
            ]
        );
    }

    #[test]
    fn malformed_allowlist_lines_are_violations_with_their_line_number() {
        let text = "# header\nleft-pad 1.0.3\nleft-pad 1.0.3 ADR-0012 extra\n\
                    left-pad 1.0.3 adr-0012\nleft-pad 1.0.3 ADR-\nleft-pad 1.0.3 ADR-12a\n\
                    ok 1.0.0 ADR-0001\n";
        let mut violations = Vec::new();
        let pins = parse_allowlist(text, &mut violations);
        assert_eq!(
            violations,
            [
                "xtask/deps-allowlist.txt:2: expected `name version ADR-NNNN`, found \
                 `left-pad 1.0.3`",
                "xtask/deps-allowlist.txt:3: expected `name version ADR-NNNN`, found \
                 `left-pad 1.0.3 ADR-0012 extra`",
                "xtask/deps-allowlist.txt:4: expected `name version ADR-NNNN`, found \
                 `left-pad 1.0.3 adr-0012`",
                "xtask/deps-allowlist.txt:5: expected `name version ADR-NNNN`, found \
                 `left-pad 1.0.3 ADR-`",
                "xtask/deps-allowlist.txt:6: expected `name version ADR-NNNN`, found \
                 `left-pad 1.0.3 ADR-12a`",
            ]
        );
        assert_eq!(pins.len(), 1);
    }

    #[test]
    fn a_malformed_allowlist_fails_the_check_by_itself() {
        let report = check(&graph(vec![member("nadir-core")]), "left-pad\n");
        assert_eq!(report.violations.len(), 1);
    }
}
