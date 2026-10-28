//! The field's map, built by hand: 64 × 48 tiles of 16 pixels, written
//! as rows of letters. It makes the ground, a tile layer of the tile set
//! `assets/sprites/tiles.qoi`, and the collision grid; the tall walls,
//! trees and rocks stand on it as y-sorted sprites (`docs/subsystems/
//! render.md`, "Tilemaps").
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

use crate::Prop;

/// The tile set: 8 × 4 tiles of 16 pixels, numbered row by row.
pub(crate) const TILE_SET: &str = "sprites/tiles.qoi";
const COLUMNS: u16 = 8;
const ROWS: u16 = 4;

/// A tile's side, in pixels.
pub(crate) const TILE: f32 = 16.0;

/// The tiles of the set.
const GRASS: u16 = 0;
const GRASS_TUFTS: u16 = 1;
const PATH: u16 = 2;
/// A stone wall seen from above, which the maze of `stress-entities` is
/// made of too.
pub(crate) const STONE: u16 = 3;
/// The top of a tall wall, over its face, tile 12.
const TALL_WALL: u16 = 4;
const FLOWERS: u16 = 5;
const WATER: u16 = 8;
const GRASS_DARK: u16 = 13;

/// The layer of what the map's walls, water, trees and rocks are, which
/// the grid's tiles are on.
pub(crate) const WORLD: LayerMask = LayerMask::bit(0);
/// The layer of the people.
pub(crate) const PEOPLE: LayerMask = LayerMask::bit(1);

/// The size of the map, in tiles.
pub(crate) const WIDTH: u32 = 64;
pub(crate) const HEIGHT: u32 = 48;

/// The map, row by row from the top.
const MAP: [&str; HEIGHT as usize] = [
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

/// The tile set of the texture `texture`.
pub(crate) fn tile_set(texture: Handle<Texture>) -> TileSet {
    TileSet {
        texture,
        columns: COLUMNS,
        rows: ROWS,
    }
}

/// What the map makes.
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

/// The sprite of a tall wall: its top and its face, one tile wide, two
/// high, y-sorted by its feet among the people.
pub(crate) fn tall_wall(set: TileSet) -> Sprite {
    let mut wall = Sprite::new(set.texture, Vec2::new(TILE, 2.0 * TILE));
    wall.uv = set.block_uv(TALL_WALL, UVec2::new(1, 2));
    wall.layer = 1;
    wall
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn the_grid_is_solid_where_the_walls_and_the_water_are() {
        let app = crate::tests::scene(crate::Scene::Field);
        let world = app.world();
        let set = tile_set(world.resource::<crate::Sprites>().tiles);
        let map = build(set);
        let collisions = world.resource::<Collisions>();
        let grid = collisions.grid();
        assert_eq!(grid.layer, WORLD);
        assert_eq!((grid.width(), grid.height()), (WIDTH, HEIGHT));
        let mut letters = [0usize; 256];
        for (y, row) in (0..).zip(MAP) {
            for (x, letter) in (0..).zip(row.bytes()) {
                let cell = IVec2::new(x, y);
                let solid = matches!(letter, b'#' | b'~' | b'W');
                assert_eq!(grid.is_solid(cell), solid, "({x}, {y})");
                let tile = map.ground.get(cell).expect("every cell has a tile");
                let frames = match letter {
                    b'~' => 4,
                    b'*' => 2,
                    _ => 1,
                };
                assert_eq!(tile.frames, frames, "({x}, {y})");
                letters[usize::from(letter)] += 1;
            }
        }
        assert_eq!(map.walls.len(), letters[usize::from(b'W')]);
        assert_eq!(
            map.props.len(),
            letters[usize::from(b'T')] + letters[usize::from(b'R')]
        );
        // A tall wall is the tile set's column 4, rows 0 and 1.
        let wall = tall_wall(set);
        assert_eq!(
            wall.uv,
            Rect::new(Vec2::new(0.5, 0.0), Vec2::new(0.625, 0.5))
        );
        assert_eq!((wall.size, wall.layer), (Vec2::new(16.0, 32.0), 1));
    }
}
