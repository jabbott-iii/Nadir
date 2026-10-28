//! The renderer: draws a frame's sprites and tile layers into the virtual
//! image, then scales that up into the output.

use nadir_assets::{AssetServer, LoadState};
use nadir_core::{UVec2, Vec2};
use nadir_gfx::{
    BlendMode, BufferDesc, BufferId, BufferKind, BufferUsage, Device, PassTarget, PipelineDesc,
    PipelineId, RenderTargetId, Sampler, TextureFormat, TextureId, Topology, VertexLayout,
    VertexStep,
};

use crate::chunks::{self, ChunkDraw, Step, TileCache};
use crate::shader::{BLIT, SPRITE, TILE_CHUNK, packed};
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

        let (instance_attributes, instance_stride) = packed::<6>(SPRITE.instance);
        let (corner, corner_stride) = packed::<1>(SPRITE.vertex_attributes);
        let sprite = device.create_pipeline(&PipelineDesc {
            label: SPRITE.name,
            shader: SPRITE.source(),
            vertex_layouts: &[
                VertexLayout {
                    stride: instance_stride,
                    step: VertexStep::Instance,
                    attributes: &instance_attributes,
                },
                VertexLayout {
                    stride: corner_stride,
                    step: VertexStep::Vertex,
                    attributes: &corner,
                },
            ],
            uniform_blocks: SPRITE.uniform_blocks,
            textures: SPRITE.textures,
            blend: BlendMode::Alpha,
            topology: Topology::TriangleStrip,
            target_format: TextureFormat::Rgba8Srgb,
        })?;
        let (tile_attributes, tile_stride) = packed::<2>(TILE_CHUNK.instance);
        let (tile_corner, tile_corner_stride) = packed::<1>(TILE_CHUNK.vertex_attributes);
        let tile_chunk = device.create_pipeline(&PipelineDesc {
            label: TILE_CHUNK.name,
            shader: TILE_CHUNK.source(),
            vertex_layouts: &[
                VertexLayout {
                    stride: tile_stride,
                    step: VertexStep::Instance,
                    attributes: &tile_attributes,
                },
                VertexLayout {
                    stride: tile_corner_stride,
                    step: VertexStep::Vertex,
                    attributes: &tile_corner,
                },
            ],
            uniform_blocks: TILE_CHUNK.uniform_blocks,
            textures: TILE_CHUNK.textures,
            blend: BlendMode::Alpha,
            topology: Topology::TriangleStrip,
            target_format: TextureFormat::Rgba8Srgb,
        })?;
        let (blit_attributes, blit_stride) = packed::<2>(BLIT.vertex_attributes);
        let blit = device.create_pipeline(&PipelineDesc {
            label: BLIT.name,
            shader: BLIT.source(),
            vertex_layouts: &[VertexLayout {
                stride: blit_stride,
                step: VertexStep::Vertex,
                attributes: &blit_attributes,
            }],
            uniform_blocks: BLIT.uniform_blocks,
            textures: BLIT.textures,
            blend: BlendMode::Opaque,
            topology: Topology::TriangleStrip,
            target_format: TextureFormat::Rgba8Srgb,
        })?;

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

fn instance_buffer(device: &mut Device, capacity: usize) -> Result<BufferId, RenderError> {
    Ok(device.create_buffer(&BufferDesc {
        label: "sprite instances",
        kind: BufferKind::Vertex,
        usage: BufferUsage::Dynamic,
        size: capacity * SPRITE_INSTANCE_FLOATS * size_of::<f32>(),
    })?)
}
