# nadir-render

The 2D renderer of the Nadir engine: y-sorted, batched sprites, tile layers
drawn in chunks, a pixel-perfect camera, and the texture and atlas assets,
drawn with [nadir-gfx](https://crates.io/crates/nadir-gfx).

It is part of [Nadir](https://github.com/jabbott-iii/Nadir), a 2D engine for
top-down games on Windows, macOS and Linux, written in Rust with no
third-party crates. Games use it through
[nadir_engine](https://crates.io/crates/nadir_engine), which re-exports it
as `nadir::render`; depend on it directly to use it on its own. In a game
of the engine, `App` makes the `Renderer` and draws each frame, the
entities' sprites fill the frame's `RenderFrame`, and the game's tile
layers are the world resource `TileLayers`.

## Status

Version 0.3: the API will change before 1.0.

Built:

- sprites, sorted by layer and then by the y of their pivot (their feet,
  by default) or a fixed depth, and drawn in instanced batches by texture;
- mirrored and rotated sprites, tinted in linear light;
- tile layers of square cells, drawn in chunks of 32 × 32 cells, only those
  in view, with animated and mirrored tiles, among the sprites at a layer
  and depth of their own;
- pixel-perfect mode: the world drawn into an image of a fixed virtual
  resolution, scaled up into the window by a whole factor, with bars
  around it, and the mapping between screen and world for the mouse;
- the `Texture` asset, a QOI file uploaded to the GPU, and the `Atlas`
  asset, a cooked atlas's sprites, frames and clips;
- a program rebuilt from new GLSL sources while the game runs, the one it
  had kept when they do not compile or do not declare its interface, and
  textures reloaded behind their handles drawn from the next frame.

Not yet:

- bitmap text (roadmap M7.1) and debug draw (M7.6);
- UI sprites, post effects, camera rotation, and the smooth-camera and
  native-resolution modes;
- the Metal shaders (M10.3).

## Usage

```toml
[dependencies]
nadir-render = "0.3"
nadir-gfx = "0.3"
nadir-assets = "0.3"
nadir-core = "0.3"
```

The renderer draws with a `nadir_gfx::Device`, finds textures in a
`nadir_assets::AssetServer`, and takes `nadir-core`'s math and color types.
This draws one sprite into a render target on an offscreen device:

```rust,no_run
use nadir_assets::{AssetServer, FinalizeCtx, OnFailure};
use nadir_core::{Color, UVec2, Vec2};
use nadir_gfx::{Device, TextureFormat};
use nadir_render::{
    Output, RenderConfig, RenderFrame, Renderer, SpriteDraw, Texture, TileLayers,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut device = Device::new_headless()?;
    let mut renderer = Renderer::new(
        &mut device,
        &RenderConfig {
            virtual_resolution: UVec2::new(320, 180),
        },
    )?;

    // A texture is a QOI file: read on a worker thread, then made on the
    // device.
    let mut assets = AssetServer::new("assets", OnFailure::Placeholder);
    let hero = assets.load::<Texture>("sprites/hero.qoi");
    assets.finish_all_loads(&mut FinalizeCtx::new().with(&mut device));

    let mut frame = RenderFrame::new();
    frame.clear = Color::rgb(30, 30, 46);
    // 16 × 16 world units, with its feet at (0, 8).
    frame.sprites.push(SpriteDraw::new(hero, Vec2::new(0.0, 8.0), Vec2::splat(16.0)));

    // An output four times the virtual resolution.
    let size = UVec2::new(1280, 720);
    let target = device.create_render_target(size, TextureFormat::Rgba8Srgb)?;
    device.begin_offscreen_frame()?;
    let output = Output {
        target: target.into(),
        size,
    };
    let stats = renderer.render(&mut device, &assets, &TileLayers::new(), &frame, output)?;
    device.end_offscreen_frame()?;
    assert_eq!(stats.sprites, 1);
    Ok(())
}
```

A tile layer is a grid of cells, each empty or a tile of its tile set. The
renderer draws the layers a frame names, from the `TileLayers` it is given:

```rust
use nadir_assets::Handle;
use nadir_core::{IVec2, UVec2, Vec2};
use nadir_render::{RenderFrame, Texture, Tile, TileLayer, TileLayerDraw, TileLayers, TileSet};

/// A field of 64 × 64 grass tiles, 16 world units a side, with one
/// animated tile: tiles 8 to 11 of the tile set, a quarter second each.
fn add_field(tiles: &mut TileLayers, texture: Handle<Texture>) -> Handle<TileLayer> {
    let tileset = TileSet {
        texture,
        columns: 8,
        rows: 8,
    };
    let mut field = TileLayer::new(tileset, UVec2::new(64, 64), 16.0, Vec2::ZERO);
    for y in 0..64 {
        for x in 0..64 {
            field.set(IVec2::new(x, y), Some(Tile::new(0)));
        }
    }
    field.set(IVec2::new(10, 10), Some(Tile::animated(8, 4, 250)));
    tiles.add(field)
}

/// Draws the field in layer 0, behind every sprite there.
fn draw_field(frame: &mut RenderFrame, field: Handle<TileLayer>) {
    frame.tile_layers.push(TileLayerDraw::new(field));
}
```

## Features

`headless-test` builds the crate's own tests that draw on an offscreen
OpenGL context, for the workspace's CI. A user does not need it.

## Platforms

The renderer draws wherever `nadir-gfx` has a device: on Linux today. On
Windows and macOS there is no OpenGL context yet (roadmap M2.6 and M2.7),
so no device to draw with. Everything that needs no GPU, such as the frame,
the tile layers, the atlas and the `SpriteBatcher`, works on all three.

## Documentation

- API reference: https://docs.rs/nadir-render
- Design: https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/render.md
- The engine: https://github.com/jabbott-iii/Nadir

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
