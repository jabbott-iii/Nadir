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

//! What the cook writes: the atlas files, page by page and pixel by pixel.

use nadir_codec::json::Value;
use nadir_cook::CookError;

use crate::common::{
    cook, dir, files, filled, frame_pixels, frames, int, kbj, pixel, plain, png, qoi, sprite, write,
};
use crate::tree;

/// A frame's numbers: page, x, y, w, h, ox, oy, sw, sh.
fn numbers(frame: &Value) -> [u32; 9] {
    ["page", "x", "y", "w", "h", "ox", "oy", "sw", "sh"].map(|key| int(frame, key))
}

fn pivot(sprite: &Value) -> Option<(f64, f64)> {
    match sprite.get("pivot")?.as_array()? {
        [x, y] => Some((x.as_f64()?, y.as_f64()?)),
        _ => None,
    }
}

#[test]
fn the_atlas_says_where_each_frame_went() {
    let root = dir("atlas");
    let (source, out) = (root.join("source"), root.join("out"));
    tree(&source);
    cook(&source, &out).expect("a cook");
    let atlas = kbj(&out, "atlas/chars.kbj");
    assert_eq!(atlas.get("version").and_then(Value::as_i64), Some(2));
    let pages = atlas.get("pages").and_then(Value::as_array).expect("pages");
    assert_eq!(pages.len(), 1);
    assert_eq!(
        pages[0].get("path").and_then(Value::as_str),
        Some("atlas/chars_0.qoi")
    );
    let page = qoi(&out, "atlas/chars_0.qoi");
    assert_eq!(
        (int(&pages[0], "w"), int(&pages[0], "h")),
        (page.header.width, page.header.height)
    );
    assert_eq!(page.header.width, 64, "the smallest page");

    let names: Vec<&str> = atlas
        .get("sprites")
        .and_then(Value::as_array)
        .expect("sprites")
        .iter()
        .map(|sprite| sprite.get("name").and_then(Value::as_str).expect("a name"))
        .collect();
    assert_eq!(names, ["chars/ghost", "chars/hero"]);

    // The sheet's two frames, each trimmed to its block of 4 × 6.
    let hero = sprite(&atlas, "chars/hero");
    assert_eq!(pivot(hero), Some((4.0, 8.0)));
    let hero_frames = frames(hero);
    assert_eq!(hero_frames.len(), 2);
    let [_, _, _, w, h, ox, oy, sw, sh] = numbers(&hero_frames[0]);
    assert_eq!([w, h, ox, oy, sw, sh], [4, 6, 1, 1, 8, 8]);
    let [_, _, _, w, h, ox, oy, sw, sh] = numbers(&hero_frames[1]);
    assert_eq!([w, h, ox, oy, sw, sh], [4, 6, 2, 1, 8, 8]);
    for (frame, color) in hero_frames
        .iter()
        .zip([[200, 10, 10, 255], [10, 200, 10, 255]])
    {
        let (w, h, pixels) = frame_pixels(&out, &atlas, frame);
        assert_eq!(pixels, color.repeat((w * h) as usize));
    }

    // An image with nothing to show: a frame of no pixels, and the default
    // pivot, the bottom of its middle.
    let ghost = sprite(&atlas, "chars/ghost");
    assert_eq!(numbers(&frames(ghost)[0]), [0, 0, 0, 0, 0, 0, 0, 8, 8]);
    assert_eq!(pivot(ghost), Some((4.0, 8.0)));
    // Of a frame wider than it is tall too.
    let world = kbj(&out, "atlas/world.kbj");
    assert_eq!(pivot(sprite(&world, "props/rock")), Some((5.0, 6.0)));
}

#[test]
fn each_frame_is_extruded_and_padded_on_a_clear_page() {
    let root = dir("extrude");
    let (source, out) = (root.join("source"), root.join("out"));
    // Frames of four colors in a quarter each, so that every edge differs.
    for (index, (w, h)) in [(5, 3), (7, 7), (2, 9), (12, 4), (1, 1)]
        .into_iter()
        .enumerate()
    {
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let quarter = u8::from(x * 2 >= w) + 2 * u8::from(y * 2 >= h);
                pixels.extend_from_slice(&[40 * quarter, index as u8 * 30, 7, 255]);
            }
        }
        write(&source, &format!("g/{index}.png"), png(w, h, &pixels, &[]));
    }
    cook(&source, &out).expect("a cook");
    let atlas = kbj(&out, "atlas/g.kbj");
    let page = qoi(&out, "atlas/g_0.qoi");
    let mut covered = vec![false; (page.header.width * page.header.height) as usize];
    let mut extruded = Vec::new();
    for index in 0..5 {
        let frame = &frames(sprite(&atlas, &format!("g/{index}")))[0];
        let [_, x, y, w, h, ..] = numbers(frame);
        // The ring around it repeats its nearest edge pixel.
        for ry in y - 1..y + h + 1 {
            for rx in x - 1..x + w + 1 {
                let nearest = (rx.clamp(x, x + w - 1), ry.clamp(y, y + h - 1));
                assert_eq!(
                    pixel(&page, rx, ry),
                    pixel(&page, nearest.0, nearest.1),
                    "({rx}, {ry})"
                );
                covered[(ry * page.header.width + rx) as usize] = true;
            }
        }
        extruded.push((x - 1, y - 1, w + 2, h + 2));
    }
    // 2 pixels between any two, with their extrusion.
    for (i, a) in extruded.iter().enumerate() {
        for b in &extruded[i + 1..] {
            let apart = a.0 + a.2 + 2 <= b.0
                || b.0 + b.2 + 2 <= a.0
                || a.1 + a.3 + 2 <= b.1
                || b.1 + b.3 + 2 <= a.1;
            assert!(apart, "{a:?} and {b:?} are closer than 2 pixels");
        }
    }
    // Clear everywhere else.
    for y in 0..page.header.height {
        for x in 0..page.header.width {
            if !covered[(y * page.header.width + x) as usize] {
                assert_eq!(pixel(&page, x, y), [0, 0, 0, 0], "({x}, {y})");
            }
        }
    }
}

#[test]
fn a_tile_set_is_a_page_of_its_own_as_it_is() {
    let root = dir("tile_set");
    let (source, out) = (root.join("source"), root.join("out"));
    tree(&source);
    cook(&source, &out).expect("a cook");
    let atlas = kbj(&out, "atlas/tiles.kbj");
    let pages = atlas.get("pages").and_then(Value::as_array).expect("pages");
    assert_eq!(pages.len(), 1, "no packed page for a group of tile sets");
    assert_eq!(
        pages[0].get("path").and_then(Value::as_str),
        Some("atlas/tiles/ground.tiles.qoi")
    );
    assert_eq!((int(&pages[0], "w"), int(&pages[0], "h")), (32, 16));
    let ground = sprite(&atlas, "tiles/ground");
    let tiles: Vec<i64> = ground
        .get("tiles")
        .and_then(Value::as_array)
        .expect("tiles")
        .iter()
        .filter_map(Value::as_i64)
        .collect();
    assert_eq!(tiles, [16, 16]);
    assert_eq!(ground.get("pivot"), None);
    assert_eq!(frames(ground).len(), 1);
    assert_eq!(numbers(&frames(ground)[0]), [0, 0, 0, 32, 16, 0, 0, 32, 16]);
    let page = qoi(&out, "atlas/tiles/ground.tiles.qoi");
    for y in 0..16u8 {
        for x in 0..32u8 {
            assert_eq!(
                pixel(&page, u32::from(x), u32::from(y)),
                [x * 8, y * 16, 50, 255]
            );
        }
    }
}

#[test]
fn sprites_and_tile_sets_pages_go_by_name_whatever_the_case_found() {
    let root = dir("by_name");
    let (source, out) = (root.join("source"), root.join("out"));
    for name in ["Hero", "arrow", "tree", "tree-big"] {
        write(
            &source,
            &format!("g/{name}.png"),
            plain(4, 4, [5, 5, 5, 255]),
        );
    }
    for name in ["Wall", "floor"] {
        write(
            &source,
            &format!("g/{name}.png"),
            plain(16, 16, [6, 6, 6, 255]),
        );
        write(
            &source,
            &format!("g/{name}.sprite.jsonc"),
            r#"{ "tiles": [8, 8] }"#,
        );
    }
    cook(&source, &out).expect("a cook");
    let atlas = kbj(&out, "atlas/g.kbj");
    let names: Vec<&str> = atlas
        .get("sprites")
        .and_then(Value::as_array)
        .expect("sprites")
        .iter()
        .filter_map(|sprite| sprite.get("name").and_then(Value::as_str))
        .collect();
    assert_eq!(
        names,
        [
            "g/arrow",
            "g/floor",
            "g/hero",
            "g/tree",
            "g/tree-big",
            "g/wall"
        ]
    );
    let pages: Vec<&str> = atlas
        .get("pages")
        .and_then(Value::as_array)
        .expect("pages")
        .iter()
        .filter_map(|page| page.get("path").and_then(Value::as_str))
        .collect();
    assert_eq!(
        pages,
        [
            "atlas/g_0.qoi",
            "atlas/g/floor.tiles.qoi",
            "atlas/g/wall.tiles.qoi"
        ]
    );
    // Frames of one size go in by name: `tree` before `tree-big`.
    let x = |name: &str| int(&frames(sprite(&atlas, name))[0], "x");
    assert!(x("g/tree") < x("g/tree-big"));
}

#[test]
fn a_sheet_of_rows_gives_its_frames_row_by_row() {
    let root = dir("rows");
    let (source, out) = (root.join("source"), root.join("out"));
    let colors = [
        [10, 0, 0, 255],
        [20, 0, 0, 255],
        [30, 0, 0, 255],
        [40, 0, 0, 255],
    ];
    let mut pixels = Vec::new();
    for y in 0..4 {
        for x in 0..6 {
            let cell = (y / 2) * 2 + x / 3;
            pixels.extend_from_slice(&colors[cell]);
        }
    }
    write(&source, "g/sheet.png", png(6, 4, &pixels, &[]));
    write(&source, "g/sheet.sprite.jsonc", r#"{ "frames": [3, 2] }"#);
    cook(&source, &out).expect("a cook");
    let atlas = kbj(&out, "atlas/g.kbj");
    let sheet = frames(sprite(&atlas, "g/sheet"));
    assert_eq!(sheet.len(), 4);
    for (frame, color) in sheet.iter().zip(colors) {
        let (w, h, pixels) = frame_pixels(&out, &atlas, frame);
        assert_eq!(pixels, color.repeat((w * h) as usize));
    }
}

#[test]
fn a_group_too_big_for_one_page_fills_two() {
    let root = dir("two_pages");
    let (source, out) = (root.join("source"), root.join("out"));
    // Two frames of 2049, which no page of 4096 holds side by side.
    let side = 2049;
    let mut pixels = filled(side, side, [70, 0, 0, 255]);
    pixels.extend(filled(side, side, [0, 80, 0, 255]));
    write(&source, "g/big.png", png(side, 2 * side, &pixels, &[]));
    write(
        &source,
        "g/big.sprite.jsonc",
        r#"{ "frames": [2049, 2049] }"#,
    );
    cook(&source, &out).expect("a cook");
    let atlas = kbj(&out, "atlas/g.kbj");
    let pages: Vec<&str> = atlas
        .get("pages")
        .and_then(Value::as_array)
        .expect("pages")
        .iter()
        .filter_map(|page| page.get("path").and_then(Value::as_str))
        .collect();
    assert_eq!(pages, ["atlas/g_0.qoi", "atlas/g_1.qoi"]);
    let big = frames(sprite(&atlas, "g/big"));
    let on: Vec<u32> = big.iter().map(|frame| int(frame, "page")).collect();
    assert_eq!(on, [0, 1]);
    for (frame, color) in big.iter().zip([[70, 0, 0, 255], [0, 80, 0, 255]]) {
        let (w, h, pixels) = frame_pixels(&out, &atlas, frame);
        assert_eq!((w, h), (side, side));
        assert!(pixels.chunks(4).all(|pixel| pixel == color));
    }
}

#[test]
fn untrimmed_frames_keep_their_cells_even_when_clear() {
    let root = dir("untrimmed");
    let (source, out) = (root.join("source"), root.join("out"));
    write(&source, "g/clear.png", png(6, 4, &[0; 6 * 4 * 4], &[]));
    write(
        &source,
        "g/clear.sprite.jsonc",
        r#"{ "frames": [3, 4], "trim": false }"#,
    );
    cook(&source, &out).expect("a cook");
    let atlas = kbj(&out, "atlas/g.kbj");
    let clear = frames(sprite(&atlas, "g/clear"));
    assert_eq!(clear.len(), 2);
    for frame in clear {
        let [_, _, _, w, h, ox, oy, sw, sh] = numbers(frame);
        assert_eq!([w, h, ox, oy, sw, sh], [3, 4, 0, 0, 3, 4]);
    }
}

#[test]
fn a_group_of_frames_with_nothing_to_show_still_has_a_page() {
    let root = dir("empty_group");
    let (source, out) = (root.join("source"), root.join("out"));
    write(&source, "g/clear.png", png(4, 4, &[0; 4 * 4 * 4], &[]));
    cook(&source, &out).expect("a cook");
    let atlas = kbj(&out, "atlas/g.kbj");
    let pages = atlas.get("pages").and_then(Value::as_array).expect("pages");
    assert_eq!(pages.len(), 1);
    let page = qoi(&out, "atlas/g_0.qoi");
    assert_eq!((page.header.width, page.header.height), (64, 64));
    assert!(page.pixels.iter().all(|&byte| byte == 0));
}

#[test]
fn the_same_sources_cook_to_the_same_bytes_in_any_folder() {
    let root = dir("twice");
    tree(&root.join("first"));
    tree(&root.join("second"));
    cook(&root.join("first"), &root.join("out1")).expect("a cook");
    cook(&root.join("second"), &root.join("out2")).expect("a cook");
    let (one, two) = (files(&root.join("out1")), files(&root.join("out2")));
    assert_eq!(
        one.len(),
        7,
        "{:?}",
        one.iter().map(|(name, _)| name).collect::<Vec<_>>()
    );
    assert_eq!(one, two);
}

#[test]
fn a_sheet_wider_than_any_texture_is_cut_and_packed() {
    let root = dir("wide");
    let (source, out) = (root.join("source"), root.join("out"));
    write(&source, "g/strip.png", plain(8320, 1, [9, 8, 7, 255]));
    write(&source, "g/strip.sprite.jsonc", r#"{ "frames": [64, 1] }"#);
    cook(&source, &out).expect("a cook");
    assert_eq!(
        frames(sprite(&kbj(&out, "atlas/g.kbj"), "g/strip")).len(),
        130
    );
}

#[test]
fn a_frame_too_wide_for_the_largest_page_with_its_extrusion_is_refused() {
    let root = dir("too_big");
    let (source, out) = (root.join("source"), root.join("out"));
    // 4094 and its extrusion fill a page of 4096 exactly.
    write(&source, "g/wide.png", plain(4094, 1, [1, 1, 1, 255]));
    cook(&source, &out).expect("a cook");
    assert_eq!(qoi(&out, "atlas/g_0.qoi").header.width, 4096);
    write(&source, "g/wide.png", plain(4095, 1, [1, 1, 1, 255]));
    assert_eq!(
        cook(&source, &out).map(drop),
        Err(CookError::TooBig {
            path: "g/wide.png".to_string(),
            frame: 0,
            size: (4095, 1),
        })
    );
}
