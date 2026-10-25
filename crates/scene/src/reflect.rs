//! [`Reflect`] and [`FromValue`]: walking a value's fields by name, and
//! turning it into a [`Value`] and back.

use std::error::Error;
use std::fmt;

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
    /// An asset handle, written as the asset's path (roadmap M5.6).
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Segment {
    Field(&'static str),
    Index(usize),
}

/// What reading or writing reflected values needs besides the values:
/// the name of the source, for messages, the path to the field at hand,
/// and the warnings so far.
pub struct ReflectContext<'a> {
    source: &'a str,
    path: Vec<Segment>,
    warnings: Vec<ReflectWarning>,
}

impl<'a> ReflectContext<'a> {
    /// A context for reading or writing `source`: a file's path, or any
    /// name that tells the reader where the data came from.
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            path: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// The name of the source.
    pub fn source(&self) -> &str {
        self.source
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
#[derive(Clone, Debug, PartialEq)]
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
