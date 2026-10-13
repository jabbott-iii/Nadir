//! The JSON writers. Both produce strict RFC 8259 JSON: no comments and no
//! trailing commas, so any JSON parser can read the result.

use std::fmt::Write as _;

use super::Value;

/// Spaces per nesting level in [`to_string_pretty`].
const INDENT: usize = 2;

/// Writes `value` as JSON on one line with no optional whitespace.
///
/// Non-finite floats (NaN and the infinities), which JSON cannot express,
/// are written as `null`. Everything else reads back as an equal value.
pub fn to_string_compact(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, None);
    out
}

/// Writes `value` as JSON for people to read: one array element or object
/// member per line, indented by two spaces. There is no trailing newline.
///
/// Non-finite floats are written as `null`, as for [`to_string_compact`].
pub fn to_string_pretty(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, Some(0));
    out
}

/// Writes `value`. `level` is `None` for compact output, or the current
/// nesting level for pretty output.
fn write_value(out: &mut String, value: &Value, level: Option<usize>) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        // Writing to a `String` cannot fail.
        Value::Int(value) => _ = write!(out, "{value}"),
        Value::Float(value) => write_float(out, *value),
        Value::String(value) => write_string(out, value),
        Value::Array(items) => {
            write_sequence(out, ['[', ']'], items, level, |out, item, inner| {
                write_value(out, item, inner)
            });
        }
        Value::Object(members) => {
            write_sequence(
                out,
                ['{', '}'],
                members,
                level,
                |out, (key, value), inner| {
                    write_string(out, key);
                    out.push_str(if inner.is_some() { ": " } else { ":" });
                    write_value(out, value, inner);
                },
            );
        }
    }
}

/// Writes the entries of an array or object between `brackets`, calling
/// `write_entry` for each with the nesting level inside the container.
fn write_sequence<T>(
    out: &mut String,
    [open, close]: [char; 2],
    entries: &[T],
    level: Option<usize>,
    write_entry: impl Fn(&mut String, &T, Option<usize>),
) {
    out.push(open);
    let inner = level.map(|level| level + 1);
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        new_line(out, inner);
        write_entry(out, entry, inner);
    }
    // An empty container stays on one line: `[]` or `{}`.
    if !entries.is_empty() {
        new_line(out, level);
    }
    out.push(close);
}

/// In pretty output, starts a new line indented to `level`. In compact
/// output, does nothing.
fn new_line(out: &mut String, level: Option<usize>) {
    if let Some(level) = level {
        out.push('\n');
        out.extend(std::iter::repeat_n(' ', level * INDENT));
    }
}

/// Writes a float so that it reads back as the same float: the shortest
/// digits that round-trip, and always with a `.` or an exponent so that it
/// is not mistaken for an integer.
fn write_float(out: &mut String, value: f64) {
    if !value.is_finite() {
        out.push_str("null");
        return;
    }
    let magnitude = value.abs();
    // Writing to a `String` cannot fail.
    if magnitude != 0.0 && !(1e-5..1e16).contains(&magnitude) {
        // Very large and very small numbers are shorter with an exponent.
        _ = write!(out, "{value:e}");
    } else {
        let start = out.len();
        _ = write!(out, "{value}");
        if !out[start..].contains('.') {
            out.push_str(".0");
        }
    }
}

fn write_string(out: &mut String, value: &str) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            // The remaining control characters have no short form. Writing
            // to a `String` cannot fail.
            control if control < ' ' => _ = write!(out, "\\u{:04x}", u32::from(control)),
            other => out.push(other),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string(text: &str) -> Value {
        Value::String(text.to_string())
    }

    fn object(members: &[(&str, Value)]) -> Value {
        Value::Object(
            members
                .iter()
                .map(|(key, value)| (key.to_string(), value.clone()))
                .collect(),
        )
    }

    fn float(value: f64) -> String {
        to_string_compact(&Value::Float(value))
    }

    #[test]
    fn scalars() {
        assert_eq!(to_string_compact(&Value::Null), "null");
        assert_eq!(to_string_compact(&Value::Bool(true)), "true");
        assert_eq!(to_string_compact(&Value::Bool(false)), "false");
        assert_eq!(to_string_compact(&Value::Int(0)), "0");
        assert_eq!(to_string_compact(&Value::Int(-5)), "-5");
        assert_eq!(
            to_string_compact(&Value::Int(i64::MAX)),
            "9223372036854775807"
        );
        assert_eq!(
            to_string_compact(&Value::Int(i64::MIN)),
            "-9223372036854775808"
        );
        assert_eq!(to_string_compact(&string("hi")), "\"hi\"");
    }

    #[test]
    fn floats_always_look_like_floats() {
        assert_eq!(float(1.0), "1.0");
        assert_eq!(float(-2.0), "-2.0");
        assert_eq!(float(0.0), "0.0");
        assert_eq!(float(-0.0), "-0.0");
        assert_eq!(float(0.5), "0.5");
        assert_eq!(float(0.1), "0.1");
        assert_eq!(float(123_456_789.125), "123456789.125");
        assert_eq!(float(1.0 / 3.0), "0.3333333333333333");
    }

    #[test]
    fn very_large_and_very_small_floats_use_an_exponent() {
        // The plain form runs from 1e-5 up to, but not including, 1e16.
        assert_eq!(float(1e-5), "0.00001");
        assert_eq!(float(9.999e-6), "9.999e-6");
        assert_eq!(float(1e-6), "1e-6");
        assert_eq!(float(9_999_999_999_999_998.0), "9999999999999998.0");
        assert_eq!(float(1e16), "1e16");
        assert_eq!(float(-1e16), "-1e16");
        assert_eq!(float(1.5e300), "1.5e300");
        assert_eq!(float(f64::MAX), "1.7976931348623157e308");
        assert_eq!(float(f64::MIN_POSITIVE), "2.2250738585072014e-308");
        assert_eq!(float(5e-324), "5e-324");
    }

    #[test]
    fn non_finite_floats_become_null() {
        assert_eq!(float(f64::NAN), "null");
        assert_eq!(float(f64::INFINITY), "null");
        assert_eq!(float(f64::NEG_INFINITY), "null");
        let list = Value::Array(vec![Value::Float(f64::NAN), Value::Float(1.0)]);
        assert_eq!(to_string_compact(&list), "[null,1.0]");
    }

    #[test]
    fn strings_are_escaped_only_where_json_requires_it() {
        let text = "a\"b\\c/d\n\r\t\u{8}\u{c}\0\u{1}\u{1f} \u{7f}é€😀\u{2028}";
        let expected = "\"a\\\"b\\\\c/d\\n\\r\\t\\b\\f\\u0000\\u0001\\u001f \u{7f}é€😀\u{2028}\"";
        assert_eq!(to_string_compact(&string(text)), expected);
        assert_eq!(to_string_compact(&string("")), "\"\"");
    }

    #[test]
    fn compact_output_has_no_whitespace() {
        let value = object(&[
            ("a", Value::Int(1)),
            (
                "b",
                Value::Array(vec![Value::Int(2), Value::Array(vec![]), object(&[])]),
            ),
            ("c d", object(&[("e", Value::Null)])),
        ]);
        assert_eq!(
            to_string_compact(&value),
            r#"{"a":1,"b":[2,[],{}],"c d":{"e":null}}"#
        );
        assert_eq!(to_string_compact(&Value::Array(vec![])), "[]");
        assert_eq!(to_string_compact(&object(&[])), "{}");
    }

    #[test]
    fn pretty_output_puts_each_entry_on_its_own_line() {
        let value = object(&[
            ("name", string("slime")),
            ("hp", Value::Int(12)),
            ("drops", Value::Array(vec![string("gel"), Value::Null])),
            (
                "stats",
                object(&[("speed", Value::Float(1.5)), ("tags", Value::Array(vec![]))]),
            ),
            ("empty", object(&[])),
            (
                "grid",
                Value::Array(vec![Value::Array(vec![Value::Int(1), Value::Int(2)])]),
            ),
        ]);
        let expected = r#"{
  "name": "slime",
  "hp": 12,
  "drops": [
    "gel",
    null
  ],
  "stats": {
    "speed": 1.5,
    "tags": []
  },
  "empty": {},
  "grid": [
    [
      1,
      2
    ]
  ]
}"#;
        assert_eq!(to_string_pretty(&value), expected);
    }

    #[test]
    fn pretty_scalars_and_empty_containers_match_compact() {
        for value in [
            Value::Null,
            Value::Int(7),
            Value::Float(0.5),
            string("x"),
            Value::Array(vec![]),
            object(&[]),
        ] {
            assert_eq!(to_string_pretty(&value), to_string_compact(&value));
        }
    }

    #[test]
    fn keys_are_written_in_order_and_escaped() {
        let value = object(&[
            ("z", Value::Int(1)),
            ("a\"\n", Value::Int(2)),
            ("", Value::Int(3)),
        ]);
        assert_eq!(to_string_compact(&value), r#"{"z":1,"a\"\n":2,"":3}"#);
        assert_eq!(
            to_string_pretty(&value),
            "{\n  \"z\": 1,\n  \"a\\\"\\n\": 2,\n  \"\": 3\n}"
        );
    }
}
