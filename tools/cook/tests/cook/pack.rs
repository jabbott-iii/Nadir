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

//! The pack, `game.kpak` (ADR-0041): every file the cook wrote, read back
//! through nadir-assets' `PackSource` as its loose file reads; files of one
//! value stored once; a damaged entry named; and a pack left as it was by a
//! cook that changes nothing. That two cooks write it byte for byte, in
//! two folders and after an incremental cook, `output.rs` and
//! `incremental.rs` check with the rest of the output.

use std::fs;
use std::time::{Duration, SystemTime};

use nadir_assets::{AssetError, AssetPath, PackSource, Source};
use nadir_codec::kpak::{self, Codec, ErrorKind};
use nadir_codec::{CodecError, Limits};
use nadir_cook::PACK;

use crate::common::{cook, dir, round_trip, write};
use crate::tree;

#[test]
fn every_file_the_cook_wrote_reads_the_same_from_the_pack() {
    let root = dir("pack_round_trip");
    let (source, out) = (root.join("source"), root.join("out"));
    tree(&source);
    write(
        &source,
        "data/items.jsonc",
        r#"{ "sword": { "damage": 3 } }"#,
    );
    cook(&source, &out).expect("a cook");
    // Three groups' atlas files and pages, and the data file.
    assert_eq!(round_trip(&out), 7);
}

#[test]
fn outputs_of_the_same_bytes_are_stored_once_and_klz_only_where_smaller() {
    let root = dir("pack_blobs");
    let (source, out) = (root.join("source"), root.join("out"));
    let long = format!("[{}]", vec!["\"the same words again\""; 200].join(", "));
    write(&source, "data/a.jsonc", "[1, 2, 3]");
    write(&source, "data/b.jsonc", "// The same value.\n[1, 2, 3,]");
    write(&source, "data/long.jsonc", &long);
    cook(&source, &out).expect("a cook");
    assert_eq!(round_trip(&out), 3);
    let bytes = fs::read(out.join(PACK)).expect("the pack");
    let toc = kpak::parse(&bytes, Limits::default()).expect("a pack");
    let entry = |path: &str| toc.entries()[toc.find(path).expect("an entry")];
    let (a, b, long) = (
        entry("data/a.kbj"),
        entry("data/b.kbj"),
        entry("data/long.kbj"),
    );
    assert_eq!((a.offset, a.codec), (b.offset, Codec::Stored));
    assert_eq!(long.codec, Codec::Klz);
    assert!(long.stored_size * 10 < long.raw_size, "{long:?}");
}

#[test]
fn a_damaged_entry_of_the_pack_is_detected_and_named() {
    let root = dir("pack_damaged");
    let (source, out) = (root.join("source"), root.join("out"));
    tree(&source);
    cook(&source, &out).expect("a cook");
    let mut bytes = fs::read(out.join(PACK)).expect("the pack");
    let toc = kpak::parse(&bytes, Limits::default()).expect("a pack");
    let index = toc.find("atlas/world.kbj").expect("the atlas file");
    let start = toc.entries()[index].offset;
    bytes[start as usize + 3] ^= 0x10;
    fs::write(out.join(PACK), &bytes).expect("damage the pack");

    let pack = PackSource::open(out.join(PACK)).expect("its TOC is whole");
    let error = pack
        .read(&AssetPath::new("atlas/world.kbj").expect("a path"))
        .expect_err("a damaged entry");
    assert!(
        matches!(
            &error,
            AssetError::Decode(CodecError::Kpak {
                offset,
                kind: ErrorKind::Damaged { name: Some(name), .. },
            }) if *offset == start && name == "atlas/world.kbj"
        ),
        "{error:?}"
    );
    assert!(
        error.to_string().contains("`atlas/world.kbj` is damaged"),
        "{error}"
    );
    // Another entry is whole, and the next cook writes the pack anew.
    let chars = AssetPath::new("atlas/chars.kbj").expect("a path");
    assert!(pack.read(&chars).is_ok());
    drop(pack);
    cook(&source, &out).expect("a cook");
    assert_eq!(round_trip(&out), 6);
}

#[test]
fn a_cook_that_changes_nothing_leaves_the_pack_as_it_was() {
    let root = dir("pack_kept");
    let (source, out) = (root.join("source"), root.join("out"));
    tree(&source);
    cook(&source, &out).expect("a cook");
    // A time long past, which the cook never reads, so that writing the
    // pack again shows on any file system's clock.
    let long_ago = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    fs::File::options()
        .write(true)
        .open(out.join(PACK))
        .and_then(|file| file.set_modified(long_ago))
        .expect("set the pack's time");
    cook(&source, &out).expect("a cook");
    let again = fs::metadata(out.join(PACK))
        .and_then(|metadata| metadata.modified())
        .expect("this system keeps modification times");
    assert_eq!(again, long_ago, "the pack was written again");
    // A change writes it again, as a clean cook writes it.
    write(
        &source,
        "props/rock.png",
        crate::common::plain(10, 6, [1, 2, 3, 255]),
    );
    cook(&source, &out).expect("a cook");
    let changed = fs::metadata(out.join(PACK))
        .and_then(|metadata| metadata.modified())
        .expect("a time");
    assert_ne!(changed, long_ago, "the pack was not written again");
    let clean = root.join("clean");
    cook(&source, &clean).expect("a cook");
    assert_eq!(
        fs::read(out.join(PACK)).expect("the pack"),
        fs::read(clean.join(PACK)).expect("the clean pack")
    );
}

#[test]
fn a_cook_of_nothing_writes_an_empty_pack() {
    let root = dir("pack_empty");
    let (source, out) = (root.join("source"), root.join("out"));
    write(&source, "notes.txt", "not an asset");
    cook(&source, &out).expect("a cook");
    assert_eq!(round_trip(&out), 0);
    assert!(PackSource::open(out.join(PACK)).expect("a pack").is_empty());
}
