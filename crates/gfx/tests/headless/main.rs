//! Tests that draw on an offscreen OpenGL context, through the public API.
//!
//! They need the `headless-test` feature and a working driver. Run them on
//! Mesa's software renderer with:
//!
//! ```text
//! LIBGL_ALWAYS_SOFTWARE=1 NADIR_EXPECT_GL_RENDERER=llvmpipe \
//!     cargo test -p nadir-gfx --features headless-test
//! ```
//!
//! Where libglvnd also has NVIDIA's EGL, add
//! `__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json`.
//!
//! Expected pixels are exact where every driver tried agrees (fifths of 255,
//! nearest-sampled texels), and within a step or two where they round
//! differently (sRGB, blending). No test reads back a value that is half
//! way between two.

mod common;
mod drawing;
mod frames;
mod golden;
mod pipelines;
mod resources;
