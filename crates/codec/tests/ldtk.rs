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

//! LDtk tests: projects of LDtk 1.5.3, written from its JSON schema by
//! `tests/ldtk/make_vectors.py` (ADR-0038), read member by member; each
//! rule of the reader broken on a copy of one, which must name where.

use nadir_codec::json::{self, Value};
use nadir_codec::ldtk::{
    self, ErrorKind, ImageExport, LayerInstance, LayerKind, Level, Project, TileInstance,
};
use nadir_codec::{CodecError, Limits};

fn text(name: &str) -> String {
    let path = format!("{}/tests/ldtk/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

fn project(name: &str) -> Project {
    ldtk::parse_project(&text(name), Limits::default())
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

/// The value of the vector `name`, changed by `edit`, as JSON text.
fn edited(name: &str, edit: impl FnOnce(&mut Value)) -> String {
    let mut value = json::parse(&text(name), Limits::default()).expect("a vector");
    edit(&mut value);
    json::to_string_compact(&value)
}

/// The value at `path` in `value`: member names, and list indexes as
/// numbers.
fn at<'v>(value: &'v mut Value, path: &str) -> &'v mut Value {
    path.split('.').fold(value, |value, step| match value {
        Value::Object(members) => {
            &mut members
                .iter_mut()
                .find(|(name, _)| name == step)
                .unwrap_or_else(|| panic!("no member {step}"))
                .1
        }
        Value::Array(items) => &mut items[step.parse::<usize>().expect("an index")],
        other => panic!("{step} of {other:?}"),
    })
}

/// The error a project gives: where, and what.
fn error(text: &str) -> (String, ErrorKind) {
    match ldtk::parse_project(text, Limits::default()) {
        Err(CodecError::Ldtk { at, kind }) => (at, kind),
        other => panic!("expected an LDtk error, got {other:?}"),
    }
}

fn layer<'l>(level: &'l Level, identifier: &str) -> &'l LayerInstance {
    level
        .layers
        .as_deref()
        .expect("its layers")
        .iter()
        .find(|layer| layer.identifier == identifier)
        .unwrap_or_else(|| panic!("no layer {identifier}"))
}

/// A tile's id, place, flips and opacity.
fn placed(tile: &TileInstance) -> (u32, [i32; 2], [i32; 2], u8, f32) {
    (tile.tile, tile.px, tile.src, tile.flip, tile.alpha)
}

#[test]
fn a_project_of_each_layer_type_is_read() {
    let project = project("project.ldtk");
    assert_eq!(project.json_version, "1.5.3");
    assert!(!project.external_levels);
    assert!(!project.backup_on_save && !project.simplified_export && !project.export_tiled);
    assert_eq!(project.image_export, ImageExport::None);

    let tiles = project.tileset(1).expect("the tile set");
    assert_eq!(tiles.identifier, "Tiles");
    assert_eq!(tiles.rel_path.as_deref(), Some("../sprites/tiles.png"));
    assert_eq!((tiles.width, tiles.height, tiles.tile_size), (128, 64, 16));
    assert_eq!((tiles.columns, tiles.rows, tiles.tile_count()), (8, 4, 32));
    assert_eq!(
        (tiles.spacing, tiles.padding, tiles.embedded),
        (0, 0, false)
    );
    let data: Vec<(u32, &str)> = tiles
        .custom_data
        .iter()
        .map(|entry| (entry.tile, entry.data.as_str()))
        .collect();
    assert_eq!(
        data,
        [
            (4, r#"{"tall": [1, 2]}"#),
            (5, r#"{"frames": 2, "ms": 600}"#),
            (8, r#"{"frames": 4, "ms": 250}"#),
        ]
    );
    let icons = project.tileset(2).expect("LDtk's icons");
    assert!(icons.embedded && icons.rel_path.is_none());

    let kinds: Vec<(&str, LayerKind)> = project
        .layers
        .iter()
        .map(|layer| (layer.identifier.as_str(), layer.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("Entities", LayerKind::Entities),
            ("Collision", LayerKind::IntGrid),
            ("Shadows", LayerKind::AutoLayer),
            ("Ground", LayerKind::Tiles),
        ],
        "top-most first"
    );
    let collision = project.layer(11).expect("the IntGrid");
    let values: Vec<(i64, Option<&str>)> = collision
        .int_grid_values
        .iter()
        .map(|value| (value.value, value.identifier.as_deref()))
        .collect();
    assert_eq!(values, [(1, Some("solid")), (2, Some("water"))]);
    assert_eq!((collision.grid_size, collision.tileset), (16, Some(1)));
    let tree = project.entity(20).expect("the tree");
    assert_eq!(
        (
            tree.identifier.as_str(),
            tree.width,
            tree.height,
            tree.pivot
        ),
        ("Tree", 16, 32, [0.5, 1.0])
    );

    let [level] = project.levels.as_slice() else {
        panic!("one level");
    };
    assert_eq!(level.identifier, "Field");
    assert_eq!((level.width, level.height, level.world), (64, 48, [0, 0]));
    assert_eq!(level.background, [0x40, 0x46, 0x5B]);
    assert_eq!(level.external_path, None);

    let collision = layer(level, "Collision");
    assert_eq!(collision.kind, LayerKind::IntGrid);
    assert_eq!(
        (collision.columns, collision.rows, collision.grid_size),
        (4, 3, 16)
    );
    assert_eq!(collision.int_grid, [1, 1, 1, 1, 1, 2, 0, 1, 1, 1, 1, 1]);
    assert_eq!(collision.auto_layer_tiles.len(), 10, "a stone on each wall");
    assert!(collision.grid_tiles.is_empty());
    assert_eq!((collision.opacity, collision.offset), (1.0, [0, 0]));
    assert!(collision.visible);
    let shadows = layer(level, "Shadows");
    assert!(shadows.int_grid.is_empty(), "only an IntGrid has values");
    assert_eq!(
        placed(&shadows.auto_layer_tiles[0]),
        (11, [32, 16], [48, 16], 0, 0.5)
    );

    let ground = layer(level, "Ground");
    let tiles: Vec<_> = ground.grid_tiles.iter().map(placed).collect();
    assert_eq!(
        tiles,
        [
            (0, [0, 0], [0, 0], 0, 1.0),
            (1, [0, 0], [16, 0], 0, 1.0),
            (8, [16, 16], [0, 16], 0, 1.0),
            (0, [32, 16], [0, 0], 3, 1.0),
            (4, [48, 16], [64, 0], 0, 1.0),
            (5, [32, 32], [80, 0], 1, 1.0),
        ],
        "in the order they are drawn, two in the first cell"
    );
    assert_eq!(ground.tileset, Some(1));
}

#[test]
fn an_entity_keeps_a_field_of_each_type() {
    let project = project("project.ldtk");
    let entities = &layer(&project.levels[0], "Entities").entities;
    let tree = &entities[0];
    assert_eq!((tree.identifier.as_str(), tree.entity_def), ("Tree", 20));
    assert_eq!(
        (tree.px, tree.cell, tree.pivot),
        ([24, 16], [1, 0], [0.5, 1.0])
    );
    assert_eq!((tree.width, tree.height), (16, 32));
    let fields: Vec<(&str, &str, String)> = tree
        .fields
        .iter()
        .map(|field| {
            (
                field.identifier.as_str(),
                field.kind.as_str(),
                json::to_string_compact(&field.value),
            )
        })
        .collect();
    let expected = [
        ("Health__max", "Int", "12"),
        ("Speed", "Float", "1.5"),
        ("Solid", "Bool", "true"),
        ("Name", "String", r#""Oak by the path""#),
        ("Note", "Multilines", r#""Two lines\nof text""#),
        ("Tint", "Color", r##""#3D8E2B""##),
        ("Target", "Point", r#"{"cx":2,"cy":1}"#),
        ("Script", "FilePath", r#""scripts/tree.txt""#),
        ("Kind", "LocalEnum.Kind", r#""Oak""#),
        ("Drops", "Array<Int>", "[1,2,3]"),
        ("Unset", "Int", "null"),
    ];
    for (name, kind, value) in expected {
        assert!(
            fields
                .iter()
                .any(|field| field == &(name, kind, value.to_string())),
            "{name}: {fields:?}"
        );
    }
    let friend = tree
        .fields
        .iter()
        .find(|field| field.kind == "EntityRef")
        .expect("a reference");
    assert!(friend.value.get("entityIid").is_some());
    let icon = tree
        .fields
        .iter()
        .find(|field| field.kind == "Tile")
        .expect("a tile");
    assert_eq!(icon.value.get("h").and_then(Value::as_i64), Some(32));
    let rock = &entities[1];
    assert_eq!(rock.identifier, "Rock");
    assert_eq!(
        json::to_string_compact(&rock.fields[0].value),
        "3",
        "its own value"
    );
}

#[test]
fn a_project_s_external_levels_are_read_against_it() {
    let project = project("external.ldtk");
    assert!(project.external_levels);
    let paths: Vec<Option<&str>> = project
        .levels
        .iter()
        .map(|level| level.external_path.as_deref())
        .collect();
    assert_eq!(
        paths,
        [
            Some("external/0000-Level_0.ldtkl"),
            Some("external/0001-Level_1.ldtkl")
        ]
    );
    assert!(project.levels.iter().all(|level| level.layers.is_none()));
    for (index, known) in project.levels.iter().enumerate() {
        let path = known.external_path.as_deref().expect("a level file");
        let level = ldtk::parse_level(&text(path), &project, Limits::default())
            .unwrap_or_else(|error| panic!("{path}: {error}"));
        assert_eq!(
            (level.iid.as_str(), level.identifier.as_str()),
            (known.iid.as_str(), known.identifier.as_str())
        );
        assert_eq!(level.uid, index as i64);
        assert_eq!(layer(&level, "Collision").int_grid.len(), 12);
        assert_eq!(layer(&level, "Entities").entities.len(), 2);
    }
}

#[test]
fn a_level_file_is_one_of_its_project_s_levels_saved_apart() {
    let external = project("external.ldtk");
    let inside = project("project.ldtk");
    let level = text("external/0000-Level_0.ldtkl");
    let no_such =
        |project: &Project, text: &str| match ldtk::parse_level(text, project, Limits::default()) {
            Err(CodecError::Ldtk { at, kind }) => (at, kind),
            other => panic!("expected an LDtk error, got {other:?}"),
        };
    // Of a project that keeps its levels inside it.
    let (place, kind) = no_such(&inside, &level);
    assert_eq!(place, "iid");
    assert!(matches!(kind, ErrorKind::NoSuchLevel { .. }), "{kind:?}");
    // Of another level.
    let other = edited("external/0000-Level_0.ldtkl", |level| {
        *at(level, "iid") = Value::String("not-a-level".to_string());
    });
    assert_eq!(
        no_such(&external, &other),
        (
            "iid".to_string(),
            ErrorKind::NoSuchLevel {
                iid: "not-a-level".to_string()
            }
        )
    );
    // With no layers.
    let empty = edited("external/0000-Level_0.ldtkl", |level| {
        *at(level, "layerInstances") = Value::Null;
    });
    assert_eq!(
        no_such(&external, &empty),
        ("layerInstances".to_string(), ErrorKind::NoLayers)
    );
    // Checked against its project's definitions.
    let stranger = edited("external/0000-Level_0.ldtkl", |level| {
        *at(level, "layerInstances.1.layerDefUid") = Value::Int(99);
    });
    assert_eq!(
        no_such(&external, &stranger),
        (
            "layerInstances[1].layerDefUid".to_string(),
            ErrorKind::NoSuchLayer { uid: 99 }
        )
    );
}

#[test]
fn only_1_5_x_is_read_and_the_others_name_the_version_to_save_with() {
    for version in ["1.5.0", "1.5.12"] {
        let text = edited("project.ldtk", |project| {
            *at(project, "jsonVersion") = Value::String(version.to_string());
        });
        assert!(
            ldtk::parse_project(&text, Limits::default()).is_ok(),
            "{version}"
        );
    }
    for version in [
        "0.9.3",
        "1.4.0",
        "2.0.0",
        "1.5",
        "1.5.3-beta",
        "1.5.3.1",
        "",
    ] {
        // A later version's members may be anything: none of them is read.
        let text = edited("project.ldtk", |project| {
            *at(project, "jsonVersion") = Value::String(version.to_string());
            *at(project, "defs") = Value::Null;
        });
        assert_eq!(
            error(&text),
            (
                "jsonVersion".to_string(),
                ErrorKind::Version {
                    found: version.to_string()
                }
            ),
            "{version}"
        );
    }
    let refused = CodecError::Ldtk {
        at: "jsonVersion".to_string(),
        kind: ErrorKind::Version {
            found: "2.0.0".to_string(),
        },
    };
    assert_eq!(
        refused.to_string(),
        "LDtk error at `jsonVersion`: LDtk 2.0.0 is not supported: save the project with LDtk 1.5.3"
    );
}

#[test]
fn each_rule_is_checked_and_names_where() {
    type Edit = fn(&mut Value);
    let cases: [(&str, Edit, &str, ErrorKind); 27] = [
        (
            "project.ldtk",
            |p| *at(p, "worlds") = Value::Array(vec![Value::Object(Vec::new())]),
            "worlds",
            ErrorKind::MultiWorlds,
        ),
        (
            "project.ldtk",
            |p| *at(p, "imageExportMode") = Value::String("Everything".to_string()),
            "imageExportMode",
            ErrorKind::UnknownValue {
                found: "Everything".to_string(),
            },
        ),
        (
            "project.ldtk",
            |p| *at(p, "defs.tilesets.1.uid") = Value::Int(1),
            "defs.tilesets[1].uid",
            ErrorKind::DuplicateUid { uid: 1 },
        ),
        (
            "project.ldtk",
            |p| *at(p, "defs.tilesets.0.customData.2.tileId") = Value::Int(32),
            "defs.tilesets[0].customData[2].tileId",
            ErrorKind::NoSuchTile { tile: 32 },
        ),
        (
            "project.ldtk",
            |p| *at(p, "defs.layers.3.tilesetDefUid") = Value::Int(7),
            "defs.layers[3].tilesetDefUid",
            ErrorKind::NoSuchTileset { uid: 7 },
        ),
        (
            "project.ldtk",
            |p| *at(p, "defs.entities.1.uid") = Value::Int(20),
            "defs.entities[1].uid",
            ErrorKind::DuplicateUid { uid: 20 },
        ),
        (
            "project.ldtk",
            |p| *at(p, "levels.0.__bgColor") = Value::String("#40465".to_string()),
            "levels[0].__bgColor",
            ErrorKind::BadColor,
        ),
        (
            "project.ldtk",
            |p| *at(p, "levels.0.layerInstances") = Value::Null,
            "levels[0]",
            ErrorKind::ExternalMismatch,
        ),
        (
            "project.ldtk",
            |p| *at(p, "levels.0.layerInstances.0.__type") = Value::String("Tiles".to_string()),
            "levels[0].layerInstances[0].__type",
            ErrorKind::KindMismatch,
        ),
        (
            "project.ldtk",
            |p| {
                let Value::Array(csv) = at(p, "levels.0.layerInstances.1.intGridCsv") else {
                    panic!("a list");
                };
                csv.pop();
            },
            "levels[0].layerInstances[1].intGridCsv",
            ErrorKind::WrongLength { expected: 12 },
        ),
        (
            "project.ldtk",
            |p| *at(p, "levels.0.layerInstances.1.intGridCsv.6") = Value::Int(3),
            "levels[0].layerInstances[1].intGridCsv[6]",
            ErrorKind::NoSuchIntGridValue { value: 3 },
        ),
        (
            "project.ldtk",
            |p| *at(p, "levels.0.layerInstances.3.gridTiles.2.t") = Value::Int(32),
            "levels[0].layerInstances[3].gridTiles[2].t",
            ErrorKind::NoSuchTile { tile: 32 },
        ),
        (
            "project.ldtk",
            |p| *at(p, "levels.0.layerInstances.3.gridTiles.2.f") = Value::Int(4),
            "levels[0].layerInstances[3].gridTiles[2].f",
            ErrorKind::OutOfRange { min: 0, max: 3 },
        ),
        (
            "project.ldtk",
            |p| *at(p, "levels.0.layerInstances.2.autoLayerTiles.0.a") = Value::Float(1.5),
            "levels[0].layerInstances[2].autoLayerTiles[0].a",
            ErrorKind::NotAFraction,
        ),
        (
            "project.ldtk",
            |p| *at(p, "levels.0.layerInstances.3.__tilesetDefUid") = Value::Null,
            "levels[0].layerInstances[3].gridTiles",
            ErrorKind::TilesWithoutTileset,
        ),
        (
            "project.ldtk",
            |p| *at(p, "levels.0.layerInstances.0.entityInstances.1.defUid") = Value::Int(22),
            "levels[0].layerInstances[0].entityInstances[1].defUid",
            ErrorKind::NoSuchEntity { uid: 22 },
        ),
        (
            "external.ldtk",
            |p| *at(p, "externalLevels") = Value::Bool(false),
            "levels[0]",
            ErrorKind::ExternalMismatch,
        ),
        (
            "project.ldtk",
            |p| *at(p, "levels.0.externalRelPath") = Value::String("x.ldtkl".to_string()),
            "levels[0]",
            ErrorKind::ExternalMismatch,
        ),
        (
            "project.ldtk",
            |p| *at(p, "defs.tilesets.0.__cWid") = Value::Int(2_000_000),
            "defs.tilesets[0]",
            ErrorKind::TooManyCells,
        ),
        (
            "project.ldtk",
            |p| *at(p, "levels.0.layerInstances.3.__cWid") = Value::Int(2_000_000),
            "levels[0].layerInstances[3]",
            ErrorKind::TooManyCells,
        ),
        (
            "project.ldtk",
            |p| *at(p, "defs.tilesets.1.embedAtlas") = Value::Int(7),
            "defs.tilesets[1].embedAtlas",
            ErrorKind::WrongType {
                expected: "a string",
            },
        ),
        (
            "project.ldtk",
            |p| *at(p, "defs.layers.0.type") = Value::String("Mosaic".to_string()),
            "defs.layers[0].type",
            ErrorKind::UnknownValue {
                found: "Mosaic".to_string(),
            },
        ),
        (
            "project.ldtk",
            |p| *at(p, "defs.layers.1.uid") = Value::Int(10),
            "defs.layers[1].uid",
            ErrorKind::DuplicateUid { uid: 10 },
        ),
        (
            "project.ldtk",
            |p| *at(p, "defs.layers.1.intGridValues.0.value") = Value::Int(0),
            "defs.layers[1].intGridValues[0].value",
            ErrorKind::OutOfRange {
                min: 1,
                max: i64::MAX,
            },
        ),
        (
            "project.ldtk",
            |p| *at(p, "levels.0.pxWid") = Value::Int(0),
            "levels[0].pxWid",
            ErrorKind::OutOfRange {
                min: 1,
                max: i64::from(u32::MAX),
            },
        ),
        (
            "project.ldtk",
            |p| *at(p, "levels.0.layerInstances.2.intGridCsv") = Value::Array(vec![Value::Int(0)]),
            "levels[0].layerInstances[2].intGridCsv",
            ErrorKind::WrongLength { expected: 0 },
        ),
        (
            "project.ldtk",
            |p| *at(p, "levels.0.layerInstances.3.__tilesetDefUid") = Value::Int(7),
            "levels[0].layerInstances[3].__tilesetDefUid",
            ErrorKind::NoSuchTileset { uid: 7 },
        ),
    ];
    for (name, edit, where_, kind) in cases {
        assert_eq!(
            error(&edited(name, edit)),
            (where_.to_string(), kind),
            "{name}: {where_}"
        );
    }
    // A member it reads, missing; and a file that is not JSON.
    let missing = edited("project.ldtk", |p| {
        let Value::Object(members) = at(p, "levels.0.layerInstances.3") else {
            panic!("an object");
        };
        members.retain(|(name, _)| name != "gridTiles");
    });
    assert_eq!(
        error(&missing),
        (
            "levels[0].layerInstances[3]".to_string(),
            ErrorKind::MissingMember { name: "gridTiles" }
        )
    );
    assert!(matches!(
        ldtk::parse_project("{", Limits::default()),
        Err(CodecError::Json { .. })
    ));
    // A member that may be `null` may be left out, as LDtk's schema allows.
    let left_out = edited("project.ldtk", |p| {
        let Value::Object(members) = at(p, "defs.tilesets.0") else {
            panic!("an object");
        };
        members.retain(|(name, _)| name != "relPath" && name != "embedAtlas");
    });
    let read = ldtk::parse_project(&left_out, Limits::default()).expect("a project");
    assert_eq!(read.tilesets[0].rel_path, None);
    assert!(!read.tilesets[0].embedded);
}
