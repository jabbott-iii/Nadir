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
- M5.9: the `Atlas` asset, a cooked atlas's regions (ADR-0033), below
- M5.5: the atlas's clips, and flips about the pivot (ADR-0035), below
- M5.17: a program rebuilt from new sources, and a texture reloaded behind
  its handle drawn (ADR-0042), in "Reloading" below
- Not built yet:
  - text, UI sprites, debug draw and post
  - the smooth-camera and native-resolution modes
  - the Metal sources (M10.3)

```rust
impl Renderer {
    pub fn new(device: &mut Device, config: &RenderConfig) -> Result<Self, RenderError>;
    /// Inside a device frame: the world pass into the virtual image, then the
    /// blit into `output` (the window, or a render target; Rgba8Srgb).
    pub fn render(&mut self, device: &mut Device, assets: &AssetServer, tiles: &TileLayers,
                  frame: &RenderFrame, output: Output) -> Result<FrameStats, RenderError>;
    pub fn reload_program(&mut self, device: &mut Device, program: Program,
                          source: ShaderSource<'_>) -> Result<(), RenderError>;  // "Reloading"
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
- **`Atlas`** (M5.9, ADR-0033; clips in M5.5, ADR-0035): the asset of a
  cooked atlas file, `atlas/<group>.kbj`, version 2, read with
  `nadir_codec::atlas::parse`
  (`docs/subsystems/assets-and-codecs.md`). It holds data and no GPU
  resource, so `finalize` needs nothing in its context, and a game makes
  it before its window opens with `AssetServer::load_now`. Its page paths
  must be asset paths ending `.qoi`, as written, lowercase. It has no
  placeholder.

  ```rust
  impl Atlas {
      pub fn pages(&self) -> &[AtlasPage];                       // path(), size()
      pub fn page(&self, frame: &AtlasFrame) -> Option<&AtlasPage>;
      pub fn sprite(&self, name: &str) -> Option<&AtlasSprite>;  // by name, binary search
      pub fn sprites(&self) -> &[AtlasSprite];
      pub fn load_pages(&self, server: &mut AssetServer, scope: Scope) -> Vec<Handle<Texture>>;  // by page index
  }
  impl AtlasSprite { name(), frames(), frame(i), pivot() /* untrimmed px; None for a tile set */,
                     tile_size(), tile_set(texture) -> Option<TileSet>,
                     clips() /* an Aseprite export's tags, by name */, clip(name) -> Option<&AtlasClip> }
  impl AtlasClip { name(), is_looping(), frames() -> &[AtlasClipFrame] /* frame(): index, duration() */ }
  impl AtlasFrame { page(), uv() /* x, y, w, h over the page's size */, size() /* trimmed */,
                    offset(), source_size(), is_empty(), pivot_in(pivot_px) -> Vec2 /* normalized in the trimmed quad */ }
  ```

  - The facade's `Sprite::from_atlas(texture, sprite, frame)` makes a
    sprite of a frame: its trimmed size, its pivot normalized in the
    trimmed quad, so that its pixels land where the untrimmed cell's
    would. The snap below rounds the pivot's offset, so that its `f32`
    rounding cannot move them. A flip mirrors the quad about that pivot,
    so a frame trimmed unevenly mirrors as its cell would.
    `Clips::add_atlas` makes the sprites' clips, each frame placed the
    same way by `ClipFrame::from_atlas`, which `nadir::animate` writes into
    the Sprite with the texture and uv (ADR-0035).
  - Sprites that share a page share a texture, so at the same feet y their
    order falls to the order they were drawn in, not to their textures'
    handles (see "Sort keys").
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
    pub flip: Flip,                       // mirrors about the pivot (ADR-0035)
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
    counts low.** The cook tool packs each atlas group's sprites, a
    folder's by default, into as few atlas pages as possible.

## Instance data

`nadir-render` denies `unsafe`, so it cannot implement `Pod` for its own
structs. Instead, instances are packed into a reused `Vec<f32>`, which `gfx`
uploads (`f32: Pod` is implemented inside `gfx`). This is fully safe, and with
`extend_from_slice` it costs the same as a struct copy.

```rust
/// Logical layout of one sprite instance in the instance buffer (15 × f32 = 60 bytes).
pub const SPRITE_INSTANCE_FLOATS: usize = 15;

fn push_instance(out: &mut Vec<f32>, s: &SpriteDraw, placeholder: bool, snap_zoom: Option<f32>) {
    // From `s`: uv (0 to 1 for a placeholder), pivot and position (below), color.to_linear()
    out.extend_from_slice(&[
        position.x, position.y,          // location 0: pos (pivot, world)
        s.size.x, s.size.y,              // location 1: size
        pivot.x, pivot.y,                // location 2: pivot (normalized), 1 - x for flip.x, 1 - y for flip.y
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
  top-left corner moved to the nearest whole virtual pixel (ADR-0018). The
  corner's offset from the position, the pivot (mirrored by the flip)
  times the size, is first rounded to 1/256 of a world unit: a normalized
  pivot rounds in `f32` (7/13 × 13 is 7.0000005), which would move a
  corner on a half-pixel tie to the pixel below (ADR-0035).
- A flip mirrors the sprite about its pivot: the uv rect is swapped, and
  the packed pivot is `1 - x` for `flip.x` and `1 - y` for `flip.y`. A
  pivot in the middle of the width mirrors in place left to right; a
  sprite with its pivot at its feet hangs below them mirrored top to
  bottom, still sorted by its feet. Rotation turns the mirrored quad
  about the pivot.
- One dynamic instance buffer, sized to the frame's peak. It grows by
  doubling and never shrinks during a session.
- The unit quad is a static 4-vertex triangle strip.

## Tilemaps

- The layer data (tile indices per cell) is owned by the facade/scene. The
  renderer owns the chunks' GPU caches, keyed by the layer's handle.
- **Chunk:** 32×32 tiles, one instance buffer of tile instances (cell, atlas
  tile index, flags). Built on load; rebuilt only when a tile in the chunk
  changes (dirty flag).
- **Culling:** draw only the chunks that intersect the camera rect plus a
  one-tile margin.
- **Animated tiles:** each instance carries the tile's `frames` and
  `frame_ms`. The shader picks the frame from `RenderFrame.time`, so
  animation needs no CPU rebuild.
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
  size)`, y-sorted by its feet at the bottom of its cell. The facade's
  `load_level` spawns a level's tall tiles, the sandbox's tall walls
  among them, as entities with a `Sprite`.

## Camera and resolution

- `Camera2D { position, zoom: f32, shake: Vec2 }`. The `Camera` uniform
  (`View::uniform`), a scale and an offset, maps the world (+Y down) to
  clip space.
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

## Reloading

As built (M5.17, ADR-0042):

```rust
pub const SHADER_DIR: &str;                  // this crate's shaders/, where it was compiled
#[non_exhaustive]
pub enum Program { Sprite, TileChunk, Blit }
impl Program {
    pub const ALL: &'static [Program];
    pub fn name(self) -> &'static str;           // its folder, and its pipeline's label
    pub fn vertex_file(self) -> &'static str;    // under SHADER_DIR: "sprite/sprite.vert.glsl"
    pub fn fragment_file(self) -> &'static str;
}
```

- **Who reads the files:** not the renderer, which has no feature and no
  file reading for it. The caller reads the sources and hands them to
  `Renderer::reload_program`; under `dev` the facade watches
  `SHADER_DIR`'s files and does so (M5.8). `SHADER_DIR` is the path on the
  machine that compiled the crate. For a crate taken from crates.io it is
  in cargo's registry, which the package's files reach but which cargo
  does not rebuild when its files change: edit the shaders of a path
  dependency or a `[patch]`, or the files and the sources compiled in
  drift apart.
- **The check:** before the device builds them, the sources are checked
  against the program's interface constant (`Interface::check`, in
  `shader.rs`), the parity check the tests ran before. Nothing they
  declare may differ from it:
  - each `in` variable of the vertex shader (`attribute` too) is one of
    its attributes, with its location and no other layout qualifier (a
    `component` binds it elsewhere), its GLSL type and its name;
  - each uniform block of either shader is one of its blocks;
  - each other uniform of either shader is one of its samplers, a
    `sampler2D`, unless it has a value of its own (`= …`): the renderer
    sets no plain uniform, and a uniform struct is refused;
  - the fragment shader has one output, an `out vec4`, not an array, at
    location 0 and index 0, and writes no `gl_FragColor` or
    `gl_FragData`: the renderer draws into one color.
  - An attribute, block or sampler the sources leave out is taken, as the
    device takes one the program does not use: it enables the attribute,
    and warns of the block or the sampler. The parity test runs the check
    on the included sources, and checks that they declare all of it.
  - It reads the declarations at the top level as the preprocessor reads
    the text before its directives, in one pass: a `\` before a newline
    joins the lines, `\r\n`, `\n\r` and a `\r` alone are each one
    newline, each comment is a space, and a `"` starts a string, to the
    next `"` or the end of its line, in which no comment starts. As Mesa's
    preprocessor does, the line of a directive it does not take apart
    (`#pragma`, `#error`, `#extension`, or one it does not know; its name
    found after spaces and comments) keeps its text. Each line that
    starts with `#` is then left out.
    It neither expands macros nor chooses a branch of an `#if`: a
    declaration a macro makes is not seen, and every branch is read (the
    same declaration in each is one). Sources whose braces do not pair
    up, or whose branches do not each hold whole statements at the top
    level and end with the braces as deep as the others, are refused.
    Locations are GLSL integers: `010` is 8, and `0x6` is 6. An
    initializer's commas, inside its parentheses, part no declarations.
  - The device then compiles and links the sources, checks the blocks'
    sizes, and refuses an active block the description leaves out. A
    block's members are checked by its size alone: they must keep their
    types and order, which Rust packs.
- **A refusal** keeps the program it had, and the next frame draws as the
  last did. It is `RenderError::Gfx`, with `GfxError::Interface` saying
  what differs, with the attribute or the blocks the program has (the
  check's, or the device's for a block's size), or `GfxError::Shader`
  with the stage and the driver's log. `RenderError` is exhaustive in
  0.2, so a refusal has no variant of its own; its message reads "the
  shaders do not fit their pipeline" for an `Interface` one.
- **A success:** the next `render` draws with the new program, and the
  old one is destroyed at once; OpenGL deletes a program in use once it is
  no longer current. Metal (M10) may need to keep it until the GPU is done
  with it, as ADR-0042's "Revisit when" says of a replaced texture.
- **What the check does not see:** an edit that keeps the interface and
  draws something wrong, such as a fragment shader reading an input the
  vertex shader does not write, which GLSL 330 leaves undefined and Mesa
  links.
- **Textures:** each frame looks each texture handle's `TextureId` up in
  the asset server, so a texture reloaded behind its handle
  (`AssetServer::reload`, M5.16) is drawn with its new texels from the
  first frame after the finish that swaps it in, and with the old ones
  until then, with no renderer change. Its size may change: a sprite's uv
  is a fraction of the texture. A reload that fails keeps the texels it
  had.

## Text

- The font asset is a BMFont (text format), cooked into our font asset:
  glyph metrics, kerning pairs and atlas pages.
- Layout:
  - left/center/right alignment
  - wrap at a max width (word boundaries, falling back to character
    boundaries)
  - `\n` line breaks
  - per-run color via a tiny markup: `{c:#ff0}text{/c}`
- A missing glyph renders the font file's `fallback`: BMFont's fallback
  glyph if the font has one, else its `?` (`assets-and-codecs.md`, "Font
  file"), and logs once per character.
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
- As built (M5.17), the interface check has unit tests:
  - the included sources pass and declare all of their interface, and
    the files under `SHADER_DIR` are those included;
  - the reader finds declarations through comments, directives, joined
    lines, newlines of every kind, spacing, bodies, arrays, values and
    GLSL integers, and the raw lines of `#pragma`, `#error` and
    `#extension`; braces, or branches, that do not hold whole
    declarations are refused; and 4,000 inputs strung at random from a
    fixed seed never make it panic;
  - a moved, retyped or added attribute (`attribute` too, or one with
    another layout qualifier), a renamed sampler or block, a block in
    either shader, a sampler of another type, a plain uniform in either
    shader, a uniform struct, and a fragment output that is not one
    `out vec4` at location 0 and index 0, or `gl_FragColor` and
    `gl_FragData`, are refused, each saying what differs, once;
  - an edit inside the interface, every qualifier the reader passes
    over, an `#if` with the same declaration in each branch, varyings in
    a block, uniforms with values, and an attribute or sampler left out
    are taken.
- A GPU unit test checks that a reload destroys the program it replaces,
  and no other, and that a refused one destroys nothing.
- The headless tests rebuild each program with its colors swapped, drawn
  from the next frame, then rebuild it from its files as they are, which
  brings the colors back; refuse sources that do not compile (each
  stage), do not link, or declare another interface (a moved attribute,
  a renamed sampler, a larger block), each drawing the last program
  after; and reload a sprite's texture, of another size, and a tile
  set's, drawn from the finish on, the sprite's old texture destroyed and
  a broken file keeping its texels.
- As built (M4.5), the unit tests cover the chunks' revisions, packing,
  culling, the uniform's words, the clock's wrap and the order of tile
  layers among sprites. The headless tests draw placement, a change, a
  replaced layer or store, empty, unknown and removed layers, texture
  states, culling, order and batch splits, animation and flips.
