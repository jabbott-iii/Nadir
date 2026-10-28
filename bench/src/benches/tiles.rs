//! Tile benchmarks.

use std::hint::black_box;
use std::num::NonZeroU32;

use nadir_core::{GenIndex, Handle, IVec2, UVec2, Vec2};
use nadir_render::{Flip, TILE_INSTANCE_FLOATS, Texture, Tile, TileLayer, TileSet};

use crate::harness::Bencher;

/// The side of the benchmark layer, in tiles: 2 × 2 chunks.
const SIDE: i32 = 64;

/// `tile_chunk_rebuild`: the renderer's work on the CPU when a tile of a
/// chunk changes, for a full chunk of 32 × 32 tiles, a quarter of them
/// animated and some flipped: packing its instances
/// (`docs/subsystems/render.md`, "Tilemaps"). It leaves out the upload,
/// which needs a GPU.
pub(crate) fn chunk_rebuild(bencher: &mut Bencher) {
    let layer = layer();
    let mut out = Vec::with_capacity(chunk_floats());
    bencher.iter(|| {
        out.clear();
        layer.pack_chunk(black_box(UVec2::ONE), &mut out)
    });
}

/// A chunk's instances, in floats.
fn chunk_floats() -> usize {
    (TileLayer::CHUNK * TileLayer::CHUNK) as usize * TILE_INSTANCE_FLOATS
}

/// A layer of 64 × 64 tiles of 16 pixels, every cell set: the same every
/// time.
fn layer() -> TileLayer {
    let set = TileSet {
        texture: Handle::<Texture>::new(GenIndex::new(1, NonZeroU32::MIN)),
        columns: 8,
        rows: 4,
    };
    let mut layer = TileLayer::new(set, UVec2::splat(SIDE as u32), 16.0, Vec2::ZERO);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let mut tile = if (x + y) % 4 == 0 {
                Tile::animated(8, 4, 250)
            } else {
                Tile::new(((x * 7 + y * 3) % 6) as u16)
            };
            tile.flip = Flip {
                x: x % 5 == 0,
                y: y % 7 == 0,
            };
            layer.set(IVec2::new(x, y), Some(tile));
        }
    }
    layer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chunk_is_full() {
        let mut out = Vec::new();
        assert_eq!(layer().pack_chunk(UVec2::ONE, &mut out), 1024);
        assert_eq!(out.len(), chunk_floats());
        // Its first tile is cell (32, 32), animated.
        assert_eq!(
            out[..TILE_INSTANCE_FLOATS],
            [32.0, 32.0, 8.0, 4.0, 250.0, 0.0]
        );
    }
}
