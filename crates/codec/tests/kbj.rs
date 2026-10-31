//! KBJ property tests: random value trees survive encoding and decoding,
//! decoding then encoding gives back the same bytes, the JSON test
//! documents survive the trip, and damaged files never panic. The
//! hand-computed vectors and each error are beside the codec, in
//! `src/kbj.rs`.

use nadir_codec::json::{self, Value};
use nadir_codec::{CodecError, Limits, kbj};
use nadir_core::Pcg32;
use nadir_core::testing::{check, check_sized};

/// A random value tree about `size` nodes large, with the edge cases of
/// each kind: integers at the edges of their LEB128 lengths, `-0.0`,
/// empty and multi-byte strings, and objects past the size where the
/// duplicate-key check sorts.
fn random_value(rng: &mut Pcg32, size: u32) -> Value {
    let mut budget = size;
    value(rng, &mut budget, 0)
}

fn value(rng: &mut Pcg32, budget: &mut u32, depth: u32) -> Value {
    *budget = budget.saturating_sub(1);
    let kinds = if *budget == 0 || depth > 6 { 5 } else { 7 };
    match rng.range(0..kinds) {
        0 => Value::Null,
        1 => Value::Bool(rng.range(0..2u32) == 1),
        2 => Value::Int(match rng.range(0..4u32) {
            0 => i64::MIN + i64::from(rng.range(0..3u32)),
            1 => i64::MAX - i64::from(rng.range(0..3u32)),
            // Around 2^(7k), where the LEB128 length changes.
            2 => {
                let k = rng.range(1..10u32);
                let edge = 1i64.checked_shl(7 * k).unwrap_or(i64::MAX) / 2;
                edge + i64::from(rng.range(-2..3i32))
            }
            _ => i64::from(rng.range(-1000..1000i32)),
        }),
        3 => Value::Float(match rng.range(0..6u32) {
            0 => -0.0,
            1 => f64::from(rng.next_f32()) * 1e300,
            2 => f64::from_bits(1 + u64::from(rng.range(0..1000u32))),
            3 => *rng
                .choose(&[f64::INFINITY, f64::NEG_INFINITY, f64::MAX, -f64::MAX])
                .expect("edges"),
            4 => f64::from_bits(0x7ff8_0000_0000_0000),
            _ => f64::from(rng.range(-100..100i32)) / 8.0,
        }),
        4 => Value::String(text(rng)),
        5 => {
            let n = rng.range(0..(*budget).min(20) + 1);
            Value::Array((0..n).map(|_| value(rng, budget, depth + 1)).collect())
        }
        _ => {
            let n = rng.range(0..(*budget).min(30) + 1);
            Value::Object(
                (0..n)
                    .map(|i| (format!("{}{i}", text(rng)), value(rng, budget, depth + 1)))
                    .collect(),
            )
        }
    }
}

/// Short pieces of text, multi-byte and control characters among them,
/// and now and then one long enough for a two-byte length.
fn text(rng: &mut Pcg32) -> String {
    let pieces = ["", "a", "key", "é", "日本", "🦀", "\u{0}", "\""];
    let mut out = String::new();
    for _ in 0..rng.range(0..3u32) {
        if rng.range(0..10u32) == 0 {
            out.push_str(&"x".repeat(130));
        } else {
            out.push_str(rng.choose(&pieces).expect("pieces"));
        }
    }
    out
}

fn decode(bytes: &[u8]) -> Result<Value, CodecError> {
    kbj::decode(bytes, Limits::default())
}

fn encode(value: &Value) -> Vec<u8> {
    kbj::encode(value, Limits::default()).expect("a value within the limits")
}

/// Equality that compares floats by their bits, so that `-0.0` is not
/// `0.0` and NaN is itself.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Float(x), Value::Float(y)) => x.to_bits() == y.to_bits(),
        (Value::Array(xs), Value::Array(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| same(x, y))
        }
        (Value::Object(xs), Value::Object(ys)) => {
            xs.len() == ys.len()
                && xs
                    .iter()
                    .zip(ys)
                    .all(|((kx, x), (ky, y))| kx == ky && same(x, y))
        }
        _ => a == b,
    }
}

#[test]
fn random_values_survive_a_round_trip() {
    check_sized("kbj round trip", 500, random_value, |value| {
        let bytes = encode(value);
        let back = decode(&bytes).map_err(|error| format!("cannot decode: {error}"))?;
        if !same(&back, value) {
            return Err(format!("decodes as {back:?}"));
        }
        // The one encoding: what decodes encodes back to the same bytes.
        if encode(&back) != bytes {
            return Err("a second encoding differs".to_string());
        }
        Ok(())
    });
}

#[test]
fn the_json_test_documents_survive_a_round_trip() {
    let dir = format!("{}/tests/json", env!("CARGO_MANIFEST_DIR"));
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let Ok(value) = json::parse(&text, Limits::default()) else {
            continue;
        };
        let bytes = encode(&value);
        assert!(same(&decode(&bytes).unwrap(), &value), "{}", path.display());
        count += 1;
    }
    assert!(count >= 4, "{count}");
}

#[test]
fn every_file_cut_short_is_rejected() {
    check_sized(
        "kbj prefixes",
        100,
        |rng, size| encode(&random_value(rng, size / 2)),
        |bytes| {
            for len in 0..bytes.len() {
                match decode(&bytes[..len]) {
                    Err(CodecError::Kbj { .. }) => {}
                    other => return Err(format!("{len} bytes give {other:?}")),
                }
            }
            Ok(())
        },
    );
}

#[test]
fn damaged_files_are_decoded_or_rejected_without_a_panic() {
    check(
        "kbj damage",
        3000,
        |rng| {
            let mut bytes = encode(&random_value(rng, 40));
            for _ in 0..1 + rng.range(0..4u32) {
                let at = rng.range(0..bytes.len() as u32) as usize;
                bytes[at] = rng.range(0..256u32) as u8;
            }
            bytes
        },
        |bytes| match decode(bytes) {
            // Whatever decodes is the one encoding of what it decodes to.
            Ok(value) if encode(&value) == *bytes => Ok(()),
            Ok(value) => Err(format!("{value:?} encodes to other bytes")),
            Err(CodecError::Kbj { .. }) => Ok(()),
            Err(other) => Err(format!("not a KBJ error: {other:?}")),
        },
    );
}
