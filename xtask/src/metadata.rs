//! The parts of `cargo metadata` that the `deps` and `layers` checks read.

use std::path::Path;

use nadir_codec::Limits;
use nadir_codec::json::{self, Value};

use crate::cargo;
use crate::error::Error;

/// The arguments that make cargo describe the workspace and everything it
/// depends on.
const ARGS: &[&str] = &["metadata", "--format-version", "1"];

/// The resolved package graph of the workspace.
#[derive(Debug)]
pub(crate) struct Metadata {
    /// Every package in the graph: workspace members and all they depend on.
    pub(crate) packages: Vec<Package>,
}

/// One package in the graph.
#[derive(Debug)]
pub(crate) struct Package {
    /// The package name, for example `nadir-core`.
    pub(crate) name: String,
    /// The exact version.
    pub(crate) version: String,
    /// Where the package comes from: a registry or git URL, or `None` for a
    /// directory on disk.
    pub(crate) source: Option<String>,
    /// Whether the package is a member of this workspace.
    pub(crate) is_member: bool,
    /// The dependencies the package's manifest declares, of every kind.
    pub(crate) dependencies: Vec<Dependency>,
}

/// One dependency declared in a manifest.
#[derive(Debug)]
pub(crate) struct Dependency {
    /// The name of the package depended on.
    pub(crate) name: String,
    /// Where it comes from, as for [`Package::source`].
    pub(crate) source: Option<String>,
    /// The manifest table it is declared in.
    pub(crate) kind: Kind,
}

/// The manifest table a dependency is declared in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// `[dependencies]`.
    Normal,
    /// `[dev-dependencies]`.
    Dev,
    /// `[build-dependencies]`.
    Build,
}

/// Asks cargo for the package graph of the workspace at `root`.
pub(crate) fn load(root: &Path) -> Result<Metadata, Error> {
    parse(&cargo::capture(root, ARGS)?)
}

/// Reads the output of `cargo metadata --format-version 1`.
pub(crate) fn parse(text: &str) -> Result<Metadata, Error> {
    let root = json::parse(text, Limits::default()).map_err(unexpected)?;
    let members = array(&root, "workspace_members")?
        .iter()
        .map(|id| {
            id.as_str()
                .ok_or_else(|| unexpected("a workspace member id is not a string"))
        })
        .collect::<Result<Vec<&str>, Error>>()?;

    let mut packages = Vec::new();
    for package in array(&root, "packages")? {
        let id = string(package, "id")?;
        let mut dependencies = Vec::new();
        for dependency in array(package, "dependencies")? {
            dependencies.push(Dependency {
                name: string(dependency, "name")?.to_string(),
                source: optional_string(dependency, "source")?,
                kind: kind(dependency)?,
            });
        }
        packages.push(Package {
            name: string(package, "name")?.to_string(),
            version: string(package, "version")?.to_string(),
            source: optional_string(package, "source")?,
            is_member: members.contains(&id),
            dependencies,
        });
    }

    // An empty graph would let every check pass without checking anything.
    if !packages.iter().any(|package| package.is_member) {
        return Err(unexpected("no package is a workspace member"));
    }
    Ok(Metadata { packages })
}

fn kind(dependency: &Value) -> Result<Kind, Error> {
    match optional_string(dependency, "kind")?.as_deref() {
        None => Ok(Kind::Normal),
        Some("dev") => Ok(Kind::Dev),
        Some("build") => Ok(Kind::Build),
        Some(other) => Err(unexpected(format!("unknown dependency kind `{other}`"))),
    }
}

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a Value, Error> {
    value
        .get(key)
        .ok_or_else(|| unexpected(format!("`{key}` is missing")))
}

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], Error> {
    field(value, key)?
        .as_array()
        .ok_or_else(|| unexpected(format!("`{key}` is not an array")))
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, Error> {
    field(value, key)?
        .as_str()
        .ok_or_else(|| unexpected(format!("`{key}` is not a string")))
}

/// A field that is a string or `null`.
fn optional_string(value: &Value, key: &str) -> Result<Option<String>, Error> {
    match field(value, key)? {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        _ => Err(unexpected(format!("`{key}` is not a string or null"))),
    }
}

fn unexpected(detail: impl ToString) -> Error {
    Error::UnexpectedOutput {
        command: cargo::display(ARGS),
        detail: detail.to_string(),
    }
}

/// Builders for metadata in the tests of the checks that read it.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// The source cargo reports for a crates.io package.
    pub(crate) const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";

    /// A workspace member with no dependencies.
    pub(crate) fn member(name: &str) -> Package {
        Package {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            source: None,
            is_member: true,
            dependencies: Vec::new(),
        }
    }

    /// A package from crates.io.
    pub(crate) fn third_party(name: &str, version: &str) -> Package {
        Package {
            name: name.to_string(),
            version: version.to_string(),
            source: Some(CRATES_IO.to_string()),
            is_member: false,
            dependencies: Vec::new(),
        }
    }

    impl Package {
        /// Adds a dependency on a directory on disk.
        pub(crate) fn path_dep(mut self, name: &str, kind: Kind) -> Self {
            self.dependencies.push(Dependency {
                name: name.to_string(),
                source: None,
                kind,
            });
            self
        }

        /// Adds a normal dependency on a crates.io package.
        pub(crate) fn registry_dep(mut self, name: &str) -> Self {
            self.dependencies.push(Dependency {
                name: name.to_string(),
                source: Some(CRATES_IO.to_string()),
                kind: Kind::Normal,
            });
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cut-down copy of real `cargo metadata` output: two workspace
    /// members and one registry package, with the fields the parser ignores
    /// mostly removed.
    const SAMPLE: &str = r#"{
        "packages": [
            {
                "name": "nadir-core",
                "version": "0.1.0",
                "id": "path+file:///work/nadir/crates/core#nadir-core@0.1.0",
                "source": null,
                "dependencies": [],
                "manifest_path": "/work/nadir/crates/core/Cargo.toml"
            },
            {
                "name": "nadir-codec",
                "version": "0.1.0",
                "id": "path+file:///work/nadir/crates/codec#nadir-codec@0.1.0",
                "source": null,
                "dependencies": [
                    {
                        "name": "nadir-core",
                        "source": null,
                        "req": "*",
                        "kind": "dev",
                        "optional": false,
                        "path": "/work/nadir/crates/core"
                    },
                    {
                        "name": "cc",
                        "source": "registry+https://github.com/rust-lang/crates.io-index",
                        "req": "^1",
                        "kind": "build",
                        "optional": false
                    },
                    {
                        "name": "left-pad",
                        "source": "registry+https://github.com/rust-lang/crates.io-index",
                        "req": "^1",
                        "kind": null,
                        "optional": false
                    }
                ]
            },
            {
                "name": "left-pad",
                "version": "1.0.3",
                "id": "registry+https://github.com/rust-lang/crates.io-index#left-pad@1.0.3",
                "source": "registry+https://github.com/rust-lang/crates.io-index",
                "dependencies": []
            }
        ],
        "workspace_members": [
            "path+file:///work/nadir/crates/core#nadir-core@0.1.0",
            "path+file:///work/nadir/crates/codec#nadir-codec@0.1.0"
        ],
        "workspace_root": "/work/nadir",
        "version": 1
    }"#;

    fn detail(result: Result<Metadata, Error>) -> String {
        match result {
            Err(Error::UnexpectedOutput { detail, .. }) => detail,
            other => panic!("expected an unexpected-output error, got {other:?}"),
        }
    }

    #[test]
    fn reads_packages_sources_and_membership() {
        let metadata = parse(SAMPLE).unwrap();
        let summary: Vec<_> = metadata
            .packages
            .iter()
            .map(|package| {
                (
                    package.name.as_str(),
                    package.version.as_str(),
                    package.source.as_deref(),
                    package.is_member,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("nadir-core", "0.1.0", None, true),
                ("nadir-codec", "0.1.0", None, true),
                ("left-pad", "1.0.3", Some(testing::CRATES_IO), false),
            ]
        );
    }

    #[test]
    fn reads_declared_dependencies_of_every_kind() {
        let metadata = parse(SAMPLE).unwrap();
        let dependencies: Vec<_> = metadata.packages[1]
            .dependencies
            .iter()
            .map(|dependency| {
                (
                    dependency.name.as_str(),
                    dependency.source.as_deref(),
                    dependency.kind,
                )
            })
            .collect();
        assert_eq!(
            dependencies,
            [
                ("nadir-core", None, Kind::Dev),
                ("cc", Some(testing::CRATES_IO), Kind::Build),
                ("left-pad", Some(testing::CRATES_IO), Kind::Normal),
            ]
        );
        assert!(metadata.packages[0].dependencies.is_empty());
    }

    #[test]
    fn text_that_is_not_json_is_an_error() {
        let message = detail(parse("error: could not find `Cargo.toml`"));
        assert!(
            message.starts_with("JSON error at line 1, column 1"),
            "{message}"
        );
    }

    #[test]
    fn a_missing_or_mistyped_field_is_named() {
        assert_eq!(
            detail(parse(r#"{"packages": []}"#)),
            "`workspace_members` is missing"
        );
        assert_eq!(
            detail(parse(r#"{"workspace_members": [], "packages": {}}"#)),
            "`packages` is not an array"
        );
        assert_eq!(
            detail(parse(
                r#"{"workspace_members": [], "packages": [{"id": "a", "dependencies": []}]}"#
            )),
            "`name` is missing"
        );
        assert_eq!(
            detail(parse(
                &SAMPLE.replace(r#""version": "0.1.0""#, r#""version": 1"#)
            )),
            "`version` is not a string"
        );
        assert_eq!(
            detail(parse(
                &SAMPLE.replace(r#""source": null"#, r#""source": false"#)
            )),
            "`source` is not a string or null"
        );
        assert_eq!(
            detail(parse(r#"{"workspace_members": [7], "packages": []}"#)),
            "a workspace member id is not a string"
        );
    }

    #[test]
    fn an_unknown_dependency_kind_is_an_error() {
        let text = SAMPLE.replace(r#""kind": "dev""#, r#""kind": "proc""#);
        assert_eq!(detail(parse(&text)), "unknown dependency kind `proc`");
    }

    #[test]
    fn a_graph_without_workspace_members_is_an_error() {
        assert_eq!(
            detail(parse(r#"{"workspace_members": [], "packages": []}"#)),
            "no package is a workspace member"
        );
        // Members are matched by id, so ids that match nothing count as none.
        let text = SAMPLE.replace(
            r#""id": "path+file:///work/"#,
            r#""id": "path+file:///other/"#,
        );
        assert_eq!(detail(parse(&text)), "no package is a workspace member");
    }
}
