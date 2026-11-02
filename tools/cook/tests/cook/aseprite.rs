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

//! Aseprite exports: their frames join their folder's atlas group, placed
//! in their canvas, and their tags become the sprite's clips.

use std::path::Path;

use nadir_codec::json::Value;
use nadir_codec::{Limits, atlas};
use nadir_cook::CookError;

use crate::common::{
    clips, cook, dir, files, frame_pixels, frames, int, jobs, kbj, plain, png, write,
};

/// A sheet of 3 frames of 8 × 8 across, each a block of one color, the
/// third with a clear border a pixel wide that the cook trims.
fn sheet() -> Vec<u8> {
    let mut pixels = vec![0u8; 24 * 8 * 4];
    for (frame, color) in [[200, 10, 10, 255], [10, 200, 10, 255], [10, 10, 200, 255]]
        .into_iter()
        .enumerate()
    {
        let inset = if frame == 2 { 1 } else { 0 };
        for y in inset..8 - inset {
            for x in inset..8 - inset {
                let at = (y * 24 + frame * 8 + x) * 4;
                pixels[at..at + 4].copy_from_slice(&color);
            }
        }
    }
    pixels
}

/// The export of [`sheet`]: frames Aseprite trimmed from a canvas of
/// 12 × 12, placed at (2, 3), (0, 0) and (4, 4), lasting 40, 60 and 80 ms,
/// with `tags`.
fn export(image: &str, tags: &str) -> String {
    let frame = |i: u32, x: u32, y: u32, ms: u32| {
        format!(
            r#"{{ "filename": "slime {i}.aseprite", "frame": {{ "x": {}, "y": 0, "w": 8, "h": 8 }},
               "rotated": false, "trimmed": true, "spriteSourceSize": {{ "x": {x}, "y": {y}, "w": 8, "h": 8 }},
               "sourceSize": {{ "w": 12, "h": 12 }}, "duration": {ms} }}"#,
            i * 8
        )
    };
    format!(
        r#"{{ "frames": [ {}, {}, {} ],
             "meta": {{ "app": "https://www.aseprite.org/", "version": "1.3", "image": "{image}",
                        "format": "RGBA8888", "size": {{ "w": 24, "h": 8 }}, "scale": "1",
                        "frameTags": {tags} }} }}"#,
        frame(0, 2, 3, 40),
        frame(1, 0, 0, 60),
        frame(2, 4, 4, 80)
    )
}

const TAGS: &str = r#"[ { "name": "squish", "from": 0, "to": 2, "direction": "pingpong" },
                        { "name": "pop", "from": 1, "to": 1, "direction": "forward", "repeat": "2" },
                        { "name": "idle", "from": 0, "to": 0, "direction": "reverse" } ]"#;

fn slime(source: &Path) {
    write(source, "anim/slime.png", png(24, 8, &sheet(), &[]));
    write(source, "anim/slime.json", export("Slime.png", TAGS));
    write(source, "anim/rock.png", plain(4, 4, [90, 90, 100, 255]));
}

/// The sprite `name` of the atlas file of `group`, as the game reads it.
fn read_sprite(out: &Path, group: &str, name: &str) -> atlas::SpriteEntry {
    let bytes = std::fs::read(out.join(format!("atlas/{group}.kbj"))).expect("the atlas file");
    let file = atlas::parse(&bytes, Limits::default()).expect("an atlas the game reads");
    file.sprites
        .into_iter()
        .find(|sprite| sprite.name == name)
        .expect("the sprite")
}

#[test]
fn an_export_s_frames_are_placed_in_its_canvas_and_its_tags_are_clips() {
    let root = dir("export");
    let (source, out) = (root.join("source"), root.join("out"));
    slime(&source);
    let report = cook(&source, &out).expect("a cook");
    // The sheet is the export's, not a sprite of its own; its name's case
    // in the export does not matter.
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let atlas_file = kbj(&out, "atlas/anim.kbj");
    let names: Vec<&str> = atlas_file
        .get("sprites")
        .and_then(Value::as_array)
        .expect("sprites")
        .iter()
        .map(|sprite| sprite.get("name").and_then(Value::as_str).expect("a name"))
        .collect();
    assert_eq!(names, ["anim/rock", "anim/slime"]);

    let slime = crate::common::sprite(&atlas_file, "anim/slime");
    let placed: Vec<[u32; 6]> = frames(slime)
        .iter()
        .map(|frame| ["w", "h", "ox", "oy", "sw", "sh"].map(|key| int(frame, key)))
        .collect();
    // Aseprite's place in the canvas, and the cook's own trim of the third.
    assert_eq!(
        placed,
        [
            [8, 8, 2, 3, 12, 12],
            [8, 8, 0, 0, 12, 12],
            [6, 6, 5, 5, 12, 12]
        ]
    );
    let blue = frame_pixels(&out, &atlas_file, &frames(slime)[2]);
    assert_eq!(blue, (6, 6, [10, 10, 200, 255].repeat(36)));

    let sprite = read_sprite(&out, "anim", "anim/slime");
    assert_eq!(
        sprite.kind,
        atlas::SpriteKind::Pivot([6.0, 12.0]),
        "the default pivot: the canvas's bottom centre"
    );
    let clips = clips(&sprite);
    assert_eq!(
        clips,
        [
            ("idle", true, vec![(0, 40)]),
            ("pop", false, vec![(1, 60), (1, 60)]),
            ("squish", true, vec![(0, 40), (1, 60), (2, 80), (1, 60)]),
        ]
    );
    assert!(read_sprite(&out, "anim", "anim/rock").clips.is_empty());
}

#[test]
fn an_export_s_sidecar_gives_its_pivot_and_whether_it_is_trimmed() {
    let root = dir("export_sidecar");
    let (source, out) = (root.join("source"), root.join("out"));
    slime(&source);
    write(
        &source,
        "anim/slime.sprite.jsonc",
        r#"{ "trim": false, "pivot": [6, 11] }"#,
    );
    cook(&source, &out).expect("a cook");
    let sprite = read_sprite(&out, "anim", "anim/slime");
    assert_eq!(sprite.kind, atlas::SpriteKind::Pivot([6.0, 11.0]));
    // Untrimmed by the cook: Aseprite's own trim stays.
    let third = sprite.frames[2];
    assert_eq!(
        (third.width, third.height, third.offset_x, third.offset_y),
        (8, 8, 4, 4)
    );
    // A sheet's frames come from the export, not the sidecar.
    write(
        &source,
        "anim/slime.sprite.jsonc",
        r#"{ "frames": [8, 8] }"#,
    );
    assert!(matches!(
        cook(&source, &out),
        Err(CookError::Config { path, .. }) if path == "anim/slime.sprite.jsonc"
    ));
}

#[test]
fn an_export_s_mistakes_are_named() {
    // A test's name, the files it writes over the export's (each a path
    // and the name of what to write there), and the error it expects.
    type Case = (
        &'static str,
        &'static [(&'static str, &'static [u8])],
        fn(&CookError) -> bool,
    );
    let cases: [Case; 6] = [
        // No sheet beside it.
        ("missing", &[], |error| {
            matches!(error, CookError::Config { path, reason }
                if path == "anim/slime.json" && reason.contains("`Slime.png` is not a PNG"))
        }),
        // A sheet of another size than it says.
        ("size", &[("anim/slime.png", &[])], |error| {
            matches!(error, CookError::Layout { path, reason }
                if path == "anim/slime.json" && reason.contains("4 × 4"))
        }),
        // A sheet with a sidecar of its own.
        (
            "sheet_sidecar",
            &[
                ("anim/sheet.png", b"sheet"),
                ("anim/slime.json", b"other"),
                ("anim/Sheet.sprite.jsonc", b"{}"),
            ],
            |error| matches!(error, CookError::Config { path, .. } if path == "anim/Sheet.sprite.jsonc"),
        ),
        // Two exports of one sheet.
        (
            "twice",
            &[("anim/slime.png", b"sheet"), ("anim/copy.json", b"copy")],
            |error| {
                matches!(error, CookError::Claimed { path, first, second }
                    if path == "anim/slime.png" && first == "anim/copy.json" && second == "anim/slime.json")
            },
        ),
        // The json-hash form.
        (
            "hash",
            &[("anim/slime.png", b"sheet"), ("anim/slime.json", b"hash")],
            |error| {
                matches!(error, CookError::Decode { path, error: nadir_codec::CodecError::Aseprite { at, .. } }
                if path == "anim/slime.json" && at == "frames")
            },
        ),
        // An image of the sprite's name beside an export of another sheet.
        (
            "name",
            &[
                ("anim/sheet.png", b"sheet"),
                ("anim/slime.json", b"other"),
                ("anim/slime.png", b"plain"),
            ],
            |error| {
                matches!(error, CookError::SpriteNameTaken { name, first, second }
                    if name == "anim/slime" && first == "anim/slime.png" && second == "anim/slime.json")
            },
        ),
    ];
    for (test, files, expected) in cases {
        let root = dir(&format!("export_{test}"));
        let source = root.join("source");
        write(&source, "anim/slime.json", export("Slime.png", TAGS));
        for &(path, what) in files {
            let bytes = match what {
                b"sheet" => png(24, 8, &sheet(), &[]),
                b"plain" => plain(2, 2, [1, 2, 3, 255]),
                b"copy" => export("slime.png", "[]").into_bytes(),
                b"hash" => br#"{ "frames": { "slime 0.aseprite": { "frame": { "x": 0, "y": 0, "w": 8, "h": 8 } } },
                                 "meta": { "image": "slime.png", "size": { "w": 24, "h": 8 } } }"#
                    .to_vec(),
                b"other" => export("sheet.png", "[]").into_bytes(),
                b"{}" => b"{}".to_vec(),
                _ => plain(4, 4, [0, 0, 0, 0]),
            };
            write(&source, path, bytes);
        }
        let error = cook(&source, &root.join("out")).expect_err(test);
        assert!(expected(&error), "{test}: {error:?}");
    }
}

#[test]
fn changing_an_export_cooks_only_its_group_again_to_what_a_clean_cook_writes() {
    let root = dir("export_changed");
    let (source, out) = (root.join("source"), root.join("out"));
    slime(&source);
    write(&source, "other/tree.png", plain(4, 8, [10, 90, 10, 255]));
    cook(&source, &out).expect("a cook");
    // A frame's time, and only that.
    let longer = export("Slime.png", TAGS).replacen(r#""duration": 60"#, r#""duration": 70"#, 1);
    write(&source, "anim/slime.json", longer);
    let report = cook(&source, &out).expect("a cook");
    assert_eq!(
        jobs(&report),
        [
            ("atlas/anim".to_string(), true),
            ("atlas/other".to_string(), false)
        ]
    );
    let pop = &read_sprite(&out, "anim", "anim/slime").clips[1];
    assert_eq!(pop.frames[0].millis, 70);
    let clean = root.join("clean");
    cook(&source, &clean).expect("a cook");
    assert_eq!(files(&out), files(&clean));
    // The sheet is the group's file too.
    let mut pixels = sheet();
    pixels[3] = 128;
    write(&source, "anim/slime.png", png(24, 8, &pixels, &[]));
    assert_eq!(
        jobs(&cook(&source, &out).expect("a cook")),
        [
            ("atlas/anim".to_string(), true),
            ("atlas/other".to_string(), false)
        ]
    );
}
