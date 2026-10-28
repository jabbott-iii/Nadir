//! Tile layers on the GPU: which chunks the view shows, their instance
//! buffers, packed again when their revision changes, and where the layers
//! go among the sprites (ADR-0023).

use std::time::Duration;

use nadir_assets::Handle;
use nadir_core::{Rect, UVec2, Vec2};
use nadir_gfx::{BufferDesc, BufferId, BufferKind, BufferUsage, Device};

use crate::plan::Batch;
use crate::{RenderError, TILE_INSTANCE_FLOATS, TileLayer, TileLayers};

/// The instances a chunk's buffer holds: one for each of its cells.
const CHUNK_INSTANCES: usize = (TileLayer::CHUNK * TileLayer::CHUNK) as usize;

/// The time in whole milliseconds, as the shader reads it: it wraps after
/// 2^32, about 49.7 days.
pub(crate) fn clock_ms(time: Duration) -> u32 {
    // The low 32 bits: the time modulo 2^32.
    time.as_millis() as u32
}

/// The `TileLayer` uniform block of the `tile_chunk` shader, as `u32`s
/// with the floats' bits, so that the clock reaches it exactly:
///
/// - words 0 to 3, `placement`: the top-left corner of cell (0, 0), moved
///   to a whole virtual pixel as sprite corners are (ADR-0018), then the
///   tile size, then 0
/// - words 4 to 7, `atlas`: the columns and rows of the tile set, at least
///   1, then 1 if every tile shows all of the texture, then 0
/// - words 8 to 11, `clock`: the time in milliseconds, then 0s
pub(crate) fn layer_uniform(
    layer: &TileLayer,
    zoom: f32,
    placeholder: bool,
    clock: u32,
) -> [u32; 12] {
    let origin = crate::view::snap(layer.origin() * zoom) * (1.0 / zoom);
    let tileset = layer.tileset();
    [
        origin.x.to_bits(),
        origin.y.to_bits(),
        layer.tile_size().to_bits(),
        0.0f32.to_bits(),
        f32::from(tileset.columns.max(1)).to_bits(),
        f32::from(tileset.rows.max(1)).to_bits(),
        if placeholder { 1.0f32 } else { 0.0 }.to_bits(),
        0.0f32.to_bits(),
        clock,
        0,
        0,
        0,
    ]
}

/// The chunks of `layer` that `view`, a rect of the world, shows, with a
/// tile more all round: an inclusive range of chunk columns and rows, or
/// `None` if it shows none.
///
/// A cell is shown when it overlaps the rect; one that only touches its
/// far edge is not. So a view as wide as a chunk, less two tiles, spans at
/// most two chunks.
pub(crate) fn visible_chunks(layer: &TileLayer, view: Rect) -> Option<(UVec2, UVec2)> {
    let size = layer.size();
    let tile = layer.tile_size();
    let min = (view.min - Vec2::splat(tile) - layer.origin()) / tile;
    let max = (view.max + Vec2::splat(tile) - layer.origin()) / tile;
    // Written so that NaN shows nothing.
    if !(min.x <= max.x && min.y <= max.y) || size.x == 0 || size.y == 0 {
        return None;
    }
    // `as` saturates, so a rect far outside the layer stays outside it.
    let first = min.floor();
    let last = max.ceil() - Vec2::ONE;
    let cells = |first: f32, last: f32, side: u32| {
        let first = (first as i64).max(0);
        let last = (last as i64).min(i64::from(side) - 1);
        (first <= last).then_some((first as u32, last as u32))
    };
    let (x0, x1) = cells(first.x, last.x, size.x)?;
    let (y0, y1) = cells(first.y, last.y, size.y)?;
    let chunk = TileLayer::CHUNK;
    Some((
        UVec2::new(x0 / chunk, y0 / chunk),
        UVec2::new(x1 / chunk, y1 / chunk),
    ))
}

/// One draw of the world pass, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step<K> {
    /// The sprites `start..start + count` of the plan's order, which are
    /// also their instances, with `texture`.
    Sprites { texture: K, start: u32, count: u32 },
    /// The tile layer of this index among the frame's layers to draw.
    Tiles(u32),
}

/// The draws of the world pass, into `out`: the sprite `batches`, with
/// each tile layer of `tiles` before the sprite at its position. `tiles`
/// holds (position in the sprites' order, layer index), in drawing order,
/// so with positions that never go down. A batch that a tile layer goes
/// into is split in two.
pub(crate) fn steps<K: Copy>(batches: &[Batch<K>], tiles: &[(u32, u32)], out: &mut Vec<Step<K>>) {
    out.clear();
    let mut next = 0;
    for batch in batches {
        let mut start = batch.start;
        let end = batch.start + batch.count;
        while let Some(&(at, tiles_index)) = tiles.get(next).filter(|&&(at, _)| at < end) {
            if at > start {
                out.push(Step::Sprites {
                    texture: batch.texture,
                    start,
                    count: at - start,
                });
                start = at;
            }
            out.push(Step::Tiles(tiles_index));
            next += 1;
        }
        if start < end {
            out.push(Step::Sprites {
                texture: batch.texture,
                start,
                count: end - start,
            });
        }
    }
    out.extend(tiles[next..].iter().map(|&(_, index)| Step::Tiles(index)));
}

/// One draw of a chunk: its buffer, and the instances in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChunkDraw {
    pub(crate) buffer: BufferId,
    pub(crate) count: u32,
}

/// What the GPU holds of one chunk.
#[derive(Clone, Copy, Debug, Default)]
struct Built {
    /// Made when the chunk first has a tile, and kept.
    buffer: Option<BufferId>,
    /// The revision packed into it: 0, which no chunk has, before the first.
    revision: u64,
    /// Its instances.
    count: u32,
}

/// What the GPU holds of one layer.
#[derive(Debug)]
struct LayerCache {
    handle: Handle<TileLayer>,
    /// The layer's chunks, columns by rows, when the cache was made.
    chunks: UVec2,
    /// Each chunk, row by row.
    built: Vec<Built>,
}

impl LayerCache {
    fn new(handle: Handle<TileLayer>, chunks: UVec2) -> Self {
        Self {
            handle,
            chunks,
            built: vec![Built::default(); (chunks.x * chunks.y) as usize],
        }
    }

    fn destroy(self, device: &mut Device) {
        for buffer in self.built.into_iter().filter_map(|built| built.buffer) {
            device.destroy(buffer);
        }
    }
}

/// The renderer's tile layers on the GPU: each chunk in a static buffer of
/// its own, packed when the view first shows it and again only after its
/// revision changes.
#[derive(Debug, Default)]
pub(crate) struct TileCache {
    layers: Vec<LayerCache>,
    /// The instances of the chunk being packed, reused.
    packed: Vec<f32>,
    /// The chunks packed so far, for the tests.
    #[cfg(test)]
    packs: usize,
}

impl TileCache {
    /// Frees what the cache holds of the layers no longer in `tiles`.
    pub(crate) fn evict(&mut self, device: &mut Device, tiles: &TileLayers) {
        let mut index = 0;
        while let Some(cache) = self.layers.get(index) {
            if tiles.get(cache.handle).is_some() {
                index += 1;
            } else {
                self.layers.swap_remove(index).destroy(device);
            }
        }
    }

    /// Brings the chunks `range` of `layer`, whose handle is `handle`, up
    /// to date on the GPU, and adds a draw for each that has tiles to
    /// `out`, row by row.
    ///
    /// # Errors
    ///
    /// [`RenderError::Gfx`] if the device refuses a buffer. The chunks
    /// done before stay done.
    pub(crate) fn prepare(
        &mut self,
        device: &mut Device,
        handle: Handle<TileLayer>,
        layer: &TileLayer,
        (first, last): (UVec2, UVec2),
        out: &mut Vec<ChunkDraw>,
    ) -> Result<(), RenderError> {
        let chunks = layer.chunks();
        let index = match self.layers.iter().position(|cache| cache.handle == handle) {
            Some(index) if self.layers[index].chunks == chunks => index,
            // The layer was replaced by one of another size.
            Some(index) => {
                let stale =
                    std::mem::replace(&mut self.layers[index], LayerCache::new(handle, chunks));
                stale.destroy(device);
                index
            }
            None => {
                self.layers.push(LayerCache::new(handle, chunks));
                self.layers.len() - 1
            }
        };
        let cache = &mut self.layers[index];
        for y in first.y..=last.y {
            for x in first.x..=last.x {
                let chunk = UVec2::new(x, y);
                let slot = layer.chunk_index(chunk);
                let revision = layer.revision(slot);
                let built = &mut cache.built[slot];
                if built.revision != revision {
                    self.packed.clear();
                    let count = layer.pack_chunk(chunk, &mut self.packed);
                    #[cfg(test)]
                    {
                        self.packs += 1;
                    }
                    if count > 0 {
                        let buffer = match built.buffer {
                            Some(buffer) => buffer,
                            None => {
                                let buffer = chunk_buffer(device)?;
                                built.buffer = Some(buffer);
                                buffer
                            }
                        };
                        device.write_buffer(buffer, 0, &self.packed)?;
                    }
                    built.revision = revision;
                    built.count = count;
                }
                if let (Some(buffer), count @ 1..) = (built.buffer, built.count) {
                    out.push(ChunkDraw { buffer, count });
                }
            }
        }
        Ok(())
    }
}

fn chunk_buffer(device: &mut Device) -> Result<BufferId, RenderError> {
    Ok(device.create_buffer(&BufferDesc {
        label: "tile chunk instances",
        kind: BufferKind::Vertex,
        // Kept from frame to frame, written again only when a tile changes.
        usage: BufferUsage::Static,
        size: CHUNK_INSTANCES * TILE_INSTANCE_FLOATS * size_of::<f32>(),
    })?)
}

#[cfg(test)]
mod tests {
    use nadir_core::{GenIndex, IVec2};

    use super::*;
    use crate::view::View;
    use crate::{Camera2D, Tile, TileSet};

    fn tileset() -> TileSet {
        TileSet {
            texture: Handle::new(GenIndex::new(0, std::num::NonZeroU32::MIN)),
            columns: 8,
            rows: 4,
        }
    }

    fn layer(size: (u32, u32), tile: f32, origin: (f32, f32)) -> TileLayer {
        TileLayer::new(
            tileset(),
            UVec2::new(size.0, size.1),
            tile,
            Vec2::new(origin.0, origin.1),
        )
    }

    fn rect(min: (f32, f32), max: (f32, f32)) -> Rect {
        Rect::new(Vec2::new(min.0, min.1), Vec2::new(max.0, max.1))
    }

    fn chunks(first: (u32, u32), last: (u32, u32)) -> Option<(UVec2, UVec2)> {
        Some((UVec2::new(first.0, first.1), UVec2::new(last.0, last.1)))
    }

    #[test]
    fn the_clock_is_whole_milliseconds_that_wrap() {
        assert_eq!(clock_ms(Duration::from_micros(350_999)), 350);
        let wrap = Duration::from_millis(1 << 32);
        assert_eq!(clock_ms(wrap + Duration::from_millis(350)), 350);
        assert_eq!(clock_ms(wrap - Duration::from_millis(1)), u32::MAX);
        assert_eq!(clock_ms(wrap), 0);
    }

    #[test]
    fn the_layer_uniform_is_placement_atlas_and_clock() {
        let mut tiles = layer((4, 4), 16.0, (10.25, -3.5));
        let words = layer_uniform(&tiles, 1.0, false, 123_456);
        let floats: Vec<f32> = words[..8].iter().map(|&w| f32::from_bits(w)).collect();
        // The corner moves to the nearest whole pixel, halves up.
        assert_eq!(floats, [10.0, -3.0, 16.0, 0.0, 8.0, 4.0, 0.0, 0.0]);
        assert_eq!(words[8..], [123_456, 0, 0, 0]);
        // At zoom 2, a world unit is two pixels: 10.25 is 20.5 pixels.
        let words = layer_uniform(&tiles, 2.0, true, u32::MAX);
        assert_eq!(f32::from_bits(words[0]), 10.5);
        assert_eq!(f32::from_bits(words[6]), 1.0, "the placeholder");
        assert_eq!(words[8], u32::MAX);
        // A tile set of no columns or rows counts as one.
        tiles = TileLayer::new(
            TileSet {
                columns: 0,
                rows: 0,
                ..tileset()
            },
            UVec2::ONE,
            8.0,
            Vec2::ZERO,
        );
        let words = layer_uniform(&tiles, 1.0, false, 0);
        assert_eq!([words[4], words[5]], [1.0f32.to_bits(); 2]);
    }

    #[test]
    fn a_view_shows_the_chunks_it_overlaps_with_a_tile_more() {
        // 100 × 70 cells of 16 pixels at the origin: chunks 0..=3 by 0..=2,
        // the last of each short.
        let tiles = layer((100, 70), 16.0, (0.0, 0.0));
        // Cells 0 to 29 by 0 to 14, and a tile more: chunk 0.
        assert_eq!(
            visible_chunks(&tiles, rect((0.0, 0.0), (480.0, 240.0))),
            chunks((0, 0), (0, 0))
        );
        // Cells 0 to 31 across, and a tile more: cell 32, the first of
        // chunk 1.
        assert_eq!(
            visible_chunks(&tiles, rect((0.0, 0.0), (500.0, 240.0))),
            chunks((0, 0), (1, 0))
        );
        // Ending one tile short of chunk 1, the margin reaches its edge
        // only: cells overlap the rect, they do not just touch it.
        assert_eq!(
            visible_chunks(&tiles, rect((16.0, 16.0), (496.0, 256.0))),
            chunks((0, 0), (0, 0))
        );
        // Starting half a tile into chunk 1: the margin reaches back into
        // chunk 0.
        assert_eq!(
            visible_chunks(&tiles, rect((520.0, 520.0), (600.0, 600.0))),
            chunks((0, 0), (1, 1))
        );
        // Past the layer: clamped to its last chunks.
        assert_eq!(
            visible_chunks(&tiles, rect((1000.0, 700.0), (5000.0, 5000.0))),
            chunks((1, 1), (3, 2))
        );
        // Far left and above, short of the margin: nothing.
        assert_eq!(
            visible_chunks(&tiles, rect((-500.0, -500.0), (-16.0, -16.0))),
            None
        );
        // Within a tile of the corner: the margin reaches cell (0, 0).
        assert_eq!(
            visible_chunks(&tiles, rect((-500.0, -500.0), (-15.0, -15.0))),
            chunks((0, 0), (0, 0))
        );
        // Below and right of the layer: nothing.
        assert_eq!(
            visible_chunks(&tiles, rect((1617.0, 0.0), (2000.0, 100.0))),
            None
        );
        assert_eq!(
            visible_chunks(&tiles, rect((f32::NAN, 0.0), (10.0, 10.0))),
            None
        );
        assert_eq!(
            visible_chunks(&tiles, rect((f32::NEG_INFINITY, 0.0), (f32::INFINITY, 1.0))),
            chunks((0, 0), (3, 0))
        );
    }

    #[test]
    fn a_layer_anywhere_is_culled_from_its_origin() {
        // Its corner at (-512, -40): the world's origin is in cell (32, 2.5),
        // so chunk 1 across.
        let tiles = layer((64, 64), 16.0, (-512.0, -40.0));
        assert_eq!(
            visible_chunks(&tiles, rect((32.0, 0.0), (64.0, 8.0))),
            chunks((1, 0), (1, 0))
        );
        // At 1-pixel tiles, a chunk is 32 pixels.
        let tiny = layer((100, 100), 1.0, (0.0, 0.0));
        assert_eq!(
            visible_chunks(&tiny, rect((40.0, 40.0), (70.0, 50.0))),
            chunks((1, 1), (2, 1))
        );
    }

    #[test]
    fn an_empty_layer_shows_nothing() {
        for size in [(0, 10), (10, 0), (0, 0)] {
            let tiles = layer(size, 16.0, (0.0, 0.0));
            assert_eq!(
                visible_chunks(&tiles, rect((-100.0, -100.0), (100.0, 100.0))),
                None,
                "{size:?}"
            );
        }
    }

    #[test]
    fn a_256_by_256_level_at_zoom_1_draws_at_most_4_chunks() {
        // render.md's budget: a level of 256 × 256 tiles in view at zoom 1
        // is at most 30 draws. With 16-pixel tiles, a 480 × 270 view and a
        // tile more all round span at most 2 × 2 chunks.
        let tiles = layer((256, 256), 16.0, (0.0, 0.0));
        let mut most = 0;
        for step in 0..400 {
            let position = Vec2::new(step as f32 * 10.25, step as f32 * 7.5);
            for shake in [Vec2::ZERO, Vec2::new(2.0, -3.0)] {
                let camera = Camera2D {
                    position,
                    zoom: 1.0,
                    shake,
                };
                let view = View::new(&camera, UVec2::new(480, 270));
                let (first, last) =
                    visible_chunks(&tiles, view.world_rect()).expect("over the level");
                let count = (last.x - first.x + 1) * (last.y - first.y + 1);
                most = most.max(count);
            }
        }
        assert_eq!(most, 4);
    }

    fn batch(texture: u32, start: u32, count: u32) -> Batch<u32> {
        Batch {
            texture,
            start,
            count,
        }
    }

    fn plan(batches: &[Batch<u32>], tiles: &[(u32, u32)]) -> Vec<Step<u32>> {
        let mut out = vec![Step::Tiles(99)];
        steps(batches, tiles, &mut out);
        out
    }

    #[test]
    fn tile_layers_go_between_the_sprites_and_split_a_batch() {
        let batches = [batch(7, 0, 3), batch(8, 3, 2)];
        let sprites = |texture, start, count| Step::Sprites {
            texture,
            start,
            count,
        };
        // No tiles: the batches as they are.
        assert_eq!(plan(&batches, &[]), [sprites(7, 0, 3), sprites(8, 3, 2)]);
        // Before every sprite, in the middle of the first batch, at a batch's
        // edge, and after every sprite.
        assert_eq!(
            plan(&batches, &[(0, 0), (1, 1), (1, 2), (3, 3), (5, 4)]),
            [
                Step::Tiles(0),
                sprites(7, 0, 1),
                Step::Tiles(1),
                Step::Tiles(2),
                sprites(7, 1, 2),
                Step::Tiles(3),
                sprites(8, 3, 2),
                Step::Tiles(4),
            ]
        );
        // With no sprites at all.
        assert_eq!(
            plan(&[], &[(0, 1), (0, 0)]),
            [Step::Tiles(1), Step::Tiles(0)]
        );
    }

    #[test]
    fn a_tile_at_the_last_sprite_splits_off_only_that_sprite() {
        let batches = [batch(1, 0, 4)];
        assert_eq!(
            plan(&batches, &[(3, 0)]),
            [
                Step::Sprites {
                    texture: 1,
                    start: 0,
                    count: 3
                },
                Step::Tiles(0),
                Step::Sprites {
                    texture: 1,
                    start: 3,
                    count: 1
                },
            ]
        );
    }

    #[test]
    fn a_full_chunk_packs_into_its_buffer() {
        // The buffer holds one instance for each cell of a chunk.
        let mut tiles = layer((32, 32), 16.0, (0.0, 0.0));
        for y in 0..32 {
            for x in 0..32 {
                tiles.set(IVec2::new(x, y), Some(Tile::new(1)));
            }
        }
        let mut out = Vec::new();
        let count = tiles.pack_chunk(UVec2::ZERO, &mut out);
        assert_eq!(count as usize, CHUNK_INSTANCES);
        assert_eq!(out.len(), CHUNK_INSTANCES * TILE_INSTANCE_FLOATS);
    }
}

/// The cache on a GPU: what it packs, keeps and frees.
#[cfg(all(test, feature = "headless-test"))]
mod gpu_tests {
    use nadir_core::{GenIndex, IVec2};
    use nadir_gfx::{GfxError, ResourceKind};

    use super::*;
    use crate::{Tile, TileSet};

    /// A device on an offscreen context, or `None` where this system has
    /// none yet and no renderer is asked for, as the headless tests do.
    fn device() -> Option<Device> {
        let expected = std::env::var("NADIR_EXPECT_GL_RENDERER").unwrap_or_default();
        match Device::new_headless() {
            Ok(device) => {
                let renderer = &device.caps().renderer;
                assert!(
                    renderer.contains(&expected),
                    "the renderer is `{renderer}`, not `{expected}`"
                );
                Some(device)
            }
            Err(GfxError::NoOffscreenContext) if expected.is_empty() => None,
            Err(error) => panic!("no headless device: {error}"),
        }
    }

    /// A layer of `side` × `side` cells, each showing `tile`.
    fn full(side: u32, tile: u16) -> TileLayer {
        let set = TileSet {
            texture: Handle::new(GenIndex::new(0, std::num::NonZeroU32::MIN)),
            columns: 4,
            rows: 1,
        };
        let mut layer = TileLayer::new(set, UVec2::splat(side), 1.0, Vec2::ZERO);
        for y in 0..side as i32 {
            for x in 0..side as i32 {
                layer.set(IVec2::new(x, y), Some(Tile::new(tile)));
            }
        }
        layer
    }

    fn live(device: &mut Device, buffer: BufferId) -> bool {
        match device.write_buffer(buffer, 0, &[0.0f32]) {
            Ok(()) => true,
            Err(GfxError::StaleHandle(ResourceKind::Buffer)) => false,
            Err(error) => panic!("{error}"),
        }
    }

    const ALL: (UVec2, UVec2) = (UVec2::ZERO, UVec2::ONE);

    #[test]
    fn a_chunk_is_packed_once_and_again_only_after_it_changes() {
        let Some(mut device) = device() else { return };
        let mut tiles = TileLayers::new();
        let handle = tiles.add(full(64, 1));
        let mut cache = TileCache::default();
        let mut draws = Vec::new();
        let layer = tiles.get(handle).expect("the layer");
        cache
            .prepare(&mut device, handle, layer, ALL, &mut draws)
            .expect("prepared");
        assert_eq!((cache.packs, draws.len()), (4, 4));
        let first = draws.clone();

        // Unchanged: nothing packed, the same buffers drawn.
        draws.clear();
        cache
            .prepare(&mut device, handle, layer, ALL, &mut draws)
            .expect("prepared");
        assert_eq!((cache.packs, &draws), (4, &first));

        // A tile of chunk (1, 0) changes: that chunk alone, into its buffer.
        tiles
            .get_mut(handle)
            .expect("the layer")
            .set(IVec2::new(40, 3), Some(Tile::new(2)));
        let layer = tiles.get(handle).expect("the layer");
        draws.clear();
        cache
            .prepare(&mut device, handle, layer, ALL, &mut draws)
            .expect("prepared");
        assert_eq!((cache.packs, &draws), (5, &first));

        // A chunk emptied keeps its buffer, and is not drawn.
        let layer = tiles.get_mut(handle).expect("the layer");
        for y in 0..32 {
            for x in 0..32 {
                layer.set(IVec2::new(x, y), None);
            }
        }
        let layer = tiles.get(handle).expect("the layer");
        draws.clear();
        cache
            .prepare(&mut device, handle, layer, ALL, &mut draws)
            .expect("prepared");
        assert_eq!((cache.packs, &draws[..]), (6, &first[1..]));
        assert!(live(&mut device, first[0].buffer));
    }

    #[test]
    fn the_chunks_of_a_layer_gone_or_resized_are_freed() {
        let Some(mut device) = device() else { return };
        let mut tiles = TileLayers::new();
        let gone = tiles.add(full(40, 1));
        let resized = tiles.add(full(40, 1));
        let mut cache = TileCache::default();
        let mut draws = Vec::new();
        for handle in [gone, resized] {
            let layer = tiles.get(handle).expect("the layer");
            cache
                .prepare(&mut device, handle, layer, ALL, &mut draws)
                .expect("prepared");
        }
        assert_eq!(draws.len(), 8);
        let buffers: Vec<BufferId> = draws.iter().map(|draw| draw.buffer).collect();
        assert!(buffers.iter().all(|&buffer| live(&mut device, buffer)));

        // Gone from the store: freed at the next evict, the other kept.
        tiles.remove(gone);
        cache.evict(&mut device, &tiles);
        assert_eq!(cache.layers.len(), 1);
        assert!(
            buffers[..4]
                .iter()
                .all(|&buffer| !live(&mut device, buffer))
        );
        assert!(buffers[4..].iter().all(|&buffer| live(&mut device, buffer)));

        // Replaced in place by a layer of one chunk: the old chunks freed.
        *tiles.get_mut(resized).expect("the layer") = full(10, 2);
        let layer = tiles.get(resized).expect("the layer");
        draws.clear();
        let one = (UVec2::ZERO, UVec2::ZERO);
        cache
            .prepare(&mut device, resized, layer, one, &mut draws)
            .expect("prepared");
        assert_eq!(draws.len(), 1);
        assert_eq!(draws[0].count, 100);
        assert!(
            buffers[4..]
                .iter()
                .all(|&buffer| !live(&mut device, buffer))
        );
        assert!(live(&mut device, draws[0].buffer));
    }
}
