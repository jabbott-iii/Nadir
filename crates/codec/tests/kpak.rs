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

//! Pack file tests: the vectors in `tests/kpak/`, a damaged entry named,
//! shared blobs, the limits, and packs cut short or with a byte changed.
//!
//! `tests/kpak/make_vectors.py` wrote the vectors, field by field from the
//! format, with no code of the reader's: packs that read, and packs that
//! each break one rule of the format, with the error and the byte it is at.

use nadir_codec::kpak::{self, Codec, ErrorKind, Toc};
use nadir_codec::{CodecError, Limits};

const VECTORS: &str = include_str!("kpak/vectors.txt");

fn vector(name: &str, extension: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/kpak/{name}.{extension}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// A pack's files, as its `.files` lists them: each path and its bytes.
fn files(name: &str) -> Vec<(String, Vec<u8>)> {
    let text = String::from_utf8(vector(name, "files")).expect("the list is UTF-8");
    text.lines()
        .map(|line| {
            let (path, hex) = line.split_once(':').expect("a path and its bytes");
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex"))
                .collect();
            (path.to_string(), bytes)
        })
        .collect()
}

fn parse(pack: &[u8]) -> Result<Toc, CodecError> {
    kpak::parse(pack, Limits::default())
}

/// The variant's name, as the vectors give it.
fn variant(kind: &ErrorKind) -> String {
    let debug = format!("{kind:?}");
    debug
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        .unwrap_or_default()
        .to_string()
}

/// The variant and the byte of a pack's error, or a panic naming `what`.
fn pack_error(result: Result<impl std::fmt::Debug, CodecError>, what: &str) -> (String, u64) {
    match result {
        Err(CodecError::Kpak { offset, kind }) => (variant(&kind), offset),
        other => panic!("{what}: expected a pack error, found {other:?}"),
    }
}

#[test]
fn every_vector_reads_or_is_refused_as_its_script_says() {
    let limits = Limits::default();
    let mut count = 0;
    for line in VECTORS.lines() {
        let words: Vec<&str> = line.split(' ').collect();
        let name = words[0];
        let pack = vector(name, "kpak");
        match words[1..] {
            ["ok"] => {
                let toc = parse(&pack).unwrap_or_else(|error| panic!("{name}: {error}"));
                let files = files(name);
                assert_eq!(toc.len(), files.len(), "{name}");
                for (path, bytes) in &files {
                    let index = toc
                        .find(path)
                        .unwrap_or_else(|| panic!("{name}: no `{path}`"));
                    assert_eq!(
                        &toc.read(&pack, index, limits).unwrap(),
                        bytes,
                        "{name}: {path}"
                    );
                    if toc.has_names() {
                        assert_eq!(toc.name(index), Some(path.as_str()), "{name}");
                    }
                }
                // Read from the file's parts, as a reader of a file does.
                let header =
                    kpak::header(&pack[..kpak::HEADER_SIZE], pack.len() as u64, limits).unwrap();
                let at = header.toc_offset as usize;
                assert_eq!(kpak::toc(&header, &pack[at..], limits), Ok(toc), "{name}");
            }
            ["error", kind, offset] => {
                let expected = (kind.to_string(), offset.parse().expect("an offset"));
                assert_eq!(pack_error(parse(&pack), name), expected, "{name}");
            }
            ["entry", path, kind, offset] => {
                let toc = parse(&pack).unwrap_or_else(|error| panic!("{name}: {error}"));
                let index = toc.find(path).expect("the entry");
                let expected = (kind.to_string(), offset.parse().expect("an offset"));
                assert_eq!(
                    pack_error(toc.read(&pack, index, limits), name),
                    expected,
                    "{name}"
                );
            }
            _ => panic!("{name}: a line the test cannot read: {line}"),
        }
        count += 1;
    }
    assert_eq!(count, 40);
}

#[test]
fn a_damaged_entry_is_detected_and_named_by_its_path() {
    let pack = vector("damaged", "kpak");
    let toc = parse(&pack).unwrap();
    let index = toc.find("notes/greeting.txt").unwrap();
    let error = toc.read(&pack, index, Limits::default()).unwrap_err();
    let CodecError::Kpak {
        offset,
        kind:
            ErrorKind::Damaged {
                ref name,
                expected,
                found,
                ..
            },
    } = error
    else {
        panic!("expected a damaged entry, found {error:?}");
    };
    assert_eq!(name.as_deref(), Some("notes/greeting.txt"));
    assert_ne!(expected, found);
    assert_eq!(
        error.to_string(),
        format!(
            "pack error at byte {offset}: the entry `notes/greeting.txt` is damaged: its CRC-32 is {found:08x}, not the TOC's {expected:08x}"
        )
    );
    // The other entry is whole.
    let one = toc.find("notes/one.txt").unwrap();
    assert_eq!(toc.read(&pack, one, Limits::default()).unwrap(), b"one");

    // Without a string table, the entry is named by its index.
    let pack = vector("damaged_no_names", "kpak");
    let toc = parse(&pack).unwrap();
    assert!(!toc.has_names());
    let index = toc.find("notes/greeting.txt").unwrap();
    let error = toc.read(&pack, index, Limits::default()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains(&format!("entry {index} is damaged")),
        "{error}"
    );
}

#[test]
fn identical_blobs_are_stored_once_and_an_empty_file_is_an_entry() {
    let pack = vector("dedup", "kpak");
    let toc = parse(&pack).unwrap();
    let entry = |path: &str| toc.entries()[toc.find(path).unwrap()];
    assert_eq!(entry("data/a.kbj").offset, entry("data/b.kbj").offset);
    assert_eq!(entry("data/c.kbj").offset, entry("data/d.kbj").offset);
    assert_ne!(entry("data/a.kbj").offset, entry("data/c.kbj").offset);
    assert_eq!(entry("data/c.kbj").codec, Codec::Klz);
    assert_eq!(entry("data/a.kbj").codec, Codec::Stored);
    let empty = entry("data/empty.kbj");
    assert_eq!((empty.stored_size, empty.raw_size), (0, 0));
    assert_eq!(
        toc.read(
            &pack,
            toc.find("data/empty.kbj").unwrap(),
            Limits::default()
        ),
        Ok(Vec::new())
    );
    for entry in toc.entries() {
        assert_eq!(entry.offset % kpak::ALIGN, 0);
    }
}

#[test]
fn a_pack_without_a_string_table_finds_its_entries_by_hash() {
    let pack = vector("no_names", "kpak");
    let toc = parse(&pack).unwrap();
    assert!(!toc.has_names());
    assert_eq!(toc.name(0), None);
    let index = toc.find("notes/one.txt").unwrap();
    assert_eq!(toc.read(&pack, index, Limits::default()).unwrap(), b"one");
    assert_eq!(toc.find("notes/two.txt"), None);
}

#[test]
fn the_limits_bound_the_entries_the_toc_and_each_entry() {
    let pack = vector("two", "kpak");
    let fewer = Limits {
        max_entries: 1,
        ..Limits::default()
    };
    assert_eq!(
        pack_error(kpak::parse(&pack, fewer), "fewer entries"),
        ("TooManyEntries".to_string(), 8)
    );
    // The TOC is 99 bytes.
    let smaller = Limits {
        max_decoded_bytes: 98,
        ..Limits::default()
    };
    assert_eq!(
        pack_error(kpak::parse(&pack, smaller), "a smaller TOC"),
        ("TocTooLarge".to_string(), 20)
    );
    // `many` holds an entry of 326 bytes, beside 41 of 18 at most, in a TOC
    // of 2,420 bytes.
    let pack = vector("many", "kpak");
    let limits = Limits {
        max_decoded_bytes: 2420,
        ..Limits::default()
    };
    let toc = kpak::parse(&pack, limits).unwrap();
    let index = toc.find("data/run.kbj").unwrap();
    let tight = Limits {
        max_decoded_bytes: 325,
        ..Limits::default()
    };
    assert_eq!(
        pack_error(toc.read(&pack, index, tight), "a smaller entry"),
        ("EntryTooLarge".to_string(), toc.entries()[index].offset)
    );
    assert_eq!(toc.read(&pack, index, limits).unwrap().len(), 326);

    // A KLZ entry stored in 293 bytes, 290 raw: its stored size is checked
    // too, at its field, byte 16 of its TOC entry.
    let pack = vector("klz_literals", "kpak");
    let header = kpak::header(&pack, pack.len() as u64, Limits::default()).unwrap();
    let toc = kpak::parse(&pack, Limits::default()).unwrap();
    let entry = toc.entries()[0];
    assert_eq!(
        (entry.codec, entry.stored_size, entry.raw_size),
        (Codec::Klz, 293, 290)
    );
    let between = Limits {
        max_decoded_bytes: 292,
        ..Limits::default()
    };
    assert_eq!(
        pack_error(kpak::parse(&pack, between), "a larger stream"),
        ("EntryTooLarge".to_string(), header.toc_offset + 16)
    );
    // And a TOC read with fewer entries allowed than its header was read
    // with is refused at the header's count, byte 8.
    let at = header.toc_offset as usize;
    let none = Limits {
        max_entries: 0,
        ..Limits::default()
    };
    assert_eq!(
        pack_error(kpak::toc(&header, &pack[at..], none), "no entries"),
        ("TooManyEntries".to_string(), 8)
    );
}

#[test]
fn a_reader_of_parts_is_told_when_they_are_not_what_the_header_says() {
    let pack = vector("two", "kpak");
    let limits = Limits::default();
    assert_eq!(
        pack_error(kpak::header(&pack[..31], 163, limits), "a short header"),
        ("UnexpectedEnd".to_string(), 31)
    );
    let header = kpak::header(&pack, pack.len() as u64, limits).unwrap();
    assert_eq!(header.entry_count, 2);
    assert!(header.names);
    let at = header.toc_offset as usize;
    assert_eq!(
        pack_error(kpak::toc(&header, &pack[at + 1..], limits), "a short TOC"),
        ("TocLength".to_string(), header.toc_offset)
    );
    let toc = kpak::toc(&header, &pack[at..], limits).unwrap();
    assert_eq!(
        pack_error(toc.decode(2, &[], limits), "no such entry"),
        ("NoEntry".to_string(), 0)
    );
    let entry = toc.entries()[0];
    let start = entry.offset as usize;
    let stored = &pack[start..start + entry.stored_size as usize];
    assert_eq!(
        pack_error(toc.decode(0, &stored[1..], limits), "short stored bytes"),
        ("StoredLength".to_string(), entry.offset)
    );
    assert!(toc.decode(0, stored, limits).is_ok());
    assert_eq!(
        pack_error(toc.read(&pack[..start + 1], 0, limits), "a pack cut short"),
        ("UnexpectedEnd".to_string(), start as u64 + 1)
    );
}

#[test]
fn packs_cut_short_or_with_a_byte_changed_never_panic() {
    let limits = Limits::default();
    for name in ["two", "dedup", "many"] {
        let pack = vector(name, "kpak");
        let mut inputs: Vec<Vec<u8>> = (0..pack.len()).map(|end| pack[..end].to_vec()).collect();
        for at in 0..pack.len() {
            for bit in [0x01, 0x80] {
                let mut changed = pack.clone();
                changed[at] ^= bit;
                inputs.push(changed);
            }
        }
        for input in &inputs {
            let Ok(toc) = kpak::parse(input, limits) else {
                continue;
            };
            for index in 0..toc.len() {
                match toc.read(input, index, limits) {
                    Ok(bytes) => assert_eq!(bytes.len(), toc.entries()[index].raw_size as usize),
                    Err(CodecError::Kpak {
                        kind: ErrorKind::Damaged { .. } | ErrorKind::BadStream { .. },
                        ..
                    }) => {}
                    Err(error) => panic!("{name}: entry {index}: {error}"),
                }
            }
        }
    }
}
