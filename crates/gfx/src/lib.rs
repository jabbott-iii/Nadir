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

//! The small render hardware interface (RHI) of the Nadir engine: textures,
//! buffers, render targets, pipelines and checked draws, over OpenGL 3.3
//! core.
//!
//! It is the layer between the OS's graphics context, which
//! [`nadir_platform`] makes, and the 2D renderer,
//! [nadir-render](https://docs.rs/nadir-render), which draws sprites and
//! tiles with it. Games do not use it directly: the engine,
//! [nadir_engine](https://docs.rs/nadir_engine), makes the [`Device`] for
//! its window and hands it to the renderer. Use it directly to write a
//! renderer of your own. It has one backend, OpenGL 3.3 core; a Metal
//! backend is roadmap task M10.2.
//!
//! # The API
//!
//! **The device.** A [`Device`] is the GPU. [`Device::new`] makes it for a
//! window's [`GpuSurface`](nadir_platform::GpuSurface), and
//! [`Device::new_headless`] for an offscreen context of its own, for tests
//! and tools. [`Device::caps`] says what it runs on and its limits
//! ([`Caps`], [`BackendKind`]).
//!
//! **Resources**, each named by a generational id that stops resolving once
//! the resource is destroyed:
//!
//! - textures: [`Device::create_texture`] from a [`TextureDesc`] and its
//!   [`TextureFormat`], changed with [`Device::update_texture`];
//!   [`TextureId`]
//! - buffers: [`Device::create_buffer`] from a [`BufferDesc`]
//!   ([`BufferKind`], [`BufferUsage`]), filled with
//!   [`Device::write_buffer`]; [`BufferId`]
//! - render targets, textures that passes draw into:
//!   [`Device::create_render_target`]; [`RenderTargetId`]
//! - pipelines: [`Device::create_pipeline`]; [`PipelineId`]
//! - timers, which measure the GPU's time over the work of a frame between
//!   [`Device::begin_timer`] and [`Device::end_timer`]:
//!   [`Device::create_timer`], on a device with [`Caps::timer_queries`].
//!   [`Device::timer_result`] gives the time once the GPU has done the
//!   work, a frame or two later, and never waits for it; [`TimerId`]
//! - [`Device::destroy`] takes any of them, as a [`ResourceId`].
//!
//! **Pipelines.** A [`PipelineDesc`] holds the [`ShaderSource`], one
//! [`VertexLayout`] per vertex buffer slot (its [`VertexStep`] and
//! [`VertexAttribute`]s, each a [`VertexFormat`]), the
//! [`UniformBlockDesc`]s and sampler names of the shaders, a [`BlendMode`],
//! a [`Topology`], and the [`TextureFormat`] it draws into.
//!
//! **Frames and passes.** A frame runs from [`Device::begin_frame`] to
//! [`Device::end_frame`], which shows it, or, on a device from
//! [`Device::new_headless`], from [`Device::begin_offscreen_frame`] to
//! [`Device::end_offscreen_frame`]. In a frame, [`Device::begin_pass`]
//! starts a [`Pass`] on a [`PassTarget`]: the window, or a render target.
//! The pass is set up with [`Pass::set_pipeline`],
//! [`Pass::set_vertex_buffer`], [`Pass::set_index_buffer`] (with an
//! [`IndexFormat`]), [`Pass::set_texture`] (a [`TextureRef`] and a
//! [`Sampler`]), [`Pass::set_uniforms`], [`Pass::set_viewport`] and
//! [`Pass::set_scissor`], and draws with [`Pass::draw`] and
//! [`Pass::draw_indexed`]. Dropping it ends it. [`Device::read_pixels`]
//! reads a render target back.
//!
//! **Data.** Buffers and uniforms take slices of [`Pod`] values, which
//! [`bytes_of`] views as bytes: `u8`, `u16`, `u32`, `i32` and `f32`, and
//! arrays of them. `Pod` is an `unsafe` trait, and crates that deny
//! `unsafe` cannot implement it, so they pack their data into such slices:
//!
//! ```
//! // Four corners of a unit quad, each a [f32; 2].
//! let corners: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
//! assert_eq!(nadir_gfx::bytes_of(&corners).len(), 32);
//! ```
//!
//! **Limits:** [`MAX_VERTEX_BUFFERS`], [`MAX_VERTEX_ATTRIBUTES`],
//! [`MAX_UNIFORM_BLOCKS`], [`MAX_UNIFORM_BYTES`] and [`MAX_TEXTURE_SLOTS`].
//!
//! # Example: a textured quad
//!
//! An indexed draw of a texture, into a render target on an offscreen
//! device, read back:
//!
//! ```no_run
//! use nadir_core::{LinearColor, UVec2};
//! use nadir_gfx::{
//!     BlendMode, BufferDesc, BufferKind, BufferUsage, Device, GfxError, IndexFormat,
//!     PipelineDesc, Sampler, ShaderSource, TextureDesc, TextureFormat, Topology,
//!     VertexAttribute, VertexFormat, VertexLayout, VertexStep,
//! };
//!
//! const VERTEX: &str = "#version 330 core
//! layout(location = 0) in vec2 position;
//! layout(location = 1) in vec2 uv;
//! out vec2 v_uv;
//! void main() {
//!     v_uv = uv;
//!     gl_Position = vec4(position, 0.0, 1.0);
//! }
//! ";
//!
//! const FRAGMENT: &str = "#version 330 core
//! uniform sampler2D image;
//! in vec2 v_uv;
//! out vec4 out_color;
//! void main() {
//!     out_color = texture(image, v_uv);
//! }
//! ";
//!
//! fn main() -> Result<(), GfxError> {
//!     let mut device = Device::new_headless()?;
//!
//!     // A 2 × 2 texture, rows from the top: red and green, then blue and
//!     // white. Its first row is v = 0.
//!     let texels: [u8; 16] = [
//!         255, 0, 0, 255, 0, 255, 0, 255, //
//!         0, 0, 255, 255, 255, 255, 255, 255,
//!     ];
//!     let texture = device.create_texture(
//!         &TextureDesc {
//!             label: "squares",
//!             size: UVec2::new(2, 2),
//!             format: TextureFormat::Rgba8Srgb,
//!         },
//!         Some(&texels),
//!     )?;
//!
//!     // The corners of clip space, each a position and a uv, so that the
//!     // texture's first row is at the top.
//!     let corners: [f32; 16] = [
//!         -1.0, -1.0, 0.0, 1.0, //
//!         1.0, -1.0, 1.0, 1.0, //
//!         -1.0, 1.0, 0.0, 0.0, //
//!         1.0, 1.0, 1.0, 0.0,
//!     ];
//!     let vertices = device.create_buffer(&BufferDesc {
//!         label: "corners",
//!         kind: BufferKind::Vertex,
//!         usage: BufferUsage::Static,
//!         size: size_of_val(&corners),
//!     })?;
//!     device.write_buffer(vertices, 0, &corners)?;
//!     let quad: [u16; 6] = [0, 1, 2, 2, 1, 3];
//!     let indices = device.create_buffer(&BufferDesc {
//!         label: "quad",
//!         kind: BufferKind::Index,
//!         usage: BufferUsage::Static,
//!         size: size_of_val(&quad),
//!     })?;
//!     device.write_buffer(indices, 0, &quad)?;
//!
//!     let pipeline = device.create_pipeline(&PipelineDesc {
//!         label: "textured",
//!         shader: ShaderSource {
//!             glsl_vs: VERTEX,
//!             glsl_fs: FRAGMENT,
//!             msl: "",
//!         },
//!         vertex_layouts: &[VertexLayout {
//!             stride: 16,
//!             step: VertexStep::Vertex,
//!             attributes: &[
//!                 VertexAttribute {
//!                     location: 0,
//!                     format: VertexFormat::Float32x2,
//!                     offset: 0,
//!                 },
//!                 VertexAttribute {
//!                     location: 1,
//!                     format: VertexFormat::Float32x2,
//!                     offset: 8,
//!                 },
//!             ],
//!         }],
//!         uniform_blocks: &[],
//!         textures: &["image"],
//!         blend: BlendMode::Alpha,
//!         topology: Topology::Triangles,
//!         target_format: TextureFormat::Rgba8Srgb,
//!     })?;
//!     let target = device.create_render_target(UVec2::new(32, 32), TextureFormat::Rgba8Srgb)?;
//!
//!     device.begin_offscreen_frame()?;
//!     {
//!         let mut pass = device.begin_pass(target.into(), Some(LinearColor::TRANSPARENT))?;
//!         pass.set_pipeline(pipeline)?;
//!         pass.set_vertex_buffer(0, vertices, 0)?;
//!         pass.set_index_buffer(indices, IndexFormat::U16)?;
//!         pass.set_texture(0, texture, Sampler::NearestClamp)?;
//!         pass.draw_indexed(0..6, 0, 0..1)?;
//!     }
//!     device.end_offscreen_frame()?;
//!
//!     // The top-left pixel shows the texture's first texel.
//!     let pixels = device.read_pixels(target)?;
//!     assert_eq!(pixels[..4], [255, 0, 0, 255]);
//!     Ok(())
//! }
//! ```
//!
//! # Example: a window
//!
//! A device for a window of [`nadir_platform`], cleared each frame until
//! the window is closed:
//!
//! ```no_run
//! use nadir_core::{LinearColor, UVec2};
//! use nadir_gfx::{Device, PassTarget};
//! use nadir_platform::{Event, Platform, WindowConfig};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut platform = Platform::new(&WindowConfig {
//!         title: "nadir-gfx".to_string(),
//!         size: UVec2::new(640, 360),
//!         min_size: UVec2::ZERO,
//!         resizable: true,
//!         fullscreen: false,
//!         vsync: true,
//!     })?;
//!     let mut device = Device::new(platform.gpu_surface())?;
//!     let mut events = Vec::new();
//!     loop {
//!         events.clear();
//!         platform.pump_events(&mut events);
//!         if events.contains(&Event::CloseRequested) {
//!             return Ok(());
//!         }
//!         device.begin_frame(platform.gpu_surface())?;
//!         let clear = LinearColor::new(0.02, 0.02, 0.05, 1.0);
//!         drop(device.begin_pass(PassTarget::Surface, Some(clear))?);
//!         device.end_frame(platform.gpu_surface())?;
//!     }
//! }
//! ```
//!
//! # Checked draws
//!
//! Setters fail at once on a stale id or an argument out of range, and
//! leave that setting unset. Each draw then checks what it reads before it
//! calls the GPU: a pipeline is set; every vertex slot, uniform block and
//! texture slot the pipeline has is set; every vertex, instance and index
//! record it reads, the base vertex included, is inside its buffer; each
//! uniform block has exactly its size. A pass may not sample its own target
//! ([`GfxError::FeedbackLoop`]), nor use a pipeline made for another format
//! ([`GfxError::FormatMismatch`]). A draw that fails draws nothing.
//!
//! The shaders' own code is trusted: an array index out of bounds in a
//! shader is undefined behavior that no check covers (see
//! [`ShaderSource`]). Each GLSL source must start with exactly
//! `#version 330 core`, with no `#extension` directive after it.
//!
//! Shaders read instance data only through [`VertexStep::Instance`]
//! attributes, never through `gl_InstanceID`: OpenGL 3.3 has no base
//! instance, so a draw of instances `n..m` starts the instance slots at
//! record `n`.
//!
//! # Errors
//!
//! Every fallible call returns a [`GfxError`], which implements
//! [`Display`](std::fmt::Display) and [`Error`](std::error::Error). The
//! errors a pass can return hold no heap data, so reporting one allocates
//! nothing. A shader that does not compile or link is [`GfxError::Shader`],
//! with the driver's log; a description that breaks a limit or does not
//! match its shaders is [`GfxError::Interface`].
//!
//! Debug builds check `glGetError` after every OpenGL call, and report
//! [`GfxError::Gl`] with the call that failed. Release builds check it once
//! a frame, before every draw, and before and after every call that
//! allocates, so no error goes unseen. Running out of GPU memory, or losing
//! the context, loses the device: every later call is
//! [`GfxError::ContextLost`].
//!
//! # Threading
//!
//! A device is neither `Send` nor `Sync`: it stays on the thread that made
//! it, where its context is current. A context has at most one device, and
//! a thread at most one whose context exists ([`GfxError::DeviceExists`]).
//! Resources are made, changed and destroyed outside passes; a [`Pass`]
//! borrows the device until it is dropped.
//!
//! # Orientation and color
//!
//! - Rectangles, such as a viewport or a texture region, are in pixels from
//!   the top left.
//! - A texture's first row of data is `v = 0`. On OpenGL a render target is
//!   stored from its bottom row: sampled, its `v = 0` is the bottom of what
//!   was drawn (clip space `y = -1`). [`Device::read_pixels`] gives rows
//!   from the top.
//! - [`TextureFormat::Rgba8Srgb`] textures decode to linear when sampled, and
//!   blending happens in linear light; the window's format is `Rgba8Srgb`
//!   (ADR-0013). [`TextureFormat::R8`] samples as `(r, 0, 0, 1)`.
//!
//! # Allocation
//!
//! Uniform data goes into a 64 KiB ring that gets new storage each frame
//! and whenever it fills, so uniforms take no heap allocation per draw.
//! What is written to a [`BufferUsage::Dynamic`] buffer is good for the
//! frame it is written in. The device keeps a copy of each index buffer,
//! to check indexed draws.
//!
//! # Platforms
//!
//! The crate is the same code on Linux, Windows and macOS. What it draws on
//! is the OpenGL context that `nadir-platform` makes, which exists on Linux
//! only so far (X11, through EGL; offscreen too). On Windows (roadmap
//! M2.6) and macOS (M2.7), a window's surface has no OpenGL functions, so
//! [`Device::new`] fails with [`GfxError::MissingFunction`], and
//! [`Device::new_headless`] fails with [`GfxError::NoOffscreenContext`].
//!
//! # Features
//!
//! `headless-test` builds the crate's own tests that draw on an offscreen
//! context, for the workspace's CI. A user does not need it.
//!
//! The design is in
//! [`docs/subsystems/gfx.md`](https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/gfx.md).

// One of the three crates allowed to contain `unsafe` (AGENTS.md, rule 3). The
// workspace denies `unsafe_code` everywhere else.
#![allow(unsafe_code)]

mod device;
mod error;
mod gl;
mod pod;
mod types;
mod validate;

#[cfg(test)]
mod tests;

// The README's examples are compiled as doc tests too.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

pub use device::{Device, Pass};
pub use error::GfxError;
pub use pod::{Pod, bytes_of};
pub use types::{
    BackendKind, BlendMode, BufferDesc, BufferId, BufferKind, BufferUsage, Caps, IndexFormat,
    MAX_TEXTURE_SLOTS, MAX_UNIFORM_BLOCKS, MAX_UNIFORM_BYTES, MAX_VERTEX_ATTRIBUTES,
    MAX_VERTEX_BUFFERS, PassTarget, PipelineDesc, PipelineId, RenderTargetId, ResourceId,
    ResourceKind, Sampler, ShaderSource, ShaderStage, TextureDesc, TextureFormat, TextureId,
    TextureRef, TimerId, Topology, UniformBlockDesc, VertexAttribute, VertexFormat, VertexLayout,
    VertexStep,
};
