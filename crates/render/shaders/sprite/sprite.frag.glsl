#version 330 core
// Straight alpha, for BlendMode::Alpha: the texel times the tint, which
// arrives linear (ADR-0013).
uniform sampler2D sprite_texture;

in vec2 v_uv;
in vec4 v_color;
out vec4 out_color;

void main() {
    out_color = texture(sprite_texture, v_uv) * v_color;
}
