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

## Input: `RenderFrame`

The facade's extract stage fills a `RenderFrame` every frame. All of its
`Vec`s are cleared and reused, never reallocated.

```rust
pub struct RenderFrame {
    pub camera: Camera2D,                 // position, zoom, rotation (0 for now), shake offset
    pub clear: Color,
    pub sprites: Vec<SpriteDraw>,         // world-space sprites
    pub tile_layers: Vec<TileLayerDraw>,  // references to cached chunk sets + layer/depth info
    pub ui_sprites: Vec<SpriteDraw>,      // screen-space
    pub texts: Vec<TextDraw>,             // world or screen space
    pub debug: DebugDrawList,             // lines/rects/circles, cleared per frame
    pub post: PostSettings,               // fade, tint, color grade LUT handle (optional)
    pub time: f32,                        // seconds, for animated tiles/shaders
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

## Camera and resolution

- `Camera2D { position, zoom: f32, shake: Vec2 }`. The view matrix maps the
  world (+Y down) to clip space.
- **Pixel-perfect mode** (default when `virtual_resolution` is set):
  1. Render the world into an RT of exactly `virtual_resolution`.
  2. Upscale to the window by the largest integer factor that fits, with
     letterbox bars in the clear color.
  3. Snap the camera position to whole virtual pixels.
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
- Tilemap: a 256×256 level visible at zoom 1 adds ≤ 1 ms CPU and ≤ 30 draw
  calls.
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
- **Benchmarks:** sort+batch for 10k sprites; chunk rebuild.
