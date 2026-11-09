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

//! The 2D renderer of the Nadir engine: y-sorted, batched sprites, tile
//! layers drawn in chunks, bitmap text, a pixel-perfect camera, and the
//! texture, atlas and font assets.
//!
//! It draws with [`nadir_gfx`] and finds its textures in a
//! [`nadir_assets::AssetServer`]. It knows nothing of entities: each frame
//! it draws a [`RenderFrame`], a list of what to draw. Games use it through
//! [nadir_engine](https://docs.rs/nadir_engine), which re-exports it as
//! `nadir::render`, and whose `nadir::prelude` holds the types a game fills
//! in. There the engine's `App` makes the [`Renderer`] for the window, its
//! systems draw the entities' sprites into the `RenderFrame`, a world
//! resource, and the game keeps its tile layers in the world resource
//! [`TileLayers`]. Depend on this crate directly to draw with a device of
//! your own.
//!
//! # The API
//!
//! **Drawing.** A [`Renderer`] is made on a [`nadir_gfx::Device`] with a
//! [`RenderConfig`], its virtual resolution. [`Renderer::render`] draws a
//! [`RenderFrame`] into an [`Output`], the window or a render target, and
//! says what it drew in [`FrameStats`]. [`Renderer::upscale`] gives where
//! the virtual image lands in an output ([`Upscale`]), and
//! [`Renderer::screen_to_world`] and [`Renderer::world_to_screen`] map
//! points between the two. Failures are [`RenderError`]s.
//!
//! **Reloading.** [`Renderer::reload_program`] builds one of the renderer's
//! [`Program`]s again from GLSL it is given, such as its files under
//! [`SHADER_DIR`] after an edit, and keeps the program it had when the
//! sources do not build or do not declare the program's interface. A
//! texture reloaded behind its handle through `nadir-assets` needs nothing
//! of the renderer: each frame looks its textures up again, so the new
//! texels are drawn from the first frame after the finish that swaps them
//! in. The engine's facade does both in development builds, under its
//! `dev` feature (roadmap M5.8).
//!
//! **What a frame draws.** A [`RenderFrame`] holds the [`Camera2D`], the
//! clear color, the [`SpriteDraw`]s (each with a [`SortMode`] and a
//! [`Flip`]), the [`TileLayerDraw`]s, the texts in a [`TextList`], and the
//! time that animated tiles show.
//!
//! **Text.** A [`TextDraw`] draws a text in a [`Font`]: laid out with a
//! [`TextLayout`] (a width to wrap at, an [`Align`]ment, and the color
//! markup, which [`Escaped`] keeps from a player's text), in a color, its
//! glyphs a mask or their own colors ([`GlyphMode`]). [`Font::layout`]
//! gives each [`PlacedGlyph`] of a text, and [`Font::measure`] its block,
//! with no GPU.
//!
//! **Tiles.** A [`TileLayer`] is a grid of cells, each empty or a [`Tile`]
//! of its [`TileSet`]. The game keeps its layers in a [`TileLayers`] store,
//! which the renderer is given beside each frame.
//!
//! **Assets**, loaded through `nadir-assets`:
//!
//! - [`Texture`]: a QOI file, decoded on a worker into a
//!   [`DecodedTexture`], then made on the device, which must be in the
//!   [`FinalizeCtx`](nadir_assets::FinalizeCtx).
//! - [`Atlas`]: a cooked atlas file, `atlas/<group>.kbj`: its
//!   [`AtlasPage`]s, and its [`AtlasSprite`]s by name, each with its
//!   [`AtlasFrame`]s and its [`AtlasClip`]s of [`AtlasClipFrame`]s. It
//!   needs no GPU.
//! - [`Font`]: a cooked font file, `atlas/<font>.font.kbj`: its metrics, its
//!   [`Glyph`]s by character, each a frame of a sprite of its atlas, and its
//!   kerning. It needs no GPU; [`Font::load_now_in`] loads it with its atlas
//!   and pages, which drawing it needs.
//!
//! **For measuring without a GPU:** [`SpriteBatcher`], the renderer's work
//! on the CPU for sprites, and [`TileLayer::pack_chunk`], for tiles. Their
//! instances are [`SPRITE_INSTANCE_FLOATS`] and [`TILE_INSTANCE_FLOATS`]
//! floats each.
//!
//! # Example: drawing a frame
//!
//! The renderer draws inside a frame of the device. Into the window, the
//! output is [`PassTarget::Surface`](nadir_gfx::PassTarget::Surface), and
//! its size the window's drawable size:
//!
//! ```
//! use nadir_assets::AssetServer;
//! use nadir_core::UVec2;
//! use nadir_gfx::{Device, PassTarget};
//! use nadir_render::{FrameStats, Output, RenderError, RenderFrame, Renderer, TileLayers};
//!
//! /// Draws `frame` into the window, `window` pixels large. Call it between
//! /// the device's `begin_frame` and `end_frame`.
//! fn draw(
//!     renderer: &mut Renderer,
//!     device: &mut Device,
//!     assets: &AssetServer,
//!     tiles: &TileLayers,
//!     frame: &RenderFrame,
//!     window: UVec2,
//! ) -> Result<FrameStats, RenderError> {
//!     let output = Output {
//!         target: PassTarget::Surface,
//!         size: window,
//!     };
//!     renderer.render(device, assets, tiles, frame, output)
//! }
//! ```
//!
//! Keep one [`RenderFrame`], and clear it each frame, so that its lists keep
//! their capacity:
//!
//! ```
//! use nadir_core::{Color, Vec2};
//! use nadir_render::{Camera2D, RenderFrame};
//!
//! let mut frame = RenderFrame::new();
//! frame.clear = Color::rgb(30, 30, 46);
//! frame.camera = Camera2D {
//!     position: Vec2::new(160.0, 90.0),
//!     zoom: 2.0,
//!     shake: Vec2::ZERO,
//! };
//! // Each frame: empty the lists, then fill them again.
//! frame.clear();
//! assert!(frame.sprites.is_empty() && frame.tile_layers.is_empty());
//! assert_eq!(frame.camera.zoom, 2.0, "the camera stays");
//! ```
//!
//! # Example: a sprite from an atlas
//!
//! The cook packs a game's images into atlases. A frame of an atlas's
//! sprite is drawn from its page, with its trimmed size, and its pivot
//! placed in the trimmed quad so that its pixels land where the untrimmed
//! cell's would:
//!
//! ```
//! use nadir_assets::{AssetError, AssetServer, FinalizeCtx, Scope};
//! use nadir_core::Vec2;
//! use nadir_render::{Atlas, SpriteDraw};
//!
//! /// The first frame of `sprites/hero`, with its pivot at `position`.
//! fn hero(server: &mut AssetServer, position: Vec2) -> Result<Option<SpriteDraw>, AssetError> {
//!     // An atlas needs no GPU, so it can be made at once, before a window
//!     // opens.
//!     let atlas = server.load_now::<Atlas>("atlas/sprites.kbj", &mut FinalizeCtx::new())?;
//!     let Some(atlas) = server.get(atlas).cloned() else {
//!         return Ok(None);
//!     };
//!     // Its pages are textures, in the order of `Atlas::pages`.
//!     let pages = atlas.load_pages(server, Scope::Global);
//!     let Some(sprite) = atlas.sprite("sprites/hero") else {
//!         return Ok(None);
//!     };
//!     let (Some(frame), Some(pivot)) = (sprite.frame(0), sprite.pivot()) else {
//!         return Ok(None);
//!     };
//!     let mut draw = SpriteDraw::new(pages[frame.page()], position, frame.size().as_vec2());
//!     draw.uv = frame.uv();
//!     draw.pivot = frame.pivot_in(pivot);
//!     Ok(Some(draw))
//! }
//! ```
//!
//! # Example: a text
//!
//! A font is loaded with its atlas and the atlas's pages, which its glyphs
//! are drawn from. Each frame, texts are pushed into the frame's
//! [`TextList`], formatted ones with no allocation once it is warm:
//!
//! ```
//! use nadir_assets::{AssetError, AssetServer, FinalizeCtx, Handle, Scope};
//! use nadir_core::{Color, Vec2};
//! use nadir_render::{Align, Font, RenderFrame, TextDraw};
//!
//! /// Loads the font the cook made of `fonts/ui.fnt`, and the atlas and
//! /// pages of its glyphs.
//! fn ui_font(server: &mut AssetServer) -> Result<Handle<Font>, AssetError> {
//!     let mut ctx = FinalizeCtx::new();
//!     Font::load_now_in(server, "atlas/fonts/ui.font.kbj", Scope::Global, &mut ctx)
//! }
//!
//! /// The score, centred at the top of a panel 120 wide at (20, 10), in
//! /// layer 5, above the world's sprites.
//! fn score(frame: &mut RenderFrame, font: Handle<Font>, score: u32) {
//!     let mut text = TextDraw::new(font, Vec2::new(20.0, 10.0));
//!     text.layout.max_width = Some(120);
//!     text.layout.align = Align::Center;
//!     text.layer = 5;
//!     frame.texts.write(text, format_args!("Score {{c:#ff0}}{score}{{/c}}"));
//! }
//!
//! let mut frame = RenderFrame::new();
//! let font = Handle::new(nadir_core::GenIndex::new(0, std::num::NonZeroU32::MIN));
//! score(&mut frame, font, 120);
//! let (_, text) = frame.texts.iter().next().expect("a text");
//! assert_eq!(text, "Score {c:#ff0}120{/c}");
//! # let _ = (ui_font, Color::WHITE);
//! ```
//!
//! # Example: reloading a shader
//!
//! The renderer reads no file. A game that watches its shader files reads
//! the one that changed, and gives the program's sources to the renderer:
//!
//! ```
//! use std::fs;
//! use std::path::Path;
//!
//! use nadir_gfx::{Device, ShaderSource};
//! use nadir_render::{Program, Renderer, SHADER_DIR};
//!
//! /// Builds `program` again from its files, or says why it could not, in
//! /// which case the renderer goes on drawing with the program it had.
//! fn reload(
//!     renderer: &mut Renderer,
//!     device: &mut Device,
//!     program: Program,
//! ) -> Result<(), String> {
//!     let read = |file: &str| {
//!         fs::read_to_string(Path::new(SHADER_DIR).join(file)).map_err(|error| error.to_string())
//!     };
//!     let vertex = read(program.vertex_file())?;
//!     let fragment = read(program.fragment_file())?;
//!     let source = ShaderSource {
//!         glsl_vs: &vertex,
//!         glsl_fs: &fragment,
//!         msl: "",
//!     };
//!     renderer
//!         .reload_program(device, program, source)
//!         .map_err(|error| error.to_string())
//! }
//!
//! // Each program's files are in the folder.
//! for &program in Program::ALL {
//!     assert!(Path::new(SHADER_DIR).join(program.vertex_file()).is_file());
//!     assert!(Path::new(SHADER_DIR).join(program.fragment_file()).is_file());
//! }
//! ```
//!
//! # Order
//!
//! Sprites are drawn by [`SpriteDraw::layer`], lowest first; within a
//! layer, by depth: the pivot's y for [`SortMode::YSort`], so that lower
//! on screen is in front, or the number of [`SortMode::Fixed`]. `Fixed(0)`
//! is behind and `Fixed(u32::MAX)` in front of every y-sorted sprite of
//! its layer. Sprites of one depth are grouped by texture, and then keep
//! the order they were added in, so the same frame draws the same way
//! every time. A tile layer is drawn at its [`TileLayerDraw::layer`] and
//! [`TileLayerDraw::depth`], before the sprites of the same layer and
//! depth. A text's glyphs are drawn at its layer, and at its depth, its
//! position's y or its fixed depth: at one depth, after every sprite, the
//! texts in the order they were pushed.
//!
//! Consecutive sprites of one texture are one instanced draw, and so are
//! consecutive glyphs of one page. Y-sorting interleaves textures, so
//! sprites that share an atlas page draw in fewer batches
//! ([`FrameStats::batches`]).
//!
//! # Pixel-perfect mode
//!
//! The renderer has one mode so far: the world is drawn into an image of
//! exactly [`RenderConfig::virtual_resolution`], which is then scaled up
//! into the output by the largest whole factor that fits, at least 1, and
//! centred, with bars of the clear color around it; an output smaller than
//! the image crops it. The camera's zoom is rounded to a whole number, at
//! least 1. The view's top-left corner, the top-left corner of each
//! unrotated sprite and glyph, and each tile layer's origin are moved to
//! whole virtual pixels, so that texels land on whole pixels (ADR-0018).
//!
//! # Textures that are not ready
//!
//! A sprite whose texture is still loading is not drawn, and is counted in
//! [`FrameStats::skipped`]. One whose texture failed shows all of the
//! texture's placeholder, an 8 × 8 magenta and black checker, whatever its
//! `uv`, when the server shows placeholders
//! ([`OnFailure::Placeholder`](nadir_assets::OnFailure::Placeholder));
//! otherwise it is skipped too. A tile layer does the same with its tile
//! set's texture, and a text's glyphs with their page: a glyph whose page
//! is not ready, or was never loaded, is not drawn, and is counted in
//! [`FrameStats::skipped`]. A text whose font, atlas or sprite is not ready
//! or does not fit is not drawn at all ([`FrameStats::skipped_texts`]). An
//! atlas or a page never loaded, and an atlas that lacks the glyphs'
//! frames, are logged once for the font. A character a font lacks draws
//! the font's fallback glyph, and is logged once.
//!
//! # Tile layers
//!
//! The renderer packs a chunk of [`TileLayer::CHUNK`] × [`TileLayer::CHUNK`]
//! cells into a GPU buffer the first time the view shows it, and again only
//! after one of its cells changes ([`TileLayer::set`]). It draws only the
//! chunks in view, each in one instanced draw ([`FrameStats::chunks`]).
//! Animated tiles change frame on the GPU, from [`RenderFrame::time`] in
//! whole milliseconds, with no work on the CPU. A layer's buffers are freed
//! at the first frame after it leaves the [`TileLayers`].
//!
//! # Errors
//!
//! [`Renderer::new`] and [`Renderer::render`] fail with
//! [`RenderError::Gfx`] when the device refuses a call, and
//! [`Renderer::new`] with [`RenderError::BadConfig`] for an empty virtual
//! resolution. [`Renderer::reload_program`] fails with
//! [`RenderError::Gfx`] when the sources declare another interface, do not
//! compile or link, or the device refuses, and keeps the program it had.
//! [`TileLayer::new`] panics on a tile size that is not a finite
//! number above 0, and [`TileLayer::set`] on a cell outside the layer.
//! [`Font::load_now_in`] fails with the font's or its atlas's
//! [`AssetError`](nadir_assets::AssetError), or when the atlas lacks the
//! glyphs' frames.
//!
//! # Allocation and threading
//!
//! The renderer's lists and buffers are reused from frame to frame, the
//! frame's texts' one buffer of characters too. Its instance buffer, of
//! sprites and glyphs, grows by doubling and never shrinks; a steady frame
//! allocates nothing on the heap. The renderer runs on the device's
//! thread, as the [`nadir_gfx::Device`] stays there.
//!
//! # Platforms
//!
//! It draws wherever `nadir-gfx` has a device: on Linux so far. Windows
//! and macOS have no OpenGL context yet (roadmap M2.6 and M2.7). What needs
//! no GPU, such as frames, tile layers, atlases, fonts and their layout, and
//! the [`SpriteBatcher`], works on all three.
//!
//! # Not built yet
//!
//! Text and sprites on screen, at the window's resolution (the UI pass,
//! roadmap M7.8), debug draw (M7.6), post effects, camera rotation, the
//! smooth-camera and native-resolution modes, and the Metal shaders
//! (M10.3).
//!
//! # Features
//!
//! `headless-test` builds the crate's own tests that draw on an offscreen
//! OpenGL context, for the workspace's CI. A user does not need it.
//!
//! The design is in
//! [`docs/subsystems/render.md`](https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/render.md).

mod atlas;
mod batcher;
mod chunks;
mod error;
mod font;
mod frame;
mod glyphs;
mod instance;
mod plan;
mod renderer;
mod shader;
mod sort;
mod text;
mod texture;
mod tiles;
mod view;

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use atlas::{Atlas, AtlasClip, AtlasClipFrame, AtlasFrame, AtlasPage, AtlasSprite};
pub use batcher::SpriteBatcher;
pub use error::RenderError;
pub use font::{Font, Glyph};
pub use frame::{Camera2D, Flip, GlyphMode, RenderFrame, SortMode, SpriteDraw, TextDraw, TextList};
pub use instance::SPRITE_INSTANCE_FLOATS;
pub use renderer::{FrameStats, Output, RenderConfig, Renderer};
pub use shader::{Program, SHADER_DIR};
pub use text::{Align, Escaped, PlacedGlyph, TextLayout};
pub use texture::{DecodedTexture, Texture};
pub use tiles::{TILE_INSTANCE_FLOATS, Tile, TileLayer, TileLayerDraw, TileLayers, TileSet};
pub use view::Upscale;
