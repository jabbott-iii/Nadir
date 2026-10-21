#version 330 core
// A quad over the whole viewport. Its clip-space corners and texture
// coordinates come from Rust (`BLIT_QUAD` in `src/renderer.rs`), which also
// accounts for GL storing a render target from its bottom row.
layout(location = 0) in vec2 position;
layout(location = 1) in vec2 uv;

out vec2 v_uv;

void main() {
    v_uv = uv;
    gl_Position = vec4(position, 0.0, 1.0);
}
