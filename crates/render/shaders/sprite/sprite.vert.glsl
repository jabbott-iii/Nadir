#version 330 core
// One instanced quad per sprite. The instance attributes follow
// `SPRITE_INSTANCE_FLOATS` in `src/instance.rs`; the parity test checks them.
layout(location = 0) in vec2 position;
layout(location = 1) in vec2 size;
layout(location = 2) in vec2 pivot;
layout(location = 3) in vec4 uv_rect;
layout(location = 4) in vec4 color;
layout(location = 5) in float rotation;
// The unit quad, from (0, 0) at the sprite's top left to (1, 1).
layout(location = 6) in vec2 corner;

// World to clip space: clip = world * view.xy + view.zw.
layout(std140) uniform Camera {
    vec4 view;
};

out vec2 v_uv;
out vec4 v_color;

void main() {
    vec2 local = (corner - pivot) * size;
    // Clockwise on screen, since +Y is down.
    float c = cos(rotation);
    float s = sin(rotation);
    vec2 world = position + vec2(local.x * c - local.y * s, local.x * s + local.y * c);
    gl_Position = vec4(world * view.xy + view.zw, 0.0, 1.0);
    v_uv = mix(uv_rect.xy, uv_rect.zw, corner);
    v_color = color;
}
