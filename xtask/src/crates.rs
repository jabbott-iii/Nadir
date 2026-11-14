// Copyright 2026 Joseph Anthony Abbott III
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! `cargo xtask crates`: the crates that crates.io gets can be published as
//! they are, all at one version (ADR-0036).
//!
//! Every workspace member that crates.io gets (no `publish = false`):
//!
//! - has the description, license and repository that crates.io shows,
//!   and that it refuses a crate without (the first two);
//! - keeps copies of the workspace's [`COPIES`], `LICENSE` and `NOTICE`, in
//!   its folder, as cargo leaves them out of its package otherwise
//!   (ADR-0037);
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
use std::{fs, io};

use crate::check::Report;
use crate::error::Error;
use crate::metadata::{self, Metadata, Package};
use crate::unsafe_audit::relative;

/// The facade, whose version is the release's. Its package name, as
/// crates.io has another `nadir`.
pub(crate) const FACADE: &str = "nadir_engine";

/// The files at the workspace's root that each crate for crates.io keeps
/// a copy of. From outside a crate's folder, cargo packages only
/// `Cargo.lock` and the files that `readme` and `license-file` name. These
/// give whoever passes the crate on the license and the notice to pass on
/// with it (the license's sections 4(a) and 4(d)).
const COPIES: [&str; 2] = ["LICENSE", "NOTICE"];

/// Runs the check on the workspace at `root`.
pub(crate) fn run(root: &Path) -> Result<Report, Error> {
    let metadata = metadata::load(root)?;
    let mut report = check(&metadata);
    report.violations.extend(copies(root, &metadata)?);
    Ok(report)
}

/// Where a member of `metadata` that crates.io gets lacks a copy of one of
/// [`COPIES`], or keeps one that differs from the workspace's at `root`.
fn copies(root: &Path, metadata: &Metadata) -> Result<Vec<String>, Error> {
    let mut violations = Vec::new();
    let mut originals = Vec::new();
    for file in COPIES {
        match read(&root.join(file))? {
            Some(bytes) => originals.push((file, bytes)),
            None => violations.push(format!("the workspace has no `{file}` at its root")),
        }
    }
    let published = metadata
        .packages
        .iter()
        .filter(|package| package.is_member && package.to_crates_io);
    for package in published {
        let name = &package.name;
        let Some(dir) = &package.dir else {
            violations.push(format!("cargo did not say where `{name}` is"));
            continue;
        };
        let folder = relative(root, dir);
        for (file, original) in &originals {
            match read(&dir.join(file))? {
                None => violations.push(format!(
                    "`{name}` has no `{folder}/{file}`: cargo would leave it out of the \
                     package, so copy the workspace's `{file}` there"
                )),
                Some(copy) if copy != *original => violations.push(format!(
                    "`{name}`'s `{folder}/{file}` differs from the workspace's `{file}`: copy \
                     it again"
                )),
                Some(_) => {}
            }
        }
    }
    Ok(violations)
}

/// The bytes of the file at `path`, or `None` if there is none.
fn read(path: &Path) -> Result<Option<Vec<u8>>, Error> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
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
        // Every crate the facade needs, nadir-debug among them since the
        // facade's `dev` uses it (M7.10), with the cook; not the demo, the
        // tools, nor the benchmarks.
        assert_eq!(
            report.notes,
            [format!(
                "crates for crates.io, at {}: nadir-assets, nadir-audio, nadir-codec, \
                 nadir-collision, nadir-cook, nadir-core, nadir-debug, nadir-ecs, nadir-gfx, \
                 nadir-input, nadir-platform, nadir-render, nadir-scene, nadir-sys, nadir_engine",
                env!("CARGO_PKG_VERSION")
            )]
        );
    }

    #[test]
    fn each_crate_for_crates_io_keeps_copies_of_license_and_notice() {
        let root =
            std::env::temp_dir().join(format!("nadir-xtask-crates-copies-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for folder in ["crates/a", "crates/b", "crates/c", "xtask"] {
            fs::create_dir_all(root.join(folder)).unwrap();
        }
        let write = |path: &str, text: &str| fs::write(root.join(path), text).unwrap();
        write("LICENSE", "Apache License\n");
        write("NOTICE", "Nadir\n");
        write("crates/a/LICENSE", "Apache License\n");
        write("crates/a/NOTICE", "Nadir\n");
        write("crates/b/LICENSE", "Apache License\n");
        write("crates/c/LICENSE", "An older license\n");
        write("crates/c/NOTICE", "Nadir\n");
        let at = |mut package: Package, folder: &str| {
            package.dir = Some(root.join(folder));
            package
        };
        let metadata = Metadata {
            packages: vec![
                at(crate_("a", "0.1.0"), "crates/a"),
                at(crate_("b", "0.1.0"), "crates/b"),
                at(crate_("c", "0.1.0"), "crates/c"),
                // Not one crates.io does not get, nor a third-party crate.
                at(unpublished("xtask"), "xtask"),
                third_party("left-pad", "1.0.3"),
                crate_("nowhere", "0.1.0"),
            ],
        };
        let violations = copies(&root, &metadata).unwrap();
        fs::remove_file(root.join("NOTICE")).unwrap();
        let without_notice = copies(&root, &metadata).unwrap();
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            violations,
            [
                "`b` has no `crates/b/NOTICE`: cargo would leave it out of the package, so \
                 copy the workspace's `NOTICE` there",
                "`c`'s `crates/c/LICENSE` differs from the workspace's `LICENSE`: copy it again",
                "cargo did not say where `nowhere` is",
            ]
        );
        assert_eq!(
            without_notice,
            [
                "the workspace has no `NOTICE` at its root",
                "`c`'s `crates/c/LICENSE` differs from the workspace's `LICENSE`: copy it again",
                "cargo did not say where `nowhere` is",
            ]
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
                "`nadir-core` is at 0.1.0, but `nadir_engine` is at 0.2.0: the crates for crates.io \
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
                    "`nadir_engine` depends on `nadir-core` by `{requirement}`, not `^0.2.0`: give it \
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
                    "`nadir_engine` depends on `nadir-debug`, which crates.io does not get \
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
                ["`nadir_engine`, the facade, is not a workspace member that crates.io gets"]
            );
        }
    }
}
