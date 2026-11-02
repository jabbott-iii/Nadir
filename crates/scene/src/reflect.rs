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

//! [`Reflect`] and [`FromValue`]: walking a value's fields by name, and
//! turning it into a [`Value`] and back.

use std::any::Any;
use std::borrow::Cow;
use std::error::Error;
use std::fmt;

use nadir_assets::{AssetServer, Scope};
use nadir_codec::json::Value;
use nadir_core::log::warn;

/// A value whose fields can be walked by name, and that turns into a
/// [`Value`] and back: what saves, prefabs, the inspector and settings
/// need. The macros [`reflect_struct!`](crate::reflect_struct),
/// [`reflect_enum!`](crate::reflect_enum) and
/// [`reflect_newtype!`](crate::reflect_newtype) implement it; so does this
/// crate for the built-in types (`docs/subsystems/scene.md`).
///
/// Both directions take a [`ReflectContext`]: it names the source being
/// read, keeps the path to the field being read, and collects warnings
/// (ADR-0020).
pub trait Reflect: 'static {
    /// The type's name: for the macros' types, its name as written
    /// (`Health`), which prefabs and saves use.
    fn type_name(&self) -> &'static str;

    /// What sort of value it is.
    fn kind(&self) -> ReflectKind;

    /// The fields of a struct, in order; none for other kinds.
    fn fields(&self) -> &'static [FieldInfo];

    /// Field `index` of a struct, or element `index` of a list or an
    /// option.
    fn field(&self, index: usize) -> Option<&dyn Reflect>;

    /// Field or element `index`, to change it.
    fn field_mut(&mut self, index: usize) -> Option<&mut dyn Reflect>;

    /// The value as a [`Value`].
    fn to_value(&self, cx: &mut ReflectContext<'_>) -> Value;

    /// Sets the value from `value`. A field that `value` does not name
    /// keeps its value; a field it names that the type does not have is a
    /// warning. On an error, what was applied before it stays applied.
    ///
    /// # Errors
    ///
    /// If `value` does not fit the type: a [`ReflectError`] naming the
    /// source and the path to the field.
    fn apply_value(
        &mut self,
        value: &Value,
        cx: &mut ReflectContext<'_>,
    ) -> Result<(), ReflectError>;
}

/// A reflected type that can be made from a [`Value`]: for a struct, its
/// `Default` with the value applied.
pub trait FromValue: Reflect + Sized {
    /// The type's name, as [`Reflect::type_name`] gives it.
    const TYPE_NAME: &'static str;

    /// Makes a value of the type from `value`.
    ///
    /// # Errors
    ///
    /// As [`Reflect::apply_value`].
    fn from_value(value: &Value, cx: &mut ReflectContext<'_>) -> Result<Self, ReflectError>;
}

/// What sort of value a [`Reflect`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReflectKind {
    /// A struct with named fields.
    Struct,
    /// An enum with unit variants, written as its variant's name.
    Enum,
    /// A number, a boolean, a string, or a value type such as `Vec2`.
    Primitive,
    /// A list: `Vec<T>`.
    List,
    /// An `Option<T>`.
    Option,
    /// An asset handle, written as the asset's path (ADR-0020, ADR-0038).
    Handle,
}

/// One field of a reflected struct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldInfo {
    /// Its name, which data uses.
    pub name: &'static str,
    /// Its type as written in the struct, for display.
    pub type_name: &'static str,
    /// Its doc comment, line by line as written, each line ending in a
    /// newline, and each `///` line keeping the space after the slashes.
    /// Empty if it has none.
    pub doc: &'static str,
}

/// One step of the path to a field, for messages.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Segment {
    Field(&'static str),
    /// A member whose name the data gives, such as a prefab's component.
    Key(String),
    Index(usize),
    /// A place given as text, where a value begins in its source:
    /// `components.Health`.
    Origin(String),
}

/// The asset server a context reads handles with, if it has one.
enum Assets<'a> {
    None,
    /// To write handles as their paths: hashing and saving never load.
    Shared(&'a AssetServer),
    /// To read them too, each loaded in the scope.
    Loading(&'a mut AssetServer, Scope),
}

/// What reading or writing reflected values needs besides the values:
/// the name of the source, for messages, the path to the field at hand,
/// and the warnings so far; and what some types need to read or write
/// themselves (ADR-0038):
///
/// - the asset server, which an asset [`Handle`](nadir_core::Handle) is
///   written and read through, as its asset's path:
///   [`ReflectContext::with_assets`] to write handles,
///   [`ReflectContext::loading`] to read them too;
/// - resources of the types that need them, such as the facade's clips and
///   collision layers: [`ReflectContext::with_resource`];
/// - whether the values are for a hash, which leaves out what changes each
///   frame: [`ReflectContext::hashing`].
pub struct ReflectContext<'a> {
    source: Cow<'a, str>,
    path: Vec<Segment>,
    warnings: Vec<ReflectWarning>,
    assets: Assets<'a>,
    resources: Vec<&'a dyn Any>,
    hashing: bool,
}

impl<'a> ReflectContext<'a> {
    /// A context for reading or writing `source`: a file's path, or any
    /// name that tells the reader where the data came from. It has no asset
    /// server and no resources, and is not for a hash.
    pub fn new(source: &'a str) -> Self {
        Self {
            source: Cow::Borrowed(source),
            path: Vec::new(),
            warnings: Vec::new(),
            assets: Assets::None,
            resources: Vec::new(),
            hashing: false,
        }
    }

    /// This context with `server`, to write asset handles as their paths.
    /// Reading a handle needs [`ReflectContext::loading`].
    pub fn with_assets(mut self, server: &'a AssetServer) -> Self {
        self.assets = Assets::Shared(server);
        self
    }

    /// This context with `server`, to read asset handles, each loaded in
    /// `scope`, as well as to write them.
    pub fn loading(mut self, server: &'a mut AssetServer, scope: Scope) -> Self {
        self.assets = Assets::Loading(server, scope);
        self
    }

    /// This context with `resource`, for the types whose values need it:
    /// [`ReflectContext::resource`] finds it by its type. One of a type
    /// given before is replaced.
    pub fn with_resource<T: Any>(mut self, resource: &'a T) -> Self {
        self.resources.retain(|known| !known.is::<T>());
        self.resources.push(resource);
        self
    }

    /// This context for a hash: [`ReflectContext::is_hashing`] is true, so
    /// that a type leaves out what the engine rewrites each frame, such as
    /// the facade's `Sprite` frame an animator shows. A context for a hash
    /// never loads an asset.
    pub fn hashing(mut self) -> Self {
        self.hashing = true;
        self
    }

    /// Whether the values are for a hash: see [`ReflectContext::hashing`].
    pub fn is_hashing(&self) -> bool {
        self.hashing
    }

    /// The asset server, to write a handle as its asset's path, if the
    /// context has one.
    pub fn assets(&self) -> Option<&AssetServer> {
        match &self.assets {
            Assets::None => None,
            Assets::Shared(server) => Some(server),
            Assets::Loading(server, _) => Some(server),
        }
    }

    /// The asset server, to load an asset, and the scope to load it in, if
    /// the context is [`ReflectContext::loading`].
    pub fn assets_mut(&mut self) -> Option<(&mut AssetServer, Scope)> {
        match &mut self.assets {
            Assets::Loading(server, scope) => Some((server, *scope)),
            Assets::None | Assets::Shared(_) => None,
        }
    }

    /// The resource of type `T` given with [`ReflectContext::with_resource`],
    /// if there is one.
    pub fn resource<T: Any>(&self) -> Option<&T> {
        self.resources
            .iter()
            .find_map(|resource| resource.downcast_ref::<T>())
    }

    /// The name of the source.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Runs `f` with `source` as the source, for a value read from another
    /// file than the rest, such as a prefab's base, and puts the source
    /// back again, whatever `f` returns. The path is kept.
    pub fn with_source<R>(&mut self, source: &str, f: impl FnOnce(&mut Self) -> R) -> R {
        let outer = std::mem::replace(&mut self.source, Cow::Owned(source.to_string()));
        let result = f(self);
        self.source = outer;
        result
    }

    /// Runs `f` with `source` as the source and `at` as the path, for a
    /// value read from where it is in another file, such as a prefab's
    /// `components.Health`, and puts both back again, whatever `f` returns.
    /// An empty `at` is the source's root.
    pub fn with_origin<R>(&mut self, source: &str, at: &str, f: impl FnOnce(&mut Self) -> R) -> R {
        let path = if at.is_empty() {
            Vec::new()
        } else {
            vec![Segment::Origin(at.to_string())]
        };
        let outer_path = std::mem::replace(&mut self.path, path);
        let result = self.with_source(source, f);
        self.path = outer_path;
        result
    }

    /// The path to the field at hand, as text, as errors give it:
    /// `entities[2].overrides`.
    pub fn at(&self) -> String {
        self.path()
    }

    /// The warnings so far, in order.
    pub fn warnings(&self) -> &[ReflectWarning] {
        &self.warnings
    }

    /// Runs `f` with field `name` added to the path, and takes it off
    /// again, whatever `f` returns.
    pub fn field<R>(&mut self, name: &'static str, f: impl FnOnce(&mut Self) -> R) -> R {
        self.path.push(Segment::Field(name));
        let result = f(self);
        self.path.pop();
        result
    }

    /// Runs `f` with member `key`, whose name the data gives, added to the
    /// path, as [`ReflectContext::field`]: a prefab's component, say.
    pub fn key<R>(&mut self, key: &str, f: impl FnOnce(&mut Self) -> R) -> R {
        self.path.push(Segment::Key(key.to_string()));
        let result = f(self);
        self.path.pop();
        result
    }

    /// Runs `f` with element `index` added to the path, as
    /// [`ReflectContext::field`].
    pub fn index<R>(&mut self, index: usize, f: impl FnOnce(&mut Self) -> R) -> R {
        self.path.push(Segment::Index(index));
        let result = f(self);
        self.path.pop();
        result
    }

    /// An error at the current path.
    pub fn error(&self, kind: ReflectErrorKind) -> ReflectError {
        ReflectError {
            source: self.source.to_string(),
            path: self.path(),
            kind,
        }
    }

    /// Logs a warning about the value at the current path, which writing it
    /// could not avoid: a handle the asset server does not know, say,
    /// written as `null`.
    pub fn warn(&self, message: &str) {
        let path = self.path();
        if path.is_empty() {
            warn!("{}: {message}", self.source);
        } else {
            warn!("{}: at `{path}`: {message}", self.source);
        }
    }

    /// The error for `found` where `expected` was due, at the current path.
    pub fn expected(&self, expected: &'static str, found: &Value) -> ReflectError {
        self.error(ReflectErrorKind::Expected {
            expected,
            found: describe(found),
        })
    }

    /// Notes, and logs, that the data gave `field`, which `type_name` has
    /// not got.
    pub fn unknown_field(&mut self, type_name: &'static str, field: &str) {
        let warning = ReflectWarning {
            source: self.source.to_string(),
            type_name,
            field: field.to_string(),
            path: self.path(),
        };
        warn!("{warning}");
        self.warnings.push(warning);
    }

    /// The path as text: `components.Health.max`, `layers[2]`.
    fn path(&self) -> String {
        let mut path = String::new();
        for segment in &self.path {
            match segment {
                Segment::Field(name) => {
                    if !path.is_empty() {
                        path.push('.');
                    }
                    path.push_str(name);
                }
                Segment::Key(name) | Segment::Origin(name) => {
                    if !path.is_empty() {
                        path.push('.');
                    }
                    path.push_str(name);
                }
                Segment::Index(index) => {
                    path.push('[');
                    path.push_str(&index.to_string());
                    path.push(']');
                }
            }
        }
        path
    }
}

/// How a [`Value`] is described in messages: "a string", "null".
fn describe(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Int(_) => "an integer",
        Value::Float(_) => "a number that is not an integer an i64 can hold",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// A field that data gave and its type has not got: kept out of the value,
/// and never silently.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReflectWarning {
    /// Where the data came from.
    pub source: String,
    /// The type that has no such field.
    pub type_name: &'static str,
    /// The field the data gave.
    pub field: String,
    /// The path to the value of that type.
    pub path: String,
}

impl fmt::Display for ReflectWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: `{}` has no field `{}`, so it was left out",
            self.source, self.type_name, self.field
        )?;
        if !self.path.is_empty() {
            write!(f, " (at `{}`)", self.path)?;
        }
        Ok(())
    }
}

/// A value that does not fit its type.
#[derive(Clone, Debug, PartialEq)]
pub struct ReflectError {
    /// Where the data came from.
    pub source: String,
    /// The path to the value: `Health.max`, `layers[2]`; empty for the
    /// value itself.
    pub path: String,
    /// What is wrong.
    pub kind: ReflectErrorKind,
}

/// What is wrong with a value: see [`ReflectError`].
///
/// Non-exhaustive (ADR-0038): types that need more of the context than a
/// value, such as handles, brought more ways to fail.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ReflectErrorKind {
    /// The value is of another sort.
    Expected {
        /// What the type needs: "an integer".
        expected: &'static str,
        /// What the value is.
        found: &'static str,
    },
    /// A number the type cannot hold.
    OutOfRange {
        /// The type.
        type_name: &'static str,
        /// The number, as written.
        value: String,
    },
    /// A name that is none of the enum's variants.
    UnknownVariant {
        /// The enum.
        type_name: &'static str,
        /// The name the data gave.
        found: String,
        /// The variants it could be.
        variants: &'static [&'static str],
    },
    /// A color that is not `#rrggbb` or `#rrggbbaa`.
    BadColor(String),
    /// An array of the wrong length.
    WrongLength {
        /// How many elements the type needs.
        expected: usize,
        /// How many there are.
        found: usize,
    },
    /// An object lacks a member the type needs.
    MissingField {
        /// The member.
        field: &'static str,
    },
    /// Reading the value needs what the context does not have, such as the
    /// asset server.
    NoContext {
        /// What it needs: "the asset server, to load an asset".
        needs: &'static str,
    },
    /// A path that is not an asset path, or not one of the type's.
    BadAssetPath {
        /// The path the data gave.
        path: String,
        /// Why not.
        reason: String,
    },
    /// A name the context knows nothing of, such as a clip's or a
    /// collision layer's.
    UnknownName {
        /// What it should name: "clip".
        what: &'static str,
        /// The name the data gave.
        name: String,
    },
}

impl fmt::Display for ReflectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.source)?;
        if !self.path.is_empty() {
            write!(f, "at `{}`: ", self.path)?;
        }
        match &self.kind {
            ReflectErrorKind::Expected { expected, found } => {
                write!(f, "expected {expected}, found {found}")
            }
            ReflectErrorKind::OutOfRange { type_name, value } => {
                write!(f, "{value} does not fit in a `{type_name}`")
            }
            ReflectErrorKind::UnknownVariant {
                type_name,
                found,
                variants,
            } => write!(
                f,
                "`{found}` is not a `{type_name}`: it is one of {}",
                variants.join(", ")
            ),
            ReflectErrorKind::BadColor(text) => {
                write!(f, "`{text}` is not a color: write `#rrggbb` or `#rrggbbaa`")
            }
            ReflectErrorKind::WrongLength { expected, found } => {
                write!(f, "expected {expected} elements, found {found}")
            }
            ReflectErrorKind::MissingField { field } => write!(f, "`{field}` is missing"),
            ReflectErrorKind::NoContext { needs } => {
                write!(
                    f,
                    "reading this needs {needs}, which the reader was not given"
                )
            }
            ReflectErrorKind::BadAssetPath { path, reason } => {
                write!(f, "`{path}` is not an asset path for this: {reason}")
            }
            ReflectErrorKind::UnknownName { what, name } => {
                write!(f, "there is no {what} named `{name}`")
            }
        }
    }
}

impl Error for ReflectError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_is_restored_whatever_the_field_returns() {
        let mut cx = ReflectContext::new("level.klvl");
        let failed: Result<(), ReflectError> = cx.field("entities", |cx| {
            cx.index(3, |cx| {
                cx.field("Health", |cx| Err(cx.expected("an integer", &Value::Null)))
            })
        });
        let error = failed.expect_err("an error");
        assert_eq!(error.path, "entities[3].Health");
        assert_eq!(
            error.to_string(),
            "level.klvl: at `entities[3].Health`: expected an integer, found null"
        );
        // The failed field left nothing behind on the path.
        assert_eq!(cx.path(), "");
        cx.field("a", |cx| cx.unknown_field("A", "b"));
        assert_eq!(
            cx.warnings()[0].to_string(),
            "level.klvl: `A` has no field `b`, so it was left out (at `a`)"
        );
    }
}
