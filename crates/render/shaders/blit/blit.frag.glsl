#version 330 core
// Opaque, for BlendMode::Opaque: a copy of the virtual image.
uniform sampler2D source;

in vec2 v_uv;
out vec4 out_color;

void main() {
    out_color = texture(source, v_uv);
}
