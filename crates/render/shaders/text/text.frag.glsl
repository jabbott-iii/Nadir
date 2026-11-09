#version 330 core
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

// Straight alpha, for BlendMode::Alpha: the glyph's alpha is a mask, and
// the tint, which arrives linear (ADR-0013), gives the color. The texel's
// own color is not read, so a page's glyphs may be any color.
uniform sampler2D text_texture;

in vec2 v_uv;
in vec4 v_color;
out vec4 out_color;

void main() {
    out_color = vec4(v_color.rgb, v_color.a * texture(text_texture, v_uv).a);
}
