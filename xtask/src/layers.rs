//! `cargo xtask layers`: workspace crates depend only on what the
//! dependency matrix allows.

use std::fmt;
use std::path::Path;

use crate::check::Report;
use crate::error::Error;
use crate::metadata::{self, Kind, Metadata};

/// What one workspace crate may depend on, among workspace crates.
#[derive(Debug, PartialEq, Eq)]
enum Allowed {
    /// These crates and no others.
    Only(&'static [&'static str]),
    /// Any member of the workspace.
    AnyWorkspaceCrate,
}

/// The dependency matrix, by package name. It mirrors
/// `docs/01-architecture.md` → "Dependency matrix"; a test below fails if
/// the two differ.
const MATRIX: &[(&str, Allowed)] = &[
    ("nadir-core", Allowed::Only(&[])),
    ("nadir-sys", Allowed::Only(&[])),
    (
        "nadir-platform",
        Allowed::Only(&["nadir-core", "nadir-sys"]),
    ),
    (
        "nadir-gfx",
        Allowed::Only(&["nadir-core", "nadir-sys", "nadir-platform"]),
    ),
    ("nadir-codec", Allowed::Only(&["nadir-core"])),
    ("nadir-ecs", Allowed::Only(&["nadir-core"])),
    (
        "nadir-input",
        Allowed::Only(&["nadir-core", "nadir-platform"]),
    ),
    (
        "nadir-assets",
        Allowed::Only(&["nadir-core", "nadir-codec"]),
    ),
    (
        "nadir-render",
        Allowed::Only(&["nadir-core", "nadir-gfx", "nadir-assets", "nadir-codec"]),
    ),
    (
        "nadir-audio",
        Allowed::Only(&[
            "nadir-core",
            "nadir-platform",
            "nadir-assets",
            "nadir-codec",
        ]),
    ),
    ("nadir-collision", Allowed::Only(&["nadir-core"])),
    (
        "nadir-scene",
        Allowed::Only(&["nadir-core", "nadir-ecs", "nadir-assets", "nadir-codec"]),
    ),
    (
        "nadir-debug",
        Allowed::Only(&[
            "nadir-core",
            "nadir-ecs",
            "nadir-scene",
            "nadir-render",
            "nadir-input",
        ]),
    ),
    // The facade: every crate above it.
    (
        "nadir",
        Allowed::Only(&[
            "nadir-core",
            "nadir-sys",
            "nadir-platform",
            "nadir-gfx",
            "nadir-codec",
            "nadir-ecs",
            "nadir-input",
            "nadir-assets",
            "nadir-render",
            "nadir-audio",
            "nadir-collision",
            "nadir-scene",
            "nadir-debug",
        ]),
    ),
    (
        "nadir-cook",
        Allowed::Only(&["nadir-core", "nadir-codec", "nadir-assets", "nadir-scene"]),
    ),
    ("xtask", Allowed::Only(&["nadir-core", "nadir-codec"])),
    ("nadir-bench", Allowed::AnyWorkspaceCrate),
    // `nadir-debug` only behind the `dev` feature: `release-no-dev` checks
    // that part.
    ("sandbox", Allowed::Only(&["nadir", "nadir-debug"])),
];

impl fmt::Display for Allowed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Only([]) => f.write_str("no workspace crate"),
            Self::Only(crates) => f.write_str(&crates.join(", ")),
            Self::AnyWorkspaceCrate => f.write_str("any workspace crate"),
        }
    }
}

/// Runs the check on the workspace at `root`.
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    Ok(check(&metadata::load(root)?, MATRIX))
}

/// Compares the dependencies each workspace member declares with its row of
/// `matrix`. Dependencies of every kind count, dev and build included.
fn check(metadata: &Metadata, matrix: &[(&str, Allowed)]) -> Report {
    let mut report = Report::default();
    let members: Vec<&str> = metadata
        .packages
        .iter()
        .filter(|package| package.is_member)
        .map(|package| package.name.as_str())
        .collect();

    for package in metadata.packages.iter().filter(|package| package.is_member) {
        let Some((_, allowed)) = matrix.iter().find(|(name, _)| *name == package.name) else {
            report.violations.push(format!(
                "`{}` has no row in the dependency matrix",
                package.name
            ));
            continue;
        };
        for dependency in &package.dependencies {
            // Registry and git packages are the `deps` check's business.
            if dependency.source.is_some() {
                continue;
            }
            let permitted = match allowed {
                Allowed::Only(crates) => crates.contains(&dependency.name.as_str()),
                Allowed::AnyWorkspaceCrate => members.contains(&dependency.name.as_str()),
            };
            if !permitted {
                let table = match dependency.kind {
                    Kind::Normal => "",
                    Kind::Dev => " (a dev-dependency)",
                    Kind::Build => " (a build-dependency)",
                };
                report.violations.push(format!(
                    "`{}` must not depend on `{}`{table}; its row allows: {allowed}",
                    package.name, dependency.name
                ));
            }
        }
    }

    report.notes.push(format!(
        "workspace crates checked against the dependency matrix: {}",
        members.len()
    ));
    if !report.violations.is_empty() {
        report.notes.push(
            "The matrix is in docs/01-architecture.md. A new edge is a design change: ask \
             before adding one."
                .to_string(),
        );
    }
    report
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::metadata::Package;
    use crate::metadata::testing::{member, third_party};

    fn violations(packages: Vec<Package>) -> Vec<String> {
        check(&Metadata { packages }, MATRIX).violations
    }

    #[test]
    fn dependencies_inside_the_matrix_pass() {
        let report = check(
            &Metadata {
                packages: vec![
                    member("nadir-core"),
                    member("nadir-sys"),
                    member("nadir-platform")
                        .path_dep("nadir-core", Kind::Normal)
                        .path_dep("nadir-sys", Kind::Normal),
                    member("nadir-codec").path_dep("nadir-core", Kind::Dev),
                    member("xtask")
                        .path_dep("nadir-core", Kind::Normal)
                        .path_dep("nadir-codec", Kind::Normal),
                ],
            },
            MATRIX,
        );
        assert_eq!(report.violations, Vec::<String>::new());
        assert_eq!(
            report.notes,
            ["workspace crates checked against the dependency matrix: 5"]
        );
    }

    #[test]
    fn a_dependency_outside_the_row_fails() {
        assert_eq!(
            violations(vec![
                member("nadir-core").path_dep("nadir-codec", Kind::Normal),
                member("nadir-codec"),
            ]),
            ["`nadir-core` must not depend on `nadir-codec`; its row allows: no workspace crate"]
        );
        assert_eq!(
            violations(vec![
                member("nadir-ecs")
                    .path_dep("nadir-core", Kind::Normal)
                    .path_dep("nadir-render", Kind::Normal),
                member("nadir-core"),
                member("nadir-render"),
            ]),
            ["`nadir-ecs` must not depend on `nadir-render`; its row allows: nadir-core"]
        );
    }

    #[test]
    fn dev_and_build_dependencies_count() {
        assert_eq!(
            violations(vec![
                member("nadir-core")
                    .path_dep("nadir-codec", Kind::Dev)
                    .path_dep("xtask", Kind::Build),
                member("nadir-codec"),
                member("xtask"),
            ]),
            [
                "`nadir-core` must not depend on `nadir-codec` (a dev-dependency); its row \
                 allows: no workspace crate",
                "`nadir-core` must not depend on `xtask` (a build-dependency); its row \
                 allows: no workspace crate",
            ]
        );
    }

    #[test]
    fn layering_is_one_way() {
        // Each pair is allowed in one direction only.
        for (lower, upper) in [
            ("nadir-core", "nadir-platform"),
            ("nadir-platform", "nadir-gfx"),
            ("nadir-codec", "nadir-assets"),
            ("nadir-scene", "nadir-debug"),
            ("nadir-debug", "nadir"),
            ("nadir", "sandbox"),
        ] {
            assert_eq!(
                violations(vec![
                    member(upper).path_dep(lower, Kind::Normal),
                    member(lower),
                ]),
                Vec::<String>::new(),
                "{upper} -> {lower}"
            );
            assert_eq!(
                violations(vec![
                    member(lower).path_dep(upper, Kind::Normal),
                    member(upper),
                ])
                .len(),
                1,
                "{lower} -> {upper}"
            );
        }
    }

    #[test]
    fn a_workspace_crate_without_a_row_fails() {
        assert_eq!(
            violations(vec![member("nadir-core"), member("nadir-net")]),
            ["`nadir-net` has no row in the dependency matrix"]
        );
        // Depending on it fails too: no row lists it.
        assert_eq!(
            violations(vec![
                member("nadir-ecs").path_dep("nadir-net", Kind::Normal),
                member("nadir-net"),
            ])
            .len(),
            2
        );
    }

    #[test]
    fn the_bench_may_use_any_workspace_crate_but_nothing_else() {
        let mut vendored = third_party("left-pad", "1.0.3");
        vendored.source = None;
        assert_eq!(
            violations(vec![
                member("nadir-bench")
                    .path_dep("nadir", Kind::Normal)
                    .path_dep("nadir-cook", Kind::Normal)
                    .path_dep("left-pad", Kind::Normal),
                member("nadir"),
                member("nadir-cook"),
                vendored,
            ]),
            ["`nadir-bench` must not depend on `left-pad`; its row allows: any workspace crate"]
        );
    }

    #[test]
    fn registry_dependencies_are_left_to_the_deps_check() {
        assert_eq!(
            violations(vec![
                member("nadir-core").registry_dep("left-pad"),
                third_party("left-pad", "1.0.3"),
            ]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn packages_outside_the_workspace_are_not_checked() {
        // A third-party package may depend on whatever it likes, and needs
        // no row.
        assert_eq!(
            violations(vec![
                member("nadir-core"),
                third_party("left-pad", "1.0.3").path_dep("nadir-core", Kind::Normal),
            ]),
            Vec::<String>::new()
        );
    }

    /// Reads the matrix table out of `docs/01-architecture.md`, in the form
    /// of [`MATRIX`] with owned strings: `None` stands for
    /// [`Allowed::AnyWorkspaceCrate`].
    fn documented_matrix() -> Vec<(String, Option<Vec<String>>)> {
        /// The names between backticks in one table cell.
        fn names(cell: &str) -> Vec<String> {
            cell.split('`')
                .skip(1)
                .step_by(2)
                // The document shortens `nadir-core` to `core` in the
                // right-hand column.
                .map(|name| {
                    if name.starts_with("nadir") {
                        name.to_string()
                    } else {
                        format!("nadir-{name}")
                    }
                })
                .collect()
        }

        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/01-architecture.md");
        let text = fs::read_to_string(path).unwrap();
        let section = text
            .split("\n## Dependency matrix\n")
            .nth(1)
            .expect("the document has a `Dependency matrix` section");
        let section = section.split("\n## ").next().unwrap();

        let mut rows: Vec<(String, Option<Vec<String>>)> = Vec::new();
        // Skip the header row and the `| :- | :- |` row below it.
        for line in section.lines().filter(|line| line.starts_with('|')).skip(2) {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            let [_, name, allowed, _] = cells.as_slice() else {
                panic!("unexpected table row: {line}");
            };
            let name = name.split('`').nth(1).unwrap().to_string();
            let allowed = match *allowed {
                "—" => Some(Vec::new()),
                "all crates above" => Some(rows.iter().map(|(name, _)| name.clone()).collect()),
                "any workspace crate" => None,
                cell => Some(names(&cell.replace("(behind `dev` feature)", ""))),
            };
            rows.push((name, allowed));
        }
        rows
    }

    #[test]
    fn the_matrix_mirrors_the_architecture_document() {
        let in_code: Vec<(String, Option<Vec<String>>)> = MATRIX
            .iter()
            .map(|(name, allowed)| {
                let allowed = match allowed {
                    Allowed::Only(crates) => {
                        Some(crates.iter().map(|name| name.to_string()).collect())
                    }
                    Allowed::AnyWorkspaceCrate => None,
                };
                (name.to_string(), allowed)
            })
            .collect();
        let documented = documented_matrix();
        assert_eq!(documented.len(), 18, "{documented:?}");
        assert_eq!(in_code, documented);
    }

    #[test]
    fn allowed_sets_are_described_in_words() {
        assert_eq!(Allowed::Only(&[]).to_string(), "no workspace crate");
        assert_eq!(
            Allowed::Only(&["nadir-core", "nadir-sys"]).to_string(),
            "nadir-core, nadir-sys"
        );
        assert_eq!(
            Allowed::AnyWorkspaceCrate.to_string(),
            "any workspace crate"
        );
    }
}
