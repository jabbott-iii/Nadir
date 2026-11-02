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

//! Tests that draw on an offscreen OpenGL context from
//! `Device::new_headless`: textures, the renderer's sprites and tile
//! layers, and the golden scenes.
//!
//! They need the `headless-test` feature and a working driver:
//!
//! ```text
//! LIBGL_ALWAYS_SOFTWARE=1 NADIR_EXPECT_GL_RENDERER=llvmpipe \
//!     cargo test -p nadir-render --features headless-test
//! ```
//!
//! Where libglvnd also has NVIDIA's EGL, add
//! `__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json`.

mod common;
mod golden;
mod sprites;
mod textures;
mod tiles;
