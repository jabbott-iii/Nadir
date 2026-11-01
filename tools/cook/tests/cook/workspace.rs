//! The workspace's own `assets/`, which the sandbox draws: each frame the
//! cook packs is its PNG's cell, pixel for pixel, and the atlas file is one
//! the game's reader takes.

use std::fs;
use std::path::Path;

use nadir_codec::json::Value;
use nadir_codec::{Limits, atlas, png};

use crate::common::{cook, dir, files, frame_pixels, frames, int, kbj, qoi as read_qoi, sprite};

fn assets() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
}

/// The source image `sprites/<name>.png`.
fn source(name: &str) -> png::Image {
    let bytes = fs::read(assets().join(format!("sprites/{name}.png"))).expect("the sprite's PNG");
    png::decode(&bytes, Limits::default()).expect("a PNG")
}

/// The pixel at `(x, y)` of a PNG.
fn pixel(image: &png::Image, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * image.width + x) * 4) as usize;
    image.pixels[at..at + 4].try_into().expect("4 bytes")
}

/// The pixels of `image` in the rectangle given.
fn crop(image: &png::Image, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
    let mut pixels = Vec::new();
    for row in y..y + h {
        for column in x..x + w {
            pixels.extend_from_slice(&pixel(image, column, row));
        }
    }
    pixels
}

#[test]
fn the_sandbox_s_sprites_cook_to_their_sources_pixels() {
    let out = dir("workspace");
    let report = cook(assets(), &out).expect("a cook");
    assert_eq!(report.jobs.len(), 1);
    assert_eq!(report.jobs[0].name, "atlas/sprites");
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    // The game's reader takes it.
    let bytes = fs::read(out.join("atlas/sprites.kbj")).expect("the atlas file");
    let parsed = atlas::parse(&bytes, Limits::default()).expect("an atlas the game reads");
    let names: Vec<&str> = parsed
        .sprites
        .iter()
        .map(|sprite| sprite.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "sprites/grass",
            "sprites/person",
            "sprites/rock",
            "sprites/tiles",
            "sprites/tree"
        ]
    );

    let atlas = kbj(&out, "atlas/sprites.kbj");
    for page in atlas.get("pages").and_then(Value::as_array).expect("pages") {
        let image = read_qoi(
            &out,
            page.get("path").and_then(Value::as_str).expect("a path"),
        );
        assert_eq!(
            (int(page, "w"), int(page, "h")),
            (image.header.width, image.header.height)
        );
    }

    // The tile set, whole.
    let tiles = &frames(sprite(&atlas, "sprites/tiles"))[0];
    assert_eq!(frame_pixels(&out, &atlas, tiles).2, source("tiles").pixels);

    // Every other frame: its cell's pixels in its trimmed rectangle, and
    // nothing the cell shows outside it.
    for (name, frame_width) in [("person", 12), ("tree", 24), ("rock", 16), ("grass", 32)] {
        let source = source(name);
        for (index, frame) in frames(sprite(&atlas, &format!("sprites/{name}")))
            .iter()
            .enumerate()
        {
            let cell_x = index as u32 * frame_width;
            let (ox, oy, w, h) = (
                int(frame, "ox"),
                int(frame, "oy"),
                int(frame, "w"),
                int(frame, "h"),
            );
            assert_eq!(
                (int(frame, "sw"), int(frame, "sh")),
                (frame_width, source.height)
            );
            let (_, _, cooked) = frame_pixels(&out, &atlas, frame);
            assert_eq!(
                cooked,
                crop(&source, cell_x + ox, oy, w, h),
                "{name} frame {index}"
            );
            for y in 0..source.height {
                for x in cell_x..cell_x + frame_width {
                    let inside =
                        (cell_x + ox..cell_x + ox + w).contains(&x) && (oy..oy + h).contains(&y);
                    if !inside {
                        assert_eq!(
                            pixel(&source, x, y)[3],
                            0,
                            "{name} ({x}, {y}) was trimmed away"
                        );
                    }
                }
            }
        }
    }
    // The person's frames are whole, as their sidecar asks until a clip's
    // frame has a size of its own (roadmap M5.5): five of 12 × 20.
    let person = frames(sprite(&atlas, "sprites/person"));
    assert_eq!(person.len(), 5);
    for frame in person {
        let placed = ["w", "h", "ox", "oy"].map(|key| int(frame, key));
        assert_eq!(placed, [12, 20, 0, 0]);
    }
    // The tree and the rock are trimmed.
    let tree = &frames(sprite(&atlas, "sprites/tree"))[0];
    assert_eq!((int(tree, "h"), int(tree, "oy")), (44, 4));

    // The group holds packed pages and a tile set's, whose files sort apart:
    // a second cook still finds it up to date, and changes nothing.
    let first = files(&out);
    let again = cook(assets(), &out).expect("a cook");
    assert!(!again.jobs[0].cooked, "the group was cooked again");
    assert!(again.warnings.is_empty(), "{:?}", again.warnings);
    assert_eq!(files(&out), first);
}
