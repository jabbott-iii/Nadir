//! The workspace's own `assets/`: its PNGs are the hand-made QOIs the
//! sandbox draws, pixel for pixel, and they cook to the same pixels.

use std::fs;
use std::path::Path;

use nadir_codec::Limits;
use nadir_codec::json::Value;
use nadir_codec::qoi::{self, Image};

use crate::common::{
    cook, dir, files, frame_pixels, frames, int, kbj, pixel, qoi as read_qoi, sprite,
};

fn assets() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
}

fn hand_made(name: &str) -> Image {
    let bytes =
        fs::read(assets().join(format!("sprites/{name}.qoi"))).expect("the hand-made sprite");
    qoi::decode(&bytes, Limits::default()).expect("a QOI file")
}

/// The pixels of `image` in the rectangle given.
fn crop(image: &Image, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
    let mut pixels = Vec::new();
    for row in y..y + h {
        for column in x..x + w {
            pixels.extend_from_slice(&pixel(image, column, row));
        }
    }
    pixels
}

#[test]
fn the_sandbox_s_sprites_cook_to_their_hand_made_pixels() {
    let out = dir("workspace");
    let report = cook(assets(), &out).expect("a cook");
    assert_eq!(report.jobs.len(), 1);
    assert_eq!(report.jobs[0].name, "atlas/sprites");
    let skipped: Vec<&str> = report
        .skipped
        .iter()
        .map(|(path, _)| path.as_str())
        .collect();
    assert_eq!(
        skipped,
        [
            "sprites/atlas.qoi",
            "sprites/grass.qoi",
            "sprites/person.qoi",
            "sprites/rock.qoi",
            "sprites/tiles.qoi",
            "sprites/tree.qoi"
        ]
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

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
    assert_eq!(
        frame_pixels(&out, &atlas, tiles).2,
        hand_made("tiles").pixels
    );

    // Every other frame: the hand-made sprite's pixels in the frame's
    // trimmed rectangle, and nothing it shows outside it.
    for (name, frame_width) in [("person", 12), ("tree", 24), ("rock", 16), ("grass", 32)] {
        let source = hand_made(name);
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
                (frame_width, source.header.height)
            );
            let (_, _, cooked) = frame_pixels(&out, &atlas, frame);
            assert_eq!(
                cooked,
                crop(&source, cell_x + ox, oy, w, h),
                "{name} frame {index}"
            );
            for y in 0..source.header.height {
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
    assert_eq!(frames(sprite(&atlas, "sprites/person")).len(), 5);

    // The group holds packed pages and a tile set's, whose files sort apart:
    // a second cook still finds it up to date, and changes nothing.
    let first = files(&out);
    let again = cook(assets(), &out).expect("a cook");
    assert!(!again.jobs[0].cooked, "the group was cooked again");
    assert!(again.warnings.is_empty(), "{:?}", again.warnings);
    assert_eq!(files(&out), first);
}
