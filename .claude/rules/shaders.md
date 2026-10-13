---
paths:
  - "crates/render/shaders/**"
  - "**/*.glsl"
  - "**/*.metal"
---

# Rules for shaders

- **Both versions or neither.** Each program lives in
  `crates/render/shaders/<name>/` as `<name>.vert.glsl`, `<name>.frag.glsl`
  (GLSL `#version 330 core`) and `<name>.metal` (entry points `vs_main` and
  `fs_main`). Changing one means changing the other in the same task.
- **The Rust interface constant for the shader is the source of truth.**
  It lists attribute locations, uniform blocks and their sizes, and texture
  slots. Update it first, then make both sources match. The parity test
  must pass.
- **GLSL 330 constraints:**
  - No `layout(binding = N)`. Uniform blocks are bound from Rust
    (`glUniformBlockBinding`), and samplers via `glUniform1i`.
  - Use `layout(location = N)` for vertex inputs only.
- **Uniform blocks follow std140:**
  - `vec2` aligns to 8 bytes; `vec3` and `vec4` align to 16.
  - Array elements align to 16.
  - Prefer `vec4` packing and avoid `vec3`.
- **MSL buffer indices:** vertex buffers 0–7, uniform blocks 8–15. Texture
  and sampler indices equal the slot order in the interface constant.
- **Coordinates:** world space is +Y down. Clip-space conversion happens
  only in the view-projection matrix built in Rust, never ad hoc in shaders.
- **Color:** textures are sRGB-sampled (hardware converts to linear). Vertex
  colors arrive linear (ADR-0013). Output is premultiplied or straight alpha
  to match the pipeline's `BlendMode`; say which in a comment at the top of
  the fragment shader.
- **After any shader change,** run `cargo xtask golden`. If the change is an
  intended visual change, re-bless the references only after the user has
  reviewed the diff images.
- **Budget:** keep the program count under 20. Past that, ADR-0005 must be
  revisited.
