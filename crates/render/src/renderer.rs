//! The renderer: draws a frame's sprites into the virtual image, then
//! scales that up into the output.

use nadir_assets::{AssetServer, LoadState};
use nadir_core::{UVec2, Vec2};
use nadir_gfx::{
    BlendMode, BufferDesc, BufferId, BufferKind, BufferUsage, Device, PassTarget, PipelineDesc,
    PipelineId, RenderTargetId, Sampler, TextureFormat, TextureId, Topology, VertexLayout,
    VertexStep,
};

use crate::instance::push_instance;
use crate::plan::{Plan, Resolved};
use crate::shader::{BLIT, SPRITE, packed};
use crate::view::{self, Upscale, View};
use crate::{Camera2D, RenderError, RenderFrame, SPRITE_INSTANCE_FLOATS, Texture};

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

/// The 2D renderer.
///
/// Each frame, [`Renderer::render`] draws the sprites into a render target
/// of the virtual resolution, sorted by layer and depth and batched by
/// texture. It then scales that image up into the output by the largest
/// whole factor that fits, with bars in the clear color around it.
pub struct Renderer {
    virtual_size: UVec2,
    sprite: PipelineId,
    blit: PipelineId,
    quad: BufferId,
    blit_quad: BufferId,
    instances: BufferId,
    /// Instances the buffer holds.
    capacity: usize,
    world: RenderTargetId,
    plan: Plan<TextureId>,
    packed: Vec<f32>,
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
            blit,
            quad,
            blit_quad,
            instances,
            capacity: FIRST_CAPACITY,
            world,
            plan: Plan::new(),
            packed: Vec::with_capacity(FIRST_CAPACITY * SPRITE_INSTANCE_FLOATS),
        })
    }

    /// Draws `frame` into `output`. Call it inside a frame of the device
    /// (`begin_frame` and `end_frame`, or their offscreen forms).
    ///
    /// Sprites whose texture is still loading are not drawn. A texture that
    /// failed to load shows its placeholder, all of it, whatever the
    /// sprite's `uv`, when the server shows placeholders
    /// ([`OnFailure::Placeholder`](nadir_assets::OnFailure)); otherwise its
    /// sprites are not drawn either.
    ///
    /// # Errors
    ///
    /// [`RenderError::Gfx`] if the device refuses a call.
    pub fn render(
        &mut self,
        device: &mut Device,
        assets: &AssetServer,
        frame: &RenderFrame,
        output: Output,
    ) -> Result<FrameStats, RenderError> {
        let textures = assets.assets::<Texture>();
        self.plan.build(&frame.sprites, |sprite| {
            let Some(textures) = textures else {
                return Resolved::Skip;
            };
            match textures.get(sprite.texture) {
                None => Resolved::Skip,
                Some(texture) if textures.state(sprite.texture) == Some(LoadState::Failed) => {
                    Resolved::Placeholder(texture.id())
                }
                Some(texture) => Resolved::Draw(texture.id()),
            }
        });

        let view = View::new(&frame.camera, self.virtual_size);
        self.packed.clear();
        for drawn in &self.plan.order {
            let sprite = &frame.sprites[drawn.sprite as usize];
            push_instance(
                &mut self.packed,
                sprite,
                drawn.placeholder,
                Some(view.zoom()),
            );
        }
        let count = self.plan.order.len();
        if count > self.capacity {
            let capacity = count.next_power_of_two().max(self.capacity * 2);
            let instances = instance_buffer(device, capacity)?;
            device.destroy(self.instances);
            self.instances = instances;
            self.capacity = capacity;
        }
        if count > 0 {
            device.write_buffer(self.instances, 0, &self.packed)?;
        }

        let clear = frame.clear.to_linear();
        {
            let mut pass = device.begin_pass(self.world.into(), Some(clear))?;
            if count > 0 {
                pass.set_pipeline(self.sprite)?;
                pass.set_vertex_buffer(0, self.instances, 0)?;
                pass.set_vertex_buffer(1, self.quad, 0)?;
                pass.set_uniforms(0, &view.uniform())?;
                for batch in &self.plan.batches {
                    pass.set_texture(0, batch.texture, Sampler::NearestClamp)?;
                    pass.draw(0..4, batch.start..batch.start + batch.count)?;
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
        Ok(FrameStats {
            sprites: count as u32,
            skipped: self.plan.skipped,
            batches: self.plan.batches.len() as u32,
        })
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
