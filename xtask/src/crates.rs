//! `cargo xtask crates`: the crates that crates.io gets can be published as
//! they are, all at one version (ADR-0036).
//!
//! Every workspace member that crates.io gets (no `publish = false`):
//!
//! - has the description, license and repository that crates.io shows,
//!   and that it refuses a crate without (the first two);
//! - is at the facade's version, [`FACADE`]'s: a release publishes them
//!   all at the version of its tag;
//! - depends on a workspace crate only if crates.io gets that crate too,
//!   and only by a requirement of that version, `^X.Y.Z`. A crate of one
//!   release then never builds against the crates of an older one, which
//!   crates.io would still have.
//!
//! The release workflow publishes them with `cargo publish --workspace`,
//! which leaves out the others and orders the rest by their dependencies.

use std::path::Path;

use crate::check::Report;
use crate::error::Error;
use crate::metadata::{self, Metadata, Package};

/// The facade, whose version is the release's. Its package name, as
/// crates.io has another `nadir`.
pub(crate) const FACADE: &str = "NadirGE";

/// Runs the check on the workspace at `root`.
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    Ok(check(&metadata::load(root)?))
}

/// Checks the members of `metadata` that crates.io gets.
fn check(metadata: &Metadata) -> Report {
    let mut report = Report::default();
    let members: Vec<&Package> = metadata
        .packages
        .iter()
        .filter(|package| package.is_member)
        .collect();
    let published: Vec<&Package> = members
        .iter()
        .copied()
        .filter(|package| package.to_crates_io)
        .collect();
    let Some(facade) = published.iter().find(|package| package.name == FACADE) else {
        report.violations.push(format!(
            "`{FACADE}`, the facade, is not a workspace member that crates.io gets"
        ));
        return report;
    };
    let version = &facade.version;
    let mut names: Vec<&str> = published
        .iter()
        .map(|package| package.name.as_str())
        .collect();
    names.sort_unstable();
    report.notes.push(format!(
        "crates for crates.io, at {version}: {}",
        names.join(", ")
    ));

    for package in &published {
        let name = &package.name;
        for (field, value) in [
            ("description", &package.about.description),
            ("license", &package.about.license),
            ("repository", &package.about.repository),
        ] {
            if value.as_deref().is_none_or(|value| value.trim().is_empty()) {
                report.violations.push(format!(
                    "`{name}` has no `{field}`, which crates.io shows: inherit it with \
                     `{field}.workspace = true` or give its own"
                ));
            }
        }
        if package.version != *version {
            report.violations.push(format!(
                "`{name}` is at {}, but `{FACADE}` is at {version}: the crates for crates.io \
                 share the workspace's version",
                package.version
            ));
        }
        for dependency in &package.dependencies {
            let Some(target) = members
                .iter()
                .find(|member| member.name == dependency.name && dependency.source.is_none())
            else {
                continue;
            };
            if !target.to_crates_io {
                report.violations.push(format!(
                    "`{name}` depends on `{}`, which crates.io does not get (`publish = false`)",
                    target.name
                ));
                continue;
            }
            let wanted = format!("^{version}");
            if dependency.requirement != wanted {
                report.violations.push(format!(
                    "`{name}` depends on `{}` by `{}`, not `{wanted}`: give it the release's \
                     version in `[workspace.dependencies]`",
                    target.name, dependency.requirement
                ));
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::testing::{member, third_party};
    use crate::metadata::{About, Kind};

    /// A member crates.io gets, with what it shows, at `version`.
    fn crate_(name: &str, version: &str) -> Package {
        let mut package = member(name);
        package.version = version.to_string();
        package.about = About {
            description: Some(format!("The {name} crate.")),
            license: Some("Apache-2.0".to_string()),
            repository: Some("https://github.com/jabbott-iii/Nadir".to_string()),
            ..About::default()
        };
        package
    }

    /// A member crates.io does not get.
    fn unpublished(name: &str) -> Package {
        let mut package = member(name);
        package.to_crates_io = false;
        package
    }

    trait Requiring {
        /// Adds a dependency on the member `name` of `kind`, by `requirement`.
        fn requiring(self, name: &str, requirement: &str, kind: Kind) -> Self;
    }

    impl Requiring for Package {
        fn requiring(mut self, name: &str, requirement: &str, kind: Kind) -> Self {
            self = self.path_dep(name, kind);
            if let Some(dependency) = self.dependencies.last_mut() {
                dependency.requirement = requirement.to_string();
            }
            self
        }
    }

    fn violations(packages: Vec<Package>) -> Vec<String> {
        check(&Metadata { packages }).violations
    }

    #[test]
    fn the_workspace_s_crates_can_be_published() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let report = run(&root).expect("cargo metadata runs");
        assert_eq!(report.violations, Vec::<String>::new());
        // Every crate the facade needs, with the cook; not the demo, the
        // tools, the benchmarks, nor the crates with no code yet.
        assert_eq!(
            report.notes,
            [format!(
                "crates for crates.io, at {}: NadirGE, nadir-assets, nadir-codec, \
                 nadir-collision, nadir-cook, nadir-core, nadir-ecs, nadir-gfx, nadir-input, \
                 nadir-platform, nadir-render, nadir-scene, nadir-sys",
                env!("CARGO_PKG_VERSION")
            )]
        );
    }

    #[test]
    fn crates_at_one_version_with_their_metadata_pass() {
        assert_eq!(
            violations(vec![
                crate_("nadir-core", "0.2.0"),
                crate_(FACADE, "0.2.0")
                    .requiring("nadir-core", "^0.2.0", Kind::Normal)
                    .requiring("nadir-core", "^0.2.0", Kind::Dev),
                unpublished("sandbox").requiring(FACADE, "*", Kind::Normal),
                // A third-party crate is the `deps` check's.
                third_party("left-pad", "1.0.3"),
            ]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_crate_without_what_crates_io_shows_fails() {
        let mut bare = crate_("nadir-core", "0.1.0");
        bare.about = About {
            description: Some("  ".to_string()),
            ..About::default()
        };
        assert_eq!(
            violations(vec![bare, crate_(FACADE, "0.1.0")]),
            [
                "`nadir-core` has no `description`, which crates.io shows: inherit it with \
                 `description.workspace = true` or give its own",
                "`nadir-core` has no `license`, which crates.io shows: inherit it with \
                 `license.workspace = true` or give its own",
                "`nadir-core` has no `repository`, which crates.io shows: inherit it with \
                 `repository.workspace = true` or give its own",
            ]
        );
        // Not one crates.io does not get.
        assert_eq!(
            violations(vec![unpublished("xtask"), crate_(FACADE, "0.1.0")]),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_crate_at_another_version_fails() {
        assert_eq!(
            violations(vec![crate_("nadir-core", "0.1.0"), crate_(FACADE, "0.2.0")]),
            [
                "`nadir-core` is at 0.1.0, but `NadirGE` is at 0.2.0: the crates for crates.io \
              share the workspace's version"
            ]
        );
    }

    #[test]
    fn a_dependency_on_an_older_release_fails() {
        for (requirement, kind) in [
            ("^0.1.0", Kind::Normal),
            ("*", Kind::Normal),
            ("=0.2.0", Kind::Build),
            ("^0.1", Kind::Dev),
        ] {
            assert_eq!(
                violations(vec![
                    crate_("nadir-core", "0.2.0"),
                    crate_(FACADE, "0.2.0").requiring("nadir-core", requirement, kind),
                ]),
                [format!(
                    "`NadirGE` depends on `nadir-core` by `{requirement}`, not `^0.2.0`: give it \
                     the release's version in `[workspace.dependencies]`"
                )],
                "{requirement}"
            );
        }
    }

    #[test]
    fn a_dependency_on_a_crate_crates_io_does_not_get_fails() {
        for kind in [Kind::Normal, Kind::Dev] {
            assert_eq!(
                violations(vec![
                    unpublished("nadir-debug"),
                    crate_(FACADE, "0.1.0").requiring("nadir-debug", "^0.1.0", kind),
                ]),
                [
                    "`NadirGE` depends on `nadir-debug`, which crates.io does not get \
                  (`publish = false`)"
                ]
            );
        }
    }

    #[test]
    fn without_the_facade_there_is_no_version_to_check() {
        for packages in [
            vec![crate_("nadir-core", "0.1.0")],
            vec![crate_("nadir-core", "0.1.0"), unpublished(FACADE)],
        ] {
            assert_eq!(
                violations(packages),
                ["`NadirGE`, the facade, is not a workspace member that crates.io gets"]
            );
        }
    }
}
