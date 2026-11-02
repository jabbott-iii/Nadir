#!/usr/bin/env python3
# Copyright 2026 Joseph Anthony Abbott III
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""LDtk test vectors for nadir-codec.

    python3 crates/codec/tests/ldtk/make_vectors.py crates/codec/tests/ldtk fuzz/corpus/ldtk

writes, into the first directory given:

  project.ldtk                       a project of LDtk 1.5.3 with one level
                                     of each layer type: entities with a
                                     field of each type, an IntGrid with
                                     tiles by rules, an auto-layer, tiles by
                                     hand, two of them in one cell
  external.ldtk                      a project that saves its two levels apart
  external/0000-Level_0.ldtkl        its levels
  external/0001-Level_1.ldtkl

and the same, flat, into the second, the `ldtk` fuzz target's seeds
(`external_level_0.ldtkl`, `external_level_1.ldtkl`).

No LDtk editor wrote them: they follow LDtk 1.5.3's JSON schema
(https://ldtk.io/files/JSON_SCHEMA.json), with every member it requires and
the values an editor would give, tab-indented as LDtk saves (ADR-0038).
Only Python's standard library is used.
"""
import json
import os
import sys

JSON_VERSION = "1.5.3"
APP_BUILD_ID = 473703

# The tile set: tiles.png, 8 x 4 tiles of 16 (the sandbox's), and LDtk's own
# icons.
TILES = 1
ICONS = 2
# The layers, top-most first, as LDtk lists them.
ENTITIES = 10
COLLISION = 11
SHADOWS = 12
GROUND = 13
# The entities and the enum.
TREE = 20
ROCK = 21
KIND = 30


def iid(n):
    """A fixed instance id, shaped as LDtk's UUIDs are."""
    return "8e1b2c30-%04x-11ef-%04x-%012x" % (n, n * 7 % 0x10000, n * 1031)


def header():
    return {
        "fileType": "LDtk Project JSON",
        "app": "LDtk",
        "doc": "https://ldtk.io/json",
        "schema": "https://ldtk.io/files/JSON_SCHEMA.json",
        "appAuthor": "Sebastien 'deepnight' Benard",
        "appVersion": JSON_VERSION,
        "url": "https://ldtk.io",
    }


def tileset_defs():
    tiles = {
        "__cWid": 8,
        "__cHei": 4,
        "identifier": "Tiles",
        "uid": TILES,
        "relPath": "../sprites/tiles.png",
        "embedAtlas": None,
        "pxWid": 128,
        "pxHei": 64,
        "tileGridSize": 16,
        "spacing": 0,
        "padding": 0,
        "tags": [],
        "tagsSourceEnumUid": None,
        "enumTags": [],
        "customData": [
            {"tileId": 4, "data": '{"tall": [1, 2]}'},
            {"tileId": 5, "data": '{"frames": 2, "ms": 600}'},
            {"tileId": 8, "data": '{"frames": 4, "ms": 250}'},
        ],
        "savedSelections": [],
        "cachedPixelData": None,
    }
    icons = dict(tiles)
    icons.update({
        "__cWid": 32,
        "__cHei": 32,
        "identifier": "Internal_Icons",
        "uid": ICONS,
        "relPath": None,
        "embedAtlas": "LdtkIcons",
        "pxWid": 512,
        "pxHei": 512,
        "customData": [],
    })
    return [tiles, icons]


def layer_def(uid, identifier, kind, tileset, values=(), source=None):
    return {
        "__type": kind,
        "identifier": identifier,
        "type": kind,
        "uid": uid,
        "doc": None,
        "uiColor": None,
        "gridSize": 16,
        "guideGridWid": 0,
        "guideGridHei": 0,
        "displayOpacity": 1,
        "inactiveOpacity": 1,
        "hideInList": False,
        "hideFieldsWhenInactive": True,
        "canSelectWhenInactive": True,
        "renderInWorldView": True,
        "pxOffsetX": 0,
        "pxOffsetY": 0,
        "parallaxFactorX": 0,
        "parallaxFactorY": 0,
        "parallaxScaling": True,
        "requiredTags": [],
        "excludedTags": [],
        "autoTilesKilledByOtherLayerUid": None,
        "uiFilterTags": [],
        "useAsyncRender": False,
        "intGridValues": [
            {"value": value, "identifier": name, "color": color, "tile": None, "groupUid": 0}
            for value, name, color in values
        ],
        "intGridValuesGroups": [],
        "autoRuleGroups": [],
        "autoSourceLayerDefUid": source,
        "tilesetDefUid": tileset,
        "tilePivotX": 0,
        "tilePivotY": 0,
        "biomeFieldUid": None,
    }


def layer_defs():
    return [
        layer_def(ENTITIES, "Entities", "Entities", None),
        layer_def(COLLISION, "Collision", "IntGrid", TILES,
                  [(1, "solid", "#4C4C4C"), (2, "water", "#3D8EDB")]),
        layer_def(SHADOWS, "Shadows", "AutoLayer", TILES, source=COLLISION),
        layer_def(GROUND, "Ground", "Tiles", TILES),
    ]


def field_def(uid, identifier, kind, internal, array=False, null=True):
    return {
        "identifier": identifier,
        "doc": None,
        "__type": kind,
        "uid": uid,
        "type": internal,
        "isArray": array,
        "canBeNull": null,
        "arrayMinLength": None,
        "arrayMaxLength": None,
        "editorDisplayMode": "Hidden",
        "editorDisplayScale": 1,
        "editorDisplayPos": "Above",
        "editorLinkStyle": "StraightArrow",
        "editorDisplayColor": None,
        "editorAlwaysShow": False,
        "editorShowInWorld": True,
        "editorCutLongValues": True,
        "editorTextSuffix": None,
        "editorTextPrefix": None,
        "useForSmartColor": False,
        "exportToToc": False,
        "searchable": False,
        "min": None,
        "max": None,
        "regex": None,
        "acceptFileTypes": None,
        "defaultOverride": None,
        "textLanguageMode": None,
        "symmetricalRef": False,
        "autoChainRef": True,
        "allowOutOfLevelRef": True,
        "allowedRefs": "OnlySame",
        "allowedRefsEntityUid": None,
        "allowedRefTags": [],
        "tilesetUid": None,
    }


# The tree's fields: one of each type, as (uid, name, __type, internal type,
# is an array, its __value).
TREE_FIELDS = [
    (40, "Health__max", "Int", "F_Int", False, 12),
    (41, "Speed", "Float", "F_Float", False, 1.5),
    (42, "Solid", "Bool", "F_Bool", False, True),
    (43, "Name", "String", "F_String", False, "Oak by the path"),
    (44, "Note", "Multilines", "F_Text", False, "Two lines\nof text"),
    (45, "Tint", "Color", "F_Color", False, "#3D8E2B"),
    (46, "Target", "Point", "F_Point", False, {"cx": 2, "cy": 1}),
    (47, "Script", "FilePath", "F_Path", False, "scripts/tree.txt"),
    (48, "Kind", "LocalEnum.Kind", "F_Enum(%d)" % KIND, False, "Oak"),
    (49, "Drops", "Array<Int>", "F_Int", True, [1, 2, 3]),
    (50, "Friend", "EntityRef", "F_EntityRef", False, {
        "entityIid": iid(301), "layerIid": iid(201), "levelIid": iid(100), "worldIid": iid(1)}),
    (51, "Icon", "Tile", "F_Tile", False, {"tilesetUid": TILES, "x": 64, "y": 0, "w": 16, "h": 32}),
    (52, "Unset", "Int", "F_Int", False, None),
]
# The rock's one field.
ROCK_FIELDS = [(53, "Health__max", "Int", "F_Int", False, 3)]


def entity_def(uid, identifier, width, height, fields):
    return {
        "identifier": identifier,
        "uid": uid,
        "tags": [],
        "exportToToc": False,
        "allowOutOfBounds": False,
        "doc": None,
        "width": width,
        "height": height,
        "resizableX": False,
        "resizableY": False,
        "minWidth": None,
        "maxWidth": None,
        "minHeight": None,
        "maxHeight": None,
        "keepAspectRatio": False,
        "tileOpacity": 1,
        "fillOpacity": 0.08,
        "lineOpacity": 0,
        "hollow": False,
        "color": "#94D9B3",
        "renderMode": "Rectangle",
        "showName": True,
        "tilesetId": None,
        "tileRenderMode": "FitInside",
        "tileRect": None,
        "uiTileRect": None,
        "nineSliceBorders": [],
        "maxCount": 0,
        "limitScope": "PerLevel",
        "limitBehavior": "MoveLastOne",
        "pivotX": 0.5,
        "pivotY": 1,
        "fieldDefs": [field_def(*field[:4], array=field[4]) for field in fields],
    }


def defs():
    return {
        "layers": layer_defs(),
        "entities": [
            entity_def(TREE, "Tree", 16, 32, TREE_FIELDS),
            entity_def(ROCK, "Rock", 16, 16, ROCK_FIELDS),
        ],
        "tilesets": tileset_defs(),
        "enums": [{
            "identifier": "Kind",
            "uid": KIND,
            "values": [
                {"id": "Oak", "tileRect": None, "color": 0},
                {"id": "Pine", "tileRect": None, "color": 0},
            ],
            "iconTilesetUid": None,
            "externalRelPath": None,
            "externalFileChecksum": None,
            "tags": [],
        }],
        "externalEnums": [],
        "levelFields": [],
    }


def tile(px, src, t, f=0, a=1, d=None):
    # A tile placed by hand keeps its cell's index (cx + cy x 4), a tile of
    # a rule the rule and the cell.
    cell = px[1] // 16 * 4 + px[0] // 16
    return {"px": px, "src": src, "f": f, "t": t, "d": d if d is not None else [cell], "a": a}


def field(spec):
    uid, name, kind, internal, array, value = spec
    return {
        "__identifier": name,
        "__type": kind,
        "__value": value,
        "__tile": None,
        "defUid": uid,
        "realEditorValues": [],
    }


def entity(n, uid, identifier, cell, size, fields):
    width, height = size
    # Its pivot, at the bottom middle, in the level's pixels.
    px = [cell[0] * 16 + width // 2, (cell[1] + 1) * 16]
    return {
        "__identifier": identifier,
        "__grid": list(cell),
        "__pivot": [0.5, 1],
        "__tags": [],
        "__tile": None,
        "__smartColor": "#94D9B3",
        "iid": iid(n),
        "width": width,
        "height": height,
        "defUid": uid,
        "px": px,
        "fieldInstances": [field(spec) for spec in fields],
        "__worldX": px[0],
        "__worldY": px[1],
    }


def layer(n, level_uid, identifier, kind, uid, tileset, csv=(), grid=(), auto=(), entities=()):
    return {
        "__identifier": identifier,
        "__type": kind,
        "__cWid": 4,
        "__cHei": 3,
        "__gridSize": 16,
        "__opacity": 1,
        "__pxTotalOffsetX": 0,
        "__pxTotalOffsetY": 0,
        "__tilesetDefUid": tileset,
        "__tilesetRelPath": "../sprites/tiles.png" if tileset is not None else None,
        "iid": iid(n),
        "levelId": level_uid,
        "layerDefUid": uid,
        "pxOffsetX": 0,
        "pxOffsetY": 0,
        "visible": True,
        "optionalRules": [],
        "intGridCsv": list(csv),
        "autoLayerTiles": list(auto),
        "seed": 1234567 + n,
        "overrideTilesetUid": None,
        "gridTiles": list(grid),
        "entityInstances": list(entities),
    }


def field_layers(level_uid, base):
    """The layers of a level of 4 x 3 cells, top-most first."""
    walls = [1, 1, 1, 1, 1, 2, 0, 1, 1, 1, 1, 1]
    return [
        layer(base + 1, level_uid, "Entities", "Entities", ENTITIES, None, entities=[
            entity(301, TREE, "Tree", (1, 0), (16, 32), TREE_FIELDS),
            entity(302, ROCK, "Rock", (2, 1), (16, 16), ROCK_FIELDS),
        ]),
        layer(base + 2, level_uid, "Collision", "IntGrid", COLLISION, TILES, csv=walls, auto=[
            # Stones where the walls are, by a rule (d: [rule, cell]).
            tile([16 * (i % 4), 16 * (i // 4)], [48, 0], 3, d=[700, i])
            for i, value in enumerate(walls) if value == 1
        ]),
        layer(base + 3, level_uid, "Shadows", "AutoLayer", SHADOWS, TILES, auto=[
            tile([32, 16], [48, 16], 11, a=0.5, d=[701, 6]),
        ]),
        layer(base + 4, level_uid, "Ground", "Tiles", GROUND, TILES, grid=[
            tile([0, 0], [0, 0], 0),
            # Tufts over the grass in the same cell: stacked.
            tile([0, 0], [16, 0], 1),
            tile([16, 16], [0, 16], 8),
            tile([32, 16], [0, 0], 0, f=3),
            tile([48, 16], [64, 0], 4),
            tile([32, 32], [80, 0], 5, f=1),
        ]),
    ]


def level(n, uid, identifier, layers, external=None):
    return {
        "identifier": identifier,
        "iid": iid(n),
        "uid": uid,
        "worldX": 64 * uid,
        "worldY": 0,
        "worldDepth": 0,
        "pxWid": 64,
        "pxHei": 48,
        "__bgColor": "#40465B",
        "bgColor": None,
        "useAutoIdentifier": True,
        "bgRelPath": None,
        "bgPos": None,
        "bgPivotX": 0.5,
        "bgPivotY": 0.5,
        "__smartColor": "#ADADB5",
        "__bgPos": None,
        "externalRelPath": external,
        "fieldInstances": [],
        "layerInstances": layers,
        "__neighbours": [],
    }


def project(levels, external_levels):
    return {
        "__header__": header(),
        "iid": iid(1),
        "jsonVersion": JSON_VERSION,
        "appBuildId": APP_BUILD_ID,
        "nextUid": 60,
        "identifierStyle": "Capitalize",
        "toc": [],
        "worldLayout": "Free",
        "worldGridWidth": 256,
        "worldGridHeight": 256,
        "defaultLevelWidth": 64,
        "defaultLevelHeight": 48,
        "defaultPivotX": 0,
        "defaultPivotY": 0,
        "defaultGridSize": 16,
        "defaultEntityWidth": 16,
        "defaultEntityHeight": 16,
        "bgColor": "#40465B",
        "defaultLevelBgColor": "#696A79",
        "minifyJson": False,
        "externalLevels": external_levels,
        "exportTiled": False,
        "simplifiedExport": False,
        "imageExportMode": "None",
        "exportLevelBg": True,
        "pngFilePattern": None,
        "backupOnSave": False,
        "backupLimit": 10,
        "backupRelPath": None,
        "levelNamePattern": "Level_%idx",
        "tutorialDesc": None,
        "customCommands": [],
        "flags": ["UseMultilinesType"],
        "defs": defs(),
        "levels": levels,
        "worlds": [],
        "dummyWorldIid": iid(2),
    }


def text(value):
    return json.dumps(value, indent="\t", ensure_ascii=False) + "\n"


def write(path, value):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as out:
        out.write(text(value))


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    vectors, corpus = sys.argv[1], sys.argv[2]

    field = level(100, 0, "Field", field_layers(0, 200))
    files = {"project.ldtk": project([field], False)}

    external = []
    for index in range(2):
        name = "Level_%d" % index
        path = "external/%04d-%s.ldtkl" % (index, name)
        whole = level(110 + index, index, name, field_layers(index, 220 + 10 * index))
        # A level file has the level, with its layers, and a header.
        files[path] = dict({"__header__": header()}, **whole)
        external.append(level(110 + index, index, name, None, external=path))
    files["external.ldtk"] = project(external, True)

    for path, value in files.items():
        write(os.path.join(vectors, path), value)
        flat = path.replace("external/0000-Level_0", "external_level_0").replace(
            "external/0001-Level_1", "external_level_1")
        write(os.path.join(corpus, flat), value)


if __name__ == "__main__":
    main()
