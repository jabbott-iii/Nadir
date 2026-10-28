//! Tile layers: grids of tiles from a tile set, which the renderer draws in
//! chunks of [`TileLayer::CHUNK`] × [`TileLayer::CHUNK`] tiles
//! (`docs/subsystems/render.md`, "Tilemaps"; ADR-0023).

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use nadir_assets::Handle;
use nadir_core::{GenArena, IVec2, Rect, UVec2, Vec2};

use crate::{Flip, Texture};

/// A tile atlas: a texture cut into `columns` × `rows` equal tiles,
/// numbered row by row from the top left.
///
/// In pixel-perfect mode a tile shows each texel as a whole number of
/// pixels when the texture's width and height are whole multiples of the
/// columns and rows, and the layer's tile size times the zoom is a whole
/// multiple of a tile's texels, as for sprites (ADR-0018).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileSet {
    /// The texture.
    pub texture: Handle<Texture>,
    /// The tiles across. 0 counts as 1.
    pub columns: u16,
    /// The tiles down. 0 counts as 1.
    pub rows: u16,
}

impl TileSet {
    /// The region of the texture, normalized, of the block of `size` tiles
    /// whose top-left tile is `index`: of that one tile for a size of
    /// (1, 1). A tall tile is drawn as a sprite of such a block.
    ///
    /// An index past the last tile wraps around to the first, as in a
    /// layer. A block that does not fit reaches past the texture's edge.
    pub fn block_uv(&self, index: u16, size: UVec2) -> Rect {
        let columns = u32::from(self.columns.max(1));
        let rows = u32::from(self.rows.max(1));
        let index = u32::from(index) % (columns * rows);
        let corner = UVec2::new(index % columns, index / columns).as_vec2();
        let grid = UVec2::new(columns, rows).as_vec2();
        Rect::new(corner / grid, (corner + size.as_vec2()) / grid)
    }
}

/// What a cell shows: a tile of its layer's tile set, or the frames of an
/// animation, which the shader picks from [`RenderFrame::time`] with no
/// work on the CPU.
///
/// [`RenderFrame::time`]: crate::RenderFrame::time
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tile {
    /// The tile, or the first frame. An index past the tile set's last
    /// tile wraps around to the first.
    pub index: u16,
    /// How many frames: tiles `index`, `index + 1` and so on, each shown in
    /// turn, then again from the first. 0 and 1 are a still tile.
    pub frames: u16,
    /// How long each frame shows, in milliseconds. 0 counts as 1. Every
    /// tile of the same frame length changes frame at the same moment.
    pub frame_ms: u16,
    /// Which way the tile is mirrored, in place.
    pub flip: Flip,
}

impl Tile {
    /// Tile `index`, still and unmirrored.
    pub const fn new(index: u16) -> Self {
        Self::animated(index, 1, 0)
    }

    /// `frames` tiles from `index`, each shown for `frame_ms`
    /// milliseconds in turn.
    pub const fn animated(index: u16, frames: u16, frame_ms: u16) -> Self {
        Self {
            index,
            frames,
            frame_ms,
            flip: Flip { x: false, y: false },
        }
    }
}

/// The `f32`s of one tile instance, in the order of the `tile_chunk`
/// shader's instance attributes (locations 0 and 1):
///
/// | Floats | Location | What |
/// | :- | :- | :- |
/// | 0, 1 | 0 | cell: its column and row in the layer |
/// | 2 to 5 | 1 | tile: index, frames, frame length in ms, flip (1: x, 2: y) |
pub const TILE_INSTANCE_FLOATS: usize = 6;

/// The last revision handed out. Revisions are counted for the whole
/// process, so that no two contents of a chunk, in any layer of any store,
/// share one, and the renderer's cache can never take a chunk for one it
/// has built (ADR-0023). 0 is never handed out.
static REVISIONS: AtomicU64 = AtomicU64::new(0);

fn next_revision() -> u64 {
    // Only uniqueness matters, which every ordering keeps.
    REVISIONS.fetch_add(1, Ordering::Relaxed) + 1
}

/// A layer of tiles: a grid of square cells in the world, each empty or
/// showing a [`Tile`] of the layer's [`TileSet`].
///
/// Cell `(0, 0)` has its top-left corner at the origin; `x` grows to the
/// right and `y` down, as in a `TileGrid` of the collision crate. The
/// renderer draws a layer in chunks of [`TileLayer::CHUNK`] cells a side,
/// those that the view shows, and packs a chunk's tiles again only after
/// one of them changes.
#[derive(Clone, Debug)]
pub struct TileLayer {
    tileset: TileSet,
    size: UVec2,
    tile_size: f32,
    origin: Vec2,
    /// Row by row.
    cells: Vec<Option<Tile>>,
    /// The revision of each chunk's contents, row by row: a new one each
    /// time one of its cells changes.
    revisions: Vec<u64>,
}

impl TileLayer {
    /// The side of a chunk, in cells.
    pub const CHUNK: u32 = 32;

    /// A layer of `size` cells, columns by rows, all empty, of `tile_size`
    /// world units a side, with its top-left corner at `origin`. A layer
    /// with no columns or no rows draws nothing.
    ///
    /// Cells are placed in `f32`, exactly up to 2^24 cells a side.
    ///
    /// # Panics
    ///
    /// If `tile_size` is not a finite number above 0, or the layer has more
    /// cells than memory can count.
    pub fn new(tileset: TileSet, size: UVec2, tile_size: f32, origin: Vec2) -> Self {
        assert!(
            tile_size.is_finite() && tile_size > 0.0,
            "a tile layer's tiles must have a finite size above 0, not {tile_size}"
        );
        let cells = usize::try_from(u64::from(size.x) * u64::from(size.y))
            .expect("a tile layer has no more cells than memory can count");
        let chunks = Self::chunks_of(size);
        let revision = next_revision();
        Self {
            tileset,
            size,
            tile_size,
            origin,
            cells: vec![None; cells],
            revisions: vec![revision; (chunks.x * chunks.y) as usize],
        }
    }

    /// The tile set.
    pub fn tileset(&self) -> TileSet {
        self.tileset
    }

    /// The number of cells: columns by rows.
    pub fn size(&self) -> UVec2 {
        self.size
    }

    /// The side of a cell, in world units.
    pub fn tile_size(&self) -> f32 {
        self.tile_size
    }

    /// The top-left corner of cell `(0, 0)`.
    pub fn origin(&self) -> Vec2 {
        self.origin
    }

    /// The number of chunks: columns by rows. The last of a row or column
    /// may be cut short by the layer's edge.
    pub fn chunks(&self) -> UVec2 {
        Self::chunks_of(self.size)
    }

    fn chunks_of(size: UVec2) -> UVec2 {
        UVec2::new(size.x.div_ceil(Self::CHUNK), size.y.div_ceil(Self::CHUNK))
    }

    /// The tile of `cell`: `None` if it is empty, or outside the layer.
    pub fn get(&self, cell: IVec2) -> Option<Tile> {
        self.index(cell).and_then(|index| self.cells[index])
    }

    /// Shows `tile` in `cell`, or empties it with `None`. The chunk that
    /// holds the cell is packed again before it is next drawn, if this
    /// changed it.
    ///
    /// # Panics
    ///
    /// If `cell` is outside the layer.
    pub fn set(&mut self, cell: IVec2, tile: Option<Tile>) {
        let Some(index) = self.index(cell) else {
            panic!(
                "cell ({}, {}) is outside the {} × {} tile layer",
                cell.x, cell.y, self.size.x, self.size.y
            );
        };
        if self.cells[index] != tile {
            self.cells[index] = tile;
            // `index` found the cell inside the layer, so it is not
            // negative.
            let chunk = self.chunk_index(UVec2::new(
                cell.x as u32 / Self::CHUNK,
                cell.y as u32 / Self::CHUNK,
            ));
            self.revisions[chunk] = next_revision();
        }
    }

    /// Appends the instances of the tiles of chunk `chunk` (its column and
    /// row of chunks) to `out`, row by row, skipping empty cells:
    /// [`TILE_INSTANCE_FLOATS`] floats each, as the renderer uploads them.
    /// Returns how many there are; none for a chunk outside the layer.
    ///
    /// The renderer does this for each chunk the view shows that changed
    /// since it last did. It is public so that it can be measured.
    pub fn pack_chunk(&self, chunk: UVec2, out: &mut Vec<f32>) -> u32 {
        let chunks = self.chunks();
        if chunk.x >= chunks.x || chunk.y >= chunks.y {
            return 0;
        }
        let first = chunk * Self::CHUNK;
        let end = (first + UVec2::splat(Self::CHUNK)).min(self.size);
        let mut count = 0;
        for y in first.y..end.y {
            let row = y as usize * self.size.x as usize;
            for x in first.x..end.x {
                let Some(tile) = self.cells[row + x as usize] else {
                    continue;
                };
                let flip = u8::from(tile.flip.x) | u8::from(tile.flip.y) << 1;
                out.extend_from_slice(&[
                    x as f32,
                    y as f32,
                    f32::from(tile.index),
                    f32::from(tile.frames),
                    f32::from(tile.frame_ms),
                    f32::from(flip),
                ]);
                count += 1;
            }
        }
        count
    }

    /// The revision of the contents of chunk `chunk`, counted row by row. A
    /// new one comes from the counter for the whole process each time the
    /// chunk changes, so two chunks share one only when they hold the same:
    /// a new layer's chunks, all empty, or a copy and its original.
    pub(crate) fn revision(&self, chunk: usize) -> u64 {
        self.revisions[chunk]
    }

    /// The index of chunk `chunk` in `revisions`; it must be in the layer.
    pub(crate) fn chunk_index(&self, chunk: UVec2) -> usize {
        chunk.y as usize * self.chunks().x as usize + chunk.x as usize
    }

    /// The index of `cell` in `cells`, if it is in the layer.
    fn index(&self, cell: IVec2) -> Option<usize> {
        let x = u32::try_from(cell.x).ok().filter(|&x| x < self.size.x)?;
        let y = u32::try_from(cell.y).ok().filter(|&y| y < self.size.y)?;
        Some(y as usize * self.size.x as usize + x as usize)
    }
}

/// The game's tile layers. The renderer is given them beside each frame,
/// whose [`TileLayerDraw`]s name the ones to draw, as its sprites name
/// textures in the asset server (ADR-0023).
#[derive(Default)]
pub struct TileLayers {
    layers: GenArena<TileLayer>,
}

impl TileLayers {
    /// No layers.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `layer`, and returns its handle.
    ///
    /// # Panics
    ///
    /// If there would be more than `u32::MAX` layers at once.
    pub fn add(&mut self, layer: TileLayer) -> Handle<TileLayer> {
        Handle::new(self.layers.insert(layer))
    }

    /// The layer of `handle`, if it is still here.
    pub fn get(&self, handle: Handle<TileLayer>) -> Option<&TileLayer> {
        self.layers.get(handle.id())
    }

    /// The layer of `handle`, to change, if it is still here.
    pub fn get_mut(&mut self, handle: Handle<TileLayer>) -> Option<&mut TileLayer> {
        self.layers.get_mut(handle.id())
    }

    /// Takes the layer of `handle` out. The renderer then frees what it
    /// keeps of it on the GPU, at its next frame.
    pub fn remove(&mut self, handle: Handle<TileLayer>) -> Option<TileLayer> {
        self.layers.remove(handle.id())
    }

    /// The number of layers.
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
}

impl fmt::Debug for TileLayers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TileLayers")
            .field("len", &self.layers.len())
            .finish_non_exhaustive()
    }
}

/// A tile layer to draw in a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileLayerDraw {
    /// The layer, in the [`TileLayers`] the renderer is given. One that is
    /// not there, or whose tile set's texture is not ready, is not drawn.
    pub tiles: Handle<TileLayer>,
    /// Higher layers are drawn in front of lower ones, sprites and tiles
    /// alike.
    pub layer: u8,
    /// Where the tiles are among the sprites of their layer, as a sprite's
    /// [`SortMode::Fixed`](crate::SortMode::Fixed): at 0 behind every
    /// y-sorted sprite, at `u32::MAX` in front of every one. They are drawn
    /// before the sprites of the same layer and depth, so tiles over every
    /// sprite of a layer go in a higher layer.
    pub depth: u32,
}

impl TileLayerDraw {
    /// The tiles of `tiles`, in layer 0, behind every sprite there.
    pub const fn new(tiles: Handle<TileLayer>) -> Self {
        Self {
            tiles,
            layer: 0,
            depth: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use nadir_core::GenIndex;

    use super::*;

    fn tileset() -> TileSet {
        TileSet {
            texture: Handle::new(GenIndex::new(0, std::num::NonZeroU32::MIN)),
            columns: 8,
            rows: 4,
        }
    }

    /// A layer of `width` × `height` 16-pixel cells at the origin.
    fn layer(width: u32, height: u32) -> TileLayer {
        TileLayer::new(tileset(), UVec2::new(width, height), 16.0, Vec2::ZERO)
    }

    fn revisions(layer: &TileLayer) -> Vec<u64> {
        layer.revisions.clone()
    }

    #[test]
    fn a_layer_is_cut_into_chunks_the_last_ones_short() {
        assert_eq!(layer(64, 48).chunks(), UVec2::new(2, 2));
        assert_eq!(layer(33, 1).chunks(), UVec2::new(2, 1));
        assert_eq!(layer(32, 32).chunks(), UVec2::new(1, 1));
        assert_eq!(layer(0, 10).chunks(), UVec2::new(0, 1));
        assert!(layer(0, 10).revisions.is_empty());
    }

    #[test]
    fn a_change_marks_only_its_own_chunk() {
        let mut layer = layer(64, 48);
        let before = revisions(&layer);
        // The last cell of chunk (0, 0), and the first of chunk (1, 0).
        layer.set(IVec2::new(31, 0), Some(Tile::new(3)));
        let after = revisions(&layer);
        assert_ne!(after[0], before[0]);
        assert_eq!(after[1..], before[1..]);
        layer.set(IVec2::new(32, 0), Some(Tile::new(3)));
        let later = revisions(&layer);
        assert_eq!(later[0], after[0]);
        assert_ne!(later[1], after[1]);
        assert_eq!(later[2..], after[2..]);
        // Row 32 is in the second row of chunks, short of 48.
        layer.set(IVec2::new(63, 47), Some(Tile::new(1)));
        assert_ne!(revisions(&layer)[3], later[3]);
        assert_eq!(layer.get(IVec2::new(63, 47)), Some(Tile::new(1)));
    }

    #[test]
    fn setting_what_is_there_changes_nothing() {
        let mut layer = layer(40, 40);
        layer.set(IVec2::new(5, 5), Some(Tile::new(2)));
        let before = revisions(&layer);
        layer.set(IVec2::new(5, 5), Some(Tile::new(2)));
        layer.set(IVec2::new(6, 5), None);
        assert_eq!(revisions(&layer), before);
        // Emptying a cell is a change.
        layer.set(IVec2::new(5, 5), None);
        assert_ne!(revisions(&layer)[0], before[0]);
        assert_eq!(layer.get(IVec2::new(5, 5)), None);
    }

    #[test]
    fn cells_outside_are_empty() {
        let layer = layer(4, 4);
        for cell in [(-1, 0), (0, -1), (4, 0), (0, 4), (i32::MIN, i32::MAX)] {
            assert_eq!(layer.get(IVec2::new(cell.0, cell.1)), None, "{cell:?}");
        }
    }

    #[test]
    #[should_panic(expected = "outside the 4 × 4 tile layer")]
    fn setting_a_cell_outside_panics() {
        layer(4, 4).set(IVec2::new(4, 0), Some(Tile::new(0)));
    }

    #[test]
    #[should_panic(expected = "finite size above 0")]
    fn tiles_have_a_size() {
        TileLayer::new(tileset(), UVec2::ONE, 0.0, Vec2::ZERO);
    }

    #[test]
    fn no_two_layers_share_a_revision_however_they_were_made() {
        // The same size, filled in the same order with other tiles: a
        // layer put in another's place must not look built.
        let fill = |index: u16| {
            let mut layer = layer(40, 40);
            for y in 0..40 {
                for x in 0..40 {
                    layer.set(IVec2::new(x, y), Some(Tile::new(index)));
                }
            }
            layer
        };
        let (a, b) = (fill(1), fill(2));
        for (a, b) in a.revisions.iter().zip(&b.revisions) {
            assert_ne!(a, b);
        }
        let (c, d) = (layer(40, 40), layer(40, 40));
        assert_ne!(c.revisions, d.revisions);
        // A copy has the same contents, so the same revisions.
        assert_eq!(a.clone().revisions, a.revisions);
        assert!(!a.revisions.contains(&0));
    }

    #[test]
    fn two_layers_changed_in_turn_never_reach_the_same_revision() {
        // Revisions counted per layer, even from a fresh start each, would
        // meet here.
        let (mut a, mut b) = (layer(4, 4), layer(4, 4));
        let mut seen_a = std::collections::HashSet::new();
        let mut seen_b = std::collections::HashSet::new();
        for turn in 0..50_000u16 {
            a.set(IVec2::ZERO, Some(Tile::new(turn % 2)));
            seen_a.insert(a.revisions[0]);
            if turn % 3 != 0 {
                b.set(IVec2::ZERO, Some(Tile::new(turn % 2)));
            }
            seen_b.insert(b.revisions[0]);
        }
        assert!(seen_a.is_disjoint(&seen_b));
    }

    #[test]
    fn a_chunk_packs_its_tiles_row_by_row() {
        let mut layer = layer(40, 34);
        let mut flipped = Tile::animated(9, 4, 120);
        flipped.flip = Flip { x: true, y: true };
        layer.set(IVec2::new(33, 1), Some(flipped));
        layer.set(IVec2::new(32, 0), Some(Tile::new(7)));
        layer.set(IVec2::new(0, 0), Some(Tile::new(1)));
        layer.set(IVec2::new(39, 33), Some(Tile::new(2)));
        let mut out = vec![-1.0];
        assert_eq!(layer.pack_chunk(UVec2::new(1, 0), &mut out), 2);
        assert_eq!(
            out,
            [
                -1.0, // what was there stays
                32.0, 0.0, 7.0, 1.0, 0.0, 0.0, //
                33.0, 1.0, 9.0, 4.0, 120.0, 3.0,
            ]
        );
        out.clear();
        assert_eq!(layer.pack_chunk(UVec2::new(1, 1), &mut out), 1);
        assert_eq!(out, [39.0, 33.0, 2.0, 1.0, 0.0, 0.0]);
        out.clear();
        assert_eq!(layer.pack_chunk(UVec2::new(2, 0), &mut out), 0);
        assert_eq!(layer.pack_chunk(UVec2::new(0, 2), &mut out), 0);
        assert!(out.is_empty());
    }

    #[test]
    fn a_block_of_tiles_is_its_part_of_the_atlas() {
        let set = tileset();
        // Tile 11 of 8 × 4 is column 3, row 1.
        assert_eq!(
            set.block_uv(11, UVec2::ONE),
            Rect::new(Vec2::new(3.0 / 8.0, 0.25), Vec2::new(0.5, 0.5))
        );
        // Two wide, three high, from tile 4.
        assert_eq!(
            set.block_uv(4, UVec2::new(2, 3)),
            Rect::new(Vec2::new(0.5, 0.0), Vec2::new(0.75, 0.75))
        );
        // Past the last tile, it wraps; 0 columns and rows count as 1.
        assert_eq!(set.block_uv(33, UVec2::ONE), set.block_uv(1, UVec2::ONE));
        let single = TileSet {
            columns: 0,
            rows: 0,
            ..set
        };
        assert_eq!(
            single.block_uv(5, UVec2::ONE),
            Rect::new(Vec2::ZERO, Vec2::ONE)
        );
    }

    #[test]
    fn layers_come_and_go_by_handle() {
        let mut layers = TileLayers::new();
        let a = layers.add(layer(2, 2));
        let b = layers.add(layer(3, 3));
        assert_eq!(layers.len(), 2);
        assert_eq!(layers.get(b).map(TileLayer::size), Some(UVec2::new(3, 3)));
        layers
            .get_mut(a)
            .expect("a is there")
            .set(IVec2::ZERO, Some(Tile::new(1)));
        assert!(layers.remove(a).is_some());
        assert!(layers.get(a).is_none());
        // A new layer in a's slot has another handle.
        let c = layers.add(layer(2, 2));
        assert_ne!(a, c);
        assert!(layers.get(a).is_none());
        assert_eq!(format!("{layers:?}"), "TileLayers { len: 2, .. }");
    }
}
