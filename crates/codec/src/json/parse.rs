//! The JSON / JSONC parser: recursive descent over the bytes of a `&str`.
//!
//! The parser only ever stops at ASCII bytes, so every position it slices
//! the text at is a character boundary.

use super::{ErrorKind, Value};
use crate::{CodecError, Limits};

/// Objects up to this many members are checked for duplicate keys as each
/// key is read, by comparing it with the ones before. Larger objects are
/// checked once at the end by sorting, so that a huge object cannot make
/// the check quadratic.
const SMALL_OBJECT: usize = 16;

/// Parses JSON or JSONC text into a [`Value`].
///
/// See the [module documentation](super) for exactly what is accepted.
///
/// # Errors
///
/// Returns [`CodecError::Json`] with the line and column of the problem if
/// the text is malformed or exceeds one of `limits`. It never panics,
/// whatever the text.
pub fn parse(text: &str, limits: Limits) -> Result<Value, CodecError> {
    let mut parser = Parser {
        text,
        bytes: text.as_bytes(),
        pos: 0,
        limits,
        key_offsets: Vec::new(),
    };
    if text.starts_with(BYTE_ORDER_MARK) {
        parser.pos = BYTE_ORDER_MARK.len_utf8();
    }
    let value = parser.value(0)?;
    parser.skip_trivia()?;
    match parser.peek() {
        None => Ok(value),
        Some(_) => Err(parser.unexpected(parser.pos, "the end of the text")),
    }
}

const BYTE_ORDER_MARK: char = '\u{feff}';

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    /// Byte offset of the next unread byte.
    pos: usize,
    limits: Limits,
    /// Where each key of every object currently being parsed starts. Used
    /// to report the position of a duplicate key in a large object.
    key_offsets: Vec<usize>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// Builds an error for the byte offset `at`, working out its line and
    /// column. This rescans the text, which is fine for a path taken once.
    fn error(&self, at: usize, kind: ErrorKind) -> CodecError {
        let before = self.text.get(..at).unwrap_or(self.text);
        let before = before.strip_prefix(BYTE_ORDER_MARK).unwrap_or(before);
        let line = before.bytes().filter(|&byte| byte == b'\n').count() + 1;
        let line_text = before.rsplit('\n').next().unwrap_or(before);
        let column = line_text.chars().count() + 1;
        CodecError::Json {
            line: u32::try_from(line).unwrap_or(u32::MAX),
            column: u32::try_from(column).unwrap_or(u32::MAX),
            kind,
        }
    }

    /// Builds the error for finding the wrong thing, or nothing, at `at`.
    fn unexpected(&self, at: usize, expected: &'static str) -> CodecError {
        let kind = match self.text.get(at..).and_then(|rest| rest.chars().next()) {
            Some(found) => ErrorKind::UnexpectedCharacter { found, expected },
            None => ErrorKind::UnexpectedEnd { expected },
        };
        self.error(at, kind)
    }

    /// Consumes `byte`, or fails saying that `expected` was needed.
    fn expect(&mut self, byte: u8, expected: &'static str) -> Result<(), CodecError> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.unexpected(self.pos, expected))
        }
    }

    /// Skips whitespace and comments.
    fn skip_trivia(&mut self) -> Result<(), CodecError> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.pos += 1,
                Some(b'/') => match self.bytes.get(self.pos + 1) {
                    Some(b'/') => {
                        let rest = self.bytes.get(self.pos..).unwrap_or_default();
                        let line_len = rest
                            .iter()
                            .position(|&byte| byte == b'\n')
                            .unwrap_or(rest.len());
                        self.pos += line_len;
                    }
                    Some(b'*') => {
                        let body = self.bytes.get(self.pos + 2..).unwrap_or_default();
                        match body.windows(2).position(|pair| pair == b"*/") {
                            Some(close) => self.pos += 2 + close + 2,
                            None => {
                                let kind = ErrorKind::UnexpectedEnd {
                                    expected: "`*/` to end the comment",
                                };
                                return Err(self.error(self.pos, kind));
                            }
                        }
                    }
                    // A lone slash is not trivia. Whoever wanted a token
                    // here reports it.
                    _ => return Ok(()),
                },
                _ => return Ok(()),
            }
        }
    }

    /// Parses one value. `depth` is the number of arrays and objects that
    /// enclose it.
    fn value(&mut self, depth: u32) -> Result<Value, CodecError> {
        self.skip_trivia()?;
        match self.peek() {
            Some(b'"') => self.string().map(Value::String),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(b'n') => self.literal("null", Value::Null),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            _ => Err(self.unexpected(self.pos, "a value")),
        }
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value, CodecError> {
        let rest = self.bytes.get(self.pos..).unwrap_or_default();
        if !rest.starts_with(word.as_bytes()) {
            return Err(self.error(self.pos, ErrorKind::InvalidLiteral));
        }
        self.pos += word.len();
        Ok(value)
    }

    /// Fails if a container may not start at `depth`.
    fn check_depth(&self, depth: u32) -> Result<(), CodecError> {
        if depth >= self.limits.max_depth {
            return Err(self.error(
                self.pos,
                ErrorKind::TooDeep {
                    limit: self.limits.max_depth,
                },
            ));
        }
        Ok(())
    }

    /// Fails if a container that already has `len` entries may not get
    /// another.
    fn check_room(&self, len: usize) -> Result<(), CodecError> {
        if len >= self.limits.max_entries {
            return Err(self.error(
                self.pos,
                ErrorKind::TooManyEntries {
                    limit: self.limits.max_entries,
                },
            ));
        }
        Ok(())
    }

    fn array(&mut self, depth: u32) -> Result<Value, CodecError> {
        self.check_depth(depth)?;
        self.pos += 1; // `[`
        let mut items = Vec::new();
        loop {
            self.skip_trivia()?;
            if self.peek() == Some(b']') {
                break;
            }
            if !items.is_empty() {
                self.expect(b',', "`,` or `]`")?;
                self.skip_trivia()?;
                // A comma may be followed directly by the closing bracket.
                if self.peek() == Some(b']') {
                    break;
                }
            }
            if self.peek().is_none() {
                return Err(self.unexpected(self.pos, "a value or `]`"));
            }
            self.check_room(items.len())?;
            items.push(self.value(depth + 1)?);
        }
        self.pos += 1; // `]`
        Ok(Value::Array(items))
    }

    fn object(&mut self, depth: u32) -> Result<Value, CodecError> {
        self.check_depth(depth)?;
        self.pos += 1; // `{`
        let first_key = self.key_offsets.len();
        let mut members: Vec<(String, Value)> = Vec::new();
        loop {
            self.skip_trivia()?;
            if self.peek() == Some(b'}') {
                break;
            }
            if !members.is_empty() {
                self.expect(b',', "`,` or `}`")?;
                self.skip_trivia()?;
                // A comma may be followed directly by the closing brace.
                if self.peek() == Some(b'}') {
                    break;
                }
            }
            if self.peek() != Some(b'"') {
                return Err(self.unexpected(self.pos, "a string key or `}`"));
            }
            self.check_room(members.len())?;
            let key_at = self.pos;
            let key = self.string()?;
            if members.len() < SMALL_OBJECT && members.iter().any(|(existing, _)| *existing == key)
            {
                return Err(self.error(key_at, ErrorKind::DuplicateKey { key }));
            }
            self.key_offsets.push(key_at);
            self.skip_trivia()?;
            self.expect(b':', "`:` after the key")?;
            let value = self.value(depth + 1)?;
            members.push((key, value));
        }
        self.pos += 1; // `}`
        if members.len() > SMALL_OBJECT {
            self.reject_duplicate_keys(&members, first_key)?;
        }
        self.key_offsets.truncate(first_key);
        Ok(Value::Object(members))
    }

    /// The duplicate-key check for large objects: sort the keys and look
    /// for equal neighbors. `first_key` is where this object's entries
    /// start in `key_offsets`.
    fn reject_duplicate_keys(
        &self,
        members: &[(String, Value)],
        first_key: usize,
    ) -> Result<(), CodecError> {
        let key_of = |index: usize| members.get(index).map(|(key, _)| key.as_str());
        let mut order: Vec<usize> = (0..members.len()).collect();
        order.sort_unstable_by(|&a, &b| key_of(a).cmp(&key_of(b)).then(a.cmp(&b)));
        // Of two equal neighbors the second comes later in the document.
        // Report the repeat that a reader would meet first.
        let first_repeat = order
            .windows(2)
            .filter(|pair| key_of(pair[0]) == key_of(pair[1]))
            .map(|pair| pair[1])
            .min();
        let Some(repeat) = first_repeat else {
            return Ok(());
        };
        let at = self
            .key_offsets
            .get(first_key + repeat)
            .copied()
            .unwrap_or(self.pos);
        let key = key_of(repeat).unwrap_or_default().to_string();
        Err(self.error(at, ErrorKind::DuplicateKey { key }))
    }

    /// Parses a string. `self.pos` is at its opening quote.
    fn string(&mut self) -> Result<String, CodecError> {
        let start = self.pos;
        self.pos += 1; // `"`
        let mut out = String::new();
        loop {
            // Copy everything up to the next byte that needs attention.
            let run_start = self.pos;
            while let Some(byte) = self.peek()
                && byte != b'"'
                && byte != b'\\'
                && byte >= 0x20
            {
                self.pos += 1;
            }
            let run = self.text.get(run_start..self.pos).unwrap_or_default();
            self.check_string_len(start, out.len(), run.len())?;
            out.push_str(run);

            match self.peek() {
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    let escaped = self.escape()?;
                    self.check_string_len(start, out.len(), escaped.len_utf8())?;
                    out.push(escaped);
                }
                Some(_) => return Err(self.error(self.pos, ErrorKind::ControlCharacter)),
                None => {
                    let kind = ErrorKind::UnexpectedEnd {
                        expected: "`\"` to end the string",
                    };
                    return Err(self.error(start, kind));
                }
            }
        }
    }

    /// Fails if a string of `len` bytes may not grow by `more`. Called
    /// before the bytes are copied, so an oversized string is never built.
    fn check_string_len(
        &self,
        string_start: usize,
        len: usize,
        more: usize,
    ) -> Result<(), CodecError> {
        let limit = self.limits.max_string_bytes;
        if len.checked_add(more).is_none_or(|total| total > limit) {
            return Err(self.error(string_start, ErrorKind::StringTooLong { limit }));
        }
        Ok(())
    }

    /// Parses an escape sequence. `self.pos` is at its backslash.
    fn escape(&mut self) -> Result<char, CodecError> {
        let start = self.pos;
        self.pos += 1; // `\`
        let Some(byte) = self.peek() else {
            return Err(self.error(
                start,
                ErrorKind::UnexpectedEnd {
                    expected: "an escape sequence",
                },
            ));
        };
        self.pos += 1;
        let simple = match byte {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => return self.unicode_escape(start),
            _ => return Err(self.error(start, ErrorKind::InvalidEscape)),
        };
        Ok(simple)
    }

    /// Parses the rest of a `\uXXXX` escape, and the low half that must
    /// follow a high surrogate. `self.pos` is just after the `u`.
    fn unicode_escape(&mut self, start: usize) -> Result<char, CodecError> {
        let first = self.hex4(start)?;
        let code = match first {
            0xD800..=0xDBFF => {
                // A high surrogate needs a low one right behind it.
                let low_start = self.pos;
                if self.bytes.get(self.pos..self.pos + 2) != Some(b"\\u") {
                    return Err(self.error(start, ErrorKind::LoneSurrogate));
                }
                self.pos += 2;
                let low = self.hex4(low_start)?;
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err(self.error(start, ErrorKind::LoneSurrogate));
                }
                0x1_0000 + ((first - 0xD800) << 10) + (low - 0xDC00)
            }
            0xDC00..=0xDFFF => return Err(self.error(start, ErrorKind::LoneSurrogate)),
            code => code,
        };
        // Every code that is left is a valid `char`: the surrogate range
        // was handled above and a pair cannot exceed U+10FFFF.
        char::from_u32(code).ok_or_else(|| self.error(start, ErrorKind::InvalidEscape))
    }

    /// Reads four hex digits. `escape_start` is where to report a failure.
    fn hex4(&mut self, escape_start: usize) -> Result<u32, CodecError> {
        let digits = self.bytes.get(self.pos..self.pos + 4).unwrap_or_default();
        if digits.len() != 4 {
            let kind = match self.bytes.get(self.pos..) {
                // The text ends inside the escape.
                Some(rest) if rest.iter().all(u8::is_ascii_hexdigit) => ErrorKind::UnexpectedEnd {
                    expected: "four hex digits",
                },
                _ => ErrorKind::InvalidEscape,
            };
            return Err(self.error(escape_start, kind));
        }
        let mut code = 0;
        for &digit in digits {
            let Some(value) = char::from(digit).to_digit(16) else {
                return Err(self.error(escape_start, ErrorKind::InvalidEscape));
            };
            code = code * 16 + value;
        }
        self.pos += 4;
        Ok(code)
    }

    /// Parses a number. `self.pos` is at its first character, a digit or a
    /// minus sign.
    fn number(&mut self) -> Result<Value, CodecError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err(self.error(start, ErrorKind::LeadingZero));
                }
            }
            _ => self.digits()?,
        }
        let mut integer = true;
        if self.peek() == Some(b'.') {
            integer = false;
            self.pos += 1;
            self.digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            integer = false;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            self.digits()?;
        }

        let text = self.text.get(start..self.pos).unwrap_or_default();
        // `-0` stays a float, so that its sign survives. An integer too
        // large for an i64 also falls through to the float path.
        if integer
            && text != "-0"
            && let Ok(value) = text.parse()
        {
            return Ok(Value::Int(value));
        }
        match text.parse::<f64>() {
            Ok(value) if value.is_finite() => Ok(Value::Float(value)),
            _ => Err(self.error(start, ErrorKind::NumberOutOfRange)),
        }
    }

    /// Consumes one or more digits.
    fn digits(&mut self) -> Result<(), CodecError> {
        if !matches!(self.peek(), Some(b'0'..=b'9')) {
            return Err(self.unexpected(self.pos, "a digit"));
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ErrorKind::{
        ControlCharacter, DuplicateKey, InvalidEscape, InvalidLiteral, LeadingZero, LoneSurrogate,
        NumberOutOfRange, StringTooLong, TooDeep, TooManyEntries, UnexpectedCharacter,
        UnexpectedEnd,
    };

    fn ok(text: &str) -> Value {
        parse(text, Limits::default())
            .unwrap_or_else(|error| panic!("{text:?} should parse, but: {error}"))
    }

    /// The line, column and kind of the error for `text`.
    fn err_with(text: &str, limits: Limits) -> (u32, u32, ErrorKind) {
        match parse(text, limits) {
            Err(CodecError::Json { line, column, kind }) => (line, column, kind),
            Err(other) => panic!("{text:?} gave an error that is not JSON's: {other}"),
            Ok(value) => panic!("{text:?} should not parse, but gave {value:?}"),
        }
    }

    fn err(text: &str) -> (u32, u32, ErrorKind) {
        err_with(text, Limits::default())
    }

    fn found(found: char, expected: &'static str) -> ErrorKind {
        UnexpectedCharacter { found, expected }
    }

    fn end(expected: &'static str) -> ErrorKind {
        UnexpectedEnd { expected }
    }

    fn string(text: &str) -> Value {
        Value::String(text.to_string())
    }

    fn ints(values: &[i64]) -> Value {
        Value::Array(values.iter().copied().map(Value::Int).collect())
    }

    fn object(members: &[(&str, Value)]) -> Value {
        Value::Object(
            members
                .iter()
                .map(|(key, value)| (key.to_string(), value.clone()))
                .collect(),
        )
    }

    const VALUE: &str = "a value";
    const END: &str = "the end of the text";
    const DIGIT: &str = "a digit";
    const ARRAY_NEXT: &str = "`,` or `]`";
    const ARRAY_ITEM: &str = "a value or `]`";
    const OBJECT_NEXT: &str = "`,` or `}`";
    const KEY: &str = "a string key or `}`";
    const COLON: &str = "`:` after the key";

    // ----------------------------------------------------------- scalars

    #[test]
    fn literals() {
        assert_eq!(ok("null"), Value::Null);
        assert_eq!(ok("true"), Value::Bool(true));
        assert_eq!(ok("false"), Value::Bool(false));
        assert_eq!(ok(" \t\r\n null \n"), Value::Null);
    }

    #[test]
    fn misspelled_literals_are_rejected() {
        assert_eq!(err("nul"), (1, 1, InvalidLiteral));
        assert_eq!(err("none"), (1, 1, InvalidLiteral));
        assert_eq!(err("tru"), (1, 1, InvalidLiteral));
        assert_eq!(err("fals"), (1, 1, InvalidLiteral));
        assert_eq!(err("nan"), (1, 1, InvalidLiteral));
        assert_eq!(err("[1, nil]"), (1, 5, InvalidLiteral));
        // Other spellings do not even start like a literal.
        assert_eq!(err("True"), (1, 1, found('T', VALUE)));
        assert_eq!(err("NULL"), (1, 1, found('N', VALUE)));
        assert_eq!(err("undefined"), (1, 1, found('u', VALUE)));
        // A literal with something stuck to it.
        assert_eq!(err("nullx"), (1, 5, found('x', END)));
        assert_eq!(err("truefalse"), (1, 5, found('f', END)));
    }

    #[test]
    fn integers() {
        assert_eq!(ok("0"), Value::Int(0));
        assert_eq!(ok("42"), Value::Int(42));
        assert_eq!(ok("-17"), Value::Int(-17));
        assert_eq!(ok("9223372036854775807"), Value::Int(i64::MAX));
        assert_eq!(ok("-9223372036854775808"), Value::Int(i64::MIN));
    }

    #[test]
    fn numbers_with_a_fraction_or_exponent_are_floats() {
        assert_eq!(ok("1.5"), Value::Float(1.5));
        assert_eq!(ok("-0.25"), Value::Float(-0.25));
        assert_eq!(ok("1.0"), Value::Float(1.0));
        assert_eq!(ok("0.1"), Value::Float(0.1));
        assert_eq!(ok("1e2"), Value::Float(100.0));
        assert_eq!(ok("1E+2"), Value::Float(100.0));
        assert_eq!(ok("25e-1"), Value::Float(2.5));
        assert_eq!(ok("-1.5E3"), Value::Float(-1500.0));
        assert_eq!(ok("0e0"), Value::Float(0.0));
        assert_eq!(ok("1.7976931348623157e308"), Value::Float(f64::MAX));
        assert_eq!(ok("5e-324"), Value::Float(5e-324));
        // Too small to represent rounds to zero, which is still a number.
        assert_eq!(ok("1e-999"), Value::Float(0.0));
    }

    #[test]
    fn integers_too_large_for_i64_become_floats() {
        assert_eq!(
            ok("9223372036854775808"),
            Value::Float(9_223_372_036_854_775_808.0)
        );
        assert_eq!(
            ok("-9223372036854775809"),
            Value::Float(-9_223_372_036_854_775_808.0)
        );
        assert_eq!(
            ok("123456789012345678901234567890"),
            Value::Float(1.234_567_890_123_456_8e29)
        );
    }

    #[test]
    fn negative_zero_keeps_its_sign() {
        for text in ["-0", "-0.0", "-0e0"] {
            match ok(text) {
                Value::Float(value) => assert!(
                    value == 0.0 && value.is_sign_negative(),
                    "{text} gave {value}"
                ),
                other => panic!("{text} gave {other:?}"),
            }
        }
        assert_eq!(ok("0"), Value::Int(0));
    }

    #[test]
    fn numbers_too_large_for_f64_are_rejected() {
        assert_eq!(err("1e999"), (1, 1, NumberOutOfRange));
        assert_eq!(err("-1e999"), (1, 1, NumberOutOfRange));
        assert_eq!(err("[0, 1.8e308]"), (1, 5, NumberOutOfRange));
        let huge = "9".repeat(400);
        assert_eq!(err(&huge), (1, 1, NumberOutOfRange));
    }

    #[test]
    fn malformed_numbers_are_rejected() {
        assert_eq!(err("01"), (1, 1, LeadingZero));
        assert_eq!(err("-007"), (1, 1, LeadingZero));
        assert_eq!(err("[1, 00]"), (1, 5, LeadingZero));
        assert_eq!(err("-"), (1, 2, end(DIGIT)));
        assert_eq!(err("1."), (1, 3, end(DIGIT)));
        assert_eq!(err("1.e5"), (1, 3, found('e', DIGIT)));
        assert_eq!(err("1e"), (1, 3, end(DIGIT)));
        assert_eq!(err("1e+"), (1, 4, end(DIGIT)));
        assert_eq!(err("1e+x"), (1, 4, found('x', DIGIT)));
        assert_eq!(err("--1"), (1, 2, found('-', DIGIT)));
        assert_eq!(err("- 1"), (1, 2, found(' ', DIGIT)));
        assert_eq!(err(".5"), (1, 1, found('.', VALUE)));
        assert_eq!(err("+1"), (1, 1, found('+', VALUE)));
        assert_eq!(err("0x10"), (1, 2, found('x', END)));
        assert_eq!(err("1_000"), (1, 2, found('_', END)));
        assert_eq!(err("1.5.2"), (1, 4, found('.', END)));
        assert_eq!(err("Infinity"), (1, 1, found('I', VALUE)));
        assert_eq!(err("-Infinity"), (1, 2, found('I', DIGIT)));
        assert_eq!(err("NaN"), (1, 1, found('N', VALUE)));
    }

    // ----------------------------------------------------------- strings

    #[test]
    fn strings() {
        assert_eq!(ok(r#""""#), string(""));
        assert_eq!(ok(r#""hello world""#), string("hello world"));
        assert_eq!(ok("\"é€😀\u{7f}\""), string("é€😀\u{7f}"));
        // Comment markers inside a string are just text.
        assert_eq!(
            ok(r#""// not a comment /* nor this */""#),
            string("// not a comment /* nor this */")
        );
    }

    #[test]
    fn escapes() {
        assert_eq!(
            ok(r#""\" \\ \/ \b \f \n \r \t""#),
            string("\" \\ / \u{8} \u{c} \n \r \t")
        );
        assert_eq!(ok(r#""\u0041\u00e9\u20AC\u0000""#), string("Aé€\0"));
        assert_eq!(ok(r#""a\u0062c""#), string("abc"));
        assert_eq!(ok(r#""\u00E9\u00e9""#), string("éé"));
        // A surrogate pair is one character.
        assert_eq!(ok(r#""\ud83d\ude00""#), string("😀"));
        assert_eq!(ok(r#""\uD83D\uDE00!""#), string("😀!"));
        assert_eq!(ok(r#""\ud800\udc00""#), string("\u{10000}"));
        assert_eq!(ok(r#""\udbff\udfff""#), string("\u{10ffff}"));
        // Escaped or not, it is the same string.
        assert_eq!(ok(r#""\ud83d\ude00""#), ok("\"😀\""));
    }

    #[test]
    fn malformed_strings_are_rejected() {
        // Errors about a whole string point at its opening quote.
        assert_eq!(err(r#""abc"#), (1, 1, end("`\"` to end the string")));
        assert_eq!(err(r#"[1, "abc]"#), (1, 5, end("`\"` to end the string")));
        assert_eq!(err("'abc'"), (1, 1, found('\'', VALUE)));
        // Errors about an escape point at its backslash.
        assert_eq!(err(r#""a\x""#), (1, 3, InvalidEscape));
        assert_eq!(err(r#""a\ ""#), (1, 3, InvalidEscape));
        assert_eq!(err(r#""a\"#), (1, 3, end("an escape sequence")));
        assert_eq!(err(r#""\u12""#), (1, 2, InvalidEscape));
        assert_eq!(err(r#""\u12G4""#), (1, 2, InvalidEscape));
        assert_eq!(err(r#""\u+123""#), (1, 2, InvalidEscape));
        assert_eq!(err(r#""\u12"#), (1, 2, end("four hex digits")));
        assert_eq!(err(r#""\u"#), (1, 2, end("four hex digits")));
    }

    #[test]
    fn unpaired_surrogates_are_rejected() {
        assert_eq!(err(r#""\ud800""#), (1, 2, LoneSurrogate));
        assert_eq!(err(r#""\udc00""#), (1, 2, LoneSurrogate));
        assert_eq!(err(r#""\ud800\u0041""#), (1, 2, LoneSurrogate));
        assert_eq!(err(r#""\ud800\ud83d\ude00""#), (1, 2, LoneSurrogate));
        assert_eq!(err(r#""\ud800A""#), (1, 2, LoneSurrogate));
        assert_eq!(err(r#""\ud800x""#), (1, 2, LoneSurrogate));
        assert_eq!(err(r#""\ud800\n""#), (1, 2, LoneSurrogate));
        assert_eq!(err(r#""ab\ude00\ud83d""#), (1, 4, LoneSurrogate));
        // The second half is itself malformed.
        assert_eq!(err(r#""\ud83d\uZZZZ""#), (1, 8, InvalidEscape));
    }

    #[test]
    fn raw_control_characters_in_strings_are_rejected() {
        assert_eq!(err("\"a\nb\""), (1, 3, ControlCharacter));
        assert_eq!(err("\"a\tb\""), (1, 3, ControlCharacter));
        assert_eq!(err("\"\0\""), (1, 2, ControlCharacter));
        assert_eq!(err("\"\u{1f}\""), (1, 2, ControlCharacter));
        assert_eq!(err("\"ok\",\n\"\r\""), (1, 5, found(',', END)));
    }

    // ------------------------------------------------- arrays and objects

    #[test]
    fn arrays() {
        assert_eq!(ok("[]"), ints(&[]));
        assert_eq!(ok("[ \n ]"), ints(&[]));
        assert_eq!(ok("[1]"), ints(&[1]));
        assert_eq!(ok(" [ 1 , 2 ,3 ] "), ints(&[1, 2, 3]));
        assert_eq!(
            ok(r#"[1, "a", null, true, [2, []], {}]"#),
            Value::Array(vec![
                Value::Int(1),
                string("a"),
                Value::Null,
                Value::Bool(true),
                Value::Array(vec![Value::Int(2), ints(&[])]),
                object(&[]),
            ])
        );
    }

    #[test]
    fn objects_keep_their_members_in_document_order() {
        assert_eq!(ok("{}"), object(&[]));
        assert_eq!(ok("{ }"), object(&[]));
        assert_eq!(
            ok(r#"{"zebra": 1, "apple": {"m": [], "b": null}, "": false}"#),
            object(&[
                ("zebra", Value::Int(1)),
                ("apple", object(&[("m", ints(&[])), ("b", Value::Null)])),
                ("", Value::Bool(false)),
            ])
        );
        // The same key in different objects is fine.
        assert_eq!(
            ok(r#"{"a": {"a": 1}, "b": {"a": 2}}"#),
            object(&[
                ("a", object(&[("a", Value::Int(1))])),
                ("b", object(&[("a", Value::Int(2))]))
            ])
        );
    }

    #[test]
    fn malformed_arrays_are_rejected() {
        assert_eq!(err("[1 2]"), (1, 4, found('2', ARRAY_NEXT)));
        assert_eq!(err("[1,,2]"), (1, 4, found(',', VALUE)));
        assert_eq!(err("[,1]"), (1, 2, found(',', VALUE)));
        assert_eq!(err("[,]"), (1, 2, found(',', VALUE)));
        assert_eq!(err("[1}"), (1, 3, found('}', ARRAY_NEXT)));
        assert_eq!(err("[1:2]"), (1, 3, found(':', ARRAY_NEXT)));
        assert_eq!(err("["), (1, 2, end(ARRAY_ITEM)));
        assert_eq!(err("[1"), (1, 3, end(ARRAY_NEXT)));
        assert_eq!(err("[1,"), (1, 4, end(ARRAY_ITEM)));
        assert_eq!(err("[[1]"), (1, 5, end(ARRAY_NEXT)));
        assert_eq!(err("]"), (1, 1, found(']', VALUE)));
    }

    #[test]
    fn malformed_objects_are_rejected() {
        assert_eq!(err("{,}"), (1, 2, found(',', KEY)));
        assert_eq!(err(r#"{"a" 1}"#), (1, 6, found('1', COLON)));
        assert_eq!(err(r#"{"a":}"#), (1, 6, found('}', VALUE)));
        assert_eq!(err(r#"{"a"}"#), (1, 5, found('}', COLON)));
        assert_eq!(err("{a: 1}"), (1, 2, found('a', KEY)));
        assert_eq!(err("{1: 2}"), (1, 2, found('1', KEY)));
        assert_eq!(err(r#"{"a": 1 "b": 2}"#), (1, 9, found('"', OBJECT_NEXT)));
        assert_eq!(err(r#"{"a":1]"#), (1, 7, found(']', OBJECT_NEXT)));
        assert_eq!(err(r#"{"a":1,,}"#), (1, 8, found(',', KEY)));
        assert_eq!(err(r#"{"a"=1}"#), (1, 5, found('=', COLON)));
        assert_eq!(err("{"), (1, 2, end(KEY)));
        assert_eq!(err(r#"{"a""#), (1, 5, end(COLON)));
        assert_eq!(err(r#"{"a":"#), (1, 6, end(VALUE)));
        assert_eq!(err(r#"{"a":1"#), (1, 7, end(OBJECT_NEXT)));
        assert_eq!(err(r#"{"a":1,"#), (1, 8, end(KEY)));
        assert_eq!(err("}"), (1, 1, found('}', VALUE)));
    }

    #[test]
    fn empty_and_trailing_text_is_rejected() {
        assert_eq!(err(""), (1, 1, end(VALUE)));
        assert_eq!(err("   "), (1, 4, end(VALUE)));
        assert_eq!(err("\n\n"), (3, 1, end(VALUE)));
        assert_eq!(err("// only a comment"), (1, 18, end(VALUE)));
        assert_eq!(err("1 2"), (1, 3, found('2', END)));
        assert_eq!(err("{} x"), (1, 4, found('x', END)));
        assert_eq!(err("[]]"), (1, 3, found(']', END)));
        assert_eq!(err("null,"), (1, 5, found(',', END)));
        assert_eq!(err(r#""a" "b""#), (1, 5, found('"', END)));
    }

    // -------------------------------------------------------------- JSONC

    #[test]
    fn line_comments() {
        assert_eq!(ok("// leading\n1"), Value::Int(1));
        assert_eq!(ok("1 // trailing"), Value::Int(1));
        assert_eq!(ok("1 //"), Value::Int(1));
        assert_eq!(ok("1//\n"), Value::Int(1));
        assert_eq!(ok("// one\r\n// two\r\n[1, // three\n 2]"), ints(&[1, 2]));
        // `#` does not start a comment.
        assert_eq!(err("# no\n1"), (1, 1, found('#', VALUE)));
    }

    #[test]
    fn block_comments() {
        assert_eq!(ok("/* leading */ 1 /* trailing */"), Value::Int(1));
        assert_eq!(ok("/**/1"), Value::Int(1));
        assert_eq!(ok("/***/1/****/"), Value::Int(1));
        assert_eq!(ok("/* * / ** // */1"), Value::Int(1));
        assert_eq!(ok("/* spans\n   several\n   lines */\n[1]"), ints(&[1]));
        assert_eq!(ok("/* a *//* b */1"), Value::Int(1));
    }

    #[test]
    fn comments_are_allowed_wherever_whitespace_is() {
        let text = r#"/*a*/ { /*b*/ "k" /*c*/ : /*d*/ [ /*e*/ 1 /*f*/ , /*g*/ 2 /*h*/ ] /*i*/ , // j
            "n" // k
            : // l
            null // m
        } // n"#;
        assert_eq!(
            ok(text),
            object(&[("k", ints(&[1, 2])), ("n", Value::Null)])
        );
    }

    #[test]
    fn malformed_comments_are_rejected() {
        // An unterminated comment is reported where it starts.
        assert_eq!(
            err("1 /* never closed"),
            (1, 3, end("`*/` to end the comment"))
        );
        assert_eq!(err("/*/"), (1, 1, end("`*/` to end the comment")));
        assert_eq!(
            err("[1, /* open\n\n"),
            (1, 5, end("`*/` to end the comment"))
        );
        // Block comments do not nest: the first `*/` ends the comment.
        assert_eq!(err("/* /* */ */ 1"), (1, 10, found('*', VALUE)));
        // A lone slash is not a comment.
        assert_eq!(err("/ 1"), (1, 1, found('/', VALUE)));
        assert_eq!(err("1 /"), (1, 3, found('/', END)));
        assert_eq!(err("[1 / 2]"), (1, 4, found('/', ARRAY_NEXT)));
    }

    #[test]
    fn trailing_commas() {
        assert_eq!(ok("[1,]"), ints(&[1]));
        assert_eq!(ok("[1, 2, ]"), ints(&[1, 2]));
        assert_eq!(
            ok("[[1,],[2,],]"),
            Value::Array(vec![ints(&[1]), ints(&[2])])
        );
        assert_eq!(ok(r#"{"a":1,}"#), object(&[("a", Value::Int(1))]));
        assert_eq!(ok("{\"a\": [1,\n],\n}"), object(&[("a", ints(&[1]))]));
        assert_eq!(ok("[1, /* gone */ ]"), ints(&[1]));
        assert_eq!(ok("[1, // gone\n]"), ints(&[1]));
    }

    #[test]
    fn only_one_trailing_comma_and_only_after_an_entry() {
        assert_eq!(err("[1,,]"), (1, 4, found(',', VALUE)));
        assert_eq!(err("[,]"), (1, 2, found(',', VALUE)));
        assert_eq!(err("{,}"), (1, 2, found(',', KEY)));
        assert_eq!(err(r#"{"a":1,,}"#), (1, 8, found(',', KEY)));
        // Not at the top level either.
        assert_eq!(err("1,"), (1, 2, found(',', END)));
    }

    #[test]
    fn a_leading_byte_order_mark_is_skipped() {
        assert_eq!(ok("\u{feff}1"), Value::Int(1));
        assert_eq!(ok("\u{feff} {}"), object(&[]));
        // It does not count as a column.
        assert_eq!(err("\u{feff}x"), (1, 1, found('x', VALUE)));
        assert_eq!(err("\u{feff}[1 2]"), (1, 4, found('2', ARRAY_NEXT)));
        // Only at the very start.
        assert_eq!(err("1\u{feff}"), (1, 2, found('\u{feff}', END)));
        assert_eq!(err("\u{feff}\u{feff}1"), (1, 1, found('\u{feff}', VALUE)));
    }

    // ----------------------------------------------------- duplicate keys

    #[test]
    fn duplicate_keys_are_rejected() {
        let duplicate = |key: &str| DuplicateKey {
            key: key.to_string(),
        };
        assert_eq!(err(r#"{"a":1,"a":2}"#), (1, 8, duplicate("a")));
        assert_eq!(err(r#"{"a":1,"b":2,"a":3}"#), (1, 14, duplicate("a")));
        assert_eq!(
            err("{\"hp\": 1,\n \"mp\": 2,\n \"hp\": 3}"),
            (3, 2, duplicate("hp"))
        );
        // Keys are compared after escapes are decoded.
        assert_eq!(err(r#"{"a":1,"\u0061":2}"#), (1, 8, duplicate("a")));
        assert_eq!(err(r#"{"\u00e9":1,"x":2,"é":3}"#), (1, 19, duplicate("é")));
        assert_eq!(err(r#"{"":1,"":2}"#), (1, 7, duplicate("")));
        // Nested objects are checked too.
        assert_eq!(err(r#"{"x":{"k":1,"k":2}}"#), (1, 13, duplicate("k")));
    }

    /// An object with keys `k0`, `k1`, ..., where `repeats` lists
    /// `(position, key number)` pairs to overwrite. Returns the text and
    /// the byte offset of each key.
    fn numbered_object(len: usize, repeats: &[(usize, usize)]) -> (String, Vec<usize>) {
        let mut text = String::from("{");
        let mut offsets = Vec::new();
        for position in 0..len {
            let number = repeats
                .iter()
                .find(|&&(at, _)| at == position)
                .map_or(position, |&(_, number)| number);
            if position > 0 {
                text.push(',');
            }
            offsets.push(text.len());
            text.push_str(&format!("\"k{number}\":{position}"));
        }
        text.push('}');
        (text, offsets)
    }

    #[test]
    fn duplicate_keys_are_found_in_objects_of_every_size() {
        // Sizes on both sides of the switch from comparing to sorting.
        for len in [2, 3, 15, 16, 17, 18, 33, 200] {
            let (text, _) = numbered_object(len, &[]);
            let members = ok(&text);
            assert_eq!(
                members.as_object().map(<[_]>::len),
                Some(len),
                "{len} distinct keys"
            );

            // The last key repeats the first, then the one just before it.
            for original in [0, len - 2] {
                let (text, offsets) = numbered_object(len, &[(len - 1, original)]);
                let column = offsets[len - 1] as u32 + 1;
                let expected = DuplicateKey {
                    key: format!("k{original}"),
                };
                assert_eq!(
                    err(&text),
                    (1, column, expected),
                    "{len} keys, last repeats k{original}"
                );
            }
        }
    }

    #[test]
    fn the_first_repeat_in_the_document_is_the_one_reported() {
        // Position 20 repeats k7 and position 25 repeats k3. Sorted by key,
        // k3 comes first; in the document, the repeat of k7 does.
        let (text, offsets) = numbered_object(40, &[(20, 7), (25, 3)]);
        assert_eq!(
            err(&text),
            (
                1,
                offsets[20] as u32 + 1,
                DuplicateKey {
                    key: "k7".to_string()
                }
            )
        );

        // The same key three times: the second occurrence is reported.
        let (text, offsets) = numbered_object(40, &[(10, 2), (30, 2)]);
        assert_eq!(
            err(&text),
            (
                1,
                offsets[10] as u32 + 1,
                DuplicateKey {
                    key: "k2".to_string()
                }
            )
        );
    }

    #[test]
    fn duplicate_positions_survive_nested_objects() {
        // A large inner object, parsed between the outer object's keys,
        // must not disturb the outer object's bookkeeping.
        let (inner, _) = numbered_object(30, &[]);
        let mut text = String::from("{");
        let mut offsets = Vec::new();
        for position in 0..20 {
            offsets.push(text.len());
            let key = if position == 19 { 4 } else { position };
            text.push_str(&format!("\"k{key}\":{inner},"));
        }
        text.push('}');
        assert_eq!(
            err(&text),
            (
                1,
                offsets[19] as u32 + 1,
                DuplicateKey {
                    key: "k4".to_string()
                }
            )
        );
    }

    // -------------------------------------------------------------- limits

    fn nested(open: &str, close: &str, depth: usize) -> String {
        format!("{}{}", open.repeat(depth), close.repeat(depth))
    }

    #[test]
    fn nesting_up_to_the_limit_is_accepted() {
        let limit = Limits::default().max_depth as usize;
        assert_eq!(limit, 64);
        let mut value = &ok(&nested("[", "]", limit));
        let mut depth = 0;
        while let Some(inner) = value.as_array() {
            depth += 1;
            match inner.first() {
                Some(next) => value = next,
                None => break,
            }
        }
        assert_eq!(depth, limit);
        assert!(
            parse(
                &nested("{\"a\":", "}", limit).replace(":}", ":1}"),
                Limits::default()
            )
            .is_ok()
        );
    }

    #[test]
    fn nesting_past_the_limit_is_an_error_at_the_bracket_that_goes_too_deep() {
        assert_eq!(err(&nested("[", "]", 65)), (1, 65, TooDeep { limit: 64 }));
        assert_eq!(err(&"[".repeat(65)), (1, 65, TooDeep { limit: 64 }));
        // Objects and arrays count alike: 32 of each is fine, one more is not.
        let mixed = "{\"a\":[".repeat(32);
        assert_eq!(
            err(&format!("{mixed}{{")),
            (1, 32 * 6 + 1, TooDeep { limit: 64 })
        );
        assert_eq!(
            err(&format!("{mixed}[")),
            (1, 32 * 6 + 1, TooDeep { limit: 64 })
        );
        assert_eq!(err(&format!("{mixed}1")), (1, 32 * 6 + 2, end(ARRAY_NEXT)));
    }

    #[test]
    fn very_deep_nesting_is_an_error_not_a_stack_overflow() {
        assert_eq!(err(&"[".repeat(1_000_000)), (1, 65, TooDeep { limit: 64 }));
        assert_eq!(
            err(&"{\"a\":".repeat(1_000_000)),
            (1, 64 * 5 + 1, TooDeep { limit: 64 })
        );
        assert_eq!(
            err(&"[{\"a\":".repeat(500_000)),
            (1, 32 * 6 + 1, TooDeep { limit: 64 })
        );
    }

    #[test]
    fn the_depth_limit_is_configurable() {
        let depth = |max_depth| Limits {
            max_depth,
            ..Limits::default()
        };
        assert!(parse("1", depth(0)).is_ok());
        assert_eq!(err_with("[]", depth(0)), (1, 1, TooDeep { limit: 0 }));
        assert_eq!(err_with("{}", depth(0)), (1, 1, TooDeep { limit: 0 }));
        assert!(parse("[[1], {\"a\": 2}]", depth(2)).is_ok());
        assert_eq!(err_with("[[[1]]]", depth(2)), (1, 3, TooDeep { limit: 2 }));
        assert_eq!(
            err_with("[[1], {\"a\": [2]}]", depth(2)),
            (1, 13, TooDeep { limit: 2 })
        );
        assert!(parse(&nested("[", "]", 300), depth(300)).is_ok());
    }

    #[test]
    fn strings_longer_than_the_limit_are_rejected() {
        let limits = Limits {
            max_string_bytes: 5,
            ..Limits::default()
        };
        let too_long = StringTooLong { limit: 5 };
        assert!(parse(r#""12345""#, limits).is_ok());
        assert_eq!(err_with(r#""123456""#, limits), (1, 1, too_long.clone()));
        // The limit is on the decoded bytes: `é` is two, however written.
        assert!(parse(r#""\u00e9\u00e9x""#, limits).is_ok());
        assert!(parse("\"ééx\"", limits).is_ok());
        assert_eq!(
            err_with(r#""\u00e9\u00e9xy""#, limits),
            (1, 1, too_long.clone())
        );
        assert_eq!(
            err_with(r#""1234\u00e9""#, limits),
            (1, 1, too_long.clone())
        );
        assert!(parse(r#""123\u00e9""#, limits).is_ok());
        assert_eq!(err_with("\"ééé\"", limits), (1, 1, too_long.clone()));
        assert_eq!(err_with(r#""12345\n""#, limits), (1, 1, too_long.clone()));
        // Keys are strings too. The error points at the string's start.
        assert_eq!(
            err_with(r#"{"123456": 1}"#, limits),
            (1, 2, too_long.clone())
        );
        assert_eq!(err_with(r#"["ok", "toolong"]"#, limits), (1, 8, too_long));
        // An empty limit still allows the empty string.
        let none = Limits {
            max_string_bytes: 0,
            ..Limits::default()
        };
        assert!(parse(r#""""#, none).is_ok());
        assert_eq!(err_with(r#""a""#, none), (1, 1, StringTooLong { limit: 0 }));
    }

    #[test]
    fn the_default_string_limit_is_one_mebibyte() {
        let exactly = format!("\"{}\"", "x".repeat(1024 * 1024));
        assert!(parse(&exactly, Limits::default()).is_ok());
        let over = format!("\"{}\"", "x".repeat(1024 * 1024 + 1));
        assert_eq!(err(&over), (1, 1, StringTooLong { limit: 1024 * 1024 }));
    }

    #[test]
    fn containers_with_too_many_entries_are_rejected() {
        let limits = Limits {
            max_entries: 3,
            ..Limits::default()
        };
        let too_many = TooManyEntries { limit: 3 };
        assert!(parse("[1,2,3]", limits).is_ok());
        assert!(parse("[1,2,3,]", limits).is_ok());
        // The error points at the entry that does not fit.
        assert_eq!(err_with("[1,2,3,4]", limits), (1, 8, too_many.clone()));
        assert!(parse(r#"{"a":1,"b":2,"c":3}"#, limits).is_ok());
        assert_eq!(
            err_with(r#"{"a":1,"b":2,"c":3,"d":4}"#, limits),
            (1, 20, too_many.clone())
        );
        // The limit is per container, not for the whole document.
        assert!(parse("[[1,2,3],[4,5,6],[7,8,9]]", limits).is_ok());
        assert_eq!(err_with("[[1,2,3],[4,5,6,7]]", limits), (1, 17, too_many));
        let none = Limits {
            max_entries: 0,
            ..Limits::default()
        };
        assert!(parse("[]", none).is_ok());
        assert_eq!(err_with("[1]", none), (1, 2, TooManyEntries { limit: 0 }));
    }

    // ------------------------------------------------------ error positions

    #[test]
    fn errors_report_the_line() {
        let text = "{\n  \"a\": 1,\n  \"b\": oops\n}";
        assert_eq!(err(text), (3, 8, found('o', VALUE)));
        // Windows line endings count once.
        assert_eq!(err("[\r\n  1,\r\n  x\r\n]"), (3, 3, found('x', VALUE)));
        // Lines inside comments count.
        assert_eq!(
            err("// one\n/* two\nthree */\n?"),
            (4, 1, found('?', VALUE))
        );
        assert_eq!(err("[\n\n\n"), (4, 1, end(ARRAY_ITEM)));
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        // `é` is 2 bytes, `€` is 3 and `😀` is 4; each is one column.
        assert_eq!(err("[\"é€😀\", x]"), (1, 9, found('x', VALUE)));
        assert_eq!(err("\t\tx"), (1, 3, found('x', VALUE)));
        assert_eq!(err("/* é */ é"), (1, 9, found('é', VALUE)));
        assert_eq!(err("[1]\u{a0}"), (1, 4, found('\u{a0}', END)));
    }

    #[test]
    fn errors_display_with_their_position() {
        let error = parse("[1 2]", Limits::default()).expect_err("missing comma");
        assert_eq!(
            error.to_string(),
            "JSON error at line 1, column 4: found '2' where `,` or `]` was expected"
        );
        let error = parse("{\n  \"a\": ", Limits::default()).expect_err("missing value");
        assert_eq!(
            error.to_string(),
            "JSON error at line 2, column 8: the text ends where a value was expected"
        );
    }
}
