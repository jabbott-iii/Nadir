//! JSON tests against the hand-made vectors in `tests/json/`, and property
//! tests of the parser and the writers working together.

use nadir_codec::json::{self, ErrorKind, Value};
use nadir_codec::{CodecError, Limits};
use nadir_core::Pcg32;
use nadir_core::testing::{check, check_sized};

fn parse(text: &str) -> Result<Value, CodecError> {
    json::parse(text, Limits::default())
}

fn parse_ok(text: &str) -> Value {
    parse(text).unwrap_or_else(|error| panic!("{error}\nin:\n{text}"))
}

fn position_and_kind(text: &str) -> (u32, u32, ErrorKind) {
    match parse(text) {
        Err(CodecError::Json { line, column, kind }) => (line, column, kind),
        Err(other) => panic!("not a JSON error: {other}"),
        Ok(value) => panic!("should not parse, but gave {value:?}"),
    }
}

fn keys(value: &Value) -> Vec<&str> {
    value
        .as_object()
        .expect("an object")
        .iter()
        .map(|(key, _)| key.as_str())
        .collect()
}

fn str_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("no string {key:?} in {value:?}"))
}

/// Equality that also tells `0.0` from `-0.0` and an integer from a float.
fn identical(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| identical(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|((ka, va), (kb, vb))| ka == kb && identical(va, vb))
        }
        _ => a == b,
    }
}

// ------------------------------------------------------------ RFC 8259

const RFC_IMAGE: &str = include_str!("json/rfc8259_image.json");
const RFC_LOCATIONS: &str = include_str!("json/rfc8259_locations.json");

#[test]
fn rfc_8259_image_example() {
    let document = parse_ok(RFC_IMAGE);
    assert_eq!(keys(&document), ["Image"]);
    let image = document.get("Image").expect("Image");
    assert_eq!(
        keys(image),
        ["Width", "Height", "Title", "Thumbnail", "Animated", "IDs"]
    );
    assert_eq!(image.get("Width"), Some(&Value::Int(800)));
    assert_eq!(image.get("Height"), Some(&Value::Int(600)));
    assert_eq!(str_of(image, "Title"), "View from 15th Floor");
    assert_eq!(image.get("Animated"), Some(&Value::Bool(false)));
    let ids: Vec<Value> = [116, 943, 234, 38793].map(Value::Int).to_vec();
    assert_eq!(image.get("IDs"), Some(&Value::Array(ids)));

    let thumbnail = image.get("Thumbnail").expect("Thumbnail");
    assert_eq!(keys(thumbnail), ["Url", "Height", "Width"]);
    // The `//` in the URL is inside a string, so it is not a comment.
    assert_eq!(
        str_of(thumbnail, "Url"),
        "http://www.example.com/image/481989943"
    );
    assert_eq!(thumbnail.get("Height").and_then(Value::as_i64), Some(125));
    assert_eq!(thumbnail.get("Width").and_then(Value::as_i64), Some(100));
}

#[test]
fn rfc_8259_locations_example() {
    let document = parse_ok(RFC_LOCATIONS);
    let places = document.as_array().expect("an array");
    assert_eq!(places.len(), 2);
    let expected_keys = [
        "precision",
        "Latitude",
        "Longitude",
        "Address",
        "City",
        "State",
        "Zip",
        "Country",
    ];

    let san_francisco = &places[0];
    assert_eq!(keys(san_francisco), expected_keys);
    assert_eq!(str_of(san_francisco, "precision"), "zip");
    assert_eq!(san_francisco.get("Latitude"), Some(&Value::Float(37.7668)));
    assert_eq!(
        san_francisco.get("Longitude"),
        Some(&Value::Float(-122.3959))
    );
    assert_eq!(str_of(san_francisco, "Address"), "");
    assert_eq!(str_of(san_francisco, "City"), "SAN FRANCISCO");
    assert_eq!(str_of(san_francisco, "State"), "CA");
    // A zip code in quotes is a string, not a number.
    assert_eq!(str_of(san_francisco, "Zip"), "94107");
    assert_eq!(str_of(san_francisco, "Country"), "US");

    let sunnyvale = &places[1];
    assert_eq!(keys(sunnyvale), expected_keys);
    assert_eq!(sunnyvale.get("Latitude"), Some(&Value::Float(37.371991)));
    assert_eq!(sunnyvale.get("Longitude"), Some(&Value::Float(-122.02602)));
    assert_eq!(str_of(sunnyvale, "City"), "SUNNYVALE");
    assert_eq!(str_of(sunnyvale, "Zip"), "94085");
}

#[test]
fn rfc_8259_scalar_examples() {
    // "Here are three small JSON texts containing only values."
    assert_eq!(
        parse_ok("\"Hello world!\""),
        Value::String("Hello world!".to_string())
    );
    assert_eq!(parse_ok("42"), Value::Int(42));
    assert_eq!(parse_ok("true"), Value::Bool(true));
}

#[test]
fn rfc_8259_examples_survive_being_written_and_read_back() {
    for text in [RFC_IMAGE, RFC_LOCATIONS] {
        let value = parse_ok(text);
        assert!(identical(
            &parse_ok(&json::to_string_compact(&value)),
            &value
        ));
        assert!(identical(
            &parse_ok(&json::to_string_pretty(&value)),
            &value
        ));
    }
    let image = json::to_string_compact(&parse_ok(RFC_IMAGE));
    assert!(image.starts_with(
        r#"{"Image":{"Width":800,"Height":600,"Title":"View from 15th Floor","Thumbnail":{"#
    ));
    assert!(image.ends_with(r#""Animated":false,"IDs":[116,943,234,38793]}}"#));
}

// --------------------------------------------------------------- JSONC

const PREFAB: &str = include_str!("json/prefab.jsonc");
const PREFAB_PRETTY: &str = include_str!("json/prefab.pretty.json");

#[test]
fn jsonc_prefab_parses_with_comments_and_trailing_commas() {
    let prefab = parse_ok(PREFAB);
    assert_eq!(keys(&prefab), ["name", "extends", "components"]);
    assert_eq!(str_of(&prefab, "name"), "slime");
    assert_eq!(str_of(&prefab, "extends"), "enemy_base");

    let components = prefab.get("components").expect("components");
    assert_eq!(
        keys(components),
        ["Sprite", "Health", "Mover", "Collider", "Loot"]
    );
    let mover = components.get("Mover").expect("Mover");
    assert_eq!(mover.get("speed"), Some(&Value::Float(22.5)));
    assert_eq!(mover.get("friction"), Some(&Value::Float(0.01)));
    assert_eq!(mover.get("flying"), Some(&Value::Bool(false)));

    let collider = components.get("Collider").expect("Collider");
    let mask = collider
        .get("mask")
        .and_then(Value::as_array)
        .expect("mask");
    assert_eq!(
        mask,
        [
            Value::String("World".to_string()),
            Value::String("Player".to_string())
        ]
    );
    assert_eq!(
        collider.get("offset"),
        Some(&Value::Array(vec![Value::Int(0), Value::Float(-3.0)]))
    );
    let radius = collider
        .get("shape")
        .and_then(|shape| shape.get("Circle"))
        .and_then(|circle| circle.get("radius"));
    assert_eq!(radius, Some(&Value::Int(6)));

    let loot = components.get("Loot").expect("Loot");
    assert!(loot.get("table").is_some_and(Value::is_null));
    assert_eq!(
        str_of(loot, "note"),
        "50% → \"gel\"\t(tab), url: http://example.com/a//b"
    );
}

#[test]
fn jsonc_prefab_pretty_prints_to_the_committed_file() {
    // The committed file may be checked out with Windows line endings.
    let expected = PREFAB_PRETTY.replace("\r\n", "\n");
    assert_eq!(
        json::to_string_pretty(&parse_ok(PREFAB)),
        expected.trim_end()
    );
    // The pretty form is strict JSON for the same value.
    assert!(identical(&parse_ok(PREFAB_PRETTY), &parse_ok(PREFAB)));
}

#[test]
fn malformed_files_report_line_and_column() {
    let expected = ErrorKind::UnexpectedCharacter {
        found: '"',
        expected: "`,` or `}`",
    };
    assert_eq!(
        position_and_kind(include_str!("json/invalid_missing_comma.jsonc")),
        (4, 3, expected)
    );

    let expected = ErrorKind::DuplicateKey {
        key: "Health".to_string(),
    };
    assert_eq!(
        position_and_kind(include_str!("json/invalid_duplicate_key.jsonc")),
        (6, 5, expected)
    );

    let expected = ErrorKind::UnexpectedEnd {
        expected: "`*/` to end the comment",
    };
    assert_eq!(
        position_and_kind(include_str!("json/invalid_unterminated_comment.jsonc")),
        (3, 3, expected)
    );
}

#[test]
fn escapes_file_decodes_every_kind_of_escape() {
    let document = parse_ok(include_str!("json/escapes.json"));
    assert_eq!(
        keys(&document),
        ["simple", "bmp", "pair", "control", "mixed", "key"]
    );
    assert_eq!(
        str_of(&document, "simple"),
        "quote \" backslash \\ slash / \u{8}\u{c}\n\r\t"
    );
    assert_eq!(str_of(&document, "bmp"), "Aé€");
    assert_eq!(str_of(&document, "pair"), "😀😀");
    assert_eq!(str_of(&document, "control"), "\0\u{1f}\u{7f}");
    assert_eq!(str_of(&document, "mixed"), "abc é é 😀 😀");
    assert_eq!(str_of(&document, "key"), "escapes work in keys too");
}

// ---------------------------------------------------------- generators

const INTERESTING_CHARS: &[char] = &[
    'a',
    'Z',
    '0',
    ' ',
    '"',
    '\\',
    '/',
    '\n',
    '\r',
    '\t',
    '\u{8}',
    '\u{c}',
    '\0',
    '\u{1f}',
    '\u{7f}',
    '\u{80}',
    'é',
    '€',
    '\u{2028}',
    '\u{feff}',
    '\u{fffd}',
    '\u{ffff}',
    '😀',
    '\u{10ffff}',
    '*',
    '{',
    '[',
    ',',
    ':',
    'u',
];

fn random_char(rng: &mut Pcg32) -> char {
    match rng.range(0..4u32) {
        0 => char::from_u32(rng.range(0x20..0x7fu32)).unwrap_or('?'),
        // Surrogates are not characters; fall back to something that is.
        1 => char::from_u32(rng.range(0..0x11_0000u32)).unwrap_or('\u{fffd}'),
        _ => rng.choose(INTERESTING_CHARS).copied().unwrap_or('?'),
    }
}

fn random_string(rng: &mut Pcg32) -> String {
    (0..rng.range(0..12u32)).map(|_| random_char(rng)).collect()
}

fn random_int(rng: &mut Pcg32) -> i64 {
    match rng.range(0..4u32) {
        0 => i64::from(rng.range(-100..100)),
        1 => i64::from(rng.next_u32() as i32),
        2 => (i64::from(rng.next_u32()) << 32) | i64::from(rng.next_u32()),
        _ => rng
            .choose(&[0, 1, -1, i64::MIN, i64::MAX, 1 << 53, -(1 << 53)])
            .copied()
            .unwrap_or(0),
    }
}

/// A finite float.
fn random_float(rng: &mut Pcg32) -> f64 {
    const SPECIAL: &[f64] = &[
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.1,
        1e-5,
        9.999e-6,
        1e16,
        9_999_999_999_999_998.0,
        f64::MAX,
        f64::MIN,
        5e-324,
        1e-320,
    ];
    match rng.range(0..5u32) {
        0 => f64::from(rng.range(-1000.0..1000.0f32)),
        1 => f64::from(rng.range(-1000..1000)),
        2 => 10f64.powi(rng.range(-320..309)),
        3 => loop {
            let bits = (u64::from(rng.next_u32()) << 32) | u64::from(rng.next_u32());
            let value = f64::from_bits(bits);
            if value.is_finite() {
                break value;
            }
        },
        _ => rng.choose(SPECIAL).copied().unwrap_or(0.0),
    }
}

/// A value with about `size` entries in its containers, nested at most six
/// deep, with no duplicate keys and no non-finite floats: everything the
/// writers promise to round-trip.
fn random_value(rng: &mut Pcg32, size: u32) -> Value {
    let mut budget = size;
    random_value_at(rng, &mut budget, 0)
}

fn random_value_at(rng: &mut Pcg32, budget: &mut u32, depth: u32) -> Value {
    let kinds = if *budget == 0 || depth >= 6 { 5 } else { 7 };
    match rng.range(0..kinds) {
        0 => Value::Null,
        1 => Value::Bool(rng.next_u32() & 1 == 0),
        2 => Value::Int(random_int(rng)),
        3 => Value::Float(random_float(rng)),
        4 => Value::String(random_string(rng)),
        container => {
            let len = rng.range(0..(*budget).min(6) + 1);
            *budget -= len;
            if container == 5 {
                Value::Array(
                    (0..len)
                        .map(|_| random_value_at(rng, budget, depth + 1))
                        .collect(),
                )
            } else {
                let mut members: Vec<(String, Value)> = Vec::new();
                for index in 0..len {
                    let mut key = random_string(rng);
                    if members.iter().any(|(existing, _)| *existing == key) {
                        key.push_str(&format!("#{index}"));
                    }
                    members.push((key, random_value_at(rng, budget, depth + 1)));
                }
                Value::Object(members)
            }
        }
    }
}

/// Fragments that are, or look like, pieces of JSON.
const TOKENS: &[&str] = &[
    "{", "}", "[", "]", ",", ":", "\"", "\\", "\\u", "\\ud83d", "\\ude00", "00e9", "/", "*", "//",
    "/*", "*/", "\n", "\r\n", " ", "\t", "null", "true", "false", "nul", "0", "1", "-", ".", "e",
    "E", "+", "12", "-0", "1e5", "a", "é", "😀", "\u{feff}", "\0", "\"a\"", "\"k\":", "[]", "{}",
    "\"\"",
];

fn random_tokens(rng: &mut Pcg32, count: u32) -> String {
    (0..count)
        .map(|_| rng.choose(TOKENS).copied().unwrap_or(""))
        .collect()
}

/// Checks the things that must hold for any text at all: the parser
/// returns instead of panicking; an error points inside the text; and
/// whatever parses can be written and read back unchanged.
fn parser_behaves(text: &str) -> Result<(), String> {
    match parse(text) {
        Ok(value) => {
            for written in [
                json::to_string_compact(&value),
                json::to_string_pretty(&value),
            ] {
                let back = parse(&written)
                    .map_err(|error| format!("wrote {written:?}, which fails: {error}"))?;
                if !identical(&back, &value) {
                    return Err(format!("wrote {written:?}, which reads back as {back:?}"));
                }
            }
            Ok(())
        }
        Err(CodecError::Json { line, column, kind }) => {
            let text = text.strip_prefix('\u{feff}').unwrap_or(text);
            let lines: Vec<&str> = text.split('\n').collect();
            let Some(line_text) = (line as usize)
                .checked_sub(1)
                .and_then(|index| lines.get(index))
            else {
                return Err(format!(
                    "{kind:?} reported at line {line}, but there are {} lines",
                    lines.len()
                ));
            };
            let columns = line_text.chars().count() as u32 + 1;
            if !(1..=columns).contains(&column) {
                return Err(format!(
                    "{kind:?} reported at line {line}, column {column} of {columns}"
                ));
            }
            Ok(())
        }
        Err(other) => Err(format!("not a JSON error: {other}")),
    }
}

// ---------------------------------------------------------- properties

#[test]
fn values_survive_being_written_and_read_back() {
    check_sized("json round-trip", 500, random_value, |value| {
        for written in [
            json::to_string_compact(value),
            json::to_string_pretty(value),
        ] {
            let back = parse(&written)
                .map_err(|error| format!("wrote {written:?}, which fails: {error}"))?;
            if !identical(&back, value) {
                return Err(format!("wrote {written:?}, which reads back as {back:?}"));
            }
        }
        Ok(())
    });
}

/// Writes `text` as a JSON string in which every character is a `\u`
/// escape. Characters outside the basic plane become surrogate pairs.
fn fully_escaped(text: &str) -> String {
    let mut out = String::from("\"");
    for unit in text.encode_utf16() {
        // Hex digits are valid in either case; use both.
        let digits = if unit & 1 == 0 {
            format!("{unit:04x}")
        } else {
            format!("{unit:04X}")
        };
        out.push('\\');
        out.push('u');
        out.push_str(&digits);
    }
    out.push('"');
    out
}

#[test]
fn escaped_strings_decode_to_the_same_text() {
    check("json unicode escapes", 1000, random_string, |text| {
        let escaped = fully_escaped(text);
        match parse(&escaped) {
            Ok(Value::String(decoded)) if decoded == *text => Ok(()),
            other => Err(format!("{escaped} decoded as {other:?}")),
        }
    });
}

#[test]
fn compact_output_is_a_single_line() {
    check_sized("json compact is one line", 200, random_value, |value| {
        let written = json::to_string_compact(value);
        if written.contains('\n') || written.contains('\r') {
            return Err(format!("wrote {written:?}"));
        }
        Ok(())
    });
}

/// Rewrites pretty-printed JSON with the JSONC extensions: a trailing comma
/// after the last entry of every container, and comments at random.
fn decorate(pretty: &str, rng: &mut Pcg32) -> String {
    let lines: Vec<&str> = pretty.lines().collect();
    let mut out = String::from("// header\n/* block\n   comment */\n");
    for (index, line) in lines.iter().enumerate() {
        out.push_str(line);
        // The last entry of a container is followed by a line that closes
        // it. A line that opens a container is not an entry.
        let closes_next = lines
            .get(index + 1)
            .is_some_and(|next| next.trim_start().starts_with(['}', ']']));
        if closes_next && !line.ends_with(['{', '[']) {
            out.push(',');
        }
        match rng.range(0..6u32) {
            0 => out.push_str(" // note \" [ { , */"),
            1 => out.push_str(" /* note \" ] } , // */"),
            2 => out.push_str("\n   /* a line\n      of its own */"),
            _ => {}
        }
        out.push('\n');
    }
    out
}

#[test]
fn comments_and_trailing_commas_do_not_change_the_value() {
    check_sized(
        "jsonc decoration",
        400,
        |rng, size| {
            let value = random_value(rng, size);
            let decorated = decorate(&json::to_string_pretty(&value), rng);
            (value, decorated)
        },
        |(value, decorated)| {
            let back = parse(decorated).map_err(|error| error.to_string())?;
            if !identical(&back, value) {
                return Err(format!("read back as {back:?}"));
            }
            Ok(())
        },
    );
}

#[test]
fn random_token_soup_never_panics() {
    check_sized(
        "json token soup",
        2000,
        |rng, size| random_tokens(rng, size / 4 + 1),
        |text| parser_behaves(text),
    );
}

#[test]
fn random_characters_never_panic() {
    check_sized(
        "json random characters",
        1000,
        |rng, size| (0..size).map(|_| random_char(rng)).collect::<String>(),
        |text| parser_behaves(text),
    );
}

/// One random edit: delete, insert, replace, truncate, or repeat a piece.
fn mutate(text: &mut String, rng: &mut Pcg32) {
    let chars: Vec<char> = text.chars().collect();
    let at = rng.range(0..chars.len() as u32 + 1) as usize;
    let (before, after) = chars.split_at(at);
    let before: String = before.iter().collect();
    let after: String = after.iter().collect();
    let token = rng.choose(TOKENS).copied().unwrap_or("");
    *text = match rng.range(0..5u32) {
        0 => before + &after.chars().skip(1).collect::<String>(),
        1 => before + token + &after,
        2 => before + token + &after.chars().skip(1).collect::<String>(),
        3 => before,
        _ => before.clone() + &after + &after,
    };
}

#[test]
fn mutated_documents_never_panic() {
    check_sized(
        "json mutation",
        2000,
        |rng, size| {
            let value = random_value(rng, size);
            let mut text = if rng.next_u32() & 1 == 0 {
                json::to_string_compact(&value)
            } else {
                json::to_string_pretty(&value)
            };
            for _ in 0..rng.range(1..4u32) {
                mutate(&mut text, rng);
            }
            text
        },
        |text| parser_behaves(text),
    );
}

#[test]
fn nesting_is_accepted_exactly_up_to_the_limit() {
    check(
        "json depth limit",
        300,
        |rng| {
            // Open a random mix of arrays and objects, then close them.
            let depth = rng.range(0..200u32);
            let kinds: Vec<bool> = (0..depth).map(|_| rng.next_u32() & 1 == 0).collect();
            let open: String = kinds
                .iter()
                .map(|&array| if array { "[" } else { "{\"k\":" })
                .collect();
            let close: String = kinds
                .iter()
                .rev()
                .map(|&array| if array { "]" } else { "}" })
                .collect();
            (depth, format!("{open}null{close}"))
        },
        |(depth, text)| match parse(text) {
            Ok(_) if *depth <= 64 => Ok(()),
            Err(CodecError::Json {
                kind: ErrorKind::TooDeep { limit: 64 },
                ..
            }) if *depth > 64 => Ok(()),
            other => Err(format!("depth {depth} gave {other:?}")),
        },
    );
}
