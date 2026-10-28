#version 330 core
// Straight alpha, for BlendMode::Alpha: the tile's texel as it is.
uniform sampler2D tile_texture;

in vec2 v_uv;
out vec4 out_color;

void main() {
    out_color = texture(tile_texture, v_uv);
}
