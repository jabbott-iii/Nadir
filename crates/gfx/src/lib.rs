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

//! Small render hardware interface (RHI) for the Nadir engine, with an
//! OpenGL 3.3 core backend and, from M10, a Metal backend.
//!
//! A [`Device`] is made for the [`GpuSurface`](nadir_platform::GpuSurface)
//! of a window, or of an offscreen context, and makes the resources:
//! textures, buffers, render targets and pipelines, each named by a
//! generational id. A frame is passes between [`Device::begin_frame`] and
//! [`Device::end_frame`], and each [`Pass`] draws into one target.
//!
//! Every draw is checked: it fails, and does not draw, if a vertex, instance
//! or index read that the device sets up would fall outside its buffer, if
//! a uniform block lacks its data, or if a resource it names was destroyed
//! ([`GfxError::StaleHandle`]). The shaders' own code is trusted (see
//! [`ShaderSource`]).
//!
//! Debug builds check `glGetError` after every OpenGL call. Release builds
//! check it once a frame, before every draw, and before and after every
//! call that allocates, so no error goes unseen.

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

pub use device::{Device, Pass};
pub use error::GfxError;
pub use pod::{Pod, bytes_of};
pub use types::{
    BackendKind, BlendMode, BufferDesc, BufferId, BufferKind, BufferUsage, Caps, IndexFormat,
    MAX_TEXTURE_SLOTS, MAX_UNIFORM_BLOCKS, MAX_UNIFORM_BYTES, MAX_VERTEX_ATTRIBUTES,
    MAX_VERTEX_BUFFERS, PassTarget, PipelineDesc, PipelineId, RenderTargetId, ResourceId,
    ResourceKind, Sampler, ShaderSource, ShaderStage, TextureDesc, TextureFormat, TextureId,
    TextureRef, Topology, UniformBlockDesc, VertexAttribute, VertexFormat, VertexLayout,
    VertexStep,
};
