//! JSON benchmarks.

use std::hint::black_box;

use nadir_codec::Limits;
use nadir_codec::json::{self, Value};
use nadir_core::Pcg32;

use crate::harness::Bencher;

/// How many records the benchmark document holds.
const RECORDS: usize = 1000;

/// `json_parse`: parses a pretty-printed document of [`RECORDS`] entity
/// records, about the shape and size of a level's entity list.
pub(crate) fn parse(bencher: &mut Bencher) {
    let text = document(RECORDS);
    bencher.iter(|| json::parse(black_box(&text), Limits::default()));
}

/// Builds the benchmark document: an object with an array of `records`
/// entity records. The same every time.
fn document(records: usize) -> String {
    let mut rng = Pcg32::new(0x6a73_6f6e, 0);
    let kinds = ["player", "slime", "chest", "door", "torch", "sign"];
    let entities = (0..records)
        .map(|index| {
            let kind = rng.choose(&kinds).copied().unwrap_or("sign");
            let position = Value::Array(vec![
                Value::Float(f64::from(rng.range(0..4096u32)) * 0.25),
                Value::Float(f64::from(rng.range(0..4096u32)) * 0.25),
            ]);
            let tags = (0..rng.range(0..4u32))
                .map(|tag| Value::String(format!("tag_{tag}")))
                .collect();
            Value::Object(vec![
                (
                    "id".to_string(),
                    Value::Int(i64::try_from(index).unwrap_or(0)),
                ),
                (
                    "name".to_string(),
                    Value::String(format!("{kind}_{index:04}")),
                ),
                (
                    "prefab".to_string(),
                    Value::String(format!("prefabs/{kind}.jsonc")),
                ),
                ("position".to_string(), position),
                (
                    "health".to_string(),
                    Value::Int(i64::from(rng.range(1..200))),
                ),
                ("visible".to_string(), Value::Bool(rng.range(0..4u32) != 0)),
                ("parent".to_string(), Value::Null),
                ("tags".to_string(), Value::Array(tags)),
                (
                    "note".to_string(),
                    Value::String("Says \"hello\"\nand waits. Déjà vu.".to_string()),
                ),
            ])
        })
        .collect();
    let level = Value::Object(vec![
        ("version".to_string(), Value::Int(1)),
        ("entities".to_string(), Value::Array(entities)),
    ]);
    json::to_string_pretty(&level)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Config;

    #[test]
    fn the_document_is_valid_and_has_every_record() {
        let text = document(25);
        let value = json::parse(&text, Limits::default()).unwrap();
        let entities = value.get("entities").and_then(Value::as_array).unwrap();
        assert_eq!(entities.len(), 25);
        let last = &entities[24];
        assert_eq!(last.get("id"), Some(&Value::Int(24)));
        let name = last.get("name").and_then(Value::as_str).unwrap();
        assert!(name.ends_with("_0024"), "{name}");
        // The kinds of value a parser has to handle are all in there.
        for needle in ["null", "true", "false", "\\n", "\\\"", ".25", "é", "[]"] {
            assert!(text.contains(needle), "{needle}");
        }
    }

    #[test]
    fn the_document_is_the_same_every_time() {
        assert_eq!(document(40), document(40));
        // A longer document starts with the shorter one's records.
        let (short, long) = (document(3), document(4));
        let shared = short.rfind("    }").unwrap();
        assert_eq!(short[..shared], long[..shared]);
    }

    #[test]
    fn the_full_document_is_a_few_hundred_kilobytes() {
        let len = document(RECORDS).len();
        assert!((200_000..600_000).contains(&len), "{len}");
    }

    #[test]
    fn the_benchmark_times_the_parser() {
        let mut bencher = Bencher::new(Config::fast());
        parse(&mut bencher);
        let measurement = bencher.finish().unwrap();
        assert!(measurement.median_ns() > 0.0);
    }
}
