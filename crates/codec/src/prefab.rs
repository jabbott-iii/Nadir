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

//! The prefab file, `data/prefabs/<name>.kbj`, which the cook writes from
//! `assets/data/prefabs/<name>.jsonc` (`docs/subsystems/assets-and-codecs.md`,
//! "Prefab file"; ADR-0038): a prefab's name, the prefab it extends, and
//! its components' values. The game reads it with [`parse`]; the cook
//! checks its source with [`read`].
//!
//! ```text
//! { "name": "slime",
//!   "extends": "enemy_base",
//!   "components": { "Health": { "current": 12, "max": 12 },
//!                   "Animator": { "clip": "enemies/slime/idle" } } }
//! ```
//!
//! It is KBJ, read with the KBJ decoder and its limits, then checked:
//!
//! - **Members:** `name` and `components`, and `extends` if the prefab has
//!   a base; no others.
//! - **Names:** a prefab's name, and the one it extends, are lowercase
//!   ASCII letters, digits and `_` ([`is_name`]); a prefab does not extend
//!   itself. That the name is its file's stem, and the rest of the chain,
//!   are nadir-scene's to check.
//! - **Components:** an object of type names, each an ASCII letter then
//!   letters, digits or `_` ([`is_component_name`]), each with a value
//!   that is not `null`: version 1 has no way to remove a base's component.
//!   What the values mean is checked where the game reflects them.
//!
//! A problem is a [`CodecError::Prefab`], which names where it is, such as
//! `components.Health`.

use std::fmt;

use crate::json::Value;
use crate::{CodecError, Limits, kbj};

/// A prefab file: a prefab's name, its base, and its components' values.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct PrefabFile {
    /// Its name, which [`is_name`] takes.
    pub name: String,
    /// The prefab it extends, if any: another name, not its own.
    pub extends: Option<String>,
    /// Its components' values, by type name, in the file's order.
    pub components: Vec<(String, Value)>,
}

/// What is wrong with a prefab file. See [`CodecError::Prefab`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// A value is not of the type its member needs.
    WrongType {
        /// What it should be: "an object", "a string" and so on.
        expected: &'static str,
    },
    /// An object lacks a member.
    MissingMember {
        /// The member.
        name: &'static str,
    },
    /// An object has a member the format does not have.
    UnknownMember {
        /// The member.
        name: String,
    },
    /// A prefab's name is not lowercase letters, digits and `_`.
    BadName,
    /// A component's type name is not a letter then letters, digits and
    /// `_`.
    BadComponentName,
    /// A component's value is `null`.
    NullComponent,
    /// A prefab extends itself.
    ExtendsItself,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongType { expected } => write!(f, "expected {expected}"),
            Self::MissingMember { name } => write!(f, "`{name}` is missing"),
            Self::UnknownMember { name } => write!(f, "`{name}` is not a member it has"),
            Self::BadName => write!(
                f,
                "a prefab's name is lowercase letters, digits and `_`, one at least"
            ),
            Self::BadComponentName => write!(
                f,
                "a component's name is a letter, then letters, digits and `_`"
            ),
            Self::NullComponent => write!(
                f,
                "a component's value is not `null`: a prefab cannot remove its base's"
            ),
            Self::ExtendsItself => write!(f, "a prefab does not extend itself"),
        }
    }
}

/// Whether `text` is a prefab's name: one or more lowercase ASCII letters,
/// digits and `_`, as a file's stem below `data/prefabs/` and an LDtk
/// entity's identifier in snake_case are (ADR-0038).
pub fn is_name(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Whether `text` is a component's type name, as `FromValue::TYPE_NAME`
/// gives it: an ASCII letter, then ASCII letters, digits and `_`.
pub fn is_component_name(text: &str) -> bool {
    let mut bytes = text.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Reads a prefab file: KBJ, decoded within `limits`, then checked as the
/// module says.
///
/// # Errors
///
/// [`CodecError::Kbj`] if the bytes are not KBJ, and
/// [`CodecError::Prefab`], naming where, if they are not a prefab file.
pub fn parse(bytes: &[u8], limits: Limits) -> Result<PrefabFile, CodecError> {
    read(&kbj::decode(bytes, limits)?)
}

/// Reads a prefab file from its value, such as its source's JSON, and
/// checks it as the module says.
///
/// # Errors
///
/// [`CodecError::Prefab`], naming where, if the value is not a prefab
/// file.
pub fn read(value: &Value) -> Result<PrefabFile, CodecError> {
    let error = |at: &str, kind| CodecError::Prefab {
        at: at.to_string(),
        kind,
    };
    let members = value.as_object().ok_or_else(|| {
        error(
            "",
            ErrorKind::WrongType {
                expected: "an object",
            },
        )
    })?;
    if let Some((name, _)) = members
        .iter()
        .find(|(name, _)| !["name", "extends", "components"].contains(&name.as_str()))
    {
        return Err(error("", ErrorKind::UnknownMember { name: name.clone() }));
    }
    let get = |name: &str| {
        members
            .iter()
            .find(|(member, _)| member == name)
            .map(|(_, value)| value)
    };
    let prefab_name = |at: &str, value: &Value| match value.as_str() {
        Some(text) if is_name(text) => Ok(text.to_string()),
        Some(_) => Err(error(at, ErrorKind::BadName)),
        None => Err(error(
            at,
            ErrorKind::WrongType {
                expected: "a string",
            },
        )),
    };

    let name = get("name").ok_or_else(|| error("", ErrorKind::MissingMember { name: "name" }))?;
    let name = prefab_name("name", name)?;
    let extends = match get("extends") {
        Some(value) => {
            let base = prefab_name("extends", value)?;
            if base == name {
                return Err(error("extends", ErrorKind::ExtendsItself));
            }
            Some(base)
        }
        None => None,
    };
    let components = get("components")
        .ok_or_else(|| error("", ErrorKind::MissingMember { name: "components" }))?;
    let components = components.as_object().ok_or_else(|| {
        error(
            "components",
            ErrorKind::WrongType {
                expected: "an object",
            },
        )
    })?;
    for (component, value) in components {
        let at = format!("components.{component}");
        if !is_component_name(component) {
            return Err(error(&at, ErrorKind::BadComponentName));
        }
        if value.is_null() {
            return Err(error(&at, ErrorKind::NullComponent));
        }
    }
    Ok(PrefabFile {
        name,
        extends,
        components: components.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    fn value(text: &str) -> Value {
        json::parse(text, Limits::default()).expect("the test's JSON")
    }

    /// The error `text` gives, read from KBJ: where, and what.
    fn error(text: &str) -> (String, ErrorKind) {
        let bytes = kbj::encode(&value(text), Limits::default()).expect("encodable");
        match parse(&bytes, Limits::default()) {
            Err(CodecError::Prefab { at, kind }) => (at, kind),
            other => panic!("expected a prefab error, got {other:?}"),
        }
    }

    const SLIME: &str = r#"{ "name": "slime", "extends": "enemy_base",
        "components": { "Health": { "current": 12, "max": 12 },
                        "Animator": { "clip": "enemies/slime/idle" } } }"#;

    #[test]
    fn a_prefab_is_read_from_kbj_and_from_its_source() {
        let bytes = kbj::encode(&value(SLIME), Limits::default()).expect("encodable");
        let prefab = parse(&bytes, Limits::default()).expect("a prefab");
        assert_eq!(prefab.name, "slime");
        assert_eq!(prefab.extends.as_deref(), Some("enemy_base"));
        let names: Vec<&str> = prefab
            .components
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, ["Health", "Animator"], "in the file's order");
        assert_eq!(
            prefab.components[0].1.get("max").and_then(Value::as_i64),
            Some(12)
        );
        assert_eq!(read(&value(SLIME)).expect("a prefab"), prefab);
        // No base, and no components, are a prefab too.
        let bare = read(&value(r#"{ "name": "marker_2", "components": {} }"#)).expect("a prefab");
        assert_eq!(bare.extends, None);
        assert!(bare.components.is_empty());
    }

    #[test]
    fn each_rule_is_checked_and_names_where() {
        let cases: [(&str, &str, ErrorKind); 11] = [
            (
                "[]",
                "",
                ErrorKind::WrongType {
                    expected: "an object",
                },
            ),
            (
                r#"{ "components": {} }"#,
                "",
                ErrorKind::MissingMember { name: "name" },
            ),
            (
                r#"{ "name": "a" }"#,
                "",
                ErrorKind::MissingMember { name: "components" },
            ),
            (
                r#"{ "name": "a", "components": {}, "tags": [] }"#,
                "",
                ErrorKind::UnknownMember {
                    name: "tags".to_string(),
                },
            ),
            (
                r#"{ "name": "Slime", "components": {} }"#,
                "name",
                ErrorKind::BadName,
            ),
            (
                r#"{ "name": "", "components": {} }"#,
                "name",
                ErrorKind::BadName,
            ),
            (
                r#"{ "name": 3, "components": {} }"#,
                "name",
                ErrorKind::WrongType {
                    expected: "a string",
                },
            ),
            (
                r#"{ "name": "a", "extends": "enemies/base", "components": {} }"#,
                "extends",
                ErrorKind::BadName,
            ),
            (
                r#"{ "name": "a", "extends": "a", "components": {} }"#,
                "extends",
                ErrorKind::ExtendsItself,
            ),
            (
                r#"{ "name": "a", "components": { "Health": null } }"#,
                "components.Health",
                ErrorKind::NullComponent,
            ),
            (
                r#"{ "name": "a", "components": { "2D": {} } }"#,
                "components.2D",
                ErrorKind::BadComponentName,
            ),
        ];
        for (text, at, kind) in cases {
            assert_eq!(error(text), (at.to_string(), kind), "{text}");
        }
        assert_eq!(
            error(r#"{ "name": "a", "components": [] }"#),
            (
                "components".to_string(),
                ErrorKind::WrongType {
                    expected: "an object"
                }
            )
        );
        assert!(matches!(
            parse(b"not KBJ", Limits::default()),
            Err(CodecError::Kbj { .. })
        ));
    }

    #[test]
    fn names_are_as_files_and_types_have_them() {
        for name in ["slime", "enemy_base", "tree2", "_", "2x"] {
            assert!(is_name(name), "{name}");
        }
        for name in ["", "Slime", "enemy-base", "a/b", "a.b", "é"] {
            assert!(!is_name(name), "{name}");
        }
        for name in ["Health", "Sprite", "Hp_2", "a"] {
            assert!(is_component_name(name), "{name}");
        }
        for name in ["", "2D", "_Hidden", "nadir::Sprite", "Health Bar"] {
            assert!(!is_component_name(name), "{name}");
        }
    }

    #[test]
    fn errors_say_where_and_what() {
        let error = CodecError::Prefab {
            at: "components.Health".to_string(),
            kind: ErrorKind::NullComponent,
        };
        assert_eq!(
            error.to_string(),
            "prefab error at `components.Health`: a component's value is not `null`: a prefab cannot remove its base's"
        );
        let error = CodecError::Prefab {
            at: String::new(),
            kind: ErrorKind::MissingMember { name: "name" },
        };
        assert_eq!(
            error.to_string(),
            "prefab error in the file: `name` is missing"
        );
    }
}
