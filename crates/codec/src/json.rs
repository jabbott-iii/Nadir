//! JSON (RFC 8259) with the JSONC extensions: `//` and `/* */` comments,
//! and a trailing comma after the last element of an array or object.
//!
//! [`parse`] reads text into a [`Value`] tree; [`to_string_compact`] and
//! [`to_string_pretty`] write one back as strict JSON.
//!
//! ```
//! use nadir_codec::Limits;
//! use nadir_codec::json::{self, Value};
//!
//! let text = r#"{
//!     // Window settings.
//!     "title": "Sandbox",
//!     "size": [480, 270,],
//! }"#;
//! let value = json::parse(text, Limits::default())?;
//! assert_eq!(value.get("title").and_then(Value::as_str), Some("Sandbox"));
//! assert_eq!(json::to_string_compact(&value), r#"{"title":"Sandbox","size":[480,270]}"#);
//! # Ok::<(), nadir_codec::CodecError>(())
//! ```
//!
//! What the parser accepts, beyond the grammar:
//!
//! - Any value at the top level, not only arrays and objects.
//! - A leading byte-order mark, which is skipped.
//! - Numbers written without a fraction or exponent become [`Value::Int`]
//!   when they fit an `i64`. Every other number becomes [`Value::Float`],
//!   and one too large for an `f64` is an error.
//!
//! What it rejects, although some parsers are lenient about it:
//!
//! - An object with the same key twice.
//! - A `\u` escape that is half of a surrogate pair.
//! - Raw control characters in strings, including tabs and newlines.
//! - `NaN`, `Infinity`, single quotes, unquoted keys, hex numbers, a
//!   leading `+`.
//!
//! Comments are not part of the value tree, so they are lost when a
//! document is parsed and written back.

mod parse;
mod write;

use std::fmt;

pub use parse::parse;
pub use write::{to_string_compact, to_string_pretty};

/// A JSON value.
///
/// Objects keep their members in document order, in a `Vec`, so that a file
/// that is read and written back keeps its layout and output is
/// deterministic.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number written as an integer that fits an `i64`.
    Int(i64),
    /// Any other number. The parser never produces NaN or an infinity.
    Float(f64),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<Value>),
    /// An object: `(key, value)` members in document order. The parser
    /// never produces two members with the same key.
    Object(Vec<(String, Value)>),
}

impl Value {
    /// Returns `true` for [`Value::Null`].
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Returns the boolean, if this is a [`Value::Bool`].
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Self::Bool(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the integer, if this is a [`Value::Int`]. A [`Value::Float`]
    /// is never converted, even when it is a whole number.
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Self::Int(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the number as an `f64`, if this is a [`Value::Float`] or a
    /// [`Value::Int`]. Integers beyond ±2⁵³ lose precision.
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Self::Float(value) => Some(value),
            Self::Int(value) => Some(value as f64),
            _ => None,
        }
    }

    /// Returns the string, if this is a [`Value::String`].
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the elements, if this is a [`Value::Array`].
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Returns the members in document order, if this is a
    /// [`Value::Object`].
    pub fn as_object(&self) -> Option<&[(String, Value)]> {
        match self {
            Self::Object(members) => Some(members),
            _ => None,
        }
    }

    /// Returns the member named `key`, if this is a [`Value::Object`] that
    /// has one. The search is linear.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object()?
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }
}

/// What is wrong with a piece of JSON text. See [`CodecError::Json`].
///
/// [`CodecError::Json`]: crate::CodecError::Json
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The text ended too soon.
    UnexpectedEnd {
        /// What should have come next.
        expected: &'static str,
    },
    /// A character that cannot appear at this point.
    UnexpectedCharacter {
        /// The character.
        found: char,
        /// What should have been there instead.
        expected: &'static str,
    },
    /// A word that looks like `null`, `true` or `false` but is not one.
    InvalidLiteral,
    /// A backslash in a string followed by something that is not an escape.
    InvalidEscape,
    /// A `\u` escape that is half of a surrogate pair without its partner.
    LoneSurrogate,
    /// A raw control character inside a string, such as a line break.
    ControlCharacter,
    /// A number with leading zeros, such as `007`.
    LeadingZero,
    /// A number too large to represent.
    NumberOutOfRange,
    /// An object has the same key twice.
    DuplicateKey {
        /// The repeated key.
        key: String,
    },
    /// Arrays and objects nest deeper than [`Limits::max_depth`].
    ///
    /// [`Limits::max_depth`]: crate::Limits::max_depth
    TooDeep {
        /// The limit that was exceeded.
        limit: u32,
    },
    /// A string is longer than [`Limits::max_string_bytes`].
    ///
    /// [`Limits::max_string_bytes`]: crate::Limits::max_string_bytes
    StringTooLong {
        /// The limit that was exceeded.
        limit: usize,
    },
    /// An array or object has more than [`Limits::max_entries`] entries.
    ///
    /// [`Limits::max_entries`]: crate::Limits::max_entries
    TooManyEntries {
        /// The limit that was exceeded.
        limit: usize,
    },
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEnd { expected } => {
                write!(f, "the text ends where {expected} was expected")
            }
            Self::UnexpectedCharacter { found, expected } => {
                write!(f, "found {found:?} where {expected} was expected")
            }
            Self::InvalidLiteral => {
                f.write_str("unknown word; the only bare words are `null`, `true` and `false`")
            }
            Self::InvalidEscape => f.write_str("invalid escape sequence in a string"),
            Self::LoneSurrogate => f.write_str("a `\\u` escape is half of a surrogate pair"),
            Self::ControlCharacter => {
                f.write_str("control character in a string; write it as an escape such as `\\n`")
            }
            Self::LeadingZero => f.write_str("numbers cannot have leading zeros"),
            Self::NumberOutOfRange => f.write_str("the number is too large"),
            Self::DuplicateKey { key } => write!(f, "the key {key:?} appears twice in one object"),
            Self::TooDeep { limit } => write!(
                f,
                "arrays and objects nest deeper than the limit of {limit}"
            ),
            Self::StringTooLong { limit } => {
                write!(f, "a string is longer than the limit of {limit} bytes")
            }
            Self::TooManyEntries { limit } => write!(
                f,
                "an array or object has more than the limit of {limit} entries"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        Value::Object(vec![
            ("name".to_string(), Value::String("slime".to_string())),
            ("hp".to_string(), Value::Int(12)),
            ("speed".to_string(), Value::Float(1.5)),
            ("boss".to_string(), Value::Bool(false)),
            ("drop".to_string(), Value::Null),
            (
                "tags".to_string(),
                Value::Array(vec![Value::Int(1), Value::Int(2)]),
            ),
        ])
    }

    #[test]
    fn accessors_return_only_the_matching_kind() {
        let value = sample();
        assert_eq!(value.get("name").and_then(Value::as_str), Some("slime"));
        assert_eq!(value.get("hp").and_then(Value::as_i64), Some(12));
        assert_eq!(value.get("speed").and_then(Value::as_f64), Some(1.5));
        assert_eq!(value.get("boss").and_then(Value::as_bool), Some(false));
        assert!(value.get("drop").is_some_and(Value::is_null));
        assert_eq!(
            value.get("tags").and_then(Value::as_array),
            Some(&[Value::Int(1), Value::Int(2)][..])
        );
        assert_eq!(value.as_object().map(<[_]>::len), Some(6));

        let name = value.get("name").expect("has a name");
        assert_eq!(name.as_i64(), None);
        assert_eq!(name.as_f64(), None);
        assert_eq!(name.as_bool(), None);
        assert_eq!(name.as_array(), None);
        assert_eq!(name.as_object(), None);
        assert!(!name.is_null());
        assert_eq!(Value::Int(1).as_str(), None);
    }

    #[test]
    fn numbers_convert_one_way_only() {
        assert_eq!(Value::Int(3).as_f64(), Some(3.0));
        assert_eq!(Value::Float(3.0).as_i64(), None);
    }

    #[test]
    fn get_finds_members_of_objects_only() {
        let value = sample();
        assert_eq!(value.get("missing"), None);
        assert_eq!(value.get("tags").and_then(|tags| tags.get("0")), None);
        assert_eq!(Value::Null.get("name"), None);
    }

    #[test]
    fn error_kinds_explain_themselves() {
        let cases = [
            (
                ErrorKind::UnexpectedEnd {
                    expected: "a value",
                },
                "the text ends where a value was expected",
            ),
            (
                ErrorKind::UnexpectedCharacter {
                    found: '}',
                    expected: "a value",
                },
                "found '}' where a value was expected",
            ),
            (
                ErrorKind::UnexpectedCharacter {
                    found: '\n',
                    expected: "`,` or `]`",
                },
                "found '\\n' where `,` or `]` was expected",
            ),
            (
                ErrorKind::DuplicateKey {
                    key: "hp".to_string(),
                },
                "the key \"hp\" appears twice in one object",
            ),
            (
                ErrorKind::TooDeep { limit: 64 },
                "arrays and objects nest deeper than the limit of 64",
            ),
            (
                ErrorKind::StringTooLong { limit: 8 },
                "a string is longer than the limit of 8 bytes",
            ),
            (
                ErrorKind::TooManyEntries { limit: 2 },
                "an array or object has more than the limit of 2 entries",
            ),
        ];
        for (kind, expected) in cases {
            assert_eq!(kind.to_string(), expected);
        }
        // The rest have fixed messages; they must at least not be empty.
        for kind in [
            ErrorKind::InvalidLiteral,
            ErrorKind::InvalidEscape,
            ErrorKind::LoneSurrogate,
            ErrorKind::ControlCharacter,
            ErrorKind::LeadingZero,
            ErrorKind::NumberOutOfRange,
        ] {
            assert!(!kind.to_string().is_empty());
        }
    }
}
