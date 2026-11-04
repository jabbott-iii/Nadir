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

//! LDtk projects, made by hand with the members LDtk 1.5.3 saves that the
//! reader takes: what their levels cook to, when they cook again, and what
//! the cook refuses, by ADR-0038's conventions.

use std::fs;
use std::path::Path;

use nadir_codec::json::{self, Value};
use nadir_codec::{CodecError, Limits, level};
use nadir_cook::{CookError, Counts, Report};

use crate::common::{age, cook, dir, files, jobs, kbj, plain, write, written};

/// The level `Meadow`, 4 × 3 cells of 16 pixels, its layers top-most
/// first, each type of LDtk's: entities (a tree with fields, and a big
/// rock), offset; a tall wall, offset; the `Collision` IntGrid with a tile
/// by rules; the ground, offset, with flipped and animated tiles; and an
/// auto-layer's tile.
const MEADOW: &str = r##"{
  "identifier": "Meadow", "iid": "meadow-iid", "uid": 0, "worldX": 0, "worldY": 0,
  "pxWid": 64, "pxHei": 48, "__bgColor": "#102030", "externalRelPath": null,
  "fieldInstances": [],
  "layerInstances": [
    { "__identifier": "Entities", "__type": "Entities", "layerDefUid": 10,
      "__cWid": 4, "__cHei": 3, "__gridSize": 16, "__opacity": 1,
      "__pxTotalOffsetX": 2, "__pxTotalOffsetY": 2, "__tilesetDefUid": null, "visible": true,
      "intGridCsv": [], "gridTiles": [], "autoLayerTiles": [],
      "entityInstances": [
        { "__identifier": "Tree", "iid": "tree-iid", "defUid": 20, "px": [24, 16], "__grid": [1, 0],
          "__pivot": [0.5, 1], "width": 16, "height": 32,
          "fieldInstances": [
            { "__identifier": "Sprite__flip__x", "__type": "Bool", "__value": true },
            { "__identifier": "Health__max", "__type": "Int", "__value": 12 },
            { "__identifier": "Health__current", "__type": "Int", "__value": null },
            { "__identifier": "Name__text", "__type": "String", "__value": "Oak" } ] },
        { "__identifier": "BigRock", "iid": "rock-iid", "defUid": 21, "px": [40, 32], "__grid": [2, 1],
          "__pivot": [0.5, 1], "width": 16, "height": 16, "fieldInstances": [] } ] },
    { "__identifier": "Walls", "__type": "Tiles", "layerDefUid": 11,
      "__cWid": 4, "__cHei": 3, "__gridSize": 16, "__opacity": 1,
      "__pxTotalOffsetX": 8, "__pxTotalOffsetY": 4, "__tilesetDefUid": 1, "visible": true,
      "intGridCsv": [], "autoLayerTiles": [], "entityInstances": [],
      "gridTiles": [ { "t": 1, "f": 0, "px": [48, 32], "src": [16, 0], "a": 1 } ] },
    { "__identifier": "Collision", "__type": "IntGrid", "layerDefUid": 12,
      "__cWid": 4, "__cHei": 3, "__gridSize": 16, "__opacity": 1,
      "__pxTotalOffsetX": 0, "__pxTotalOffsetY": 0, "__tilesetDefUid": 1, "visible": true,
      "intGridCsv": [1, 1, 1, 1, 1, 2, 0, 1, 1, 1, 1, 1], "gridTiles": [], "entityInstances": [],
      "autoLayerTiles": [ { "t": 4, "f": 0, "px": [16, 16], "src": [0, 16], "a": 1 } ] },
    { "__identifier": "Ground", "__type": "Tiles", "layerDefUid": 13,
      "__cWid": 4, "__cHei": 3, "__gridSize": 16, "__opacity": 1,
      "__pxTotalOffsetX": 8, "__pxTotalOffsetY": 4, "__tilesetDefUid": 1, "visible": true,
      "intGridCsv": [], "autoLayerTiles": [], "entityInstances": [],
      "gridTiles": [
        { "t": 0, "f": 0, "px": [0, 0], "src": [0, 0], "a": 1 },
        { "t": 2, "f": 1, "px": [16, 0], "src": [32, 0], "a": 1 },
        { "t": 3, "f": 3, "px": [0, 16], "src": [48, 0], "a": 1 },
        { "t": 7, "f": 2, "px": [32, 32], "src": [48, 16], "a": 1 } ] },
    { "__identifier": "Paths", "__type": "AutoLayer", "layerDefUid": 14,
      "__cWid": 4, "__cHei": 3, "__gridSize": 16, "__opacity": 1,
      "__pxTotalOffsetX": 0, "__pxTotalOffsetY": 0, "__tilesetDefUid": 1, "visible": true,
      "intGridCsv": [], "gridTiles": [], "entityInstances": [],
      "autoLayerTiles": [ { "t": 6, "f": 0, "px": [48, 0], "src": [32, 16], "a": 1 } ] } ] }"##;

/// A project of the tile set `Ground`, `tiles/ground.png` (4 × 2 tiles of
/// 16, tile 1 the top of a tall block of 1 × 2, tiles 2 and 3 an
/// animation), LDtk's own icons, five layers and two entities, with
/// `levels` as its levels.
fn project_text(levels: &str, external: bool) -> String {
    format!(
        r#"{{
  "jsonVersion": "1.5.3", "externalLevels": {external}, "backupOnSave": false,
  "simplifiedExport": false, "imageExportMode": "None", "exportTiled": false, "worlds": [],
  "defs": {{
    "tilesets": [
      {{ "uid": 1, "identifier": "Ground", "relPath": "../tiles/ground.png", "embedAtlas": null,
         "pxWid": 64, "pxHei": 32, "tileGridSize": 16, "spacing": 0, "padding": 0,
         "__cWid": 4, "__cHei": 2,
         "customData": [
           {{ "tileId": 1, "data": "{{\"tall\": [1, 2]}}" }},
           {{ "tileId": 2, "data": "{{\"frames\": 2, \"ms\": 300}}" }} ] }},
      {{ "uid": 2, "identifier": "Internal_Icons", "relPath": null, "embedAtlas": "LdtkIcons",
         "pxWid": 512, "pxHei": 512, "tileGridSize": 16, "spacing": 0, "padding": 0,
         "__cWid": 32, "__cHei": 32, "customData": [] }} ],
    "layers": [
      {{ "uid": 10, "identifier": "Entities", "type": "Entities", "gridSize": 16,
         "tilesetDefUid": null, "intGridValues": [] }},
      {{ "uid": 11, "identifier": "Walls", "type": "Tiles", "gridSize": 16,
         "tilesetDefUid": 1, "intGridValues": [] }},
      {{ "uid": 12, "identifier": "Collision", "type": "IntGrid", "gridSize": 16, "tilesetDefUid": 1,
         "intGridValues": [ {{ "value": 1, "identifier": "stone" }}, {{ "value": 2, "identifier": "water" }} ] }},
      {{ "uid": 13, "identifier": "Ground", "type": "Tiles", "gridSize": 16,
         "tilesetDefUid": 1, "intGridValues": [] }},
      {{ "uid": 14, "identifier": "Paths", "type": "AutoLayer", "gridSize": 16,
         "tilesetDefUid": 1, "intGridValues": [] }} ],
    "entities": [
      {{ "uid": 20, "identifier": "Tree", "width": 16, "height": 32, "pivotX": 0.5, "pivotY": 1 }},
      {{ "uid": 21, "identifier": "BigRock", "width": 16, "height": 16, "pivotX": 0.5, "pivotY": 1 }} ] }},
  "levels": [ {levels} ] }}"#
    )
}

/// What `MEADOW` cooks to: the auto-layer, the ground, then the
/// `Collision` layer's tile by rules, drawn bottom up; the grid; the tall
/// wall, with its layer's offset; the entities, as prefabs, with their
/// layer's offset and their fields as overrides in name order.
const COOKED: &str = r#"{
  "version": 1, "size": [64, 48], "background": [16, 32, 48], "assets": ["atlas/tiles.kbj"],
  "tilesets": [ { "atlas": "atlas/tiles.kbj", "sprite": "tiles/ground", "columns": 4, "rows": 2,
                  "animations": [ { "tile": 2, "frames": 2, "ms": 300 } ] } ],
  "layers": [
    { "tileset": 0, "size": [4, 3], "cell": 16, "origin": [0, 0], "layer": 0, "depth": 0,
      "cells": [0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 0] },
    { "tileset": 0, "size": [4, 3], "cell": 16, "origin": [8, 4], "layer": 0, "depth": 1,
      "cells": [1, 65539, 0, 0, 196612, 0, 0, 0, 0, 0, 131080, 0] },
    { "tileset": 0, "size": [4, 3], "cell": 16, "origin": [0, 0], "layer": 0, "depth": 2,
      "cells": [0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0] } ],
  "grid": { "size": [4, 3], "cell": 16, "origin": [0, 0], "layer": "World",
            "solid": [1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1] },
  "tall": [ { "tileset": 0, "tile": 1, "size": [1, 2], "feet": [64, 52], "layer": 1 } ],
  "entities": [
    { "prefab": "tree", "position": [26, 18],
      "overrides": { "Health": { "max": 12 }, "Name": { "text": "Oak" },
                     "Sprite": { "flip": { "x": true } } } },
    { "prefab": "big_rock", "position": [42, 34], "overrides": {} } ] }"#;

/// JSON text of `value`.
fn text(value: &Value) -> String {
    json::to_string_compact(value)
}

fn parse(text: &str) -> Value {
    json::parse(text, Limits::default()).expect("the tests' JSON")
}

/// The value at `path` in `value`, such as `levels[0].layerInstances[3]`;
/// a member not there is added, `null`, to be set.
fn at<'v>(mut value: &'v mut Value, path: &str) -> &'v mut Value {
    for part in path.split('.') {
        let (name, indices) = part.split_at(part.find('[').unwrap_or(part.len()));
        if !name.is_empty() {
            let Value::Object(members) = value else {
                panic!("`{name}` of {path} is not in an object");
            };
            let index = match members.iter().position(|(key, _)| key == name) {
                Some(index) => index,
                None => {
                    members.push((name.to_string(), Value::Null));
                    members.len() - 1
                }
            };
            value = &mut members[index].1;
        }
        for index in indices.split(['[', ']']).filter(|index| !index.is_empty()) {
            let index: usize = index.parse().expect("an index");
            let Value::Array(items) = value else {
                panic!("[{index}] of {path} is not in a list");
            };
            value = &mut items[index];
        }
    }
    value
}

/// The project at `levels/meadow.ldtk`, with `edit` made to it, and the
/// rest: its tile set's image and sidecar, and the prefabs `tree` and
/// `big_rock`.
fn sources(root: &Path, edit: impl FnOnce(&mut Value)) {
    let mut project = parse(&project_text(MEADOW, false));
    edit(&mut project);
    write(root, "levels/meadow.ldtk", text(&project));
    write(root, "tiles/ground.png", plain(64, 32, [40, 120, 40, 255]));
    write(
        root,
        "tiles/ground.sprite.jsonc",
        r#"{ "tiles": [16, 16] }"#,
    );
    write(
        root,
        "data/prefabs/tree.jsonc",
        r#"{ "name": "tree", "components": {} }"#,
    );
    write(
        root,
        "data/prefabs/big_rock.jsonc",
        r#"{ "name": "big_rock", "components": {} }"#,
    );
}

/// The same project, saving its level apart: `levels/meadow.ldtk`, and the
/// level in `levels/meadow/0000-Meadow.ldtkl`, with `edit` made to it.
fn sources_apart(root: &Path, edit: impl FnOnce(&mut Value)) {
    sources(root, |_| {});
    let mut project = parse(&project_text(MEADOW, true));
    *at(&mut project, "levels[0].layerInstances") = Value::Null;
    *at(&mut project, "levels[0].externalRelPath") =
        Value::String("meadow/0000-Meadow.ldtkl".to_string());
    write(root, "levels/meadow.ldtk", text(&project));
    let mut level = parse(MEADOW);
    edit(&mut level);
    write(root, "levels/meadow/0000-Meadow.ldtkl", text(&level));
}

/// The jobs of [`sources`], each cooked or not.
fn all(cooked: [bool; 4]) -> Vec<(String, bool)> {
    [
        "atlas/tiles",
        "data/prefabs/big_rock",
        "data/prefabs/tree",
        "ldtk/levels/meadow",
    ]
    .into_iter()
    .zip(cooked)
    .map(|(name, cooked)| (name.to_string(), cooked))
    .collect()
}

#[test]
fn a_project_cooks_each_level_to_a_level_file_the_game_reads() {
    let root = dir("ldtk_level");
    let (source, out) = (root.join("source"), root.join("out"));
    sources(&source, |_| {});
    let report = cook(&source, &out).expect("a cook");
    assert_eq!(jobs(&report), all([true; 4]));
    assert_eq!(
        report.jobs[3].counts,
        Some(Counts::Levels {
            levels: 1,
            entities: 2
        })
    );
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // Its members in their order, as the level file has them.
    assert_eq!(kbj(&out, "levels/meadow.klvl"), parse(COOKED));
    let bytes = fs::read(out.join("levels/meadow.klvl")).expect("the level file");
    let file = level::parse(&bytes, Limits::default()).expect("a level the game reads");
    assert_eq!(file.entities[1].prefab, "big_rock");
    // The tile set's atlas is what the atlas job wrote.
    assert!(out.join("atlas/tiles.kbj").exists());
}

#[test]
fn a_hidden_layer_is_not_drawn() {
    let root = dir("ldtk_hidden");
    let (source, out) = (root.join("source"), root.join("out"));
    sources(
        &source,
        set("levels[0].layerInstances[1].visible", Value::Bool(false)),
    );
    cook(&source, &out).expect("a cook");
    let cooked = kbj(&out, "levels/meadow.klvl");
    let expected = parse(COOKED);
    assert_eq!(cooked.get("tall"), Some(&parse("[]")));
    assert_eq!(cooked.get("layers"), expected.get("layers"));
    assert_eq!(cooked.get("entities"), expected.get("entities"));
}

#[test]
fn a_level_of_too_many_cells_is_refused_before_they_are_made() {
    // Nine layers of a million cells, each with one tile: the reader takes
    // each, and the cook refuses the ninth from the bottom.
    let root = dir("ldtk_cells");
    let source = root.join("source");
    sources(&source, |project| {
        let mut big = parse(MEADOW);
        let layer = at(&mut big, "layerInstances[3]");
        *at(layer, "__cWid") = Value::Int(1000);
        *at(layer, "__cHei") = Value::Int(1000);
        let layer = layer.clone();
        *at(project, "levels[0].layerInstances") = Value::Array(vec![layer; 9]);
    });
    assert_eq!(
        cook(&source, &root.join("out")),
        Err(CookError::Config {
            path: "levels/meadow.ldtk".to_string(),
            reason: "at `levels[0].layerInstances[0].__cWid`: the level's tile layers and grid hold more than 8388608 cells together".to_string(),
        })
    );
}

#[test]
fn a_level_saved_apart_cooks_as_the_project_s_own_does() {
    let root = dir("ldtk_apart");
    let (inside, apart) = (root.join("inside"), root.join("apart"));
    sources(&inside, |_| {});
    sources_apart(&apart, |_| {});
    cook(&inside, &root.join("out_inside")).expect("a cook");
    let report = cook(&apart, &root.join("out_apart")).expect("a cook");
    assert_eq!(jobs(&report), all([true; 4]));
    assert_eq!(
        fs::read(root.join("out_apart/levels/meadow.klvl")).expect("the level"),
        fs::read(root.join("out_inside/levels/meadow.klvl")).expect("the level")
    );
}

#[test]
fn two_cooks_are_the_same_byte_for_byte_and_a_second_changes_nothing() {
    let root = dir("ldtk_twice");
    let source = root.join("source");
    sources_apart(&source, |_| {});
    let (first, second) = (root.join("first"), root.join("second"));
    cook(&source, &first).expect("a cook");
    cook(&source, &second).expect("a cook");
    assert_eq!(files(&first), files(&second));
    let before = files(&first);
    assert_eq!(
        jobs(&cook(&source, &first).expect("a cook")),
        all([false; 4])
    );
    assert_eq!(files(&first), before);
}

#[test]
fn projects_are_jobs_in_name_order_and_their_names_make_jobs() {
    let root = dir("ldtk_names");
    let (source, out) = (root.join("source"), root.join("out"));
    sources(&source, |_| {});
    // Found before `meadow.ldtk`, as `Z` sorts before `m`.
    let mut zoo = parse(&project_text(MEADOW, false));
    *at(&mut zoo, "levels[0].identifier") = Value::String("Zoo".to_string());
    write(&source, "levels/Zoo.ldtk", text(&zoo));
    let report = cook(&source, &out).expect("a cook");
    let names: Vec<&str> = report.jobs.iter().map(|job| job.name.as_str()).collect();
    assert_eq!(&names[3..], ["ldtk/levels/meadow", "ldtk/levels/zoo"]);
    assert!(out.join("levels/zoo.klvl").exists());
    // A name that would make a job's name the cache cannot keep.
    fs::rename(
        source.join("levels/Zoo.ldtk"),
        source.join("levels/zoo..ldtk"),
    )
    .expect("rename");
    assert_eq!(
        cook(&source, &out),
        Err(CookError::BadPath {
            path: "levels/zoo..ldtk".to_string(),
            reason: "its name without `.ldtk` ends in a dot or a space",
        })
    );
}

/// Cooks `source` into `out`, then a clean cook of it, and checks that
/// both wrote the same files.
fn cooks_as_a_clean_cook(source: &Path, out: &Path, clean: &Path) -> Report {
    let report = cook(source, out).expect("a cook");
    cook(source, clean).expect("a clean cook");
    assert_eq!(files(out), files(clean));
    report
}

#[test]
fn an_edit_to_a_level_file_or_a_tile_set_cooks_the_level_again_as_a_clean_cook_does() {
    let root = dir("ldtk_edit");
    let (source, out) = (root.join("source"), root.join("out"));
    sources_apart(&source, |_| {});
    cook(&source, &out).expect("a cook");
    let before = fs::read(out.join("levels/meadow.klvl")).expect("the level");

    // A tile of the level file: only the project's job.
    sources_apart(&source, |level| {
        *at(level, "layerInstances[3].gridTiles[0].t") = Value::Int(6);
    });
    let report = cooks_as_a_clean_cook(&source, &out, &root.join("clean_level"));
    assert_eq!(jobs(&report), all([false, false, false, true]));
    assert_ne!(
        fs::read(out.join("levels/meadow.klvl")).expect("the level"),
        before
    );

    // The tile set's sidecar, its pixels, and its folder's settings: the
    // atlas group's job and the project's.
    write(
        &source,
        "tiles/ground.sprite.jsonc",
        "// The ground.\n{ \"tiles\": [16, 16] }",
    );
    let report = cooks_as_a_clean_cook(&source, &out, &root.join("clean_sidecar"));
    assert_eq!(jobs(&report), all([true, false, false, true]));
    write(&source, "tiles/ground.png", plain(64, 32, [1, 2, 3, 255]));
    let report = cooks_as_a_clean_cook(&source, &out, &root.join("clean_png"));
    assert_eq!(jobs(&report), all([true, false, false, true]));
    write(&source, "tiles/atlas.jsonc", r#"{ "group": "world" }"#);
    let report = cooks_as_a_clean_cook(&source, &out, &root.join("clean_group"));
    assert_eq!(report.jobs[0].name, "atlas/world");
    let cooked = kbj(&out, "levels/meadow.klvl");
    assert_eq!(cooked.get("assets"), Some(&parse(r#"["atlas/world.kbj"]"#)));

    // A prefab that goes: the level's job runs again, and says which.
    fs::remove_file(source.join("data/prefabs/big_rock.jsonc")).expect("remove the prefab");
    let error = cook(&source, &out).expect_err("a missing prefab");
    assert_eq!(
        error,
        CookError::Config {
            path: "levels/meadow/0000-Meadow.ldtkl".to_string(),
            reason: "at `layerInstances[0].entityInstances[1].__identifier`: the entity `BigRock` spawns the prefab `big_rock`, and there is no `data/prefabs/big_rock.jsonc`".to_string(),
        }
    );
}

#[test]
fn a_prefab_edited_to_the_same_value_leaves_its_file_and_the_level_as_they_were() {
    let root = dir("ldtk_same");
    let (source, out) = (root.join("source"), root.join("out"));
    sources(&source, |_| {});
    cook(&source, &out).expect("a cook");
    let long_ago = age(&out);
    // A comment: the prefab's job and the level's run again, and each
    // writes the bytes already there, so that a game watching the files
    // reloads neither (ADR-0042).
    write(
        &source,
        "data/prefabs/tree.jsonc",
        "// A tree.\n{ \"name\": \"tree\", \"components\": {} }",
    );
    let report = cooks_as_a_clean_cook(&source, &out, &root.join("clean"));
    assert_eq!(jobs(&report), all([false, false, true, true]));
    assert_eq!(written(&out, long_ago), [".cache.kbj"]);
}

/// What the cook says of the project of [`sources`] with `edit` made to it:
/// the file it names and why.
fn refused(edit: impl FnOnce(&mut Value)) -> (String, String) {
    let root = dir("ldtk_refused");
    let source = root.join("source");
    sources(&source, edit);
    match cook(&source, &root.join("out")) {
        Err(CookError::Config { path, reason }) => (path, reason),
        other => panic!("not refused as it should be: {other:?}"),
    }
}

/// Checks that `edit` is refused at `member` of the project, saying `why`.
fn refused_at(member: &str, why: &str, edit: impl FnOnce(&mut Value)) {
    let (path, reason) = refused(edit);
    assert_eq!(path, "levels/meadow.ldtk", "{member}");
    let expected = format!("at `{member}`: ");
    assert!(
        reason.starts_with(&expected) && reason.contains(why),
        "{member}: {reason}"
    );
}

fn set(path: &'static str, value: Value) -> impl FnOnce(&mut Value) {
    move |project| *at(project, path) = value
}

#[test]
fn each_mistake_adr_0038_names_is_refused_with_its_file_and_member() {
    const LEVEL: &str = "levels[0].layerInstances";
    let half = Value::Float(0.5);
    // Opacity, of a layer the cook draws and of a tile.
    refused_at(
        &format!("{LEVEL}[3].__opacity"),
        "a layer's opacity must be 1",
        set("levels[0].layerInstances[3].__opacity", half.clone()),
    );
    refused_at(
        &format!("{LEVEL}[3].gridTiles[0].a"),
        "a tile's opacity must be 1",
        set("levels[0].layerInstances[3].gridTiles[0].a", half),
    );
    // Two tiles in one cell.
    refused_at(
        &format!("{LEVEL}[3].gridTiles[4]"),
        "two tiles in the cell (0, 0)",
        |project| {
            let Value::Array(tiles) = at(project, "levels[0].layerInstances[3].gridTiles") else {
                panic!("tiles");
            };
            tiles.push(parse(
                r#"{ "t": 5, "f": 0, "px": [0, 0], "src": [16, 16], "a": 1 }"#,
            ));
        },
    );
    // A tile set with spacing or padding.
    for member in ["spacing", "padding"] {
        refused_at(
            &format!("defs.tilesets[0].{member}"),
            &format!("has {member}"),
            |project| *at(project, &format!("defs.tilesets[0].{member}")) = Value::Int(1),
        );
    }
    // An offset on the `Collision` layer.
    refused_at(
        &format!("{LEVEL}[2].__pxTotalOffsetX"),
        "the `Collision` layer has no offset",
        set(
            "levels[0].layerInstances[2].__pxTotalOffsetX",
            Value::Int(16),
        ),
    );
    refused_at(
        &format!("{LEVEL}[2].__pxTotalOffsetY"),
        "the `Collision` layer has no offset",
        set(
            "levels[0].layerInstances[2].__pxTotalOffsetY",
            Value::Int(16),
        ),
    );
    // The side outputs.
    for (member, on) in [
        ("backupOnSave", Value::Bool(true)),
        ("simplifiedExport", Value::Bool(true)),
        (
            "imageExportMode",
            Value::String("OneImagePerLevel".to_string()),
        ),
        ("exportTiled", Value::Bool(true)),
    ] {
        refused_at(
            member,
            "which the cook would find among the sources",
            |project| {
                *at(project, member) = on;
            },
        );
    }
    // The fields version 1 cannot write, and one that overrides nothing.
    const FIELD: &str = "levels[0].layerInstances[0].entityInstances[0].fieldInstances[1]";
    for kind in ["Point", "Color", "Tile", "EntityRef", "Array<Int>"] {
        refused_at(
            &format!("{FIELD}.__type"),
            &format!("a field of type `{kind}` cannot override a component in version 1"),
            |project| *at(project, &format!("{FIELD}.__type")) = Value::String(kind.to_string()),
        );
    }
    refused_at(
        &format!("{FIELD}.__identifier"),
        "the field `Speed` overrides nothing",
        |project| *at(project, &format!("{FIELD}.__identifier")) = Value::String("Speed".into()),
    );
    // Even with no value, before an instance sets one.
    refused_at(
        &format!("{FIELD}.__type"),
        "a field of type `Point` cannot override a component in version 1",
        |project| {
            *at(project, &format!("{FIELD}.__type")) = Value::String("Point".to_string());
            *at(project, &format!("{FIELD}.__value")) = Value::Null;
        },
    );
    // Two fields where one overrides the other's.
    refused_at(
        "levels[0].layerInstances[0].entityInstances[0].fieldInstances[4].__identifier",
        "the field `Health__max__x` overrides what `Health__max` does",
        |project| {
            let path = "levels[0].layerInstances[0].entityInstances[0].fieldInstances";
            let Value::Array(fields) = at(project, path) else {
                panic!("fields");
            };
            fields.push(parse(
                r#"{ "__identifier": "Health__max__x", "__type": "Int", "__value": 1 }"#,
            ));
        },
    );
    // Two levels of one name.
    refused_at(
        "levels[1].identifier",
        "the level `MEADOW` would be `levels/meadow.klvl`, as the level `Meadow` of `levels/meadow.ldtk` would",
        |project| {
            let mut second = parse(MEADOW);
            *at(&mut second, "identifier") = Value::String("MEADOW".to_string());
            *at(&mut second, "iid") = Value::String("other-iid".to_string());
            let Value::Array(levels) = at(project, "levels") else {
                panic!("levels");
            };
            levels.push(second);
        },
    );
    // A level whose name cannot name a file.
    refused_at(
        "levels[0].identifier",
        "the level `Été` would be `levels/Été.klvl`",
        set("levels[0].identifier", Value::String("Été".to_string())),
    );
    // An entity whose prefab is not there.
    refused_at(
        &format!("{LEVEL}[0].entityInstances[0].__identifier"),
        "the entity `Bush` spawns the prefab `bush`, and there is no `data/prefabs/bush.jsonc`",
        set(
            "levels[0].layerInstances[0].entityInstances[0].__identifier",
            Value::String("Bush".to_string()),
        ),
    );
    // A tile set's image outside the source folder, or not there.
    refused_at(
        "defs.tilesets[0].relPath",
        "`../../outside.png` leaves the source folder",
        set(
            "defs.tilesets[0].relPath",
            Value::String("../../outside.png".to_string()),
        ),
    );
    refused_at(
        "defs.tilesets[0].relPath",
        "the image `../tiles/grass.png` of the tile set `Ground` is not a PNG the cook found",
        set(
            "defs.tilesets[0].relPath",
            Value::String("../tiles/grass.png".to_string()),
        ),
    );
    // A flipped tall tile.
    refused_at(
        &format!("{LEVEL}[1].gridTiles[0].f"),
        "a tall tile cannot be flipped",
        set("levels[0].layerInstances[1].gridTiles[0].f", Value::Int(1)),
    );
    // Custom data the cook does not read, or animations that overlap.
    refused_at(
        "defs.tilesets[0].customData[0].data",
        "a tile's custom data is",
        set(
            "defs.tilesets[0].customData[0].data",
            Value::String("tall".to_string()),
        ),
    );
    refused_at(
        "defs.tilesets[0].customData[2].data",
        "the animation of tile 3 starts inside that of tile 2, of 2 frames",
        |project| {
            let Value::Array(data) = at(project, "defs.tilesets[0].customData") else {
                panic!("custom data");
            };
            data.push(parse(
                r#"{ "tileId": 3, "data": "{\"frames\": 2, \"ms\": 100}" }"#,
            ));
        },
    );
    // A layer whose cells are not its tiles' size, or a tile off its grid.
    refused_at(
        &format!("{LEVEL}[3].__gridSize"),
        "the layer's cells are 8 pixels, and its tile set's tiles 16",
        set("levels[0].layerInstances[3].__gridSize", Value::Int(8)),
    );
    refused_at(
        &format!("{LEVEL}[3].gridTiles[1].px"),
        "a tile's corner is on a corner of one of its layer's cells",
        set(
            "levels[0].layerInstances[3].gridTiles[1].px",
            parse("[17, 0]"),
        ),
    );
    // A `Collision` layer that is not an IntGrid, and a second one.
    refused_at(
        &format!("{LEVEL}[3].__type"),
        "the `Collision` layer is an IntGrid",
        |project| {
            *at(project, "levels[0].layerInstances[2].__identifier") =
                Value::String("Grid".to_string());
            *at(project, "levels[0].layerInstances[3].__identifier") =
                Value::String("Collision".to_string());
        },
    );
    refused_at(
        &format!("{LEVEL}[1]"),
        "a level has one `Collision` layer",
        set(
            "levels[0].layerInstances[1].__identifier",
            Value::String("Collision".to_string()),
        ),
    );
}

#[test]
fn a_tile_set_s_image_is_a_tile_set_of_the_cook_s() {
    let root = dir("ldtk_not_tiles");
    let source = root.join("source");
    sources(&source, |_| {});
    fs::remove_file(source.join("tiles/ground.sprite.jsonc")).expect("remove the sidecar");
    let (path, reason) = match cook(&source, &root.join("out")) {
        Err(CookError::Config { path, reason }) => (path, reason),
        other => panic!("{other:?}"),
    };
    assert_eq!(path, "levels/meadow.ldtk");
    assert_eq!(
        reason,
        "at `defs.tilesets[0].relPath`: the image `tiles/ground.png` of the tile set `Ground` is not cooked as a tile set: give it a sidecar, `tiles/ground.sprite.jsonc`, of `{ \"tiles\": [16, 16] }`"
    );
    // Of another size than the project says.
    sources(&source, |_| {});
    write(&source, "tiles/ground.png", plain(32, 32, [1, 2, 3, 255]));
    let (_, reason) = match cook(&source, &root.join("out")) {
        Err(CookError::Config { path, reason }) => (path, reason),
        other => panic!("{other:?}"),
    };
    assert!(
        reason.starts_with("at `defs.tilesets[0].pxWid`: the tile set `Ground` is 64 × 32"),
        "{reason}"
    );
    write(&source, "tiles/ground.png", plain(64, 48, [1, 2, 3, 255]));
    let (_, reason) = match cook(&source, &root.join("out")) {
        Err(CookError::Config { path, reason }) => (path, reason),
        other => panic!("{other:?}"),
    };
    assert!(
        reason.starts_with("at `defs.tilesets[0].pxHei`: the tile set `Ground` is 64 × 32"),
        "{reason}"
    );
}

#[test]
fn level_files_are_their_project_s_and_their_errors_name_them() {
    let root = dir("ldtk_level_files");
    let source = root.join("source");
    // A mistake in a level file is named in it.
    sources_apart(&source, |level| {
        *at(level, "layerInstances[3].gridTiles[0].a") = Value::Float(0.25);
    });
    assert_eq!(
        cook(&source, &root.join("out")),
        Err(CookError::Config {
            path: "levels/meadow/0000-Meadow.ldtkl".to_string(),
            reason: "at `layerInstances[3].gridTiles[0].a`: a tile's opacity must be 1: the cook draws every tile opaque".to_string(),
        })
    );
    // One no project saves a level in.
    sources_apart(&source, |_| {});
    write(&source, "levels/old/0000-Old.ldtkl", MEADOW);
    assert_eq!(
        cook(&source, &root.join("out")),
        Err(CookError::Config {
            path: "levels/old/0000-Old.ldtkl".to_string(),
            reason: "no LDtk project the cook found saves a level here: a level file is cooked with its project".to_string(),
        })
    );
    fs::remove_dir_all(source.join("levels/old")).expect("remove it");
    // One of another level than the project saves there.
    let mut other = parse(MEADOW);
    *at(&mut other, "iid") = Value::String("other-iid".to_string());
    write(&source, "levels/meadow/0000-Meadow.ldtkl", text(&other));
    match cook(&source, &root.join("out")) {
        Err(CookError::Decode { path, error }) => {
            assert_eq!(path, "levels/meadow/0000-meadow.ldtkl");
            assert!(matches!(error, CodecError::Ldtk { .. }), "{error}");
        }
        other => panic!("{other:?}"),
    }
    // Two levels, each saved in the other's file, and both in one.
    let meadow = parse(MEADOW);
    let mut pond = meadow.clone();
    *at(&mut pond, "identifier") = Value::String("Pond".to_string());
    *at(&mut pond, "iid") = Value::String("pond-iid".to_string());
    let saved = |level: &Value, file: &str| {
        let mut level = level.clone();
        *at(&mut level, "layerInstances") = Value::Null;
        *at(&mut level, "externalRelPath") = Value::String(file.to_string());
        text(&level)
    };
    let both = |meadow_file: &str, pond_file: &str| {
        let levels = format!(
            "{}, {}",
            saved(&meadow, meadow_file),
            saved(&pond, pond_file)
        );
        project_text(&levels, true)
    };
    write(
        &source,
        "levels/meadow.ldtk",
        both("meadow/0000-Meadow.ldtkl", "meadow/0001-Pond.ldtkl"),
    );
    write(&source, "levels/meadow/0000-Meadow.ldtkl", text(&pond));
    write(&source, "levels/meadow/0001-Pond.ldtkl", text(&meadow));
    assert_eq!(
        cook(&source, &root.join("out")),
        Err(CookError::Config {
            path: "levels/meadow/0000-Meadow.ldtkl".to_string(),
            reason: "at `iid`: it is the level of iid \"pond-iid\", and `levels/meadow.ldtk` saves its level `Meadow`, of iid \"meadow-iid\", here".to_string(),
        })
    );
    write(
        &source,
        "levels/meadow.ldtk",
        both("meadow/0000-Meadow.ldtkl", "meadow/0000-Meadow.ldtkl"),
    );
    write(&source, "levels/meadow/0000-Meadow.ldtkl", text(&meadow));
    fs::remove_file(source.join("levels/meadow/0001-Pond.ldtkl")).expect("remove it");
    assert_eq!(
        cook(&source, &root.join("out")),
        Err(CookError::Config {
            path: "levels/meadow/0000-Meadow.ldtkl".to_string(),
            reason:
                "two levels of `levels/meadow.ldtk` are saved here: a level file holds one level"
                    .to_string(),
        })
    );
    // A project of another version is the reader's to refuse.
    sources(
        &source,
        set("jsonVersion", Value::String("1.4.0".to_string())),
    );
    fs::remove_dir_all(source.join("levels/meadow")).expect("remove the level files");
    match cook(&source, &root.join("out")) {
        Err(CookError::Decode { path, error }) => {
            assert_eq!(path, "levels/meadow.ldtk");
            assert_eq!(
                error.to_string(),
                "LDtk error at `jsonVersion`: LDtk 1.4.0 is not supported: save the project with LDtk 1.5.3"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_level_s_own_fields_are_not_cooked_and_say_so() {
    let root = dir("ldtk_level_fields");
    let source = root.join("source");
    sources(
        &source,
        set(
            "levels[0].fieldInstances",
            parse(r#"[ { "__identifier": "Music", "__type": "String", "__value": "field" } ]"#),
        ),
    );
    let report = cook(&source, &root.join("out")).expect("a cook");
    assert_eq!(
        report.warnings,
        [
            "`levels/meadow.ldtk`: the fields of the level `Meadow` are not cooked: a level file has no place for them yet"
        ]
    );
}

#[test]
fn prefabs_are_checked_as_prefab_files_named_for_them() {
    let root = dir("ldtk_prefabs");
    let source = root.join("source");
    sources(&source, |_| {});
    write(
        &source,
        "data/prefabs/tree.jsonc",
        r#"{ "name": "oak", "components": {} }"#,
    );
    assert_eq!(
        cook(&source, &root.join("out")),
        Err(CookError::Config {
            path: "data/prefabs/tree.jsonc".to_string(),
            reason: "the prefab `oak` is in the file of the prefab `tree`: a prefab's file is named for it".to_string(),
        })
    );
    write(
        &source,
        "data/prefabs/tree.jsonc",
        r#"{ "name": "tree", "components": {}, "tags": [] }"#,
    );
    match cook(&source, &root.join("out")) {
        Err(CookError::Decode { path, error }) => {
            assert_eq!(path, "data/prefabs/tree.jsonc");
            assert!(matches!(error, CodecError::Prefab { .. }), "{error}");
        }
        other => panic!("{other:?}"),
    }
    // Data below a prefab's folder is not a prefab.
    write(
        &source,
        "data/prefabs/tree.jsonc",
        r#"{ "name": "tree", "components": {} }"#,
    );
    write(&source, "data/prefabs/notes/list.jsonc", r#"[1, 2]"#);
    cook(&source, &root.join("out")).expect("a cook");
}
