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

//! The cache: what a cook skips, cooks again, and removes.

use std::fs;

use nadir_codec::json::Value;
use nadir_codec::{Limits, kbj};

use crate::common::{age, cook, cook_with, dir, files, jobs, names, plain, write, written};
use crate::tree;

fn all(cooked: bool) -> Vec<(String, bool)> {
    ["atlas/chars", "atlas/tiles", "atlas/world"]
        .into_iter()
        .map(|name| (name.to_string(), cooked))
        .collect()
}

#[test]
fn a_second_cook_skips_every_group_and_changes_nothing() {
    let root = dir("second");
    let (source, out) = (root.join("source"), root.join("out"));
    tree(&source);
    assert_eq!(jobs(&cook(&source, &out).expect("a cook")), all(true));
    let first = files(&out);
    assert_eq!(jobs(&cook(&source, &out).expect("a cook")), all(false));
    assert_eq!(files(&out), first);
    // Forced, it cooks them all again, to the same bytes.
    assert_eq!(
        jobs(&cook_with(&source, &out, true, "test").expect("a cook")),
        all(true)
    );
    assert_eq!(files(&out), first);
}

#[test]
fn changing_one_png_cooks_only_its_group_again() {
    let root = dir("one_png");
    let (source, out) = (root.join("source"), root.join("out"));
    tree(&source);
    cook(&source, &out).expect("a cook");
    let before = files(&out);
    write(&source, "props/rock.png", plain(10, 6, [1, 2, 3, 255]));
    let report = cook(&source, &out).expect("a cook");
    assert_eq!(
        jobs(&report),
        [
            ("atlas/chars".to_string(), false),
            ("atlas/tiles".to_string(), false),
            ("atlas/world".to_string(), true),
        ]
    );
    let after = files(&out);
    let changed: Vec<&str> = after
        .iter()
        .filter(|file| !before.contains(file))
        .map(|(name, _)| name.as_str())
        .collect();
    // Its page changed, and the pack that holds it; where the rock is, and
    // so its regions, did not.
    assert_eq!(changed, [".cache.kbj", "atlas/world_0.qoi", "game.kpak"]);

    // And it comes to what a clean cook of the same files writes.
    let clean = root.join("clean");
    cook(&source, &clean).expect("a cook");
    assert_eq!(files(&clean), after);
}

#[test]
fn an_output_is_written_only_when_its_bytes_change() {
    let root = dir("times");
    let (source, out) = (root.join("source"), root.join("out"));
    tree(&source);
    cook(&source, &out).expect("a cook");
    let long_ago = age(&out);
    // A cook that changes nothing writes nothing: not the cache, nor the
    // pack.
    cook(&source, &out).expect("a cook");
    assert_eq!(written(&out, long_ago), Vec::<String>::new());
    // Forced, every group is cooked again, to the bytes already there.
    assert_eq!(
        jobs(&cook_with(&source, &out, true, "test").expect("a cook")),
        all(true)
    );
    assert_eq!(written(&out, long_ago), Vec::<String>::new());
    // One image's change writes its page, and the cache and the pack, and
    // not the group's atlas file, whose regions are the same (ADR-0042).
    write(&source, "props/rock.png", plain(10, 6, [1, 2, 3, 255]));
    cook(&source, &out).expect("a cook");
    assert_eq!(
        written(&out, long_ago),
        [".cache.kbj", "atlas/world_0.qoi", "game.kpak"]
    );
    let clean = root.join("clean");
    cook(&source, &clean).expect("a cook");
    assert_eq!(files(&clean), files(&out));
}

#[test]
fn a_sidecar_or_a_folder_s_settings_count_as_the_group_s_files() {
    let root = dir("settings");
    let (source, out) = (root.join("source"), root.join("out"));
    tree(&source);
    cook(&source, &out).expect("a cook");
    // A sidecar that changes.
    write(
        &source,
        "chars/hero.sprite.jsonc",
        r#"{ "frames": [8, 8], "trim": false }"#,
    );
    let cooked: Vec<(String, bool)> = jobs(&cook(&source, &out).expect("a cook"))
        .into_iter()
        .filter(|(_, cooked)| *cooked)
        .collect();
    assert_eq!(cooked, [("atlas/chars".to_string(), true)]);
    // A folder that moves to another group: both are cooked, and the group
    // it left goes.
    write(&source, "props/atlas.jsonc", r#"{ "group": "chars" }"#);
    let report = cook(&source, &out).expect("a cook");
    assert_eq!(
        jobs(&report),
        [
            ("atlas/chars".to_string(), true),
            ("atlas/tiles".to_string(), false)
        ]
    );
    assert!(!out.join("atlas/world.kbj").exists());
    assert!(!out.join("atlas/world_0.qoi").exists());
    let clean = root.join("clean");
    cook(&source, &clean).expect("a cook");
    assert_eq!(files(&clean), files(&out));
}

#[test]
fn a_changed_or_missing_output_is_made_again() {
    let root = dir("outputs");
    let (source, out) = (root.join("source"), root.join("out"));
    tree(&source);
    cook(&source, &out).expect("a cook");
    let first = files(&out);
    fs::remove_file(out.join("atlas/tiles/ground.tiles.qoi")).expect("remove a page");
    fs::write(out.join("atlas/chars_0.qoi"), b"not a page").expect("spoil a page");
    let report = cook(&source, &out).expect("a cook");
    assert_eq!(
        jobs(&report),
        [
            ("atlas/chars".to_string(), true),
            ("atlas/tiles".to_string(), true),
            ("atlas/world".to_string(), false),
        ]
    );
    assert_eq!(files(&out), first);
}

#[test]
fn files_no_group_wrote_are_removed_from_the_atlas_folder_only() {
    let root = dir("stale");
    let (source, out) = (root.join("source"), root.join("out"));
    tree(&source);
    cook(&source, &out).expect("a cook");
    write(&out, "atlas/old_0.qoi", "a page of a group that is gone");
    write(&out, "atlas/deep/er/x.kbj.tmp", "a cook stopped halfway");
    write(&out, "notes.txt", "not the cook's");
    fs::remove_dir_all(source.join("tiles")).expect("remove a group's folder");
    // Forced, which reads no cache, as well.
    let report = cook_with(&source, &out, true, "test").expect("a cook");
    assert_eq!(
        jobs(&report),
        [
            ("atlas/chars".to_string(), true),
            ("atlas/world".to_string(), true)
        ]
    );
    assert_eq!(
        names(&out),
        [
            ".cache.kbj",
            "atlas/chars.kbj",
            "atlas/chars_0.qoi",
            "atlas/world.kbj",
            "atlas/world_0.qoi",
            "game.kpak",
            "notes.txt"
        ]
    );
    assert!(!out.join("atlas/deep").exists(), "the empty folders go");
}

#[test]
fn another_cooker_cooks_everything_again() {
    let root = dir("cooker");
    let (source, out) = (root.join("source"), root.join("out"));
    tree(&source);
    cook_with(&source, &out, false, "one").expect("a cook");
    assert_eq!(
        jobs(&cook_with(&source, &out, false, "two").expect("a cook")),
        all(true)
    );
    assert_eq!(
        jobs(&cook_with(&source, &out, false, "two").expect("a cook")),
        all(false)
    );
}

#[test]
fn a_damaged_cache_is_not_used_and_says_why() {
    let root = dir("damaged");
    let (source, out) = (root.join("source"), root.join("out"));
    tree(&source);
    cook(&source, &out).expect("a cook");
    fs::write(out.join(".cache.kbj"), b"KBJ1\x07").expect("spoil the cache");
    let report = cook(&source, &out).expect("a cook");
    assert_eq!(jobs(&report), all(true));
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.starts_with("the cache `.cache.kbj` cannot be used")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn a_cache_that_names_files_outside_the_output_is_not_used() {
    let root = dir("outside");
    let (source, out) = (root.join("source"), root.join("out"));
    tree(&source);
    cook(&source, &out).expect("a cook");
    // A file beside the output, which a cache could name with `..`.
    write(&root, "precious.txt", "keep me");
    let bytes = fs::read(out.join(".cache.kbj")).expect("the cache");
    let mut cache = kbj::decode(&bytes, Limits::default()).expect("a cache");
    let Value::Object(members) = &mut cache else {
        panic!("an object")
    };
    let Some((_, Value::Array(jobs_list))) = members.iter_mut().find(|(key, _)| key == "jobs")
    else {
        panic!("jobs");
    };
    let Value::Object(job) = &mut jobs_list[0] else {
        panic!("a job")
    };
    let Some((_, Value::Array(outputs))) = job.iter_mut().find(|(key, _)| key == "outputs") else {
        panic!("outputs");
    };
    outputs.push(Value::Array(vec![
        Value::String("../precious.txt".to_string()),
        Value::String("0000000000000000".to_string()),
    ]));
    fs::write(
        out.join(".cache.kbj"),
        kbj::encode(&cache, Limits::default()).expect("encoded"),
    )
    .expect("write the cache");
    let report = cook(&source, &out).expect("a cook");
    assert_eq!(jobs(&report), all(true), "the cache is not used");
    assert_eq!(
        fs::read_to_string(root.join("precious.txt")).expect("still there"),
        "keep me"
    );
}

#[test]
fn a_rename_of_case_alone_leaves_what_a_clean_cook_writes() {
    let root = dir("rename_case");
    let (source, out) = (root.join("source"), root.join("out"));
    write(&source, "g/B.png", plain(4, 4, [1, 1, 1, 255]));
    write(&source, "g/a.png", plain(3, 3, [2, 2, 2, 255]));
    cook(&source, &out).expect("a cook");
    // A rename the file system may not see as one, but whose asset paths,
    // and so the cache's, are the same.
    fs::rename(source.join("g/B.png"), source.join("g/b.png")).expect("rename");
    let report = cook(&source, &out).expect("a cook");
    assert_eq!(jobs(&report), [("atlas/g".to_string(), false)]);
    let clean = root.join("clean");
    cook(&source, &clean).expect("a cook");
    assert_eq!(files(&clean), files(&out));
}
