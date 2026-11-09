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

//! The field's map, as it was built by hand until the field became the
//! LDtk level `assets/levels/field.ldtk` (roadmap M5.12, M5.6): 64 × 48
//! tiles of 16 pixels, written as rows of letters. Its ground is a tile
//! layer of the tile set `assets/sprites/tiles.png`, cooked, with the
//! collision grid under it; the tall walls, trees and rocks stand on it as
//! y-sorted sprites (`docs/subsystems/render.md`, "Tilemaps"). The game
//! loads the level; the tests find this map in it, cell for cell.
//!
//! | Letter | Tile | Solid |
//! | :- | :- | :- |
//! | `.` `,` `;` | grass, three kinds | no |
//! | `:` | path | no |
//! | `*` | flowers, swaying: 2 frames | no |
//! | `~` | water, rippling: 4 frames | yes |
//! | `#` | a stone wall, seen from above | yes |
//! | `W` | grass under a tall wall: a sprite of two tiles, its face in the cell and its top in the one above | yes |
//! | `T` `R` | grass under a tree or a rock, which stands at the cell's bottom with a collider at its foot | no |
//!
//! Stone walls go all round, since cells outside a grid are not solid.

use nadir::prelude::*;

#[cfg(test)]
use crate::Prop;

/// The tile set's tiles across and down, numbered row by row: 8 × 4 of 16
/// pixels, as the map's tile numbers count them.
pub(crate) const COLUMNS: u16 = 8;
pub(crate) const ROWS: u16 = 4;

/// A tile's side, in pixels.
#[cfg(test)]
pub(crate) const TILE: f32 = 16.0;

/// A stone wall seen from above, which the maze of `stress-entities` is
/// made of too.
pub(crate) const STONE: u16 = 3;

/// The tiles of the set.
#[cfg(test)]
mod tile {
    pub(super) const GRASS: u16 = 0;
    pub(super) const GRASS_TUFTS: u16 = 1;
    pub(super) const PATH: u16 = 2;
    /// The top of a tall wall, over its face, tile 12.
    pub(super) const TALL_WALL: u16 = 4;
    pub(super) const FLOWERS: u16 = 5;
    pub(super) const WATER: u16 = 8;
    pub(super) const GRASS_DARK: u16 = 13;
}
#[cfg(test)]
use tile::*;

/// The layer of what the map's walls, water, trees and rocks are, which
/// the grid's tiles are on.
pub(crate) const WORLD: LayerMask = LayerMask::bit(0);
/// The layer of the people.
pub(crate) const PEOPLE: LayerMask = LayerMask::bit(1);

/// The size of the map, in tiles.
#[cfg(test)]
pub(crate) const WIDTH: u32 = 64;
#[cfg(test)]
pub(crate) const HEIGHT: u32 = 48;

/// The map, row by row from the top.
#[cfg(test)]
pub(crate) const MAP: [&str; HEIGHT as usize] = [
    "################################################################",
    "#................,,............................................#",
    "#........;..;..,..;...........;...,.,.;..............,.........#",
    "#.....****...........,.........;.........,................;;...#",
    "#.....****............;;......,~~~~~~~~.;...;..................#",
    "#....;****.....;......,.......~~~~~~~~~~......,...............;#",
    "#.......................T.....~~~~~~~~~~..,.;..........,.......#",
    "#........,....................~~~~~~~~~~.....,.................#",
    "#...;....................;....~~~~~~~~~~.....T.............,,..#",
    "#;....T................;.......~~~~~~~~........................#",
    "#.......;..........,.....,..................,...;..............#",
    "#........................................,.....................#",
    "#::::::::::::::::::::::::::::::::::::::::::::::::::........,...#",
    "#.............,..;..,.............;...............:........;...#",
    "#.........;.......,,...........;...............,..:.......T..;.#",
    "#...........R....................,...;............:.,;.........#",
    "#.......................;...........T,.;..........:..;.........#",
    "#..,.......................;...,.................,:.....,;.....#",
    "#............;.....;.....,.R......................:..........;.#",
    "#..,......................;..............;........:......,.....#",
    "#....;WWWWWWWW;;WWWWWWW...............,...........:.*****......#",
    "#......,........................,.............;...:.*****......#",
    "#..,...........,..................................:.*****......#",
    "#.,......................;........................:......;.....#",
    "#.......;.;............;....;.;.........W.........:...;........#",
    "#....................,.....;.........,..W.........:.........;.,#",
    "#,...R.......,....;.T.......,,..........W.;.......:............#",
    "#,....................,.................W.........:.,.........,#",
    "#.................;....,............,.,.W....,....:....;.......#",
    "#,....;......,...........;.......;,.....W.........:............#",
    "#..;.;....****................T.........W.........:...R........#",
    "#;.;......****............,.............W.........:............#",
    "#.........****......;....,..............W..;......:.;..........#",
    "#.,.....................;.....;..;......W.........:,....;......#",
    "#.....,.......;....,..;...T......;......W.........:........;..,#",
    "#......................;............,.........;...:............#",
    "#.;................;........................WWWWWW:WWWWWWWW....#",
    "#..........................,.;.;................,.:.,..........#",
    "#.....,...;.....R.........;...;.......;;....;;....:........;...#",
    "#..,.,......;..............;..........;,.....;....:....;..;....#",
    "#...;...............:::::::::::::::::::::::::::::::............#",
    "#.......;.......;.........,.......................,.;..........#",
    "#.;.....T.....,.........******................................;#",
    "#.................,.....******.................................#",
    "#.....,................;......,...........R.......;....;T;.....#",
    "#;....;................,..;............;.........;.............#",
    "#...,......................;............;......................#",
    "################################################################",
];

/// What the map makes.
#[cfg(test)]
pub(crate) struct Map {
    /// The ground: every cell's tile.
    pub(crate) ground: TileLayer,
    /// The solid cells, on [`WORLD`].
    pub(crate) grid: TileGrid,
    /// Where each tall wall's feet are: the bottom middle of its cell.
    pub(crate) walls: Vec<Vec2>,
    /// The trees and rocks, with their feet.
    pub(crate) props: Vec<(Prop, Vec2)>,
}

/// Reads the map, with `set` as the ground's tile set.
#[cfg(test)]
pub(crate) fn build(set: TileSet) -> Map {
    let mut ground = TileLayer::new(set, UVec2::new(WIDTH, HEIGHT), TILE, Vec2::ZERO);
    let mut grid = TileGrid::new(WIDTH, HEIGHT, TILE, Vec2::ZERO);
    grid.layer = WORLD;
    let mut walls = Vec::new();
    let mut props = Vec::new();
    for (y, row) in (0..).zip(MAP) {
        for (x, letter) in (0..).zip(row.bytes()) {
            let cell = IVec2::new(x, y);
            let feet = Vec2::new((x as f32 + 0.5) * TILE, (y + 1) as f32 * TILE);
            let tile = match letter {
                b'#' => Tile::new(STONE),
                b'~' => Tile::animated(WATER, 4, 250),
                b':' => Tile::new(PATH),
                b'*' => Tile::animated(FLOWERS, 2, 600),
                b',' => Tile::new(GRASS_TUFTS),
                b';' => Tile::new(GRASS_DARK),
                _ => Tile::new(GRASS),
            };
            ground.set(cell, Some(tile));
            match letter {
                b'#' | b'~' => grid.set_solid(cell, true),
                b'W' => {
                    grid.set_solid(cell, true);
                    walls.push(feet);
                }
                b'T' => props.push((Prop::Tree, feet)),
                b'R' => props.push((Prop::Rock, feet)),
                _ => {}
            }
        }
    }
    Map {
        ground,
        grid,
        walls,
        props,
    }
}

#[cfg(test)]
mod tests {
    use nadir::CurrentLevel;
    use nadir::assets::Scope;
    use nadir::scene::level::{Level, LevelFile};
    use nadir::scene::{PrefabLibrary, TypeRegistry};

    use super::*;

    /// The field as the cook writes it from `assets/levels/field.ldtk`:
    /// `levels/field.klvl`, read as the game reads it.
    fn cooked(app: &App) -> LevelFile {
        let mut server = app.world().resource_mut::<AssetServer>();
        let level = server
            .load_now::<Level>("levels/field.klvl", &mut FinalizeCtx::new())
            .unwrap_or_else(|error| panic!("the field's level: {error}; run `cargo xtask cook`"));
        server.get(level).expect("the level").file().clone()
    }

    #[test]
    fn the_map_is_its_size_and_walled_all_round() {
        for (y, row) in MAP.iter().enumerate() {
            assert_eq!(row.len(), WIDTH as usize, "row {y}");
            let edge = y == 0 || y == HEIGHT as usize - 1;
            for (x, letter) in row.bytes().enumerate() {
                assert!(b".,;:*~#WTR".contains(&letter), "({x}, {y}) is {letter}");
                if edge || x == 0 || x == WIDTH as usize - 1 {
                    assert_eq!(letter, b'#', "({x}, {y}) is on the edge");
                }
            }
        }
    }

    #[test]
    fn the_loaded_field_is_the_map_with_its_walls_and_props_on_it() {
        let app = crate::tests::scene(crate::Scene::Field);
        let world = app.world();
        let set = world.resource::<crate::Sprites>().tiles;
        assert_eq!((set.columns, set.rows), (COLUMNS, ROWS));
        let map = build(set);
        let level = world.resource::<CurrentLevel>();
        assert_eq!(level.name(), Some("field"));

        // The grid, solid where the walls and the water are.
        let collisions = world.resource::<Collisions>();
        let grid = collisions.grid();
        assert_eq!(grid.layer, WORLD);
        assert_eq!((grid.width(), grid.height()), (WIDTH, HEIGHT));
        // The ground, each cell's tile the map's, animated as it animates.
        let [ground] = level.tile_layers() else {
            panic!("one tile layer: {:?}", level.tile_layers());
        };
        let layers = world.resource::<TileLayers>();
        let tiles = layers.get(ground.tiles).expect("the ground");
        assert_eq!(tiles.tileset(), set);
        for (y, row) in (0..).zip(MAP) {
            for (x, letter) in (0..).zip(row.bytes()) {
                let cell = IVec2::new(x, y);
                let solid = matches!(letter, b'#' | b'~' | b'W');
                assert_eq!(grid.is_solid(cell), solid, "({x}, {y})");
                assert_eq!(tiles.get(cell), map.ground.get(cell), "({x}, {y})");
            }
        }

        // The level's entities: the tall walls, then the trees and rocks,
        // in the map's order, each at its feet.
        let entities = level.entities();
        assert_eq!(entities.len(), map.walls.len() + map.props.len());
        let transforms = world.read::<Transform>();
        let feet: Vec<Vec2> = entities
            .iter()
            .map(|&entity| transforms.get(entity).expect("a Transform").position)
            .collect();
        let made: Vec<Vec2> = map
            .walls
            .iter()
            .copied()
            .chain(map.props.iter().map(|&(_, feet)| feet))
            .collect();
        assert_eq!(feet, made);
        // A tall wall is the tile set's column 4, rows 0 and 1: its top and
        // its face, one tile wide, two high, y-sorted among the people.
        let sprites = world.read::<Sprite>();
        for &entity in &entities[..map.walls.len()] {
            let wall = sprites.get(entity).expect("a wall's sprite");
            assert_eq!(wall.texture, set.texture);
            assert_eq!(
                wall.uv,
                Rect::new(Vec2::new(0.5, 0.0), Vec2::new(0.625, 0.5))
            );
            assert_eq!((wall.size, wall.layer), (Vec2::new(16.0, 32.0), 1));
            assert_eq!(set.block_uv(TALL_WALL, UVec2::new(1, 2)), wall.uv);
        }
    }

    #[test]
    fn the_cooked_level_is_the_map_cell_for_cell() {
        let app = crate::tests::scene(crate::Scene::Field);
        let set = app.world().resource::<crate::Sprites>().tiles;
        let map = build(set);
        let level = cooked(&app);
        assert_eq!(level.size, [WIDTH * 16, HEIGHT * 16]);
        // The sandbox's clear colour, which the level gives the frames.
        assert_eq!(level.background, [84, 84, 96]);
        assert_eq!(level.assets, ["atlas/sprites.kbj"]);

        // The tile set, with its water and flowers animated as `build`
        // animates them.
        let [tiles] = level.tilesets.as_slice() else {
            panic!("one tile set: {:?}", level.tilesets);
        };
        assert_eq!(
            (tiles.atlas.as_str(), tiles.sprite.as_str()),
            ("atlas/sprites.kbj", "sprites/tiles")
        );
        assert_eq!((tiles.columns, tiles.rows), (COLUMNS, ROWS));
        let animations: Vec<(u16, u16, u16)> = tiles
            .animations
            .iter()
            .map(|animation| (animation.tile, animation.frames, animation.millis))
            .collect();
        assert_eq!(animations, [(FLOWERS, 2, 600), (WATER, 4, 250)]);

        // One tile layer, the ground: each cell's tile is the map's.
        let [ground] = level.layers.as_slice() else {
            panic!("one tile layer: {:?}", level.layers.len());
        };
        assert_eq!(
            (ground.tileset, ground.size, ground.cell),
            (0, [WIDTH, HEIGHT], 16)
        );
        assert_eq!((ground.origin, ground.layer, ground.depth), ([0, 0], 0, 0));
        let grid = level.grid.as_ref().expect("the field's grid");
        assert_eq!(
            (grid.size, grid.cell, grid.origin),
            ([WIDTH, HEIGHT], 16, [0, 0])
        );
        assert_eq!(grid.layer, "World");
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let place = (y * WIDTH + x) as usize;
                let cell = IVec2::new(x as i32, y as i32);
                let at = ground.cells[place].unwrap_or_else(|| panic!("({x}, {y}) has no tile"));
                let mut tile = match tiles.animation(at.tile) {
                    Some(animation) => Tile::animated(at.tile, animation.frames, animation.millis),
                    None => Tile::new(at.tile),
                };
                tile.flip = Flip {
                    x: at.flip_x,
                    y: at.flip_y,
                };
                assert_eq!(Some(tile), map.ground.get(cell), "({x}, {y})");
                assert_eq!(grid.solid[place], map.grid.is_solid(cell), "({x}, {y})");
            }
        }

        // The tall walls, in the map's order: its top and its face, with
        // their feet at the bottom middle of the wall's cell.
        let walls: Vec<Vec2> = level
            .tall
            .iter()
            .map(|wall| {
                assert_eq!((wall.tileset, wall.tile, wall.size), (0, TALL_WALL, [1, 2]));
                assert_eq!(wall.layer, 1);
                Vec2::new(wall.feet[0] as f32, wall.feet[1] as f32)
            })
            .collect();
        assert_eq!(walls, map.walls);

        // The trees and rocks, in the map's order, as prefabs at their
        // feet.
        let props: Vec<(&str, Vec2)> = level
            .entities
            .iter()
            .map(|entity| {
                assert!(entity.overrides.is_empty(), "{entity:?}");
                let [x, y] = entity.position;
                (entity.prefab.as_str(), Vec2::new(x as f32, y as f32))
            })
            .collect();
        let made: Vec<(&str, Vec2)> = map
            .props
            .iter()
            .map(|&(prop, feet)| match prop {
                Prop::Tree => ("tree", feet),
                Prop::Rock => ("rock", feet),
            })
            .collect();
        assert_eq!(props, made);
    }

    #[test]
    fn the_field_s_prefabs_are_the_trees_and_rocks_it_makes_by_hand() {
        let mut app =
            crate::tests::scene(crate::Scene::Field).collision_layers(&["World", "People"]);
        let world = app.world();
        let registry = world.resource::<TypeRegistry>();
        let mut library = PrefabLibrary::new();
        for name in ["tree", "rock"] {
            // Loading checks each component against the registry.
            let mut server = world.resource_mut::<AssetServer>();
            library
                .load(&mut server, &registry, name, Scope::Global)
                .unwrap_or_else(|error| panic!("the prefab `{name}`: {error}"));
        }
        let mut commands = world.commands();
        let spawned = nadir::loading_context(world, "test", Scope::Global, |mut cx| {
            ["tree", "rock"].map(|name| {
                library
                    .spawn(name, &[], &registry, &mut commands, &mut cx)
                    .unwrap_or_else(|error| panic!("a `{name}`: {error}"))
            })
        });
        drop((commands, registry));
        app.world_mut().apply_commands();

        let world = app.world();
        let sprites = world.resource::<crate::Sprites>();
        let made = [
            (sprites.tree, Vec2::new(4.0, 3.0)),
            (sprites.rock, Vec2::new(7.0, 4.0)),
        ];
        for (entity, (region, half)) in spawned.into_iter().zip(made) {
            let mut sprite = region.sprite;
            sprite.layer = 1;
            assert_eq!(world.read::<Sprite>().get(entity), Some(&sprite));
            assert_eq!(
                world.read::<Collider>().get(entity),
                Some(&crate::at_feet(half, WORLD, WORLD))
            );
        }
    }
}
