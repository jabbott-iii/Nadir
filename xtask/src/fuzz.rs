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

//! `cargo xtask fuzz-smoke`: throws mutated inputs at the parsers for a
//! while, and fails if one of them panics or hangs.
//!
//! This is the dependency-free "smoke" fuzzer of `docs/04-quality.md`. It
//! has no coverage feedback and keeps nothing between runs: every run
//! starts from the seed files in `fuzz/corpus/<target>/`.

mod engine;
mod mutate;

use std::env::{self, VarError};
use std::fmt::Display;
use std::fs;
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use nadir_codec::inflate::ErrorKind as InflateError;
use nadir_codec::{
    CodecError, Limits, adler32, aseprite, atlas, bmfont, inflate, json, kbj, klz, kpak, ldtk,
    level, png, prefab, qoa, qoi, wav,
};
use nadir_core::cli::Args;
use nadir_core::hash::{crc32, fnv1a64};
use nadir_core::testing::{DEFAULT_SEED, SEED_VAR};

use self::engine::{Config, Finding, Kind, Outcome};
use crate::error::Error;

/// A parser that can be fuzzed.
struct Target {
    /// The name for `--target`, and of the corpus directory.
    name: &'static str,
    /// Hands one input to the parser. Rejecting the input is fine;
    /// panicking, or never returning, is a finding.
    run: fn(&[u8]),
}

/// Every fuzz target. A new parser adds itself here, with seeds in
/// `fuzz/corpus/<name>/` (`.claude/rules/parsers.md`).
const TARGETS: &[Target] = &[
    Target {
        name: "json",
        run: parse_json,
    },
    Target {
        name: "qoi",
        run: decode_qoi,
    },
    Target {
        name: "inflate",
        run: inflate_streams,
    },
    Target {
        name: "png",
        run: decode_png,
    },
    Target {
        name: "klz",
        run: klz_streams,
    },
    Target {
        name: "kbj",
        run: decode_kbj,
    },
    Target {
        name: "bmfont",
        run: parse_bmfont,
    },
    Target {
        name: "wav",
        run: decode_wav,
    },
    Target {
        name: "atlas",
        run: parse_atlas,
    },
    Target {
        name: "aseprite",
        run: parse_aseprite,
    },
    Target {
        name: "ldtk",
        run: parse_ldtk,
    },
    Target {
        name: "level",
        run: parse_level_file,
    },
    Target {
        name: "prefab",
        run: parse_prefab,
    },
    Target {
        name: "kpak",
        run: parse_pack,
    },
    Target {
        name: "qoa",
        run: decode_qoa,
    },
];

/// How long each target is fuzzed when `--secs` is not given.
const DEFAULT_SECS: u64 = 60;

/// How long a target may spend on one input before that counts as a hang.
/// `cargo xtask` builds optimized (the `xtask` profile), where inputs
/// mutated from the seeds take a few milliseconds at most. The slowest
/// valid inputs are PNGs at the 64 KiB input cap that inflate to a large
/// image: [`PNG_LIMIT`] keeps those to about a third of a second on a
/// development machine (16-bit RGBA, 2048 × 2048, interlaced: 325 ms,
/// decoded twice), a sixth of this. A slow path short of the limit shows in
/// the run's slowest input, which each target's line reports.
const HANG_LIMIT: Duration = Duration::from_secs(2);

/// The longest input a mutation may produce.
const MAX_INPUT_LEN: usize = 64 * 1024;

/// Where the seeds of each target are, relative to the workspace root.
const CORPUS: &str = "fuzz/corpus";

/// Where failing inputs are saved, relative to the workspace root.
const CRASHES: &str = "fuzz/crashes";

/// The `json` target: `nadir_codec::json::parse`, with the limits the game
/// uses.
fn parse_json(input: &[u8]) {
    // The parser takes text. Bytes that are not UTF-8 become U+FFFD, so
    // that every input reaches it.
    let text = String::from_utf8_lossy(input);
    let _ = json::parse(&text, Limits::default());
}

/// The `qoi` target: `nadir_codec::qoi::decode`, with the limits the game
/// uses.
fn decode_qoi(input: &[u8]) {
    let _ = qoi::decode(input, Limits::default());
}

/// The most bytes the `inflate` target lets a stream decompress to. A 64 KiB
/// input can give about 67 MB, so with the default limit `TooMuchOutput`
/// could never happen; with this one it often does, and runs stay fast.
const INFLATE_LIMIT: usize = 1 << 20;

/// The `inflate` target: `nadir_codec::inflate`, reading the input as a
/// zlib stream and, after its two header bytes, as a bare DEFLATE stream.
///
/// It also checks the answers, so that a wrong one is a finding: a zlib
/// stream that decodes holds a bare stream that decodes to the same bytes,
/// and decodes again with a limit of exactly its length, but not one less.
/// The messages give lengths, not the bytes, which could be megabytes.
fn inflate_streams(input: &[u8]) {
    let limits = |max_decoded_bytes| Limits {
        max_decoded_bytes,
        ..Limits::default()
    };
    let _ = inflate::raw(input.get(2..).unwrap_or_default(), limits(INFLATE_LIMIT));
    let Ok(out) = inflate::zlib(input, limits(INFLATE_LIMIT)) else {
        return;
    };
    let inside = input
        .get(2..input.len().saturating_sub(4))
        .unwrap_or_default();
    let bare = inflate::raw(inside, limits(INFLATE_LIMIT));
    assert!(
        bare.as_ref() == Ok(&out),
        "the bare stream inside a zlib stream of {} bytes decodes to {:?}",
        out.len(),
        bare.as_ref().map(Vec::len)
    );
    let exact = inflate::zlib(input, limits(out.len()));
    assert!(
        exact.as_ref() == Ok(&out),
        "a limit of exactly the output's {} bytes gives {:?}",
        out.len(),
        exact.as_ref().map(Vec::len)
    );
    if let Some(less) = out.len().checked_sub(1) {
        assert!(
            matches!(
                inflate::zlib(input, limits(less)),
                Err(CodecError::Inflate {
                    kind: InflateError::TooMuchOutput { .. },
                    ..
                })
            ),
            "a limit one byte short of the output's {} bytes is not enforced",
            out.len()
        );
    }
}

/// The most bytes the `klz` target lets a stream claim, so that a claimed
/// size it has no room for is the common case, and runs stay fast.
const KLZ_LIMIT: usize = 1 << 20;

/// The `klz` target: `nadir_codec::klz`. The input's first 4 bytes,
/// little-endian and up to [`KLZ_LIMIT`], are the size the stream must
/// give, as a pack's table of contents gives it; the rest is the stream.
///
/// It also checks the answers, so that a wrong one is a finding: a stream
/// that ends short of its size decodes at the size it reached, and the
/// input compressed decompresses to the input.
fn klz_streams(input: &[u8]) {
    let limits = Limits {
        max_decoded_bytes: KLZ_LIMIT,
        ..Limits::default()
    };
    if let Some((&size, stream)) = input.split_first_chunk::<4>() {
        let raw_size = u32::from_le_bytes(size) as usize % (KLZ_LIMIT + 1);
        if let Err(CodecError::Klz {
            kind: klz::ErrorKind::TooShort { found, .. },
            ..
        }) = klz::decompress(stream, raw_size, limits)
        {
            let again = klz::decompress(stream, found, limits);
            assert!(
                again.as_ref().map(Vec::len) == Ok(found),
                "a stream that ends after {found} bytes gives {:?} at that size",
                again.as_ref().map(Vec::len)
            );
        }
    }
    let stream = klz::compress(input);
    let back = klz::decompress(&stream, input.len(), Limits::default());
    assert!(
        back.as_deref() == Ok(input),
        "{} bytes compress to a stream that decompresses to {:?}",
        input.len(),
        back.as_ref().map(Vec::len)
    );
}

/// The `kbj` target: `nadir_codec::kbj::decode`, with the limits the game
/// uses. It also checks that whatever decodes encodes back to the same
/// bytes, the one encoding of its value.
fn decode_kbj(input: &[u8]) {
    if let Ok(value) = kbj::decode(input, Limits::default()) {
        assert!(
            kbj::encode(&value, Limits::default()).as_deref() == Ok(input),
            "a file of {} bytes decodes to a value that encodes otherwise",
            input.len()
        );
    }
}

/// The `bmfont` target: `nadir_codec::bmfont::parse`, with the default
/// limits.
fn parse_bmfont(input: &[u8]) {
    // The parser takes text. Bytes that are not UTF-8 become U+FFFD, so
    // that every input reaches it.
    let text = String::from_utf8_lossy(input);
    let _ = bmfont::parse(&text, Limits::default());
}

/// The `wav` target: `nadir_codec::wav::decode`, with the default limits.
fn decode_wav(input: &[u8]) {
    let _ = wav::decode(input, Limits::default());
}

/// The `qoa` target: `nadir_codec::qoa::decode`, with the limits the game
/// uses. A sound it accepts has every sample its header gives, and its
/// samples, whatever signal they make, encode again to a file that decodes
/// to as many, which fuzzes the encoder too.
fn decode_qoa(input: &[u8]) {
    let Ok(sound) = qoa::decode(input, Limits::default()) else {
        return;
    };
    let header = sound.header;
    assert_eq!(
        sound.samples.len(),
        header.samples as usize * usize::from(header.channels),
        "a sound without the samples its header gives"
    );
    let file = qoa::encode(&header, &sound.samples).expect("a sound the decoder gave encodes");
    let again = qoa::decode(&file, Limits::default()).expect("what the encoder wrote decodes");
    assert_eq!(again.header, header, "the encoder wrote another header");
    assert_eq!(
        again.samples.len(),
        sound.samples.len(),
        "the encoder lost samples"
    );
}

/// The `atlas` target: `nadir_codec::atlas::parse`, with the default
/// limits. What it accepts must keep the promises the game's atlas asset
/// and the facade's clips rely on: each frame on a page that exists and
/// inside it, and inside its cell; the names in order; a tile set one
/// whole page, with no clips; each clip of frames the sprite has, that
/// last, its name with no `/`.
fn parse_atlas(input: &[u8]) {
    let Ok(file) = atlas::parse(input, Limits::default()) else {
        return;
    };
    assert!(!file.pages.is_empty(), "an atlas with no page");
    for pair in file.sprites.windows(2) {
        assert!(
            pair[0].name.as_bytes() < pair[1].name.as_bytes(),
            "sprites out of order"
        );
    }
    let within =
        |start: u32, length: u32, end: u32| u64::from(start) + u64::from(length) <= u64::from(end);
    for sprite in &file.sprites {
        assert!(!sprite.frames.is_empty(), "a sprite with no frame");
        for frame in &sprite.frames {
            let page = &file.pages[frame.page as usize];
            assert!(
                within(frame.x, frame.width, page.width)
                    && within(frame.y, frame.height, page.height),
                "a frame off its page"
            );
            assert!(
                within(frame.offset_x, frame.width, frame.source_width)
                    && within(frame.offset_y, frame.height, frame.source_height)
                    && frame.source_width > 0
                    && frame.source_height > 0,
                "a frame outside its cell"
            );
        }
        if let atlas::SpriteKind::Tiles([width, height]) = sprite.kind {
            let frame = &sprite.frames[0];
            let page = &file.pages[frame.page as usize];
            assert!(
                sprite.frames.len() == 1
                    && (frame.width, frame.height) == (page.width, page.height)
                    && frame.width % width == 0
                    && frame.height % height == 0,
                "a tile set that is not a page of whole tiles"
            );
            assert!(sprite.clips.is_empty(), "a tile set with clips");
        }
        for pair in sprite.clips.windows(2) {
            assert!(
                pair[0].name.as_bytes() < pair[1].name.as_bytes(),
                "clips out of order"
            );
        }
        for clip in &sprite.clips {
            assert!(
                !clip.name.is_empty() && !clip.name.contains('/'),
                "a clip's name that would not make its full name"
            );
            assert!(!clip.frames.is_empty(), "a clip with no frame");
            assert!(
                clip.frames
                    .iter()
                    .all(|step| (step.frame as usize) < sprite.frames.len() && step.millis > 0),
                "a clip of a frame the sprite lacks, or of one that lasts zero"
            );
        }
    }
}

/// The `aseprite` target: `nadir_codec::aseprite::parse`, with the default
/// limits. What it accepts must keep the promises the cook relies on: one
/// frame at least, each inside the sheet and inside the canvas they all
/// share, lasting; each tag's clip of frames the export has, all of them
/// together within the limit, the names apart and with no `/`.
fn parse_aseprite(input: &[u8]) {
    // The reader takes text. Bytes that are not UTF-8 become U+FFFD, so
    // that every input reaches it.
    let text = String::from_utf8_lossy(input);
    let limits = Limits::default();
    let Ok(export) = aseprite::parse(&text, limits) else {
        return;
    };
    let within =
        |start: u32, length: u32, end: u32| u64::from(start) + u64::from(length) <= u64::from(end);
    assert!(!export.frames.is_empty(), "an export with no frame");
    assert!(
        !export.image.is_empty() && !export.image.contains(['/', '\\']),
        "a sheet that is not a file name"
    );
    let canvas = (
        export.frames[0].source_width,
        export.frames[0].source_height,
    );
    for frame in &export.frames {
        assert!(
            frame.width > 0
                && frame.height > 0
                && within(frame.x, frame.width, export.width)
                && within(frame.y, frame.height, export.height),
            "a frame off its sheet"
        );
        assert!(
            (frame.source_width, frame.source_height) == canvas
                && within(frame.offset_x, frame.width, frame.source_width)
                && within(frame.offset_y, frame.height, frame.source_height),
            "a frame outside its canvas"
        );
        assert!(frame.duration > 0, "a frame that lasts zero");
    }
    let mut total = 0;
    let mut names: Vec<&str> = Vec::new();
    for tag in &export.tags {
        assert!(
            !tag.frames.is_empty()
                && tag
                    .frames
                    .iter()
                    .all(|&frame| (frame as usize) < export.frames.len()),
            "a clip of a frame the export lacks"
        );
        assert!(
            !tag.name.is_empty() && !tag.name.contains('/'),
            "a tag's name that would not make a clip's"
        );
        total += tag.frames.len();
        names.push(&tag.name);
    }
    assert!(total <= limits.max_entries, "clips past the limit");
    names.sort_unstable();
    assert!(
        names.windows(2).all(|pair| pair[0] != pair[1]),
        "two tags of one name"
    );
}

/// The project the `ldtk` target reads its inputs against as level files:
/// the seed `external.ldtk`, which saves its two levels apart.
fn ldtk_project() -> &'static ldtk::Project {
    static PROJECT: OnceLock<ldtk::Project> = OnceLock::new();
    PROJECT.get_or_init(|| {
        ldtk::parse_project(
            include_str!("../../fuzz/corpus/ldtk/external.ldtk"),
            Limits::default(),
        )
        .expect("the seed external.ldtk is a project the reader takes")
    })
}

/// The `ldtk` target: `nadir_codec::ldtk::parse_project`, and
/// `parse_level` against [`ldtk_project`], with the default limits. What
/// they accept must keep the promises the cook relies on: a version of
/// 1.5.x; definitions of their own uids, whose references are to ones the
/// project has; each level saved apart exactly as the project says, with a
/// size; each layer of its definition's type, of a grid that its IntGrid
/// fills with values it defines; each tile of its layer's tile set, flipped
/// 0 to 3 and from 0 to 1 opaque; each entity of a definition the project
/// has.
fn parse_ldtk(input: &[u8]) {
    // The reader takes text. Bytes that are not UTF-8 become U+FFFD, so
    // that every input reaches it.
    let text = String::from_utf8_lossy(input);
    let limits = Limits::default();
    if let Ok(project) = ldtk::parse_project(&text, limits) {
        assert!(
            project.json_version.starts_with("1.5."),
            "a version other than 1.5.x"
        );
        let unique = |uids: Vec<i64>| {
            uids.iter()
                .enumerate()
                .all(|(index, uid)| !uids[..index].contains(uid))
        };
        assert!(
            unique(project.tilesets.iter().map(|tileset| tileset.uid).collect())
                && unique(project.layers.iter().map(|layer| layer.uid).collect())
                && unique(project.entities.iter().map(|entity| entity.uid).collect()),
            "two definitions of one uid"
        );
        for tileset in &project.tilesets {
            assert!(
                tileset.tile_count() <= limits.max_entries as u64,
                "a tile set of more tiles than the limit"
            );
            assert!(
                tileset
                    .custom_data
                    .iter()
                    .all(|data| u64::from(data.tile) < tileset.tile_count()),
                "custom data of a tile the set lacks"
            );
        }
        for layer in &project.layers {
            assert!(
                layer
                    .tileset
                    .is_none_or(|uid| project.tileset(uid).is_some()),
                "a layer of a tile set the project lacks"
            );
            assert!(
                layer.int_grid_values.iter().all(|value| value.value >= 1),
                "an IntGrid value below 1"
            );
        }
        for level in &project.levels {
            assert!(
                level.external_path.is_some() == level.layers.is_none()
                    && level.layers.is_none() == project.external_levels,
                "a level saved apart, or not, against its project"
            );
            check_ldtk_level(&project, level, limits);
        }
    }
    let project = ldtk_project();
    if let Ok(level) = ldtk::parse_level(&text, project, limits) {
        assert!(level.layers.is_some(), "a level file with no layers");
        assert!(
            project
                .levels
                .iter()
                .any(|known| known.iid == level.iid && known.external_path.is_some()),
            "a level file of a level the project does not save apart"
        );
        check_ldtk_level(project, &level, limits);
    }
}

/// The promises of [`parse_ldtk`] that a level of `project` keeps.
fn check_ldtk_level(project: &ldtk::Project, level: &ldtk::Level, limits: Limits) {
    assert!(level.width >= 1 && level.height >= 1, "a level of no size");
    for layer in level.layers.iter().flatten() {
        let definition = project
            .layer(layer.layer_def)
            .expect("a layer of a definition the project has");
        assert_eq!(layer.kind, definition.kind, "a layer of another type");
        let cells = u64::from(layer.columns) * u64::from(layer.rows);
        assert!(
            cells <= limits.max_entries as u64 && layer.grid_size >= 1,
            "a layer of more cells than the limit, or of cells of no size"
        );
        if layer.kind == ldtk::LayerKind::IntGrid {
            assert_eq!(layer.int_grid.len() as u64, cells, "an IntGrid not filled");
            assert!(
                layer.int_grid.iter().all(|&value| value == 0
                    || definition
                        .int_grid_values
                        .iter()
                        .any(|known| known.value == value)),
                "an IntGrid value the layer does not define"
            );
        } else {
            assert!(
                layer.int_grid.is_empty(),
                "values in a layer of another type"
            );
        }
        let tiles = layer.grid_tiles.iter().chain(&layer.auto_layer_tiles);
        match layer.tileset.and_then(|uid| project.tileset(uid)) {
            Some(tileset) => {
                for tile in tiles {
                    assert!(
                        u64::from(tile.tile) < tileset.tile_count()
                            && tile.flip <= 3
                            && (0.0..=1.0).contains(&tile.alpha),
                        "a tile its set lacks, or flipped or faded out of range"
                    );
                }
            }
            None => assert!(
                layer.tileset.is_none() && tiles.count() == 0,
                "tiles with no tile set, or of one the project lacks"
            ),
        }
        assert!(
            layer
                .entities
                .iter()
                .all(|entity| project.entity(entity.entity_def).is_some()),
            "an entity of a definition the project lacks"
        );
    }
}

/// The `level` target: `nadir_codec::level::parse`, with the default
/// limits. What it accepts must keep the promises the game's level loading
/// relies on: a size; its assets in order, each tile set's atlas among
/// them; each tile set of at most 65535 tiles, its animations in order,
/// apart and of its tiles; each layer and tall tile of a tile set it has,
/// each cell of its set, one cell for each of a layer's or grid's; each
/// tall block inside its set; each entity a prefab's name, its overrides
/// named as components, none `null`.
fn parse_level_file(input: &[u8]) {
    let Ok(file) = level::parse(input, Limits::default()) else {
        return;
    };
    assert!(
        file.size
            .iter()
            .all(|side| (1..=level::MAX_SIDE).contains(side)),
        "a level of no size, or too large"
    );
    assert!(
        file.camera
            .is_none_or(|camera| camera.width >= 1 && camera.height >= 1),
        "a camera of no size"
    );
    assert!(
        [&file.music, &file.ambience]
            .into_iter()
            .flatten()
            .all(|path| !path.is_empty()),
        "music or ambience of no path"
    );
    assert!(
        file.assets.windows(2).all(|pair| pair[0] < pair[1])
            && file.assets.iter().all(|asset| !asset.is_empty()),
        "assets out of order, or empty"
    );
    for tileset in &file.tilesets {
        assert!(
            file.assets.contains(&tileset.atlas) && !tileset.sprite.is_empty(),
            "a tile set's atlas that is not an asset, or of no sprite"
        );
        let count = tileset.tile_count();
        assert!(
            (1..=level::MAX_TILES).contains(&count),
            "a tile set of no tiles, or of too many"
        );
        let mut next = 0;
        for animation in &tileset.animations {
            let end = u32::from(animation.tile) + u32::from(animation.frames);
            assert!(
                u32::from(animation.tile) >= next
                    && end <= count
                    && animation.frames >= 2
                    && animation.millis >= 1,
                "an animation out of order, overlapping, past its set, or of no time"
            );
            next = end;
        }
    }
    let count = |index: u32| file.tilesets[index as usize].tile_count();
    for layer in &file.layers {
        let cells = u64::from(layer.size[0]) * u64::from(layer.size[1]);
        assert_eq!(
            layer.cells.len() as u64,
            cells,
            "a layer's cells do not fill it"
        );
        let tiles = count(layer.tileset);
        assert!(
            layer
                .cells
                .iter()
                .flatten()
                .all(|cell| u32::from(cell.tile) < tiles),
            "a cell of a tile its set lacks"
        );
        assert!(
            layer.size[0] >= 1 && layer.size[1] >= 1 && (1..=level::MAX_CELL).contains(&layer.cell),
            "a layer of no cells, or of cells of no size or too large"
        );
    }
    if let Some(grid) = &file.grid {
        let cells = u64::from(grid.size[0]) * u64::from(grid.size[1]);
        assert_eq!(
            grid.solid.len() as u64,
            cells,
            "a grid's cells do not fill it"
        );
        assert!(
            grid.size[0] >= 1 && grid.size[1] >= 1 && (1..=level::MAX_CELL).contains(&grid.cell),
            "a grid of no cells, or of cells of no size or too large"
        );
        assert!(!grid.layer.is_empty(), "a grid on no collision layer");
    }
    for tall in &file.tall {
        let tileset = &file.tilesets[tall.tileset as usize];
        let columns = u32::from(tileset.columns);
        let (column, row) = (
            u32::from(tall.tile) % columns,
            u32::from(tall.tile) / columns,
        );
        assert!(
            tall.size[0] >= 1
                && tall.size[1] >= 1
                && column + u32::from(tall.size[0]) <= columns
                && row + u32::from(tall.size[1]) <= u32::from(tileset.rows),
            "a tall block outside its set"
        );
    }
    for entity in &file.entities {
        assert!(
            prefab::is_name(&entity.prefab),
            "an entity of no prefab's name"
        );
        assert!(
            entity
                .overrides
                .iter()
                .all(|(name, value)| prefab::is_component_name(name) && !value.is_null()),
            "an override of no component's name, or null"
        );
    }
}

/// The `prefab` target: `nadir_codec::prefab::parse`, with the default
/// limits. What it accepts must keep the promises nadir-scene relies on:
/// a prefab's name, and a base's that is not its own; its components named
/// as types, none `null`.
fn parse_prefab(input: &[u8]) {
    let Ok(file) = prefab::parse(input, Limits::default()) else {
        return;
    };
    assert!(prefab::is_name(&file.name), "a prefab of no name");
    assert!(
        file.extends
            .as_deref()
            .is_none_or(|base| prefab::is_name(base) && base != file.name),
        "a base of no name, or the prefab itself"
    );
    assert!(
        file.components
            .iter()
            .all(|(name, value)| prefab::is_component_name(name) && !value.is_null()),
        "a component of no type's name, or null"
    );
}

/// The most bytes the `kpak` target lets a TOC, or an entry, be, as
/// [`KLZ_LIMIT`] bounds a stream: a 64 KiB input of KLZ can give 16 MiB.
const KPAK_LIMIT: usize = 1 << 20;

/// The limits the `kpak` target reads with: the game's, with
/// [`KPAK_LIMIT`].
fn kpak_limits() -> Limits {
    Limits {
        max_decoded_bytes: KPAK_LIMIT,
        ..Limits::default()
    }
}

/// The `kpak` target: `nadir_codec::kpak::parse`, then each entry read, on
/// the input and on a copy with its TOC's size and its checksums mended
/// ([`repaired_pack`]), so that mutations get past the CRCs to the entries,
/// the paths and the KLZ streams.
fn parse_pack(input: &[u8]) {
    check_pack(input);
    if let Some(repaired) = repaired_pack(input)
        && repaired != input
    {
        check_pack(&repaired);
    }
}

/// Reads `input` as a pack. What it accepts must keep the promises
/// nadir-assets' `PackSource` relies on: the TOC of the header, the same
/// read whole or in parts; its entries in ascending order of their path
/// hashes, each between the header and the TOC at a multiple of 16, a
/// stored one of its raw size; each path of its entry's hash, and found by
/// it; and each entry read to its raw size, its stored bytes if stored, or
/// refused as damaged or as a bad stream, and nothing else.
fn check_pack(input: &[u8]) {
    let limits = kpak_limits();
    let Ok(toc) = kpak::parse(input, limits) else {
        return;
    };
    let header = kpak::header(input, input.len() as u64, limits);
    let header = header.expect("a pack that reads has a header that reads");
    let start = header.toc_offset as usize;
    assert!(
        kpak::toc(&header, &input[start..], limits).as_ref() == Ok(&toc),
        "the TOC read in parts is not the TOC read whole"
    );
    assert!(
        toc.len() == header.entry_count as usize && toc.has_names() == header.names,
        "a TOC of {} entries, names {}, for a header of {}, names {}",
        toc.len(),
        toc.has_names(),
        header.entry_count,
        header.names
    );
    assert!(
        toc.entries()
            .windows(2)
            .all(|pair| pair[0].path_hash < pair[1].path_hash),
        "entries out of order, or one twice"
    );
    for (index, entry) in toc.entries().iter().enumerate() {
        let end = entry.offset + u64::from(entry.stored_size);
        assert!(
            entry.offset >= kpak::HEADER_SIZE as u64
                && entry.offset.is_multiple_of(kpak::ALIGN)
                && end <= header.toc_offset,
            "an entry of {} bytes at byte {} of a pack whose TOC is at {}",
            entry.stored_size,
            entry.offset,
            header.toc_offset
        );
        assert!(
            entry.codec == kpak::Codec::Klz || entry.stored_size == entry.raw_size,
            "a stored entry of {} bytes, {} raw",
            entry.stored_size,
            entry.raw_size
        );
        if let Some(name) = toc.name(index) {
            assert!(
                !name.is_empty() && fnv1a64(name.as_bytes()) == entry.path_hash,
                "a path of {} bytes that is not its entry's",
                name.len()
            );
            assert!(
                toc.find(name) == Some(index),
                "a path the TOC does not find"
            );
        }
        match toc.read(input, index, limits) {
            Ok(bytes) => assert!(
                bytes.len() == entry.raw_size as usize
                    && (entry.codec == kpak::Codec::Klz
                        || bytes[..] == input[entry.offset as usize..end as usize]),
                "an entry of {} raw bytes read as {}",
                entry.raw_size,
                bytes.len()
            ),
            Err(CodecError::Kpak {
                kind: kpak::ErrorKind::Damaged { .. } | kpak::ErrorKind::BadStream { .. },
                ..
            }) => {}
            Err(error) => panic!("an entry of a pack that reads gives: {error}"),
        }
    }
}

/// A copy of `input` with what a mutation most often breaks mended, or
/// `None` if it is shorter than a header, or its TOC cannot be found:
///
/// - the header's magic, version, reserved field and unknown flags, as a
///   pack has them;
/// - the TOC: where the header says, if that is inside the input at a
///   multiple of 16, or else the header's TOC size back from the end,
///   moved to the next multiple of 16 by zeros before it; its size, to the
///   end; and its entry count, to the entries it has room for;
/// - each entry's path hash, of its path, when every path can be read, and
///   the entries, with their paths, put back in order of their hashes;
/// - each entry's CRC-32, of its stored bytes where they are in the input,
///   and the TOC's.
fn repaired_pack(input: &[u8]) -> Option<Vec<u8>> {
    const SIZE: usize = kpak::ENTRY_SIZE;
    let u32_at = |bytes: &[u8], at: usize| {
        u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
    };
    let u64_at = |bytes: &[u8], at: usize| {
        u64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"))
    };
    let mut pack = input.to_vec();
    if pack.len() < kpak::HEADER_SIZE {
        return None;
    }
    pack[..4].copy_from_slice(&kpak::MAGIC);
    pack[4..6].copy_from_slice(&kpak::VERSION.to_le_bytes());
    let names = pack[6] & 1 != 0;
    pack[6..8].copy_from_slice(&u16::from(names).to_le_bytes());
    pack[28..32].fill(0);

    let aligned = |at: usize| at >= kpak::HEADER_SIZE && at.is_multiple_of(kpak::ALIGN as usize);
    let toc = match usize::try_from(u64_at(&pack, 12)) {
        Ok(toc) if toc <= pack.len() && aligned(toc) => toc,
        _ => {
            let toc = pack.len().checked_sub(u32_at(&pack, 20) as usize)?;
            if toc < kpak::HEADER_SIZE {
                return None;
            }
            let padding = toc.next_multiple_of(kpak::ALIGN as usize) - toc;
            pack.splice(toc..toc, std::iter::repeat_n(0, padding));
            toc + padding
        }
    };
    pack[12..20].copy_from_slice(&(toc as u64).to_le_bytes());
    let toc_size = u32::try_from(pack.len() - toc).ok()?;
    pack[20..24].copy_from_slice(&toc_size.to_le_bytes());
    let per_entry = SIZE + if names { 2 } else { 0 };
    let count = (u32_at(&pack, 8) as usize).min(toc_size as usize / per_entry);
    pack[8..12].copy_from_slice(&(count as u32).to_le_bytes());

    // The entries and their paths, put back in order of their hashes.
    let table = toc + count * SIZE;
    let mut entries: Vec<([u8; SIZE], Option<Vec<u8>>)> = pack[toc..table]
        .as_chunks::<SIZE>()
        .0
        .iter()
        .map(|&entry| (entry, None))
        .collect();
    let mut end = table;
    if names {
        for (entry, path) in &mut entries {
            let Some(length) = pack.get(end..end + 2) else {
                break;
            };
            let length = usize::from(u16::from_le_bytes([length[0], length[1]]));
            let Some(name) = pack.get(end + 2..end + 2 + length) else {
                break;
            };
            entry[..8].copy_from_slice(&fnv1a64(name).to_le_bytes());
            *path = Some(name.to_vec());
            end += 2 + length;
        }
    }
    if !names || entries.iter().all(|(_, path)| path.is_some()) {
        entries.sort_by_key(|(entry, _)| u64_at(entry, 0));
        let mut at = toc;
        for (entry, _) in &entries {
            pack[at..at + SIZE].copy_from_slice(entry);
            at += SIZE;
        }
        for (_, path) in &entries {
            if let Some(path) = path {
                let length = path.len() as u16;
                pack[at..at + 2].copy_from_slice(&length.to_le_bytes());
                pack[at + 2..at + 2 + path.len()].copy_from_slice(path);
                at += 2 + path.len();
            }
        }
    }

    for index in 0..count {
        let at = toc + index * SIZE;
        let (offset, stored) = (u64_at(&pack, at + 8), u32_at(&pack, at + 16));
        let stored = usize::try_from(offset)
            .ok()
            .and_then(|start| pack.get(start..start.checked_add(stored as usize)?));
        if let Some(stored) = stored {
            let crc = crc32(stored);
            pack[at + 28..at + 32].copy_from_slice(&crc.to_le_bytes());
        }
    }
    let crc = crc32(&pack[toc..]);
    pack[24..28].copy_from_slice(&crc.to_le_bytes());
    Some(pack)
}

/// The most bytes of pixels the `png` target decodes to: a 2048 × 2048
/// RGBA image, a sixteenth of the game's limit, so that an image of 64 KiB
/// that inflates a thousand to one stays far from [`HANG_LIMIT`].
const PNG_LIMIT: usize = 16 << 20;

/// The limits the `png` target decodes with: the game's, with
/// [`PNG_LIMIT`].
fn png_limits() -> Limits {
    Limits {
        max_decoded_bytes: PNG_LIMIT,
        ..Limits::default()
    }
}

/// The `png` target: `nadir_codec::png::decode`, with [`png_limits`], on
/// the input and on repaired copies of it.
fn decode_png(input: &[u8]) {
    let _ = png::decode(input, png_limits());
    let _ = repaired_pngs(input);
}

/// Decodes repaired copies of a PNG file, so that more mutations reach the
/// decoder's later stages: one with its structure and checksums repaired,
/// and, if that one's image data is the wrong size for its header or is
/// followed by more, one with the image data mended too. Returns each copy
/// and what it decoded to.
fn repaired_pngs(input: &[u8]) -> Vec<(Vec<u8>, Result<png::Image, CodecError>)> {
    let mut decoded = Vec::new();
    let Some(mut repaired) = Repaired::new(input) else {
        return decoded;
    };
    repaired.fix_adler32();
    let bytes = repaired.to_bytes();
    let result = png::decode(&bytes, png_limits());
    let mended = match &result {
        Err(CodecError::Png {
            kind:
                png::ErrorKind::TooLittleImageData { needed }
                | png::ErrorKind::TooMuchImageData { needed },
            ..
        }) => *needed <= REPAIR_LIMIT && repaired.repack(*needed),
        Err(CodecError::Png {
            kind: png::ErrorKind::TrailingImageData,
            ..
        }) => repaired.trim_image_data(),
        _ => false,
    };
    decoded.push((bytes, result));
    if mended {
        let bytes = repaired.to_bytes();
        let result = png::decode(&bytes, png_limits());
        decoded.push((bytes, result));
    }
    decoded
}

/// The most bytes a repair inflates image data to. The decoder itself stops
/// at what the header needs; without this, one high-ratio stream would cost
/// the repair seconds in a debug build.
const REPAIR_LIMIT: usize = 1 << 20;

/// The chunk types a repair looks for when it has lost its place: every
/// chunk the PNG Third Edition defines.
const KNOWN_CHUNKS: [&[u8; 4]; 25] = [
    b"IHDR", b"PLTE", b"IDAT", b"IEND", b"tRNS", b"cHRM", b"gAMA", b"iCCP", b"sBIT", b"sRGB",
    b"cICP", b"mDCV", b"cLLI", b"tEXt", b"zTXt", b"iTXt", b"bKGD", b"hIST", b"pHYs", b"sPLT",
    b"eXIf", b"tIME", b"acTL", b"fcTL", b"fdAT",
];

/// Whether `input` holds a known chunk type at `at`.
fn known_type_at(input: &[u8], at: usize) -> bool {
    input
        .get(at..)
        .and_then(<[u8]>::first_chunk::<4>)
        .is_some_and(|kind| KNOWN_CHUNKS.contains(&kind))
}

/// Where the first known chunk type at `from` or later is.
fn next_known_type(input: &[u8], from: usize) -> Option<usize> {
    let rest = input.get(from..)?;
    let position = rest
        .windows(4)
        .position(|window| KNOWN_CHUNKS.iter().any(|known| known.as_slice() == window))?;
    Some(from + position)
}

/// A PNG file read leniently, to be written back with the PNG signature, a
/// right CRC for every chunk, and an `IEND` at the end.
///
/// Without this, almost every mutation would stop at the signature or a
/// CRC, and the decoder's later stages would see little but the seeds.
/// The walk starts at `IHDR`, wherever the first few bytes put it, takes
/// the first chunk as `IHDR` of 13 bytes, and ends at the first `IEND`. An insertion or deletion moves the chunks after it:
/// where the next chunk's type is not letters, or is unknown with a known
/// type a few bytes away, the walk finds the next known type and gives the
/// bytes before it to the chunk before. A chunk whose length runs past the
/// end of the input ends at the next known type, or else takes the rest.
struct Repaired {
    chunks: Vec<([u8; 4], Vec<u8>)>,
}

impl Repaired {
    /// `None` if the input has no chunk after the signature.
    fn new(input: &[u8]) -> Option<Self> {
        // Each chunk's type and where its data is. Ranges, not copies, so
        // that a chunk cut short when the walk finds its place again costs
        // nothing to cut.
        let mut chunks: Vec<([u8; 4], Range<usize>)> = Vec::new();
        // Where the last chunk read starts, and where its data starts.
        let mut last_chunk = 0;
        let mut last_data = 0;
        // IHDR's type belongs at 12; a damaged signature may have moved it.
        let start = (4..=20)
            .find(|&at| input.get(at..at + 4) == Some(b"IHDR"))
            .map_or(8, |kind| kind - 4);
        let mut at = start;
        while let Some(&[l0, l1, l2, l3, t0, t1, t2, t3]) =
            input.get(at..).and_then(<[u8]>::first_chunk::<8>)
        {
            // The first chunk is IHDR, of 13 bytes, whatever its type and
            // length say: unit tests cover the checks those would fail.
            let header = at == start;
            let kind = if header { *b"IHDR" } else { [t0, t1, t2, t3] };
            let letters = kind.iter().all(u8::is_ascii_alphabetic);
            // A known type up to 3 bytes either side of an unknown one:
            // up to 3 bytes were inserted or deleted before it.
            let shifted = if letters && !KNOWN_CHUNKS.contains(&&kind) {
                (1..=3)
                    .flat_map(|by| [at.checked_sub(by), Some(at + by)])
                    .flatten()
                    .find(|&start| start > last_chunk && known_type_at(input, start + 4))
            } else {
                None
            };
            let lost = if letters {
                shifted
            } else {
                // After a deletion the next chunk starts before `at`, so
                // look from the last chunk's data on.
                let from = if chunks.is_empty() { at + 5 } else { last_data };
                next_known_type(input, from).map(|kind| kind - 4)
            };
            if let Some(next) = lost {
                // The bytes up to the next chunk, but its length and the
                // CRC before it, are the last chunk's (the first, IHDR,
                // keeps its 13): none if the next starts inside it. A chunk
                // found there starts after the last one, so the walk always
                // moves on.
                if chunks.len() > 1
                    && let Some((_, data)) = chunks.last_mut()
                {
                    data.end = next.saturating_sub(4).max(last_data);
                }
                at = next;
                continue;
            }
            if !letters {
                break;
            }
            let data_at = at + 8;
            let rest = input.get(data_at..).unwrap_or_default();
            let claimed = if header {
                13
            } else {
                u32::from_be_bytes([l0, l1, l2, l3]) as usize
            };
            let length = if claimed.saturating_add(4) <= rest.len() {
                claimed
            } else {
                // Past the end: up to the next known type, or all of it.
                next_known_type(input, data_at)
                    .map_or(rest.len(), |kind| (kind - 4).saturating_sub(4 + data_at))
            };
            chunks.push((kind, data_at..data_at + length));
            if &kind == b"IEND" {
                break;
            }
            last_chunk = at;
            last_data = data_at;
            at = data_at + length + 4;
        }
        if chunks.is_empty() {
            return None;
        }
        let mut chunks: Vec<([u8; 4], Vec<u8>)> = chunks
            .into_iter()
            .map(|(kind, data)| (kind, input.get(data).unwrap_or_default().to_vec()))
            .collect();
        if chunks.last().is_none_or(|(kind, _)| kind != b"IEND") {
            chunks.push((*b"IEND", Vec::new()));
        }
        // IEND, last, takes the rest of the input when its length is
        // mutated.
        if let Some((_, data)) = chunks.last_mut() {
            data.clear();
        }
        Some(Self { chunks })
    }

    /// The image data, all `IDAT` chunks' data in order.
    fn image_data(&self) -> Vec<u8> {
        self.idats()
            .flat_map(|(_, data)| data.iter().copied())
            .collect()
    }

    fn idats(&self) -> impl Iterator<Item = &([u8; 4], Vec<u8>)> {
        self.chunks.iter().filter(|(kind, _)| kind == b"IDAT")
    }

    /// The image data's zlib stream inflated, if it inflates to at most
    /// [`REPAIR_LIMIT`] bytes.
    fn inflate(&self) -> Result<Vec<u8>, CodecError> {
        let limits = Limits {
            max_decoded_bytes: REPAIR_LIMIT,
            ..Limits::default()
        };
        inflate::zlib(&self.image_data(), limits)
    }

    /// Writes `bytes` over the image data from `offset`, across the `IDAT`
    /// chunks they fall in.
    fn write_image_data(&mut self, offset: usize, bytes: &[u8]) {
        let mut start = 0;
        for (kind, data) in &mut self.chunks {
            if kind != b"IDAT" {
                continue;
            }
            for (i, byte) in data.iter_mut().enumerate() {
                if let Some(&new) = (start + i).checked_sub(offset).and_then(|j| bytes.get(j)) {
                    *byte = new;
                }
            }
            start += data.len();
        }
    }

    /// Rewrites the image data's Adler-32 when it is the only thing wrong
    /// with its zlib stream.
    fn fix_adler32(&mut self) {
        if let Err(CodecError::Inflate {
            offset,
            kind: InflateError::BadChecksum { computed, .. },
        }) = self.inflate()
        {
            self.write_image_data(offset, &computed.to_be_bytes());
        }
    }

    /// Cuts the image data where its zlib stream ends, when bytes follow it.
    /// False if they do not.
    fn trim_image_data(&mut self) -> bool {
        let Err(CodecError::Inflate {
            offset,
            kind: InflateError::TrailingBytes,
        }) = self.inflate()
        else {
            return false;
        };
        let mut start = 0;
        for (kind, data) in &mut self.chunks {
            if kind == b"IDAT" {
                let len = data.len();
                data.truncate(offset.saturating_sub(start));
                start += len;
            }
        }
        true
    }

    /// Replaces the image data with what it inflates to, cut or padded with
    /// zeros to `needed` bytes, as a zlib stream of stored blocks split
    /// over the same `IDAT` chunks. False if the image data does not
    /// inflate.
    fn repack(&mut self, needed: usize) -> bool {
        let Ok(mut raw) = self.inflate() else {
            return false;
        };
        raw.resize(needed, 0);
        let mut stream = vec![0x78, 0x01];
        let blocks: Vec<&[u8]> = if raw.is_empty() {
            vec![&[]]
        } else {
            raw.chunks(0xffff).collect()
        };
        for (i, block) in blocks.iter().enumerate() {
            stream.push(u8::from(i + 1 == blocks.len()));
            let len = block.len() as u16;
            stream.extend_from_slice(&len.to_le_bytes());
            stream.extend_from_slice(&(!len).to_le_bytes());
            stream.extend_from_slice(block);
        }
        stream.extend_from_slice(&adler32::checksum(&raw).to_be_bytes());
        let count = self.idats().count().max(1);
        let share = stream.len().div_ceil(count);
        let mut pieces = stream.chunks(share.max(1));
        for (kind, data) in &mut self.chunks {
            if kind == b"IDAT" {
                *data = pieces.next().unwrap_or_default().to_vec();
            }
        }
        true
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
        for (kind, data) in &self.chunks {
            let start = out.len();
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            let crc = crc32(out.get(start + 4..).unwrap_or_default());
            out.extend_from_slice(&crc.to_be_bytes());
        }
        out
    }
}

/// Runs `fuzz-smoke` with the arguments that follow the command name.
/// Returns whether every target ran clean.
pub(crate) fn command(args: Args, root: &Path, out: &mut dyn Write) -> Result<bool, Error> {
    let seed = match env::var(SEED_VAR) {
        Ok(text) => Some(text),
        Err(VarError::NotPresent) => None,
        Err(VarError::NotUnicode(_)) => {
            return Err(Error::Usage(format!("{SEED_VAR} is not valid Unicode")));
        }
    };
    let options = Options::parse(args, TARGETS, seed.as_deref())?;
    let budget = Budget {
        max_inputs: u64::MAX,
        hang_limit: HANG_LIMIT,
    };
    fuzz(&options, root, budget, out)
}

/// The limits on one target's run that the command line does not set.
#[derive(Clone, Copy)]
struct Budget {
    /// Stop after this many inputs, if the time has not run out first.
    max_inputs: u64,
    /// How long the target may spend on one input.
    hang_limit: Duration,
}

/// What the command line and the environment asked for.
struct Options<'a> {
    targets: Vec<&'a Target>,
    /// How long each target is fuzzed.
    duration: Duration,
    seed: u64,
}

impl<'a> Options<'a> {
    /// Reads `[--secs N] [--target NAME]`. `seed` is the value of
    /// `NADIR_SEED`, if it is set.
    fn parse(mut args: Args, targets: &'a [Target], seed: Option<&str>) -> Result<Self, Error> {
        let usage = |error: nadir_core::CoreError| Error::Usage(error.to_string());
        let secs: Option<u64> = args.parsed("--secs").map_err(usage)?;
        let name = args.value("--target").map_err(usage)?;
        if let Some(argument) = args.finish().map_err(usage)?.first() {
            return Err(Error::Usage(format!(
                "`fuzz-smoke` takes only options, but got `{argument}`"
            )));
        }

        let chosen = match &name {
            None => targets.iter().collect(),
            Some(name) => match targets.iter().find(|target| target.name == name) {
                Some(target) => vec![target],
                None => {
                    let known: Vec<&str> = targets.iter().map(|target| target.name).collect();
                    return Err(Error::Usage(format!(
                        "unknown fuzz target `{name}`; the targets are: {}",
                        known.join(", ")
                    )));
                }
            },
        };
        let seed = match seed {
            None => DEFAULT_SEED,
            Some(text) => parse_seed(text).ok_or_else(|| {
                Error::Usage(format!(
                    "{SEED_VAR} must be a u64, in decimal or as 0x-prefixed hex, but it is {text:?}"
                ))
            })?,
        };
        Ok(Self {
            targets: chosen,
            duration: Duration::from_secs(secs.unwrap_or(DEFAULT_SECS)),
            seed,
        })
    }
}

/// Reads a seed the way `nadir_core::testing` does.
fn parse_seed(text: &str) -> Option<u64> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

/// Fuzzes each chosen target in turn and prints what happened.
fn fuzz(
    options: &Options<'_>,
    root: &Path,
    budget: Budget,
    out: &mut dyn Write,
) -> Result<bool, Error> {
    let output = Error::Output;
    writeln!(
        out,
        "fuzz-smoke: {}, {} s each, seed {:#x}",
        plural(options.targets.len(), "target"),
        options.duration.as_secs(),
        options.seed
    )
    .map_err(output)?;
    // The first lines are worth seeing before a long run, not after it.
    out.flush().map_err(output)?;

    let mut findings = 0;
    for target in &options.targets {
        let seeds = load_corpus(&root.join(CORPUS).join(target.name))?;
        let seed_count = seeds.len();
        let config = Config {
            // Each target draws from its own random stream, so adding a
            // target does not change what the others are given.
            seed: options.seed ^ fnv1a64(target.name.as_bytes()),
            duration: options.duration,
            max_inputs: budget.max_inputs,
            hang_limit: budget.hang_limit,
            max_len: MAX_INPUT_LEN,
            crashes: root.join(CRASHES).join(target.name),
        };
        let outcome = engine::run(target.run, seeds, config)?;
        out.write_all(describe(target.name, seed_count, &outcome, root).as_bytes())
            .map_err(output)?;
        out.flush().map_err(output)?;
        findings += outcome.findings.len();
        if outcome.hung {
            // The thread that hung is still busy, and stays so until this
            // process exits. Carrying on would only skew the other targets.
            break;
        }
    }

    if findings == 0 {
        writeln!(out, "fuzz-smoke: ok").map_err(output)?;
    } else {
        writeln!(
            out,
            "To see a finding again, copy its input into {CORPUS}/<target>/: seeds run first, \
             unchanged.\nfuzz-smoke: FAILED ({})",
            plural(findings, "finding")
        )
        .map_err(output)?;
    }
    Ok(findings == 0)
}

/// The lines that report one target's run.
fn describe(name: &str, seeds: usize, outcome: &Outcome, root: &Path) -> String {
    let seconds = outcome.elapsed.as_secs_f64();
    let rate = if seconds > 0.0 {
        outcome.inputs as f64 / seconds
    } else {
        0.0
    };
    let mut text = format!(
        "{name}: {}, {} in {seconds:.1} s ({rate:.0} per second)",
        plural(seeds, "seed"),
        plural(outcome.inputs, "input"),
    );
    if let Some((took, len)) = outcome.slowest {
        text.push_str(&format!(
            ", the slowest {:.1} ms ({})",
            took.as_secs_f64() * 1000.0,
            plural(len, "byte")
        ));
    }
    text.push('\n');
    for finding in &outcome.findings {
        text.push_str(&describe_finding(name, finding, root));
    }
    if outcome.hung {
        text.push_str(&format!("{name}: stopped early, because the target hung\n"));
    }
    text
}

fn describe_finding(name: &str, finding: &Finding, root: &Path) -> String {
    let what = match finding.kind {
        Kind::Panic if finding.site == finding.message => format!("panic: {}", finding.message),
        Kind::Panic => format!("panic at {}: {}", finding.site, finding.message),
        Kind::Hang => format!("hang: {}", finding.message),
    };
    let path = finding.path.strip_prefix(root).unwrap_or(&finding.path);
    format!(
        "finding: {name}: {what}\n         input saved to {} ({}), which is 1 of {} that did this\n",
        path.display(),
        plural(finding.len, "byte"),
        plural(finding.hits, "input"),
    )
}

/// `1 seed`, `2 seeds`.
fn plural<N: Display + PartialEq + From<u8>>(count: N, noun: &str) -> String {
    if count == N::from(1) {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// Reads the seeds of one target: every file in `dir`, in name order.
/// Hidden files are skipped. A target without seeds cannot be fuzzed.
fn load_corpus(dir: &Path) -> Result<Vec<Vec<u8>>, Error> {
    let read_error = |source| Error::Read {
        path: dir.to_path_buf(),
        source,
    };
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(dir).map_err(read_error)? {
        let entry = entry.map_err(read_error)?;
        let hidden = entry.file_name().to_string_lossy().starts_with('.');
        if !hidden && entry.file_type().map_err(read_error)?.is_file() {
            files.push(entry.path());
        }
    }
    files.sort();

    let mut seeds = Vec::with_capacity(files.len());
    for path in files {
        match fs::read(&path) {
            Ok(seed) => seeds.push(seed),
            Err(source) => return Err(Error::Read { path, source }),
        }
    }
    if seeds.is_empty() {
        return Err(Error::NoSeeds {
            path: dir.to_path_buf(),
        });
    }
    Ok(seeds)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh workspace root for one test, under `target/tmp`, holding
    /// the given files (path relative to the root, and contents).
    fn root(test: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../target/tmp")
            .join(format!("xtask-fuzz-{test}"));
        // It may be left over from an earlier run.
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        for (path, contents) in files {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        root
    }

    fn accept(_: &[u8]) {}

    fn reject_long_inputs(input: &[u8]) {
        assert!(input.len() <= 40, "more than 40 bytes");
    }

    const FAKES: &[Target] = &[
        Target {
            name: "accept",
            run: accept,
        },
        Target {
            name: "short",
            run: reject_long_inputs,
        },
    ];

    /// A budget of `max_inputs` inputs, with no hang limit to speak of.
    fn inputs(max_inputs: u64) -> Budget {
        Budget {
            max_inputs,
            hang_limit: Duration::from_secs(3600),
        }
    }

    fn options<'a>(args: &[&str], seed: Option<&str>) -> Result<Options<'a>, Error> {
        Options::parse(Args::new(args.iter().copied()), FAKES, seed)
    }

    fn usage_error(args: &[&str], seed: Option<&str>) -> String {
        match options(args, seed) {
            Err(Error::Usage(message)) => message,
            Err(other) => panic!("expected a usage error, got {other:?}"),
            Ok(_) => panic!("expected a usage error for {args:?}"),
        }
    }

    /// Runs `fuzz` on fake targets and returns its verdict and output, with
    /// the measured time and rate replaced by `?`.
    fn fuzz_output(args: &[&str], root: &Path, max_inputs: u64) -> (bool, String) {
        let options = options(args, Some("7")).unwrap();
        let mut out = Vec::new();
        let passed = fuzz(&options, root, inputs(max_inputs), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<String> = text
            .lines()
            .map(|line| match line.split_once(" in ") {
                // The times vary from run to run, and so does which input
                // is the slowest.
                Some((counts, rest)) if rest.contains("per second)") => {
                    let slowest = if rest.contains(", the slowest ") {
                        ", the slowest ? ms (? bytes)"
                    } else {
                        ""
                    };
                    format!("{counts} in ? s (? per second){slowest}")
                }
                _ => line.to_string(),
            })
            .collect();
        (passed, lines.join("\n"))
    }

    #[test]
    fn options_default_to_every_target_for_a_minute_with_the_fixed_seed() {
        let options = options(&[], None).unwrap();
        let names: Vec<&str> = options.targets.iter().map(|target| target.name).collect();
        assert_eq!(names, ["accept", "short"]);
        assert_eq!(options.duration, Duration::from_secs(60));
        assert_eq!(options.seed, DEFAULT_SEED);
    }

    #[test]
    fn options_take_the_time_and_one_target() {
        let options = options(&["--target", "short", "--secs=5"], None).unwrap();
        let names: Vec<&str> = options.targets.iter().map(|target| target.name).collect();
        assert_eq!(names, ["short"]);
        assert_eq!(options.duration, Duration::from_secs(5));
        assert_eq!(
            self::options(&["--secs", "0"], None).unwrap().duration,
            Duration::ZERO
        );
    }

    #[test]
    fn options_take_the_seed_from_the_environment() {
        assert_eq!(options(&[], Some("12345")).unwrap().seed, 12345);
        assert_eq!(options(&[], Some(" 0x1F ")).unwrap().seed, 0x1f);
        assert_eq!(options(&[], Some("0XFF")).unwrap().seed, 0xff);
        assert_eq!(
            options(&[], Some("18446744073709551615")).unwrap().seed,
            u64::MAX
        );
        for bad in [
            "",
            "soon",
            "-1",
            "0x",
            "0xZZ",
            "18446744073709551616",
            "1.5",
        ] {
            assert_eq!(
                usage_error(&[], Some(bad)),
                format!(
                    "NADIR_SEED must be a u64, in decimal or as 0x-prefixed hex, but it is {bad:?}"
                )
            );
        }
    }

    #[test]
    fn bad_options_are_usage_errors() {
        assert_eq!(
            usage_error(&["--target", "yaml"], None),
            "unknown fuzz target `yaml`; the targets are: accept, short"
        );
        // Names are matched whole.
        assert!(usage_error(&["--target", "acc"], None).starts_with("unknown fuzz target `acc`"));
        assert_eq!(
            usage_error(&["--secs", "soon"], None),
            "invalid value `soon` for option `--secs`"
        );
        assert_eq!(
            usage_error(&["--secs", "-5"], None),
            "invalid value `-5` for option `--secs`"
        );
        assert_eq!(
            usage_error(&["--secs"], None),
            "option `--secs` needs a value"
        );
        assert_eq!(
            usage_error(&["--target"], None),
            "option `--target` needs a value"
        );
        assert_eq!(
            usage_error(&["--fast"], None),
            "unexpected argument `--fast`"
        );
        assert_eq!(
            usage_error(&["json"], None),
            "`fuzz-smoke` takes only options, but got `json`"
        );
    }

    #[test]
    fn the_real_targets_have_distinct_names_and_include_json() {
        let names: Vec<&str> = TARGETS.iter().map(|target| target.name).collect();
        assert!(names.contains(&"json"));
        for (index, name) in names.iter().enumerate() {
            assert!(!names[..index].contains(name), "{name} is listed twice");
        }
    }

    #[test]
    fn the_json_target_takes_any_bytes() {
        for input in [
            &b""[..],
            b"[1, 2, 3]",
            b"{\"a\": tru",
            b"\xff\xfe\x00[",
            &[b'['; 5000],
        ] {
            parse_json(input);
        }
    }

    #[test]
    fn the_qoa_target_takes_any_bytes() {
        let mut huge = b"qoaf\xff\xff\xff\xff\x08\x00\xac\x44\x14\x00\x40\x88".to_vec();
        huge.extend_from_slice(&[0x5a; 5000]);
        for input in [&b""[..], b"qoaf", &huge, &[0xff; 5000]] {
            decode_qoa(input);
        }
    }

    #[test]
    fn the_qoi_target_takes_any_bytes() {
        let mut huge = b"qoif\xff\xff\xff\xff\xff\xff\xff\xff\x04\x00".to_vec();
        huge.extend_from_slice(&[0xfd; 100]);
        for input in [&b""[..], b"qoif", &huge, &[0xc0; 5000]] {
            decode_qoi(input);
        }
    }

    #[test]
    fn the_inflate_target_takes_any_bytes() {
        // After a zlib header, which the target skips for its bare stream:
        // a stored block that claims more than there is, and a dynamic
        // header with every count at its most.
        for input in [
            &b""[..],
            &[0x78, 0x9c],
            &[0x78, 0x01, 0x01, 0xff, 0xff, 0x00, 0x00, 0x01],
            &[0x78, 0x01, 0xfd, 0xff, 0xff, 0xff],
            &[0x02; 3000],
        ] {
            inflate_streams(input);
        }
        // Streams that decode, so that the target's checks run: "hello",
        // and 2,400 empty fixed blocks (four to 5 bytes) before an empty
        // final one.
        let hello = [
            0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00, 0x06, 0x2c, 0x02, 0x15,
        ];
        let mut empty_blocks = vec![0x78, 0x01];
        for _ in 0..600 {
            empty_blocks.extend_from_slice(&[0x02, 0x08, 0x20, 0x80, 0x00]);
        }
        empty_blocks.extend_from_slice(&[0x03, 0x00, 0x00, 0x00, 0x00, 0x01]);
        for input in [&hello[..], &empty_blocks] {
            assert!(inflate::zlib(input, Limits::default()).is_ok());
            inflate_streams(input);
        }
    }

    #[test]
    fn the_klz_target_takes_any_bytes() {
        // Each a size, then a stream: "abc" claimed as 10 bytes, so that the
        // target decodes it again at 3; claimed as 2; a match before the
        // start; a size past what the stream could give; no stream; no
        // size.
        for input in [
            &b"\x0a\x00\x00\x00\x30abc"[..],
            b"\x02\x00\x00\x00\x30abc",
            b"\x1e\x00\x00\x00\x0f\x05\x00\x00",
            b"\xff\xff\x0f\x00\x0f\x05\x00\xff",
            b"\x00\x00\x00\x00",
            b"",
            &[0xff; 3000],
        ] {
            klz_streams(input);
        }
        assert!(matches!(
            klz::decompress(b"\x30abc", 10, Limits::default()),
            Err(CodecError::Klz {
                kind: klz::ErrorKind::TooShort { found: 3, .. },
                ..
            })
        ));
        assert!(matches!(
            klz::decompress(b"\x0f\x05\x00\x00", 30, Limits::default()),
            Err(CodecError::Klz {
                kind: klz::ErrorKind::OffsetTooFar { offset: 5, .. },
                ..
            })
        ));
    }

    /// The seed of two entries, with paths, both whole.
    fn two_entries() -> Vec<u8> {
        let limits = kpak_limits();
        seeds_of("kpak")
            .into_iter()
            .find(|seed| {
                kpak::parse(seed, limits).is_ok_and(|toc| {
                    toc.len() == 2
                        && toc.has_names()
                        && (0..2).all(|index| toc.read(seed, index, limits).is_ok())
                })
            })
            .expect("a seed of two entries")
    }

    #[test]
    fn the_kpak_target_takes_any_bytes() {
        let two = &two_entries();
        let mut huge = two.clone();
        // A million entries and a TOC of 4 GiB, past the end.
        huge[8..12].copy_from_slice(&1_000_000u32.to_le_bytes());
        huge[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
        let mut past = two.clone();
        // A TOC that starts past the end.
        past[12..20].copy_from_slice(&u64::MAX.to_le_bytes());
        for input in [&b""[..], b"KPAK", &two[..40], &huge, &past, &[0xff; 3000]] {
            parse_pack(input);
        }
        assert_eq!(repaired_pack(&two[..31]), None);
        // A TOC size past the start: no TOC to be found.
        let mut lost = past.clone();
        lost[20..24].copy_from_slice(&200u32.to_le_bytes());
        assert_eq!(repaired_pack(&lost), None);
    }

    #[test]
    fn a_pack_repair_mends_the_toc_size_and_the_checksums() {
        let limits = kpak_limits();
        let two = &two_entries();
        assert_eq!(repaired_pack(two).as_ref(), Some(two));
        let header = kpak::header(two, two.len() as u64, limits).unwrap();
        let toc = kpak::parse(two, limits).unwrap();
        let first = toc.entries()[0];
        let start = first.offset as usize;
        // A byte of an entry changed, and the TOC's checksum: both mended.
        let mut damaged = two.clone();
        damaged[start] ^= 1;
        damaged[24] ^= 1;
        assert!(kpak::parse(&damaged, limits).is_err());
        let none_damaged = |pack: &[u8]| {
            let toc = kpak::parse(pack, limits).unwrap();
            (0..toc.len()).all(|index| {
                !matches!(
                    toc.read(pack, index, limits),
                    Err(CodecError::Kpak {
                        kind: kpak::ErrorKind::Damaged { .. },
                        ..
                    })
                )
            })
        };
        let repaired = repaired_pack(&damaged).unwrap();
        assert_ne!(
            kpak::parse(&repaired, limits).unwrap().entries()[0].crc32,
            first.crc32
        );
        assert!(none_damaged(&repaired));
        // The TOC's start lost, and the header's magic, version, unknown
        // flags and reserved field: found again, and restored.
        let mut lost = two.clone();
        lost[12..20].copy_from_slice(&u64::MAX.to_le_bytes());
        lost[..8].copy_from_slice(b"KPAX\x02\x00\x07\x00");
        lost[28..32].copy_from_slice(b"\x01\x00\x00\x80");
        assert!(kpak::parse(&lost, limits).is_err());
        assert_eq!(repaired_pack(&lost).as_ref(), Some(two));
        // A byte added before the TOC: the TOC found from the end, moved to
        // a multiple of 16, and the CRC of the entry whose bytes moved
        // mended.
        let mut shifted = two.clone();
        shifted.insert(toc.entries()[1].offset as usize, b'!');
        shifted[12..20].copy_from_slice(&u64::MAX.to_le_bytes());
        let repaired = repaired_pack(&shifted).unwrap();
        assert_eq!(repaired.len(), two.len() + 16);
        assert!(none_damaged(&repaired));
        // The two entries and their paths swapped, and a byte of a path
        // hash changed: the hash taken from its path, and the order mended.
        let at = header.toc_offset as usize;
        let names = at + 2 * kpak::ENTRY_SIZE;
        let length = usize::from(u16::from_le_bytes([two[names], two[names + 1]]));
        let (first_name, second_name) = two[names..].split_at(2 + length);
        let mut reordered = two[..at].to_vec();
        reordered.extend_from_slice(&two[at + kpak::ENTRY_SIZE..names]);
        reordered.extend_from_slice(&two[at..at + kpak::ENTRY_SIZE]);
        reordered.extend_from_slice(second_name);
        reordered.extend_from_slice(first_name);
        reordered[at + 3] ^= 0x40;
        assert!(kpak::parse(&reordered, limits).is_err());
        assert_eq!(repaired_pack(&reordered).as_ref(), Some(two));
        // A byte added after the TOC: its size mended.
        let mut longer = two.clone();
        longer.push(0);
        let repaired = repaired_pack(&longer).unwrap();
        assert!(kpak::parse(&repaired, limits).is_err_and(|error| matches!(
            error,
            CodecError::Kpak {
                kind: kpak::ErrorKind::TrailingBytes,
                ..
            }
        )));
    }

    #[test]
    fn the_kbj_target_takes_any_bytes() {
        for input in [
            &b""[..],
            b"KBJ1",
            b"KBJ1\x06\x02\x00\x03\x01",
            b"KBJ1\x07\xff",
        ] {
            decode_kbj(input);
        }
    }

    #[test]
    fn the_png_target_decodes_to_16_mib_at_most() {
        // A PNG of an RGBA8 header and no image data.
        let header = |width: u32, height: u32| {
            let mut ihdr = b"IHDR".to_vec();
            ihdr.extend_from_slice(&width.to_be_bytes());
            ihdr.extend_from_slice(&height.to_be_bytes());
            ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
            let mut png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0d".to_vec();
            png.extend_from_slice(&ihdr);
            png.extend_from_slice(&crc32(&ihdr).to_be_bytes());
            png.extend_from_slice(b"\x00\x00\x00\x00IEND\xae\x42\x60\x82");
            png
        };
        let too_many = |result: Result<png::Image, CodecError>| {
            matches!(
                result,
                Err(CodecError::Png {
                    kind: png::ErrorKind::TooManyBytes { .. },
                    ..
                })
            )
        };
        assert!(too_many(png::decode(&header(2049, 2048), png_limits())));
        assert!(!too_many(png::decode(&header(2048, 2048), png_limits())));
        // The game's limits take the larger image.
        assert!(!too_many(png::decode(
            &header(2049, 2048),
            Limits::default()
        )));
    }

    #[test]
    fn the_png_target_takes_any_bytes() {
        let mut huge = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        huge.extend_from_slice(&[0xff; 100]);
        // An 8192 × 8192 header with 20 bytes of image data.
        let mut big = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x20\x00\x00\x00\x20\x00\x08\x06\x00\x00\x00\x00\x00\x00\x00".to_vec();
        big.extend_from_slice(b"\x00\x00\x00\x14IDAT");
        big.extend_from_slice(&[0x78, 0x01, 0x01, 0x0f, 0x00, 0xf0, 0xff]);
        big.extend_from_slice(&[0; 13]);
        // Chunk types to find again among bytes that are not letters.
        let mut junk = big[..33].to_vec();
        for _ in 0..400 {
            junk.extend_from_slice(b"\xff\xff\xff\xffIDAT");
        }
        // Thousands of chunks whose lengths reach 8 bytes short of the end,
        // where the walk loses its place, and each of which it must cut
        // short when it finds the next: the repaired copy stays as small
        // as the input. The same with IHDR, which only the first chunk is.
        let overlap = |kind: &[u8; 4]| {
            let mut input = big[..33].to_vec();
            while input.len() < 60_000 {
                let length = (61_000 - input.len()) as u32;
                input.extend_from_slice(&length.to_be_bytes());
                input.extend_from_slice(kind);
            }
            input.resize(61_020, 0);
            input
        };
        let (overlapping, overlapping_headers) = (overlap(b"IDAT"), overlap(b"IHDR"));
        for input in [
            &b""[..],
            b"\x89PNG\r\n\x1a\n",
            &huge,
            &big,
            &junk,
            &overlapping,
            &overlapping_headers,
            &[0x49; 5000],
        ] {
            decode_png(input);
        }
        for input in [&overlapping, &overlapping_headers] {
            assert!(kinds(input).len() > 5000);
            for (bytes, _) in repaired_pngs(input) {
                assert!(bytes.len() < 2 * input.len(), "{}", bytes.len());
            }
        }
    }

    fn png_seeds() -> Vec<Vec<u8>> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(CORPUS)
            .join("png");
        load_corpus(&dir).unwrap()
    }

    fn decoded(input: &[u8]) -> png::Image {
        png::decode(input, Limits::default()).unwrap()
    }

    /// What the first repaired copy of `input` decodes to.
    fn first_repair(input: &[u8]) -> Result<png::Image, CodecError> {
        repaired_pngs(input).swap_remove(0).1
    }

    fn kinds(input: &[u8]) -> Vec<[u8; 4]> {
        let repaired = Repaired::new(input).unwrap();
        repaired.chunks.iter().map(|(kind, _)| *kind).collect()
    }

    /// Where the first IDAT chunk's data starts, and its length.
    fn first_idat(png: &[u8]) -> (usize, usize) {
        let kind = png.windows(4).position(|w| w == b"IDAT").unwrap();
        let length = u32::from_be_bytes(png[kind - 4..kind].try_into().unwrap());
        (kind + 4, length as usize)
    }

    #[test]
    fn a_repair_mends_the_checksums_and_the_signature() {
        for seed in png_seeds() {
            let image = decoded(&seed);
            assert_eq!(Repaired::new(&seed).unwrap().to_bytes(), seed);
            // IHDR's CRC broken, and the signature changed: both mended.
            let mut damaged = seed.clone();
            damaged[29] ^= 0x01;
            damaged[1] = b'X';
            assert_eq!(first_repair(&damaged).unwrap(), image);
            // A byte of the signature lost: IHDR is found where it is.
            let mut damaged = seed.clone();
            damaged.remove(3);
            assert_eq!(first_repair(&damaged).unwrap(), image);
            // Only the Adler-32 wrong: mended.
            let mut repaired = Repaired::new(&seed).unwrap();
            let end = repaired.image_data().len();
            let last = repaired.image_data()[end - 1];
            repaired.write_image_data(end - 1, &[last ^ 1]);
            assert_eq!(first_repair(&repaired.to_bytes()).unwrap(), image);
        }
        assert!(Repaired::new(b"\x89PNG\r\n\x1a\n").is_none());
    }

    #[test]
    fn a_repair_finds_its_place_after_a_byte_is_inserted_or_lost() {
        for seed in png_seeds() {
            let (data, length) = first_idat(&seed);
            if length < 4 {
                continue;
            }
            // A byte lost in the chunk before the image data, if it is
            // not IHDR: the walk finds the next chunk a byte early, where
            // its type is shifted, not damaged.
            let second = 8 + 25;
            if &seed[second + 4..second + 8] != b"IDAT" {
                let mut damaged = seed.clone();
                damaged.remove(second + 8);
                assert_eq!(kinds(&damaged), kinds(&seed));
            }
            let image_data = Repaired::new(&seed).unwrap().image_data().len();
            for (edit, change) in [(0, 1), (1, -1)] {
                let mut damaged = seed.clone();
                if edit == 0 {
                    damaged.insert(data + 2, 0xa5);
                } else {
                    damaged.remove(data + 2);
                }
                // The chunks after the damaged one are found again, and the
                // image data has the byte more or less.
                assert_eq!(kinds(&damaged), kinds(&seed), "edit {edit}");
                let repaired = Repaired::new(&damaged).unwrap();
                assert_eq!(
                    repaired.image_data().len() as isize,
                    image_data as isize + change,
                    "edit {edit}"
                );
            }
        }
    }

    #[test]
    fn a_repair_mends_lengths_that_lose_chunks() {
        for seed in png_seeds() {
            let image = decoded(&seed);
            // IHDR claims 141 bytes, or another type: it is IHDR, with 13
            // bytes, and the chunks after it are kept.
            let mut damaged = seed.clone();
            damaged[11] = 141;
            damaged[14] = b'Q';
            assert_eq!(first_repair(&damaged).unwrap(), image);
            // A byte inserted in IHDR's CRC: the chunks after it are found
            // a byte later, and IHDR keeps its 13 bytes.
            let mut damaged = seed.clone();
            damaged.insert(30, 0x00);
            assert_eq!(first_repair(&damaged).unwrap(), image);
            // A chunk before the image data claims more than the file has:
            // it ends at the next chunk, and the image data is kept.
            let second = 8 + 25;
            let mut damaged = seed.clone();
            damaged[second..second + 4].copy_from_slice(&0x00ff_ffffu32.to_be_bytes());
            assert_eq!(kinds(&damaged), kinds(&seed));
            // Chunks after IEND are dropped.
            let mut damaged = seed.clone();
            damaged.extend_from_slice(&seed[8..33]);
            assert_eq!(first_repair(&damaged).unwrap(), image);
            // IEND claims 4 bytes: it takes what follows it, and is
            // emptied.
            let mut damaged = seed.clone();
            let iend = damaged.len() - 12;
            damaged[iend + 3] = 4;
            assert_eq!(first_repair(&damaged).unwrap(), image);
        }
    }

    #[test]
    fn a_repair_mends_image_data_of_the_wrong_size() {
        for seed in png_seeds() {
            let image = decoded(&seed);
            // Twice as tall: the image data is too short until it is
            // repacked, padded with rows of filter 0. Without interlacing
            // the new rows follow the old ones; with it the passes move,
            // so the old bytes may not be rows any more.
            let mut taller = seed.clone();
            let height = u32::from_be_bytes(taller[20..24].try_into().unwrap());
            taller[20..24].copy_from_slice(&(height * 2).to_be_bytes());
            let tries = repaired_pngs(&taller);
            assert!(
                matches!(
                    tries[0].1,
                    Err(CodecError::Png {
                        kind: png::ErrorKind::TooLittleImageData { .. },
                        ..
                    })
                ),
                "{:?}",
                tries[0].1
            );
            match &tries[1].1 {
                Ok(repacked) => assert_eq!(repacked.height, height * 2),
                Err(CodecError::Png {
                    kind: png::ErrorKind::BadFilter { .. } | png::ErrorKind::BadPaletteIndex { .. },
                    ..
                }) if seed[28] == 1 => {}
                other => panic!("{other:?}"),
            }
            // As many IDAT chunks as before.
            let mut repaired = Repaired::new(&taller).unwrap();
            repaired.fix_adler32();
            let needed = 2 * repaired.inflate().unwrap().len();
            assert!(repaired.repack(needed));
            assert_eq!(
                repaired.idats().count(),
                Repaired::new(&seed).unwrap().idats().count()
            );
            // Bytes after the zlib stream, in the last IDAT chunk: cut.
            let mut repaired = Repaired::new(&seed).unwrap();
            let last = repaired
                .chunks
                .iter_mut()
                .rev()
                .find(|(kind, data)| kind == b"IDAT" && !data.is_empty())
                .unwrap();
            last.1.extend_from_slice(b"junk");
            let tries = repaired_pngs(&repaired.to_bytes());
            assert!(
                matches!(
                    tries[0].1,
                    Err(CodecError::Png {
                        kind: png::ErrorKind::TrailingImageData,
                        ..
                    })
                ),
                "{:?}",
                tries[0].1
            );
            assert_eq!(tries[1].1.as_ref().unwrap(), &image);
        }
    }

    #[test]
    fn the_inflate_seeds_are_streams_the_codec_accepts() {
        // zlib made some, the property tests' encoder others, and one is
        // by hand: a match from 32,768 bytes back, which zlib does not
        // write.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(CORPUS)
            .join("inflate");
        for seed in load_corpus(&dir).unwrap() {
            inflate::zlib(&seed, Limits::default()).unwrap();
        }
    }

    #[test]
    fn the_png_seeds_are_images_the_codec_accepts() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(CORPUS)
            .join("png");
        for seed in load_corpus(&dir).unwrap() {
            png::decode(&seed, Limits::default()).unwrap();
        }
    }

    #[test]
    fn every_real_target_has_seeds_and_accepts_them() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for target in TARGETS {
            let seeds = load_corpus(&workspace.join(CORPUS).join(target.name)).unwrap();
            assert!(seeds.len() >= 3, "{}", target.name);
            for seed in &seeds {
                (target.run)(seed);
            }
        }
    }

    #[test]
    fn the_json_seeds_are_documents_the_parser_accepts() {
        // A seed that is rejected right away makes a poor starting point.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(CORPUS)
            .join("json");
        let seeds = load_corpus(&dir).unwrap();
        let accepted = seeds
            .iter()
            .filter(|seed| {
                std::str::from_utf8(seed)
                    .is_ok_and(|text| json::parse(text, Limits::default()).is_ok())
            })
            .count();
        assert_eq!(accepted, seeds.len());
    }

    #[test]
    fn the_qoi_seeds_are_images_the_codec_writes_back_byte_for_byte() {
        // The seeds were made by a separate port of the reference encoder,
        // so this also checks `qoi::encode` against it.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(CORPUS)
            .join("qoi");
        for seed in load_corpus(&dir).unwrap() {
            let image = qoi::decode(&seed, Limits::default()).unwrap();
            assert_eq!(qoi::encode(&image.header, &image.pixels).unwrap(), seed);
        }
    }

    /// The seeds of one target.
    fn seeds_of(target: &str) -> Vec<Vec<u8>> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(CORPUS)
            .join(target);
        load_corpus(&dir).unwrap()
    }

    #[test]
    fn the_klz_seeds_are_streams_of_their_size() {
        for seed in seeds_of("klz") {
            let (size, stream) = seed.split_first_chunk::<4>().unwrap();
            let size = u32::from_le_bytes(*size) as usize;
            assert_eq!(
                klz::decompress(stream, size, Limits::default())
                    .unwrap()
                    .len(),
                size
            );
        }
    }

    #[test]
    fn the_kbj_seeds_are_files_the_codec_writes_back_byte_for_byte() {
        let seeds = seeds_of("kbj");
        // One holds objects of more than 16 members, whose repeated keys
        // the decoder finds by sorting.
        let large = |seed: &[u8]| {
            fn largest(value: &json::Value) -> usize {
                match value {
                    json::Value::Array(items) => items.iter().map(largest).max().unwrap_or(0),
                    json::Value::Object(members) => members
                        .iter()
                        .map(|(_, value)| largest(value))
                        .max()
                        .unwrap_or(0)
                        .max(members.len()),
                    _ => 0,
                }
            }
            largest(&kbj::decode(seed, Limits::default()).unwrap()) > 16
        };
        assert!(seeds.iter().any(|seed| large(seed)));
        for seed in seeds {
            let value = kbj::decode(&seed, Limits::default()).unwrap();
            assert_eq!(kbj::encode(&value, Limits::default()).unwrap(), seed);
        }
    }

    #[test]
    fn the_bmfont_seeds_are_fonts_the_parser_accepts() {
        let fonts: Vec<bmfont::Font> = seeds_of("bmfont")
            .iter()
            .map(|seed| bmfont::parse(std::str::from_utf8(seed).unwrap(), Limits::default()))
            .collect::<Result<_, _>>()
            .unwrap();
        // Among them, a font of Windows-1252 ids with a fallback glyph:
        // its `char id=145` is U+2018.
        let ansi = fonts
            .iter()
            .find(|font| font.face == "Garamond")
            .expect("the ANSI seed");
        assert!(ansi.fallback.is_some());
        assert!(ansi.glyphs.iter().any(|glyph| glyph.id == 0x2018));
        assert!(ansi.glyphs.iter().all(|glyph| glyph.id != 145));
    }

    #[test]
    fn the_atlas_seeds_are_files_the_codec_accepts() {
        let files: Vec<atlas::AtlasFile> = seeds_of("atlas")
            .iter()
            .map(|seed| atlas::parse(seed, Limits::default()))
            .collect::<Result<_, _>>()
            .unwrap();
        // Among them, clips, and the sandbox's cooked sprites.
        assert!(
            files
                .iter()
                .any(|file| file.sprites.iter().any(|sprite| sprite.clips.len() > 1))
        );
        assert!(files.iter().any(|file| {
            file.sprites
                .iter()
                .any(|sprite| sprite.name == "sprites/person")
        }));
    }

    #[test]
    fn the_ldtk_seeds_are_projects_and_level_files_the_reader_accepts() {
        let project = ldtk_project();
        let mut projects = 0;
        let mut levels = 0;
        for seed in seeds_of("ldtk") {
            let text = std::str::from_utf8(&seed).unwrap();
            match ldtk::parse_project(text, Limits::default()) {
                Ok(_) => projects += 1,
                Err(_) => {
                    ldtk::parse_level(text, project, Limits::default()).unwrap();
                    levels += 1;
                }
            }
        }
        // A project that keeps its level inside it, the one that saves them
        // apart, and both its level files.
        assert_eq!((projects, levels), (2, 2));
    }

    #[test]
    fn the_level_and_prefab_seeds_are_files_the_codec_accepts() {
        let levels: Vec<level::LevelFile> = seeds_of("level")
            .iter()
            .map(|seed| level::parse(seed, Limits::default()))
            .collect::<Result<_, _>>()
            .unwrap();
        // Among them, a grid, tall tiles, overrides and a camera.
        assert!(levels.iter().any(|file| file.grid.is_some()));
        assert!(levels.iter().any(|file| !file.tall.is_empty()));
        assert!(levels.iter().any(|file| file.camera.is_some()));
        assert!(levels.iter().any(|file| {
            file.entities
                .iter()
                .any(|entity| !entity.overrides.is_empty())
        }));
        let prefabs: Vec<prefab::PrefabFile> = seeds_of("prefab")
            .iter()
            .map(|seed| prefab::parse(seed, Limits::default()))
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(prefabs.iter().any(|file| file.extends.is_some()));
        assert!(prefabs.iter().any(|file| file.components.is_empty()));
    }

    #[test]
    fn the_kpak_seeds_are_packs_the_codec_reads() {
        let limits = kpak_limits();
        let packs: Vec<(Vec<u8>, kpak::Toc)> = seeds_of("kpak")
            .into_iter()
            .map(|seed| {
                let toc = kpak::parse(&seed, limits).unwrap();
                (seed, toc)
            })
            .collect();
        // Among them, a pack of no entries, one with no string table, a blob
        // shared, a KLZ entry, and a damaged entry.
        assert!(packs.iter().any(|(_, toc)| toc.is_empty()));
        assert!(packs.iter().any(|(_, toc)| !toc.has_names()));
        assert!(packs.iter().any(|(_, toc)| {
            let entries = toc.entries();
            entries.iter().enumerate().any(|(index, entry)| {
                entry.stored_size > 0
                    && entries[index + 1..]
                        .iter()
                        .any(|other| other.offset == entry.offset)
            })
        }));
        assert!(packs.iter().any(|(_, toc)| {
            toc.entries()
                .iter()
                .any(|entry| entry.codec == kpak::Codec::Klz)
        }));
        assert!(packs.iter().any(|(seed, toc)| {
            (0..toc.len()).any(|index| toc.read(seed, index, limits).is_err())
        }));
    }

    #[test]
    fn the_aseprite_seeds_are_exports_the_reader_accepts() {
        let exports: Vec<aseprite::Export> = seeds_of("aseprite")
            .iter()
            .map(|seed| aseprite::parse(std::str::from_utf8(seed).unwrap(), Limits::default()))
            .collect::<Result<_, _>>()
            .unwrap();
        // Among them, a ping-pong of repeats, and an export with no tags.
        assert!(exports.iter().any(|export| {
            export
                .tags
                .iter()
                .any(|tag| tag.repeat > 0 && tag.direction != aseprite::Direction::Forward)
        }));
        assert!(exports.iter().any(|export| export.tags.is_empty()));
    }

    #[test]
    fn the_wav_seeds_are_files_the_codec_accepts() {
        for seed in seeds_of("wav") {
            wav::decode(&seed, Limits::default()).unwrap();
        }
    }

    #[test]
    fn the_qoa_seeds_are_files_the_codec_accepts() {
        // The seeds come from a separate port of the reference encoder,
        // `crates/codec/tests/qoa/make_vectors.py`.
        for seed in seeds_of("qoa") {
            qoa::decode(&seed, Limits::default()).unwrap();
        }
    }

    #[test]
    fn a_clean_run_reports_each_target_and_passes() {
        let root = root(
            "clean",
            &[
                ("fuzz/corpus/accept/a.json", b"[1]"),
                ("fuzz/corpus/accept/b.json", b"[2]"),
            ],
        );
        assert_eq!(
            fuzz_output(&["--target", "accept", "--secs", "30"], &root, 200),
            (
                true,
                "fuzz-smoke: 1 target, 30 s each, seed 0x7\n\
                 accept: 2 seeds, 200 inputs in ? s (? per second), the slowest ? ms (? bytes)\n\
                 fuzz-smoke: ok"
                    .to_string()
            )
        );
        assert!(!root.join("fuzz/crashes").exists());
    }

    #[test]
    fn a_run_with_no_time_runs_only_the_seeds() {
        // Each target has its own corpus directory.
        let root = root(
            "seeds-only",
            &[
                ("fuzz/corpus/accept/a.json", b"[1]"),
                ("fuzz/corpus/accept/b.json", b"[2]"),
                ("fuzz/corpus/short/only.json", b"{}"),
            ],
        );
        assert_eq!(
            fuzz_output(&["--secs", "0"], &root, u64::MAX),
            (
                true,
                "fuzz-smoke: 2 targets, 0 s each, seed 0x7\n\
                 accept: 2 seeds, 2 inputs in ? s (? per second), the slowest ? ms (? bytes)\n\
                 short: 1 seed, 1 input in ? s (? per second), the slowest ? ms (? bytes)\n\
                 fuzz-smoke: ok"
                    .to_string()
            )
        );
    }

    #[test]
    fn a_finding_fails_the_run_and_names_the_saved_input() {
        let root = root("finding", &[("fuzz/corpus/short/only.json", b"[1, 2, 3]")]);
        let (passed, output) = fuzz_output(&["--target", "short"], &root, 3000);
        assert!(!passed);

        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 6, "{output}");
        assert_eq!(lines[0], "fuzz-smoke: 1 target, 60 s each, seed 0x7");
        assert_eq!(
            lines[1],
            "short: 1 seed, 3000 inputs in ? s (? per second), the slowest ? ms (? bytes)"
        );
        // The panic site is the source path as the compiler was given it,
        // which has `\` between components on Windows.
        let finding = lines[2].replace('\\', "/");
        assert!(
            finding.starts_with("finding: short: panic at xtask/src/fuzz.rs:")
                && finding.ends_with(": more than 40 bytes"),
            "{output}"
        );
        // The path is relative to the root, and the file is there.
        let (path, rest) = lines[3]
            .strip_prefix("         input saved to ")
            .and_then(|rest| rest.split_once(" ("))
            .unwrap();
        assert!(
            path.replace('\\', "/")
                .starts_with("fuzz/crashes/short/panic-")
                && path.ends_with(".bin"),
            "{output}"
        );
        let saved = fs::read(root.join(path)).unwrap();
        assert!(saved.len() > 40);
        assert!(
            rest.starts_with(&format!("{} bytes), which is 1 of ", saved.len()))
                && rest.ends_with(" inputs that did this"),
            "{output}"
        );
        assert_eq!(
            lines[4],
            "To see a finding again, copy its input into fuzz/corpus/<target>/: seeds run \
             first, unchanged."
        );
        assert_eq!(lines[5], "fuzz-smoke: FAILED (1 finding)");
    }

    #[test]
    fn a_saved_input_in_the_corpus_fails_at_once() {
        let long = [b'7'; 41];
        let root = root(
            "regression",
            &[
                ("fuzz/corpus/short/a.json", b"[1]"),
                ("fuzz/corpus/short/panic-0123.bin", &long),
            ],
        );
        let (passed, output) = fuzz_output(&["--target", "short", "--secs", "0"], &root, u64::MAX);
        assert!(!passed);
        assert!(output.contains("short: 2 seeds, 2 inputs in"), "{output}");
        assert!(
            output.contains("(41 bytes), which is 1 of 1 input that did this"),
            "{output}"
        );
    }

    #[test]
    fn findings_are_described_by_kind() {
        let root = Path::new("/work/nadir");
        let finding = |kind, site: &str, message: &str, hits| Finding {
            kind,
            site: site.to_string(),
            message: message.to_string(),
            path: root.join("fuzz/crashes/json/x.bin"),
            len: 1,
            hits,
        };
        let outcome = Outcome {
            inputs: 1500,
            elapsed: Duration::from_millis(2500),
            findings: vec![
                finding(Kind::Panic, "a.rs:1:2", "index out of bounds", 7),
                finding(Kind::Panic, "no place known", "no place known", 1),
                finding(Kind::Hang, "", "did not finish within 2 s", 1),
            ],
            hung: true,
            slowest: Some((Duration::from_micros(12_340), 512)),
        };
        let path = Path::new("fuzz/crashes/json/x.bin").display().to_string();
        assert_eq!(
            describe("json", 9, &outcome, root),
            format!(
                "json: 9 seeds, 1500 inputs in 2.5 s (600 per second), the slowest 12.3 ms \
                 (512 bytes)\n\
                 finding: json: panic at a.rs:1:2: index out of bounds\n         \
                 input saved to {path} (1 byte), which is 1 of 7 inputs that did this\n\
                 finding: json: panic: no place known\n         \
                 input saved to {path} (1 byte), which is 1 of 1 input that did this\n\
                 finding: json: hang: did not finish within 2 s\n         \
                 input saved to {path} (1 byte), which is 1 of 1 input that did this\n\
                 json: stopped early, because the target hung\n"
            )
        );
    }

    #[test]
    fn a_run_that_took_no_time_has_no_rate() {
        let outcome = Outcome {
            inputs: 3,
            elapsed: Duration::ZERO,
            findings: Vec::new(),
            hung: false,
            slowest: None,
        };
        assert_eq!(
            describe("json", 3, &outcome, Path::new(".")),
            "json: 3 seeds, 3 inputs in 0.0 s (0 per second)\n"
        );
    }

    #[test]
    fn the_corpus_is_read_in_name_order_without_hidden_files() {
        let root = root(
            "corpus-order",
            &[
                ("fuzz/corpus/accept/b.json", b"second"),
                ("fuzz/corpus/accept/a.json", b"first"),
                ("fuzz/corpus/accept/c", b"\xff\x00third"),
                ("fuzz/corpus/accept/.gitkeep", b""),
                ("fuzz/corpus/accept/nested/d.json", b"not a seed"),
            ],
        );
        assert_eq!(
            load_corpus(&root.join("fuzz/corpus/accept")).unwrap(),
            [&b"first"[..], b"second", b"\xff\x00third"]
        );
    }

    #[test]
    fn a_missing_or_empty_corpus_is_an_error() {
        let root = root("corpus-empty", &[("fuzz/corpus/accept/.gitkeep", b"")]);
        let empty = root.join("fuzz/corpus/accept");
        let error = load_corpus(&empty).unwrap_err();
        assert!(
            matches!(&error, Error::NoSeeds { path } if *path == empty),
            "{error:?}"
        );
        let missing = root.join("fuzz/corpus/short");
        let error = load_corpus(&missing).unwrap_err();
        assert!(
            matches!(&error, Error::Read { path, .. } if *path == missing),
            "{error:?}"
        );
        // And so the whole command fails, before any target has run.
        let options = options(&[], None).unwrap();
        let mut out = Vec::new();
        let error = fuzz(&options, &root, inputs(1), &mut out).unwrap_err();
        assert!(matches!(error, Error::NoSeeds { .. }), "{error:?}");
    }

    #[test]
    fn each_target_draws_from_its_own_random_stream() {
        use std::sync::Mutex;
        static FIRST: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
        static SECOND: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
        fn first(input: &[u8]) {
            FIRST.lock().unwrap().push(input.to_vec());
        }
        fn second(input: &[u8]) {
            SECOND.lock().unwrap().push(input.to_vec());
        }
        let targets = [
            Target {
                name: "first",
                run: first,
            },
            Target {
                name: "second",
                run: second,
            },
        ];
        let root = root(
            "streams",
            &[
                ("fuzz/corpus/first/seed", b"[1, 2, 3]"),
                ("fuzz/corpus/second/seed", b"[1, 2, 3]"),
            ],
        );
        let options = Options::parse(Args::new(Vec::<String>::new()), &targets, None).unwrap();
        let mut out = Vec::new();
        assert!(fuzz(&options, &root, inputs(100), &mut out).unwrap());
        // The same seeds and the same seed number, yet different inputs.
        assert_eq!(FIRST.lock().unwrap().len(), 100);
        assert_ne!(*FIRST.lock().unwrap(), *SECOND.lock().unwrap());
    }

    #[test]
    fn the_seed_decides_the_inputs() {
        use std::sync::Mutex;
        static SEEN: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
        fn record(input: &[u8]) {
            SEEN.lock().unwrap().push(input.to_vec());
        }
        let targets = [Target {
            name: "record",
            run: record,
        }];
        let root = root("seed", &[("fuzz/corpus/record/seed", b"[1, 2, 3]")]);
        for seed in ["1", "2", "1"] {
            let args = Args::new(Vec::<String>::new());
            let options = Options::parse(args, &targets, Some(seed)).unwrap();
            let mut out = Vec::new();
            assert!(fuzz(&options, &root, inputs(60), &mut out).unwrap());
        }
        let seen = SEEN.lock().unwrap();
        assert_eq!(seen.len(), 180);
        let (first, second, third) = (&seen[..60], &seen[60..120], &seen[120..]);
        assert_eq!(first, third);
        assert_ne!(first, second);
    }

    #[test]
    fn a_hang_ends_the_run_before_the_remaining_targets() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static RELEASE: AtomicBool = AtomicBool::new(false);
        fn stuck(_: &[u8]) {
            while !RELEASE.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        fn after(_: &[u8]) {
            panic!("the target after the one that hung was run");
        }
        let targets = [
            Target {
                name: "stuck",
                run: stuck,
            },
            Target {
                name: "after",
                run: after,
            },
        ];
        let root = root(
            "hang",
            &[
                ("fuzz/corpus/stuck/seed", b"[1]"),
                ("fuzz/corpus/after/seed", b"[1]"),
            ],
        );
        let options = Options::parse(Args::new(Vec::<String>::new()), &targets, None).unwrap();
        let budget = Budget {
            max_inputs: 10,
            hang_limit: Duration::from_millis(40),
        };
        let mut out = Vec::new();
        let passed = fuzz(&options, &root, budget, &mut out);
        // Let the stuck thread go, whatever the assertions below say.
        RELEASE.store(true, Ordering::Relaxed);

        assert!(!passed.unwrap());
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 7, "{text}");
        assert!(
            lines[1].starts_with("stuck: 1 seed, 0 inputs in "),
            "{text}"
        );
        assert_eq!(
            lines[2],
            "finding: stuck: hang: did not finish within 0.04 s"
        );
        assert!(
            lines[3].ends_with("(3 bytes), which is 1 of 1 input that did this"),
            "{text}"
        );
        assert_eq!(lines[4], "stuck: stopped early, because the target hung");
        assert_eq!(lines[6], "fuzz-smoke: FAILED (1 finding)");
        assert!(!text.contains("after:"), "{text}");
        assert!(!root.join("fuzz/crashes/after").exists());
    }

    #[test]
    fn counts_are_spelled_out_in_the_singular_and_plural() {
        assert_eq!(plural(0_u64, "seed"), "0 seeds");
        assert_eq!(plural(1_u64, "seed"), "1 seed");
        assert_eq!(plural(2_usize, "seed"), "2 seeds");
    }
}
