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
