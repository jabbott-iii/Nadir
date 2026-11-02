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

//! Aseprite tests: hand-written exports in the dialects of Aseprite 1.2 and
//! 1.3, laid out as its `doc_exporter.cpp` writes `--format json-array`
//! data, each checked field by field. The error cases, one by one, and the
//! clips of each direction are beside the reader, in `src/aseprite.rs`.

use nadir_codec::Limits;
use nadir_codec::aseprite::{self, Direction, Export, ExportFrame};

fn vector(name: &str) -> Export {
    let path = format!("{}/tests/aseprite/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"));
    aseprite::parse(&text, Limits::default()).unwrap_or_else(|error| panic!("{path}: {error}"))
}

/// A frame's rectangle in the sheet, its place in the canvas, and its
/// time.
fn placed(frame: &ExportFrame) -> ([u32; 4], [u32; 4], u32) {
    (
        [frame.x, frame.y, frame.width, frame.height],
        [
            frame.offset_x,
            frame.offset_y,
            frame.source_width,
            frame.source_height,
        ],
        frame.duration,
    )
}

/// Each tag's name, clip and whether it loops.
fn clips(export: &Export) -> Vec<(&str, Vec<u32>, bool)> {
    export
        .tags
        .iter()
        .map(|tag| (tag.name.as_str(), tag.frames.clone(), tag.looping))
        .collect()
}

#[test]
fn aseprite_1_2_with_layers_and_slices() {
    let export = vector("v1_2_layers_slices");
    assert_eq!(export.image, "knight.png");
    assert_eq!((export.width, export.height), (48, 16));
    let frames: Vec<_> = export.frames.iter().map(placed).collect();
    assert_eq!(
        frames,
        [
            ([0, 0, 16, 16], [0, 0, 16, 16], 100),
            ([16, 0, 16, 16], [0, 0, 16, 16], 150),
            ([32, 0, 16, 16], [0, 0, 16, 16], 100),
        ]
    );
    assert_eq!(
        clips(&export),
        [("idle", vec![0], true), ("swing", vec![1, 2], true)]
    );
    assert_eq!(export.tags[1].direction, Direction::PingPong);
}

#[test]
fn aseprite_1_3_with_repeats_and_user_data() {
    let export = vector("v1_3_tags");
    // The name as the artist typed it: the cook matches it by asset path.
    assert_eq!(export.image, "Runner.png");
    assert_eq!(export.frames.len(), 4);
    assert_eq!(
        placed(&export.frames[3]),
        ([24, 32, 24, 32], [0, 0, 24, 32], 80)
    );
    assert_eq!(
        clips(&export),
        [
            ("run", [0, 1, 2, 3].repeat(3), false),
            ("back", vec![3, 2, 1, 0], true),
            ("bounce", vec![2, 1, 2], false),
        ]
    );
    let bounce = &export.tags[2];
    assert_eq!((bounce.from, bounce.to, bounce.repeat), (1, 2, 2));
    assert_eq!(bounce.direction, Direction::PingPongReverse);
}

#[test]
fn a_trimmed_packed_sheet_with_merged_frames() {
    let export = vector("trimmed_packed");
    let frames: Vec<_> = export.frames.iter().map(placed).collect();
    assert_eq!(
        frames,
        [
            ([1, 1, 10, 14], [3, 2, 16, 16], 120),
            ([13, 1, 12, 9], [2, 7, 16, 16], 120),
            // A duplicate of the first, sharing its pixels.
            ([1, 1, 10, 14], [3, 2, 16, 16], 120),
            ([27, 1, 4, 4], [6, 12, 16, 16], 120),
        ]
    );
    assert_eq!(clips(&export), [("squish", vec![0, 1, 2, 3, 2, 1], true)]);
}

#[test]
fn an_export_without_tags() {
    let export = vector("no_tags");
    assert_eq!(export.frames.len(), 3);
    assert_eq!(placed(&export.frames[2]), ([0, 16, 8, 8], [0, 0, 8, 8], 60));
    assert!(export.tags.is_empty());
}

#[test]
fn the_sandbox_s_person_is_an_export_the_cook_takes() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/sprites/person.json"
    );
    let text = std::fs::read_to_string(path).expect("the sandbox's export");
    let export = aseprite::parse(&text, Limits::default()).expect("an export");
    assert_eq!(export.image, "person.png");
    assert_eq!((export.width, export.height), (60, 20));
    assert!(export.frames.iter().all(|frame| frame.duration == 100));
    assert_eq!(
        clips(&export),
        [("stand", vec![0], true), ("walk", vec![1, 2, 3, 4], true)]
    );
}
