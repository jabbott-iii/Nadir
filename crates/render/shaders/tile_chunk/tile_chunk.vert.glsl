#version 330 core
// One instanced quad per tile of a chunk. The instance attributes follow
// `TILE_INSTANCE_FLOATS` in `src/tiles.rs`; the parity test checks them.
// The cell's column and row in the layer.
layout(location = 0) in vec2 cell;
// The tile: index, frames, frame length in ms, flip (1: x, 2: y).
layout(location = 1) in vec4 tile;
// The unit quad, from (0, 0) at the tile's top left to (1, 1).
layout(location = 2) in vec2 corner;

// World to clip space: clip = world * view.xy + view.zw.
layout(std140) uniform Camera {
    vec4 view;
};

// The layer, packed by `layer_uniform` in `src/chunks.rs`:
// - placement: the top-left corner of cell (0, 0), then the tile size
// - atlas: the tile set's columns and rows, then 1 when every tile shows
//   all of the texture (a failed tile set's placeholder)
// - clock.x: the frame's time in milliseconds, wrapping
layout(std140) uniform TileLayer {
    vec4 placement;
    vec4 atlas;
    uvec4 clock;
};

out vec2 v_uv;

void main() {
    vec2 world = placement.xy + (cell + corner) * placement.z;
    gl_Position = vec4(world * view.xy + view.zw, 0.0, 1.0);

    // The frame and the tile, in whole numbers. GLSL 330 converts nothing
    // to uint by itself, so every operand says so.
    uint frames = max(uint(tile.y), 1u);
    uint frame_ms = max(uint(tile.z), 1u);
    uint columns = max(uint(atlas.x), 1u);
    uint rows = max(uint(atlas.y), 1u);
    uint index = (uint(tile.x) + (clock.x / frame_ms) % frames) % (columns * rows);
    uint flip = uint(tile.w);

    vec2 texel_corner = corner;
    if ((flip & 1u) != 0u) {
        texel_corner.x = 1.0 - texel_corner.x;
    }
    if ((flip & 2u) != 0u) {
        texel_corner.y = 1.0 - texel_corner.y;
    }
    vec2 first = vec2(float(index % columns), float(index / columns));
    vec2 grid = vec2(float(columns), float(rows));
    v_uv = atlas.z > 0.5 ? texel_corner : (first + texel_corner) / grid;
}
