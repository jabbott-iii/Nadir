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

//! The renderer: draws a frame's sprites and tile layers into the virtual
//! image, then scales that up into the output.

use std::mem;

use nadir_assets::{AssetServer, LoadState};
use nadir_core::{UVec2, Vec2};
use nadir_gfx::{
    BufferDesc, BufferId, BufferKind, BufferUsage, Device, GfxError, PassTarget, PipelineDesc,
    PipelineId, RenderTargetId, Sampler, ShaderSource, TextureFormat, TextureId, Topology,
    VertexLayout, VertexStep,
};

use crate::chunks::{self, ChunkDraw, Step, TileCache};
use crate::shader::{BLIT, Interface, Program, SPRITE, TILE_CHUNK, packed};
use crate::view::{self, Upscale, View};
use crate::{
    Camera2D, RenderError, RenderFrame, SPRITE_INSTANCE_FLOATS, SpriteBatcher, Texture, TileLayers,
};

/// How the renderer draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderConfig {
    /// The size of the virtual image the world is drawn into, in pixels.
    /// The image is then scaled up into the output by a whole factor
    /// (pixel-perfect mode).
    pub virtual_resolution: UVec2,
}

/// Where a frame is shown: the window, or a render target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Output {
    /// The target. It must be [`TextureFormat::Rgba8Srgb`], as the window
    /// is.
    pub target: PassTarget,
    /// Its size in pixels: for the window, its drawable size.
    pub size: UVec2,
}

/// What a frame drew.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// The sprites drawn.
    pub sprites: u32,
    /// The sprites not drawn because their texture is not ready: still
    /// loading, failed with no placeholder to show
    /// ([`OnFailure::Nothing`](nadir_assets::OnFailure)), or unknown to the
    /// asset server.
    pub skipped: u32,
    /// The instanced draws of sprites.
    pub batches: u32,
    /// The tiles drawn, of every tile layer.
    pub tiles: u32,
    /// The instanced draws of tiles: one for each chunk drawn.
    pub chunks: u32,
}

/// The unit quad, as a triangle strip: (0, 0) to (1, 1).
const QUAD: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];

/// The blit's quad, as a triangle strip: clip-space corners, then texture
/// coordinates. GL stores a render target from its bottom row, so the
/// bottom of clip space (y = -1) samples v = 0.
const BLIT_QUAD: [f32; 16] = [
    -1.0, -1.0, 0.0, 0.0, //
    1.0, -1.0, 1.0, 0.0, //
    -1.0, 1.0, 0.0, 1.0, //
    1.0, 1.0, 1.0, 1.0,
];

/// The most attributes a vertex buffer slot of a program has room for: as
/// many as OpenGL 3.3 has in all.
const ATTRIBUTES: usize = 16;

/// The instances the buffer holds at first. It grows by doubling, and
/// never shrinks.
const FIRST_CAPACITY: usize = 256;

/// A tile layer of the frame that draws at least one chunk.
#[derive(Clone, Copy, Debug)]
struct ReadyLayer {
    /// Its layer and depth, as the high bits of a sprite's sort key.
    layer: u8,
    depth: u32,
    /// Its place among the frame's tile layers.
    submission: u32,
    texture: TextureId,
    uniform: [u32; 12],
    /// Its draws, in `Renderer::chunk_draws`.
    first: u32,
    end: u32,
}

/// The program a pass has bound last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bound {
    Nothing,
    Sprites,
    Tiles,
}

/// The 2D renderer.
///
/// Each frame, [`Renderer::render`] draws the sprites and tile layers into
/// a render target of the virtual resolution: the sprites sorted by layer
/// and depth and batched by texture, each tile layer at its own layer and
/// depth among them, a chunk at a time. It then scales that image up into
/// the output by the largest whole factor that fits, with bars in the
/// clear color around it.
pub struct Renderer {
    virtual_size: UVec2,
    sprite: PipelineId,
    tile_chunk: PipelineId,
    blit: PipelineId,
    quad: BufferId,
    blit_quad: BufferId,
    instances: BufferId,
    /// Instances the buffer holds.
    capacity: usize,
    world: RenderTargetId,
    batcher: SpriteBatcher<TextureId>,
    tiles: TileCache,
    // The tile work of a frame, in lists reused from frame to frame.
    ready: Vec<ReadyLayer>,
    chunk_draws: Vec<ChunkDraw>,
    /// (Position among the sprites, index in `ready`) of each ready layer.
    positions: Vec<(u32, u32)>,
    steps: Vec<Step<TextureId>>,
}

impl Renderer {
    /// Makes the renderer's pipelines, buffers and virtual image.
    ///
    /// # Errors
    ///
    /// [`RenderError::BadConfig`] for an empty virtual resolution, and
    /// [`RenderError::Gfx`] if the device refuses, for instance a virtual
    /// resolution over [`nadir_gfx::Caps::max_texture_size`].
    pub fn new(device: &mut Device, config: &RenderConfig) -> Result<Self, RenderError> {
        let virtual_size = config.virtual_resolution;
        if virtual_size.x == 0 || virtual_size.y == 0 {
            return Err(RenderError::BadConfig("the virtual resolution is empty"));
        }

        let sprite = pipeline(device, &SPRITE, SPRITE.source())?;
        let tile_chunk = pipeline(device, &TILE_CHUNK, TILE_CHUNK.source())?;
        let blit = pipeline(device, &BLIT, BLIT.source())?;

        let quad = device.create_buffer(&BufferDesc {
            label: "unit quad",
            kind: BufferKind::Vertex,
            usage: BufferUsage::Static,
            size: size_of_val(&QUAD),
        })?;
        device.write_buffer(quad, 0, &QUAD)?;
        let blit_quad = device.create_buffer(&BufferDesc {
            label: "blit quad",
            kind: BufferKind::Vertex,
            usage: BufferUsage::Static,
            size: size_of_val(&BLIT_QUAD),
        })?;
        device.write_buffer(blit_quad, 0, &BLIT_QUAD)?;
        let instances = instance_buffer(device, FIRST_CAPACITY)?;
        let world = device.create_render_target(virtual_size, TextureFormat::Rgba8Srgb)?;
        Ok(Self {
            virtual_size,
            sprite,
            tile_chunk,
            blit,
            quad,
            blit_quad,
            instances,
            capacity: FIRST_CAPACITY,
            world,
            batcher: SpriteBatcher::new(),
            tiles: TileCache::default(),
            ready: Vec::new(),
            chunk_draws: Vec::new(),
            positions: Vec::new(),
            steps: Vec::new(),
        })
    }

    /// Builds `program` again from `source`, for shaders edited while the
    /// game runs: the next [`Renderer::render`] draws with it, and the
    /// program it replaces is destroyed. Call it between frames, or in one
    /// before `render`.
    ///
    /// The sources may change anything but the program's interface, which
    /// the sources it was compiled with declare, its files under
    /// [`SHADER_DIR`](crate::SHADER_DIR) as they were:
    ///
    /// - each `in` variable of the vertex shader is one of the program's
    ///   attributes, with its location and no other layout qualifier, its
    ///   type and its name;
    /// - each uniform block, in either shader, is one of its blocks, whose
    ///   members must stay as they are: the device checks a block by its
    ///   size alone;
    /// - each other uniform is one of its samplers, a `sampler2D`, or has a
    ///   value of its own (`= …`);
    /// - the fragment shader has one output, an `out vec4` at location 0
    ///   and index 0, and writes no `gl_FragColor` or `gl_FragData`.
    ///
    /// An attribute, a block or a sampler the sources leave out is taken.
    /// The declarations are read as written, before a macro is expanded or
    /// a branch of an `#if` chosen: a declaration a macro makes is not
    /// seen, and every branch is read. Sources whose braces do not pair up,
    /// or whose branches do not each hold whole declarations, are refused.
    ///
    /// The renderer reads no file: the caller reads the sources, and gives
    /// them here. A device without Metal reads only the GLSL.
    ///
    /// # Errors
    ///
    /// [`RenderError::Gfx`] with [`GfxError::Interface`] saying what differs
    /// when the sources declare another interface, or a uniform block of
    /// another size; with [`GfxError::Shader`] and the driver's log when
    /// they do not compile or link; with another [`GfxError`] when the
    /// device refuses a call. The renderer keeps the program it had, and
    /// the next frame draws as the last did.
    pub fn reload_program(
        &mut self,
        device: &mut Device,
        program: Program,
        source: ShaderSource<'_>,
    ) -> Result<(), RenderError> {
        let interface = program.interface();
        interface
            .check(source.glsl_vs, source.glsl_fs)
            .map_err(|reason| GfxError::Interface {
                label: interface.name.to_string(),
                reason,
            })?;
        let rebuilt = pipeline(device, interface, source)?;
        let slot = match program {
            Program::Sprite => &mut self.sprite,
            Program::TileChunk => &mut self.tile_chunk,
            Program::Blit => &mut self.blit,
        };
        device.destroy(mem::replace(slot, rebuilt));
        Ok(())
    }

    /// Draws `frame` into `output`, with the tile layers its
    /// [`TileLayerDraw`](crate::TileLayerDraw)s name in `tiles`. Call it
    /// inside a frame of the device (`begin_frame` and `end_frame`, or their
    /// offscreen forms).
    ///
    /// Sprites whose texture is still loading are not drawn. A texture that
    /// failed to load shows its placeholder, all of it, whatever the
    /// sprite's `uv`, when the server shows placeholders
    /// ([`OnFailure::Placeholder`](nadir_assets::OnFailure)); otherwise its
    /// sprites are not drawn either. Tile layers do the same with their tile
    /// set's texture, every tile showing all of a placeholder.
    ///
    /// A tile layer that is not in `tiles` is not drawn. The renderer keeps
    /// each chunk of a layer on the GPU once the view has shown it, packs it
    /// again only after one of its tiles changes, and frees a layer's chunks
    /// at the first frame after it leaves `tiles`.
    ///
    /// # Errors
    ///
    /// [`RenderError::Gfx`] if the device refuses a call.
    pub fn render(
        &mut self,
        device: &mut Device,
        assets: &AssetServer,
        tiles: &TileLayers,
        frame: &RenderFrame,
        output: Output,
    ) -> Result<FrameStats, RenderError> {
        let textures = assets.assets::<Texture>();
        let texture = |handle| {
            let textures = textures?;
            let texture = textures.get(handle)?;
            let failed = textures.state(handle) == Some(LoadState::Failed);
            Some((texture.id(), failed))
        };
        self.batcher.build(&frame.sprites, &frame.camera, |sprite| {
            texture(sprite.texture)
        });

        let view = View::new(&frame.camera, self.virtual_size);
        self.prepare_tiles(device, tiles, frame, &view, texture)?;
        let mut stats = self.batcher.stats();
        let count = stats.sprites as usize;
        if count > self.capacity {
            let capacity = count.next_power_of_two().max(self.capacity * 2);
            let instances = instance_buffer(device, capacity)?;
            device.destroy(self.instances);
            self.instances = instances;
            self.capacity = capacity;
        }
        if count > 0 {
            device.write_buffer(self.instances, 0, self.batcher.instances())?;
        }

        let clear = frame.clear.to_linear();
        stats.batches = 0;
        {
            let mut pass = device.begin_pass(self.world.into(), Some(clear))?;
            pass.set_uniforms(0, &view.uniform())?;
            let mut bound = Bound::Nothing;
            for &step in &self.steps {
                match step {
                    Step::Sprites {
                        texture,
                        start,
                        count,
                    } => {
                        // A tile layer between two batches, or in one,
                        // leaves its own program and buffers bound.
                        if bound != Bound::Sprites {
                            pass.set_pipeline(self.sprite)?;
                            pass.set_vertex_buffer(0, self.instances, 0)?;
                            pass.set_vertex_buffer(1, self.quad, 0)?;
                            bound = Bound::Sprites;
                        }
                        pass.set_texture(0, texture, Sampler::NearestClamp)?;
                        pass.draw(0..4, start..start + count)?;
                        stats.batches += 1;
                    }
                    Step::Tiles(index) => {
                        let layer = self.ready[index as usize];
                        if bound != Bound::Tiles {
                            pass.set_pipeline(self.tile_chunk)?;
                            pass.set_vertex_buffer(1, self.quad, 0)?;
                            bound = Bound::Tiles;
                        }
                        pass.set_uniforms(1, &layer.uniform)?;
                        pass.set_texture(0, layer.texture, Sampler::NearestClamp)?;
                        for chunk in &self.chunk_draws[layer.first as usize..layer.end as usize] {
                            pass.set_vertex_buffer(0, chunk.buffer, 0)?;
                            pass.draw(0..4, 0..chunk.count)?;
                            stats.tiles += chunk.count;
                            stats.chunks += 1;
                        }
                    }
                }
            }
        }
        {
            let upscale = self.upscale(output.size);
            let mut pass = device.begin_pass(output.target, Some(clear))?;
            pass.set_viewport(upscale.rect);
            pass.set_pipeline(self.blit)?;
            pass.set_vertex_buffer(0, self.blit_quad, 0)?;
            pass.set_texture(0, self.world, Sampler::NearestClamp)?;
            pass.draw(0..4, 0..1)?;
        }
        Ok(stats)
    }

    /// Before the world pass, which borrows the device: frees the chunks of
    /// layers gone from `tiles`, culls and uploads the chunks of the frame's
    /// tile layers, and orders the pass's draws, with each layer that draws
    /// a chunk among the sprites.
    fn prepare_tiles(
        &mut self,
        device: &mut Device,
        tiles: &TileLayers,
        frame: &RenderFrame,
        view: &View,
        texture: impl Fn(nadir_assets::Handle<Texture>) -> Option<(TextureId, bool)>,
    ) -> Result<(), RenderError> {
        self.tiles.evict(device, tiles);
        self.ready.clear();
        self.chunk_draws.clear();
        let clock = chunks::clock_ms(frame.time);
        let shown = view.world_rect();
        // A frame of more than u32::MAX tile layers is far past any
        // budget; such layers are not drawn.
        for (submission, draw) in (0..u32::MAX).zip(&frame.tile_layers) {
            let Some(layer) = tiles.get(draw.tiles) else {
                continue;
            };
            let Some((texture, placeholder)) = texture(layer.tileset().texture) else {
                continue;
            };
            let Some(range) = chunks::visible_chunks(layer, shown) else {
                continue;
            };
            // Fewer draws than chunks in all the layers, so they fit.
            let first = self.chunk_draws.len() as u32;
            self.tiles
                .prepare(device, draw.tiles, layer, range, &mut self.chunk_draws)?;
            let end = self.chunk_draws.len() as u32;
            if end > first {
                self.ready.push(ReadyLayer {
                    layer: draw.layer,
                    depth: draw.depth,
                    submission,
                    texture,
                    uniform: chunks::layer_uniform(layer, view.zoom(), placeholder, clock),
                    first,
                    end,
                });
            }
        }
        self.ready
            .sort_unstable_by_key(|layer| (layer.layer, layer.depth, layer.submission));
        self.positions.clear();
        for (index, layer) in (0..).zip(&self.ready) {
            let position = self.batcher.position(layer.layer, layer.depth);
            self.positions.push((position, index));
        }
        chunks::steps(self.batcher.batches(), &self.positions, &mut self.steps);
        Ok(())
    }

    /// Where the virtual image lands in an output of `size`.
    pub fn upscale(&self, size: UVec2) -> Upscale {
        Upscale::new(self.virtual_size, size)
    }

    /// The world point under `screen`, in pixels from the top left of an
    /// output of `size`, as `camera` shows it: for aiming with the mouse.
    pub fn screen_to_world(&self, camera: &Camera2D, size: UVec2, screen: Vec2) -> Vec2 {
        let view = View::new(camera, self.virtual_size);
        view.unproject(view::screen_to_virtual(&self.upscale(size), screen))
    }

    /// Where `world` is in an output of `size`, in pixels from its top
    /// left, as `camera` shows it.
    pub fn world_to_screen(&self, camera: &Camera2D, size: UVec2, world: Vec2) -> Vec2 {
        let view = View::new(camera, self.virtual_size);
        view::virtual_to_screen(&self.upscale(size), view.project(world))
    }
}

/// Makes the pipeline of the program `interface` from `source`: its
/// instances, if it has any, in vertex buffer slot 0, and its vertices in
/// the next.
fn pipeline(
    device: &mut Device,
    interface: &Interface,
    source: ShaderSource<'_>,
) -> Result<PipelineId, RenderError> {
    let (instance, instance_stride) = packed::<ATTRIBUTES>(interface.instance);
    let (vertex, vertex_stride) = packed::<ATTRIBUTES>(interface.vertex_attributes);
    let layouts = [
        VertexLayout {
            stride: instance_stride,
            step: VertexStep::Instance,
            attributes: &instance[..interface.instance.len()],
        },
        VertexLayout {
            stride: vertex_stride,
            step: VertexStep::Vertex,
            attributes: &vertex[..interface.vertex_attributes.len()],
        },
    ];
    let vertex_layouts = if interface.instance.is_empty() {
        &layouts[1..]
    } else {
        &layouts[..]
    };
    const {
        assert!(
            SPRITE.instance.len() <= ATTRIBUTES
                && SPRITE.vertex_attributes.len() <= ATTRIBUTES
                && TILE_CHUNK.instance.len() <= ATTRIBUTES
                && TILE_CHUNK.vertex_attributes.len() <= ATTRIBUTES
                && BLIT.instance.len() <= ATTRIBUTES
                && BLIT.vertex_attributes.len() <= ATTRIBUTES,
            "a program has more attributes than a slot has room for"
        );
    }
    Ok(device.create_pipeline(&PipelineDesc {
        label: interface.name,
        shader: source,
        vertex_layouts,
        uniform_blocks: interface.uniform_blocks,
        textures: interface.textures,
        blend: interface.blend,
        topology: Topology::TriangleStrip,
        target_format: TextureFormat::Rgba8Srgb,
    })?)
}

fn instance_buffer(device: &mut Device, capacity: usize) -> Result<BufferId, RenderError> {
    Ok(device.create_buffer(&BufferDesc {
        label: "sprite instances",
        kind: BufferKind::Vertex,
        usage: BufferUsage::Dynamic,
        size: capacity * SPRITE_INSTANCE_FLOATS * size_of::<f32>(),
    })?)
}

/// Reloads on a GPU: what is destroyed, and what is kept.
#[cfg(all(test, feature = "headless-test"))]
mod gpu_tests {
    use nadir_gfx::{GfxError, ResourceKind};

    use super::*;

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

    /// The renderer's programs, in the order of `Program::ALL`.
    fn programs(renderer: &Renderer) -> [PipelineId; 3] {
        [renderer.sprite, renderer.tile_chunk, renderer.blit]
    }

    #[test]
    fn a_reload_destroys_the_program_it_replaces_and_a_refused_one_keeps_it() {
        let Some(mut device) = device() else { return };
        let size = UVec2::new(4, 4);
        let mut renderer = Renderer::new(
            &mut device,
            &RenderConfig {
                virtual_resolution: size,
            },
        )
        .expect("a renderer");
        let target = device
            .create_render_target(size, TextureFormat::Rgba8Srgb)
            .expect("a target");
        let alive = |device: &mut Device, pipeline: PipelineId| {
            device.begin_offscreen_frame().expect("a frame");
            let set = device
                .begin_pass(target.into(), None)
                .expect("a pass")
                .set_pipeline(pipeline);
            device.end_offscreen_frame().expect("the end of the frame");
            match set {
                Ok(()) => true,
                Err(GfxError::StaleHandle(ResourceKind::Pipeline)) => false,
                Err(error) => panic!("{error}"),
            }
        };
        assert_eq!(Program::ALL.len(), 3);
        for (index, &program) in Program::ALL.iter().enumerate() {
            let before = programs(&renderer);
            let source = program.interface().source();
            renderer
                .reload_program(&mut device, program, source)
                .expect("the program rebuilt");
            let after = programs(&renderer);
            for (slot, (old, new)) in before.iter().zip(&after).enumerate() {
                if slot == index {
                    assert_ne!(old, new, "{}", program.name());
                    assert!(!alive(&mut device, *old), "{}: the old one", program.name());
                } else {
                    assert_eq!(old, new, "{}: another program", program.name());
                }
                assert!(alive(&mut device, *new));
            }

            let broken = ShaderSource {
                glsl_fs: "#version 330 core\nout vec4 out_color;\nvoid main() { oops }",
                ..source
            };
            assert!(matches!(
                renderer.reload_program(&mut device, program, broken),
                Err(RenderError::Gfx(GfxError::Shader { .. }))
            ));
            assert_eq!(programs(&renderer), after, "{}: refused", program.name());
            assert!(alive(&mut device, after[index]));
        }
    }
}
