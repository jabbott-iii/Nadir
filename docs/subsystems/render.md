# Subsystem: `nadir-render`

The 2D renderer, built on `nadir-gfx`. It is ECS-agnostic: it consumes a
`RenderFrame` and knows nothing about entities.

## Responsibilities

- Sprite batching with instancing; sort keys including y-sort.
- Tilemap chunks: build, cull, animate.
- The camera; virtual resolution with integer upscaling.
- Bitmap text: layout and rendering.
- Debug draw: lines, rects, circles.
- A fixed pass list: world → (lights, later) → post → upscale → UI → debug.
- The texture, atlas and font asset types, and their loaders, registered with
  `nadir-assets`.
  - `Texture` (M3.2) is a QOI file uploaded by `finalize`, which needs the
    `gfx::Device` in the `FinalizeCtx`. An sRGB QOI becomes `Rgba8Srgb`,
    and a linear one `Rgba8`.
  - Its placeholder is an 8×8 checker of single texels, magenta and black,
    with magenta in the top-left corner.
  - Its GPU tests use `Device::new_headless`, behind render's
    `headless-test` feature.

## Built so far

- M3.2: the `Texture` asset.
- M3.3: the `Renderer` for sprites, in pixel-perfect mode:
  - the `sprite` and `blit` programs, with a GLSL parity test
  - instance packing, sort keys and batching
  - `Camera2D`, the integer upscale with its letterbox, and
    `screen_to_world` and `world_to_screen`
  - golden scenes: `y_sort`, and `upscale_320x180`, `upscale_300x200` and
    `upscale_333x147`
- M4.5: tile layers (ADR-0023), in "Tilemaps" below:
  - `TileSet`, `Tile`, `TileLayer`, the `TileLayers` store and
    `TileLayerDraw`
  - the `tile_chunk` program, the chunk cache, culling, animated and
    flipped tiles, and tile layers ordered among the sprites
  - golden scenes: `tile_layers`, `animated_tiles_0ms` and
    `animated_tiles_350ms`
- Not built yet:
  - text, UI sprites, debug draw and post
  - the smooth-camera and native-resolution modes
  - shader hot reload (M5.8), and the Metal sources (M10.3)

```rust
impl Renderer {
    pub fn new(device: &mut Device, config: &RenderConfig) -> Result<Self, RenderError>;
    /// Inside a device frame: the world pass into the virtual image, then the
    /// blit into `output` (the window, or a render target; Rgba8Srgb).
    pub fn render(&mut self, device: &mut Device, assets: &AssetServer, tiles: &TileLayers,
                  frame: &RenderFrame, output: Output) -> Result<FrameStats, RenderError>;
    pub fn upscale(&self, output_size: UVec2) -> Upscale;          // factor + where the image lands
    pub fn screen_to_world(&self, camera: &Camera2D, output_size: UVec2, screen: Vec2) -> Vec2;
    pub fn world_to_screen(&self, camera: &Camera2D, output_size: UVec2, world: Vec2) -> Vec2;
}
pub struct Output { pub target: PassTarget, pub size: UVec2 }      // the caller knows the window's size
pub struct FrameStats { pub sprites: u32, pub skipped: u32, pub batches: u32,
                        pub tiles: u32, pub chunks: u32 }
```

- Textures are looked up in the `AssetServer`.
  - A sprite whose texture is still loading is not drawn (`skipped`).
  - One whose texture failed draws all of its placeholder, uv 0 to 1,
    whatever its `uv`, when the server shows placeholders. Without them
    (`OnFailure::Nothing`) it is skipped too.
- `RenderFrame` so far has `camera`, `clear`, `sprites`, and since M4.5
  `tile_layers` and `time`. The other fields below come with their tasks.
  `Camera2D` has no rotation yet.
- `FrameStats.batches` counts the sprite draws made, a batch split by a
  tile layer counting twice. `tiles` and `chunks` count the tiles and the
  chunk draws. `SpriteBatcher::stats` knows no tile layers: its `batches`
  is before any split, and its `tiles` and `chunks` are 0.

## Input: `RenderFrame`

The facade's extract stage fills a `RenderFrame` every frame. All of its
`Vec`s are cleared and reused, never reallocated.

```rust
pub struct RenderFrame {
    pub camera: Camera2D,                 // position, zoom, rotation (0 for now), shake offset
    pub clear: Color,
    pub sprites: Vec<SpriteDraw>,         // world-space sprites
    pub tile_layers: Vec<TileLayerDraw>,  // layers in the TileLayers store + layer/depth info
    pub ui_sprites: Vec<SpriteDraw>,      // screen-space
    pub texts: Vec<TextDraw>,             // world or screen space
    pub debug: DebugDrawList,             // lines/rects/circles, cleared per frame
    pub post: PostSettings,               // fade, tint, color grade LUT handle (optional)
    pub time: Duration,                   // for animated tiles/shaders (and the facade's clips, ADR-0024); whole ms reach the shader
}

pub struct SpriteDraw {
    pub texture: Handle<Texture>,         // usually an atlas page
    pub uv: Rect,                         // normalized atlas region
    pub position: Vec2,                   // world position of the pivot (interpolated)
    pub size: Vec2,
    pub pivot: Vec2,                      // normalized; top-down default (0.5, 1.0) = feet
    pub rotation: f32,
    pub color: Color,
    pub layer: u8,
    pub sort: SortMode,                   // YSort | Fixed(u32)
    pub flip: Flip,
}
```

## Sort keys

Every world sprite gets a `u64` key, and the keys are sorted with
`sort_unstable_by_key`. Stability comes from the low bits.

```
 63        56 55                              24 23              8 7       0
┌───────────┬──────────────────────────────────┬─────────────────┬─────────┐
│ layer u8  │ depth u32                        │ texture id u16  │ seq u8* │
└───────────┴──────────────────────────────────┴─────────────────┴─────────┘
```

- **YSort:**
  - `depth = sortable_bits(position.y)`, where position is the pivot (feet).
  - `sortable_bits(f) = if f.to_bits() >> 31 == 1 { !f.to_bits() } else { f.to_bits() | 0x8000_0000 }`.
    This makes the unsigned key order match the float order, negatives
    included.
- **Fixed(n):** `depth = n`. Used for ground decals, UI-in-world and similar.
- **seq:** a submission counter modulo 256. It's a tiebreaker for sprites
  with equal depth, which avoids flicker between frames. Only the low 8 bits
  are used; that's acceptable because exact ties are rare and order within a
  tie is cosmetic.
  - As built, the renderer sorts `(key >> 8, submission index)` pairs, so
    the full index breaks ties instead of `seq`.
  - Ties then keep their submission order exactly, even when sprites are
    added before them: sprites submitted at 255 and 256 have `seq` 255 and
    0.
  - Exact ties are common, since every `Fixed(n)` sprite of a layer
    shares a depth.
- `Fixed(0)` is behind, and `Fixed(u32::MAX)` in front of, every y-sorted
  sprite of its layer. In-world UI goes in a higher layer.
- **Batching:**
  - Consecutive sprites with the same texture and pipeline go into one
    instanced draw.
  - Y-sorting interleaves textures, so **atlasing is what keeps draw
    counts low.** The cook tool packs all sprites for a level into as few
    atlas pages as possible.

## Instance data

`nadir-render` denies `unsafe`, so it cannot implement `Pod` for its own
structs. Instead, instances are packed into a reused `Vec<f32>`, which `gfx`
uploads (`f32: Pod` is implemented inside `gfx`). This is fully safe, and with
`extend_from_slice` it costs the same as a struct copy.

```rust
/// Logical layout of one sprite instance in the instance buffer (15 × f32 = 60 bytes).
pub const SPRITE_INSTANCE_FLOATS: usize = 15;

fn push_instance(out: &mut Vec<f32>, s: &SpriteDraw, uv: [f32; 4], color: [f32; 4]) {
    out.extend_from_slice(&[
        s.position.x, s.position.y,      // location 0: pos (pivot, world)
        s.size.x, s.size.y,              // location 1: size
        s.pivot.x, s.pivot.y,            // location 2: pivot (normalized)
        uv[0], uv[1], uv[2], uv[3],      // location 3: uv rect (flip applied by swapping)
        color[0], color[1], color[2], color[3], // location 4: linear color
        s.rotation,                      // location 5: rotation (radians, clockwise)
    ]);
}
```

- The vertex layout in the `sprite` pipeline and the shader interface
  constant must match this order. The parity test checks it.
- The unit quad's corner, 0 to 1 from the sprite's top left, is location 6,
  in a second vertex slot.
- The blit draws a quad whose clip-space corners and uv come from Rust
  (`BLIT_QUAD`), as the shaders rule asks: GL's bottom-up render targets
  are accounted for there.
- In pixel-perfect mode, an unrotated sprite's position is packed with its
  top-left corner moved to the nearest whole virtual pixel (ADR-0018).
- One dynamic instance buffer, sized to the frame's peak. It grows by
  doubling and never shrinks during a session.
- The unit quad is a static 4-vertex triangle strip.

## Tilemaps

- The layer data (tile indices per cell) is owned by the facade/scene. The
  renderer owns `TileChunkSet` GPU caches keyed by layer ID.
- **Chunk:** 32×32 tiles, one instance buffer of tile instances (cell, atlas
  tile index, flags). Built on load; rebuilt only when a tile in the chunk
  changes (dirty flag).
- **Culling:** draw only the chunks that intersect the camera rect plus a
  one-tile margin.
- **Animated tiles:** each instance carries `anim_frames` and `anim_ms`. The
  shader picks the frame from `RenderFrame.time`, so animation needs no CPU
  rebuild.
- **Tall tiles** (trees, wall tops): the level importer emits them as
  y-sorted sprites, not chunk tiles. Otherwise they can't interleave with
  characters.

As built (M4.5, ADR-0023):

```rust
pub struct TileSet { pub texture: Handle<Texture>, pub columns: u16, pub rows: u16 }
impl TileSet { pub fn block_uv(&self, index: u16, size: UVec2) -> Rect; }   // a tall tile's region
pub struct Tile { pub index: u16, pub frames: u16, pub frame_ms: u16, pub flip: Flip }
impl TileLayer {
    pub const CHUNK: u32 = 32;
    pub fn new(tileset: TileSet, size: UVec2, tile_size: f32, origin: Vec2) -> Self;
    pub fn get(&self, cell: IVec2) -> Option<Tile>;
    pub fn set(&mut self, cell: IVec2, tile: Option<Tile>);           // panics outside
    pub fn pack_chunk(&self, chunk: UVec2, out: &mut Vec<f32>) -> u32; // public for the bench
}
impl TileLayers { pub fn add(&mut self, layer: TileLayer) -> Handle<TileLayer>; get, get_mut, remove }
pub struct TileLayerDraw { pub tiles: Handle<TileLayer>, pub layer: u8, pub depth: u32 }
```

- **The data:** a `TileLayer` is a grid of square cells, each empty or a
  `Tile` of its `TileSet`, placed as a `TileGrid` is. The game keeps its
  layers in a `TileLayers` store; the facade's is a world resource. The
  renderer is given the store beside each frame, whose `TileLayerDraw`s
  name the layers to draw.
- **The tile set** is a whole texture cut into `columns` × `rows` equal
  tiles, numbered row by row. 0 counts as 1, and an index past the last
  tile wraps to the first. In pixel-perfect mode the layer's origin is
  moved to a whole virtual pixel, as sprite corners are. Its texels land
  on whole pixels when the tile size times the zoom is a whole multiple
  of a tile's texels.
- **Revisions, not dirty flags:**
  - Each chunk has a revision from one counter for the whole process. A
    new layer takes a fresh one, and each `set` that changes a cell gives
    its chunk a fresh one.
  - The renderer packs a chunk the first time the view shows it, and again
    only when its revision differs from the one it packed.
  - A layer replaced in place, or a store replaced whose handles come
    again, is never mistaken for one already packed. Frames that are not
    drawn need nothing.
- **The cache:** each chunk with tiles gets a `Static` buffer of 1,024
  instances the first time it has some. A layer's buffers are freed at the
  first frame after it leaves the store. A layer with no cells, or none in
  view, makes no buffer and no draw.
- **An instance** is `TILE_INSTANCE_FLOATS` = 6 `f32`s: the cell's column
  and row, then the tile's index, frames, frame length in ms, and flip
  bits (1: x, 2: y).
- **The `tile_chunk` program:**
  - Its `TileLayer` uniform block holds the snapped origin and the tile
    size, the tile set's columns and rows and a placeholder flag, and the
    time in whole milliseconds as a `uint`. Rust packs it as `u32`s, so
    the clock arrives bit-exact.
  - The vertex shader picks the frame, `(ms / frame_ms) % frames`, and the
    tile in `uint`, and flips the tile's corner.
  - The block is set once per layer, and each visible non-empty chunk is
    one instanced draw.
- **Culling:** the view's world rect (shake included), a tile wider all
  round, to the chunks whose cells overlap it. A cell that only touches
  its far edge does not count. A 480 × 270 view of 16-pixel tiles at zoom
  1 spans at most 2 × 2 chunks per layer.
- **Order:**
  - A tile layer sorts as a sprite at `SortMode::Fixed(depth)` in its
    layer, and before the sprites of the same layer and depth. So ground
    decals at `Fixed(0)` lie on ground tiles at depth 0, and tiles over
    every sprite of a layer go in a higher layer.
  - It goes into the sorted sprites at its place, splitting a batch it
    falls inside, and the sprite program, buffers and texture are bound
    again after it.
- **Textures:** a layer whose tile set's texture is loading is not drawn.
  A failed one shows all of its placeholder in every tile, when the server
  shows placeholders; otherwise nothing.
- **Time:** `RenderFrame.time` is a `Duration`, not the `f32` seconds
  first planned, which lose millisecond precision after about 4.6 hours.
  The facade sets it each frame from the simulation clock, ticks and
  `alpha`. The shader gets whole milliseconds, which wrap after 2^32,
  about 49.7 days.
- **Tall tiles:** a sprite whose `uv` is `TileSet::block_uv(index,
  size)`, y-sorted by its feet at the bottom of its cell. The sandbox
  spawns its tall walls as entities with a `Sprite`.

## Camera and resolution

- `Camera2D { position, zoom: f32, shake: Vec2 }`. The view matrix maps the
  world (+Y down) to clip space.
- **Pixel-perfect mode** (default when `virtual_resolution` is set):
  1. Render the world into an RT of exactly `virtual_resolution`.
  2. Upscale to the window by the largest integer factor that fits, with
     letterbox bars in the clear color.
  3. Snap the camera position to whole virtual pixels. As built
     (ADR-0018):
     - The view's top-left corner snaps, not its centre, so that an odd
       virtual size stays on the pixel grid.
     - Halves round up, `floor(x + 0.5)`, so each pixel the camera moves
       moves the view by one.
     - Unrotated sprites snap their corners too.
     - The zoom is rounded to a whole number, at least 1.
     - The factor is the largest whole number that fits both sides, at
       least 1. The image is centred, with the odd pixel of a bar on the
       right or at the bottom, and cropped when the output is smaller than
       the virtual resolution.
  4. Optional smooth-camera mode: render with a one-pixel border and offset
     the upscale blit by the sub-pixel remainder, so slow pans don't stair-step.
- **Smooth mode:** render at native resolution, with zoom as a float.
- `screen_to_world` and `world_to_screen` account for the letterbox and
  scale, for mouse aiming.
- UI renders after the upscale, at native resolution. It has two choices:
  - a pixel-UI scale (an integer multiple of virtual pixels), or
  - a free scale from the settings UI-scale value.

## Text

- The font asset is a BMFont (text format), cooked into our font asset:
  glyph metrics, kerning pairs and atlas pages.
- Layout:
  - left/center/right alignment
  - wrap at a max width (word boundaries, falling back to character
    boundaries)
  - `\n` line breaks
  - per-run color via a tiny markup: `{c:#ff0}text{/c}`
- A missing glyph renders the font's `?` and logs once per character.
- Glyph quads go through the `text` pipeline (alpha mask × tint). For pixel
  fonts, the font texture can also be RGBA and use the sprite pipeline.

## Debug draw

- `DebugDrawList`: `line(a, b, color)`, `rect(r, color)`, `circle(c, r, color, segments)`,
  `text(pos, str)`.
- Compiled in every build, but only the facade's `dev` systems fill it. In
  release it stays empty and costs one branch.

## Performance targets

Measured on min-spec; see `docs/00-overview.md`.

- 10k sprites: sort + batch + upload ≤ 2 ms CPU; ≤ 30 draw calls with the
  sandbox atlases.
  - The CPU part before the upload is `SpriteBatcher`, public so that
    `nadir-bench` can time it without a GPU (`sprite_sort_batch`): the sort
    keys, the sort, the batches and the packed instances. The `Renderer`
    holds one; it adds the asset lookups and the upload.
- Tilemap: a 256×256 level visible at zoom 1 adds ≤ 1 ms CPU and ≤ 30 draw
  calls.
  - As built (M4.5), the draws hold by construction for 16-pixel tiles: at
    most 4 chunks a layer (a unit test sweeps the camera over such a
    level). The CPU work is the cull, O(chunks in view), and packing a
    chunk that changed: `tile_chunk_rebuild`, 3.3 µs for a full chunk on
    the i7-11800H.
- Zero heap allocations per frame in steady state.

## Tests

- **Unit:**
  - sort-key monotonicity (property test over random floats, including
    negatives and `-0.0`)
  - batching boundaries
  - chunk dirty tracking
  - text layout (wrapping, alignment, kerning)
  - `screen_to_world` round-trip
- **Golden:**
  - y-sort scene (character walking behind and in front of a tree)
  - tilemap with animated tiles at fixed `time` values
  - text alignment
  - pixel-perfect upscale at 3 window sizes
  - debug draw primitives
- **Benchmarks:** sort+batch for 10k sprites; chunk rebuild
  (`tile_chunk_rebuild`, M4.5).
- As built (M4.5), the unit tests cover the chunks' revisions, packing,
  culling, the uniform's words, the clock's wrap and the order of tile
  layers among sprites. The headless tests draw placement, a change, a
  replaced layer or store, empty, unknown and removed layers, texture
  states, culling, order and batch splits, animation and flips.
