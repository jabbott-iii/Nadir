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

//! The workspace's own `assets/`, which the sandbox draws and plays: each
//! frame the cook packs is its PNG's cell, pixel for pixel, the person's
//! clips are their Aseprite export's tags, and the atlas file is one the
//! game's reader takes; the field's LDtk project cooks to its level file,
//! and its prefabs to theirs; its sounds, mono at 48 kHz, are not
//! resampled. The sandbox's tests check the level against its map.

use std::fs;
use std::path::Path;

use nadir_codec::json::Value;
use nadir_codec::{Limits, atlas, kpcm, level, pcm, png, prefab, qoa, wav};
use nadir_cook::{Counts, SoundEncoding};

use crate::common::{
    clips, cook, dir, files, frame_pixels, frames, int, kbj, qoi as read_qoi, round_trip, sprite,
};

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
    let names: Vec<&str> = report.jobs.iter().map(|job| job.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "atlas/sprites",
            "audio/ambience/pond",
            "audio/music/north",
            "audio/music/south",
            "audio/sfx/step",
            "data/prefabs/rock",
            "data/prefabs/tree",
            "ldtk/levels/field"
        ]
    );
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let counts = |name: &str| {
        report
            .jobs
            .iter()
            .find(|job| job.name == name)
            .unwrap_or_else(|| panic!("the job {name}"))
            .counts
    };

    // The field: one level, whose trees and rocks are its 16 entities, which
    // the game's reader takes; and its prefabs, which the prefab reader
    // takes.
    assert_eq!(
        counts("ldtk/levels/field"),
        Some(Counts::Levels {
            levels: 1,
            entities: 16
        })
    );
    let field = fs::read(out.join("levels/field.klvl")).expect("the field's level file");
    let field = level::parse(&field, Limits::default()).expect("a level the game reads");
    assert_eq!(field.size, [1024, 768]);
    for name in ["rock", "tree"] {
        let path = format!("data/prefabs/{name}.kbj");
        let bytes = fs::read(out.join(&path)).expect("the prefab file");
        let file = prefab::parse(&bytes, Limits::default()).expect("a prefab the game reads");
        assert_eq!(file.name, name);
    }

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
    // The person's five frames of 12 × 20, from their Aseprite export,
    // trimmed: the standing frame and every other step are 10 wide, a pixel
    // in.
    let person = frames(sprite(&atlas, "sprites/person"));
    assert_eq!(person.len(), 5);
    let placed: Vec<[u32; 4]> = person
        .iter()
        .map(|frame| ["w", "h", "ox", "oy"].map(|key| int(frame, key)))
        .collect();
    assert_eq!(
        placed,
        [
            [10, 20, 1, 0],
            [12, 20, 0, 0],
            [10, 20, 1, 0],
            [12, 20, 0, 0],
            [10, 20, 1, 0]
        ]
    );
    // Its clips, the export's tags: standing, and the walk of four frames,
    // each 100 ms, looping.
    let person = parsed
        .sprites
        .iter()
        .find(|sprite| sprite.name == "sprites/person")
        .expect("the person");
    let clips = clips(person);
    assert_eq!(
        clips,
        [
            ("stand", true, vec![(0, 100)]),
            ("walk", true, vec![(1, 100), (2, 100), (3, 100), (4, 100)])
        ]
    );
    assert!(
        parsed
            .sprites
            .iter()
            .filter(|sprite| sprite.name != "sprites/person")
            .all(|sprite| sprite.clips.is_empty())
    );
    // The tree and the rock are trimmed.
    let tree = &frames(sprite(&atlas, "sprites/tree"))[0];
    assert_eq!((int(tree, "h"), int(tree, "oy")), (44, 4));

    // The sounds, made by games/sandbox/make_sounds.py at the rate the cook
    // cooks to: the step and the pond's water as sound files, sample for
    // sample, and the two tunes as music, QOA, which the game's reader
    // takes.
    for (job, length, file) in [
        ("audio/ambience/pond", 192_000, "audio/ambience/pond.kpcm"),
        ("audio/music/north", 384_000, "audio/music/north.qoa"),
        ("audio/music/south", 384_000, "audio/music/south.qoa"),
        ("audio/sfx/step", 3840, "audio/sfx/step.kpcm"),
    ] {
        let source = fs::read(assets().join(format!("{job}.wav"))).expect("the sound's WAV");
        let source = wav::decode(&source, Limits::default()).expect("a WAV");
        assert_eq!((source.channels, source.sample_rate), (1, 48_000), "{job}");
        let encoding = if file.ends_with(".qoa") {
            SoundEncoding::Qoa
        } else {
            SoundEncoding::Pcm16
        };
        assert_eq!(
            counts(job),
            Some(Counts::Sound {
                encoding,
                frames: length,
                sample_rate: 48_000
            }),
            "{job}"
        );
        let bytes = fs::read(out.join(file)).expect("the sound's file");
        if encoding == SoundEncoding::Qoa {
            let music = qoa::decode(&bytes, Limits::default()).expect("music the game reads");
            assert_eq!(
                music.header,
                qoa::Header {
                    channels: 1,
                    sample_rate: 48_000,
                    samples: length as u32
                }
            );
        } else {
            let sound = kpcm::decode(&bytes, Limits::default()).expect("a sound the game reads");
            assert_eq!(
                sound.header,
                kpcm::Header {
                    channels: 1,
                    sample_rate: 48_000,
                    frames: length as u32
                }
            );
            let samples: Vec<i16> = source.samples.iter().map(|&x| pcm::to_i16(x)).collect();
            assert!(sound.samples == samples, "{job}: its samples, as the WAV's");
        }
    }

    // The group holds packed pages and a tile set's, whose files sort apart:
    // a second cook still finds it up to date, and changes nothing.
    let first = files(&out);
    let again = cook(assets(), &out).expect("a cook");
    assert!(
        again.jobs.iter().all(|job| !job.cooked),
        "a job was cooked again"
    );
    assert!(again.warnings.is_empty(), "{:?}", again.warnings);
    assert_eq!(files(&out), first);
}

#[test]
fn the_sandbox_s_pack_reads_as_its_loose_files_do() {
    let out = dir("workspace_pack");
    cook(assets(), &out).expect("a cook");
    // The atlas file and its two pages, the four sounds, the two prefabs and
    // the field.
    assert_eq!(round_trip(&out), 10);
}
