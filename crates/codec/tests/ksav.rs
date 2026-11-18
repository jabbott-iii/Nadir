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

//! Save file tests: the vectors in `tests/ksav/`, the bytes the writer
//! writes, the limits, and saves cut short or with a bit changed.
//!
//! `tests/ksav/make_vectors.py` wrote the vectors, field by field from the
//! format, with a KBJ writer of its own and no code of the reader's: saves
//! that read, saves that each break one rule of the container, with the
//! error and its byte, and saves whose payload breaks one rule of version
//! 1's, with the error and its place.

use nadir_codec::json::{self, Value};
use nadir_codec::ksav::{self, DataErrorKind, ErrorKind};
use nadir_codec::{CodecError, Limits};

const VECTORS: &str = include_str!("ksav/vectors.txt");

fn vector(name: &str, extension: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/ksav/{name}.{extension}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// The variant's name, as the vectors give it.
fn variant(kind: &impl std::fmt::Debug) -> String {
    let debug = format!("{kind:?}");
    debug
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        .unwrap_or_default()
        .to_string()
}

/// The text of the hex `hex`, or empty for `-`.
fn unhex(hex: &str) -> String {
    if hex == "-" {
        return String::new();
    }
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex"))
        .collect();
    String::from_utf8(bytes).expect("UTF-8")
}

/// The names of the vectors that read.
fn ok_names() -> Vec<&'static str> {
    VECTORS
        .lines()
        .filter(|line| line.split(' ').nth(1) == Some("ok"))
        .map(|line| line.split(' ').next().expect("a name"))
        .collect()
}

#[test]
fn every_vector_reads_or_is_refused_as_its_script_says() {
    let limits = Limits::default();
    let mut counts = [0; 3];
    for line in VECTORS.lines() {
        let words: Vec<&str> = line.split(' ').collect();
        let name = words[0];
        let file = vector(name, "ksav");
        match words[1..] {
            ["ok", saved_at, game_version] => {
                let (header, payload) =
                    ksav::parse(&file, limits).unwrap_or_else(|error| panic!("{name}: {error}"));
                assert_eq!(header.version, 1, "{name}");
                assert_eq!(header.saved_at, saved_at.parse::<i64>().unwrap(), "{name}");
                assert_eq!(header.game_version, unhex(game_version), "{name}");
                assert_eq!(
                    header.payload_size as usize,
                    file.len() - header.payload_offset()
                );
                assert_eq!(ksav::header(&file, limits), Ok(header.clone()), "{name}");
                let text = String::from_utf8(vector(name, "json")).unwrap();
                let expected = json::parse(&text, limits).unwrap();
                assert_eq!(payload, expected, "{name}");
                let data = ksav::read(&payload).unwrap_or_else(|error| panic!("{name}: {error}"));
                // The payload is written back as it was, and so is the file:
                // every save has one encoding.
                assert_eq!(data.to_value(), payload, "{name}");
                let again = ksav::encode(&header.game_version, header.saved_at, &payload, limits);
                assert_eq!(again.as_deref(), Ok(file.as_slice()), "{name}");
                counts[0] += 1;
            }
            ["error", kind, offset] => {
                match ksav::parse(&file, limits) {
                    Err(CodecError::Ksav {
                        offset: at,
                        kind: found,
                    }) => {
                        assert_eq!(
                            (variant(&found), at),
                            (kind.to_string(), offset.parse().unwrap()),
                            "{name}: {found}"
                        );
                    }
                    other => panic!("{name}: expected a save file error, found {other:?}"),
                }
                counts[1] += 1;
            }
            ["data", kind, at] => {
                let (_, payload) =
                    ksav::parse(&file, limits).unwrap_or_else(|error| panic!("{name}: {error}"));
                let at = if at == "-" { "" } else { at };
                match ksav::read(&payload) {
                    Err(CodecError::KsavData {
                        at: found_at,
                        kind: found,
                    }) => {
                        assert_eq!(
                            (variant(&found), found_at.as_str()),
                            (kind.to_string(), at),
                            "{name}: {found}"
                        );
                    }
                    other => panic!("{name}: expected a save data error, found {other:?}"),
                }
                counts[2] += 1;
            }
            _ => panic!("{name}: a line the test cannot read: {line}"),
        }
    }
    assert_eq!(counts, [5, 19, 24]);
}

#[test]
fn the_errors_say_what_and_where() {
    let file = vector("damaged", "ksav");
    let error = ksav::parse(&file, Limits::default()).unwrap_err();
    let CodecError::Ksav {
        offset: 8,
        kind: ErrorKind::Damaged { expected, found },
    } = error
    else {
        panic!("expected a damaged save, found {error:?}");
    };
    assert_ne!(expected, found);
    assert!(
        error
            .to_string()
            .starts_with("save file error at byte 8: the file is damaged: its CRC-32 is "),
        "{error}"
    );
    let file = vector("version_2", "ksav");
    assert_eq!(
        ksav::parse(&file, Limits::default())
            .unwrap_err()
            .to_string(),
        "save file error at byte 4: version 2 of the save file is not supported: this reads 1 to 1"
    );
    let file = vector("duplicate_id", "ksav");
    let (_, payload) = ksav::parse(&file, Limits::default()).unwrap();
    let error = ksav::read(&payload).unwrap_err();
    assert_eq!(
        error,
        CodecError::KsavData {
            at: "entities[1].id".into(),
            kind: DataErrorKind::DuplicateId { id: 4 }
        }
    );
    assert_eq!(
        error.to_string(),
        "save data error at `entities[1].id`: another entity has the save id 4"
    );
}

#[test]
fn a_payload_over_the_limit_is_refused_before_it_is_read() {
    let file = vector("many", "ksav");
    let header = ksav::header(&file, Limits::default()).unwrap();
    let size = header.payload_size as usize;
    let limits = |max_decoded_bytes| Limits {
        max_decoded_bytes,
        ..Limits::default()
    };
    assert!(ksav::parse(&file, limits(size)).is_ok());
    assert_eq!(
        ksav::parse(&file, limits(size - 1)),
        Err(CodecError::Ksav {
            offset: 20,
            kind: ErrorKind::PayloadTooLarge {
                size: header.payload_size,
                limit: size - 1,
            },
        })
    );
    // The KBJ inside keeps to the limits too.
    let tight = Limits {
        max_entries: 299,
        ..Limits::default()
    };
    assert!(matches!(
        ksav::parse(&file, tight),
        Err(CodecError::Ksav {
            kind: ErrorKind::BadPayload { .. },
            ..
        })
    ));
}

#[test]
fn every_prefix_and_every_bit_changed_is_refused() {
    let limits = Limits::default();
    // Not `many`, of 300 entities: every bit of its 8 KiB is checked by the
    // same CRC, at a cost.
    for name in ok_names().into_iter().filter(|&name| name != "many") {
        let file = vector(name, "ksav");
        for len in 0..file.len() {
            assert!(
                ksav::parse(&file[..len], limits).is_err(),
                "{name}: the first {len} bytes read"
            );
        }
        for at in 0..file.len() {
            for bit in 0..8 {
                let mut changed = file.clone();
                changed[at] ^= 1 << bit;
                // Past the CRC, the CRC sees it; before it, the magic, the
                // version, the game version's length (which the payload's
                // size no longer adds up with) and the CRC are fields.
                assert!(
                    ksav::parse(&changed, limits).is_err(),
                    "{name}: bit {bit} of byte {at} was not seen"
                );
            }
        }
    }
}

#[test]
fn ids_survive_the_payload_whatever_their_size() {
    let file = vector("far_ids", "ksav");
    let (_, payload) = ksav::parse(&file, Limits::default()).unwrap();
    let data = ksav::read(&payload).unwrap();
    assert_eq!(data.next_id, u64::MAX);
    let ids: Vec<u64> = data.entities.iter().map(|entity| entity.id).collect();
    assert_eq!(ids, [i64::MAX as u64, 1 << 63, 0]);
    assert_eq!(
        data.entities[1].components,
        [(
            "Follow".to_string(),
            Value::Object(vec![(
                "target".to_string(),
                Value::String(u64::MAX.to_string())
            )])
        )]
    );
    let every = ksav::read(
        &ksav::parse(&vector("every_value", "ksav"), Limits::default())
            .unwrap()
            .1,
    )
    .unwrap();
    assert_eq!(every.entities[0].prefab.as_deref(), Some("crate_2"));
    assert_eq!(every.globals[0].0, "Notes");
}
