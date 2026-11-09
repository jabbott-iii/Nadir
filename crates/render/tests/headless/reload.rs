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

//! Reloads drawn: programs rebuilt from new sources, and textures reloaded
//! behind their handles.

use std::fs;
use std::path::Path;

use nadir_assets::{Handle, LoadState};
use nadir_core::{Color, IRect, IVec2, UVec2, Vec2};
use nadir_gfx::{GfxError, ResourceKind, ShaderSource, ShaderStage};
use nadir_render::{
    Camera2D, Font, Program, RenderError, RenderFrame, SHADER_DIR, SpriteDraw, TextDraw, Texture,
    Tile, TileLayer, TileLayerDraw, TileSet,
};

use crate::common::{Art, Stage, solid, write_image};

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 255];

/// The view: a sprite over x 0 to 4, a tile layer over x 4 to 8, a text
/// over x 8 to 12, and the clear color over x 12 to 16.
const SIZE: UVec2 = UVec2::new(16, 4);

/// What a scene draws with.
struct Scene {
    sprite: Handle<Texture>,
    tiles: Handle<TileLayer>,
    font: Handle<Font>,
}

/// A stage for `test`, with a red sprite, a layer of one red tile, and a
/// font of one white glyph, a block, drawn red.
fn scene(test: &str) -> Option<(Stage, Scene)> {
    let mut stage = Stage::new(test, SIZE)?;
    let sprite = stage.image("sprite.qoi", 4, 4, &solid(16, RED));
    let texture = stage.image("tiles.qoi", 4, 4, &solid(16, RED));
    let block = Art {
        id: '#',
        rows: &["####"; 4],
        offset: (0, 0),
        advance: 4,
    };
    let font = stage.font("block", 4, &[block], '#', &[]);
    stage.finish();
    let set = TileSet {
        texture,
        columns: 1,
        rows: 1,
    };
    let mut layer = TileLayer::new(set, UVec2::ONE, 4.0, Vec2::new(4.0, 0.0));
    layer.set(IVec2::ZERO, Some(Tile::new(0)));
    let tiles = stage.tiles.add(layer);
    Some((
        stage,
        Scene {
            sprite,
            tiles,
            font,
        },
    ))
}

/// Draws the scene, and gives the colors of the sprite, the tile, the text
/// and the clear color.
fn draw(stage: &mut Stage, scene: &Scene) -> [[u8; 4]; 4] {
    let mut frame = RenderFrame::new();
    frame.camera = Camera2D {
        position: SIZE.as_vec2() * 0.5,
        ..Camera2D::default()
    };
    frame.sprites.push(SpriteDraw::new(
        scene.sprite,
        Vec2::new(2.0, 4.0),
        Vec2::splat(4.0),
    ));
    frame.tile_layers.push(TileLayerDraw::new(scene.tiles));
    let mut text = TextDraw::new(scene.font, Vec2::new(8.0, 0.0));
    text.color = Color::rgb(255, 0, 0);
    frame.texts.push(text, "#");
    let (stats, pixels) = stage.draw(&frame, SIZE);
    assert_eq!((stats.sprites, stats.tiles, stats.glyphs), (1, 1, 1));
    [2, 6, 10, 14].map(|x| pixels[(2 * SIZE.x + x) as usize])
}

/// A program's sources, read from its files under `SHADER_DIR`.
fn sources(program: Program) -> (String, String) {
    let read = |file: &str| {
        fs::read_to_string(Path::new(SHADER_DIR).join(file)).expect("the program's file")
    };
    (read(program.vertex_file()), read(program.fragment_file()))
}

/// `fragment` with its color's red and blue swapped at the end of `main`.
fn swapped(fragment: &str) -> String {
    let end = fragment.rfind('}').expect("the end of main");
    format!(
        "{}    out_color = out_color.bgra;\n{}",
        &fragment[..end],
        &fragment[end..]
    )
}

/// Rebuilds `program` from `vertex` and `fragment`.
fn reload(
    stage: &mut Stage,
    program: Program,
    vertex: &str,
    fragment: &str,
) -> Result<(), RenderError> {
    let source = ShaderSource {
        glsl_vs: vertex,
        glsl_fs: fragment,
        msl: "",
    };
    stage
        .renderer
        .reload_program(&mut stage.device, program, source)
}

#[test]
fn a_reloaded_program_draws_with_its_new_sources_from_the_next_frame() {
    // What each program's swap turns blue: the blit swaps the whole image,
    // whose clear color is the same either way.
    let expected = [
        (Program::Sprite, [BLUE, RED, RED, CLEAR]),
        (Program::TileChunk, [RED, BLUE, RED, CLEAR]),
        (Program::Blit, [BLUE, BLUE, BLUE, CLEAR]),
        (Program::Text, [RED, RED, BLUE, CLEAR]),
    ];
    assert_eq!(expected.len(), Program::ALL.len());
    for (program, after) in expected {
        let Some((mut stage, scene)) = scene(&format!("reload_{}", program.name())) else {
            return;
        };
        assert_eq!(draw(&mut stage, &scene), [RED, RED, RED, CLEAR]);
        let (vertex, fragment) = sources(program);
        reload(&mut stage, program, &vertex, &swapped(&fragment)).expect("a program");
        assert_eq!(draw(&mut stage, &scene), after, "{}", program.name());
        // Its files as they are bring it back.
        reload(&mut stage, program, &vertex, &fragment).expect("a program");
        assert_eq!(draw(&mut stage, &scene), [RED, RED, RED, CLEAR]);
    }
}

#[test]
fn sources_that_do_not_build_are_refused_and_the_last_program_stays() {
    let Some((mut stage, scene)) = scene("reload_broken") else {
        return;
    };
    let (vertex, fragment) = sources(Program::Sprite);
    reload(&mut stage, Program::Sprite, &vertex, &swapped(&fragment)).expect("a program");
    let last = draw(&mut stage, &scene);
    assert_eq!(last, [BLUE, RED, RED, CLEAR]);

    let broken_fragment = fragment.replace("out_color = texture(", "out_color = = texture(");
    let broken_vertex = vertex.replace("v_color = color;", "v_color = color");
    // A fragment shader without `main` compiles, and does not link.
    let unlinked = fragment.replace("void main()", "void colour()");
    for (edited_vertex, edited_fragment, failed) in [
        (&vertex, &broken_fragment, ShaderStage::Fragment),
        (&broken_vertex, &fragment, ShaderStage::Vertex),
        (&vertex, &unlinked, ShaderStage::Link),
    ] {
        assert_ne!((edited_vertex, edited_fragment), (&vertex, &fragment));
        match reload(&mut stage, Program::Sprite, edited_vertex, edited_fragment) {
            Err(RenderError::Gfx(GfxError::Shader {
                label,
                stage: at,
                log,
            })) => {
                assert_eq!((label.as_str(), at), ("sprite", failed), "{log}");
                assert!(!log.is_empty(), "the driver's log");
            }
            other => panic!("{failed:?}: {other:?}"),
        }
        assert_eq!(
            draw(&mut stage, &scene),
            last,
            "{failed:?}: the last program"
        );
    }
}

#[test]
fn sources_of_another_interface_are_refused_and_the_last_program_stays() {
    let Some((mut stage, scene)) = scene("reload_interface") else {
        return;
    };
    let (vertex, fragment) = sources(Program::TileChunk);
    reload(&mut stage, Program::TileChunk, &vertex, &swapped(&fragment)).expect("a program");
    let last = draw(&mut stage, &scene);
    assert_eq!(last, [RED, BLUE, RED, CLEAR]);

    let moved = vertex.replace(
        "location = 2) in vec2 corner",
        "location = 3) in vec2 corner",
    );
    let renamed = fragment.replace("tile_texture", "tile_picture");
    // The device finds this one: the block is declared, and larger.
    let grown = vertex.replace("uvec4 clock;", "uvec4 clock;\n    vec4 more;");
    for (edited_vertex, edited_fragment, says) in [
        (&moved, &fragment, "`layout(location = 3) in vec2 corner;`"),
        (&vertex, &renamed, "`uniform sampler2D tile_picture;`"),
        (&grown, &fragment, "uniform block `TileLayer` is 64 bytes"),
    ] {
        assert_ne!((edited_vertex, edited_fragment), (&vertex, &fragment));
        match reload(
            &mut stage,
            Program::TileChunk,
            edited_vertex,
            edited_fragment,
        ) {
            Err(RenderError::Gfx(GfxError::Interface { label, reason })) => {
                assert_eq!(label, "tile_chunk");
                assert!(reason.contains(says), "{reason}");
            }
            other => panic!("{says}: {other:?}"),
        }
        assert_eq!(draw(&mut stage, &scene), last, "{says}: the last program");
    }
}

#[test]
fn a_font_s_page_reloaded_behind_its_handle_draws_from_the_next_frame() {
    let Some((mut stage, scene)) = scene("reload_font_page") else {
        return;
    };
    assert_eq!(draw(&mut stage, &scene), [RED, RED, RED, CLEAR]);
    // The block's page, 5 × 4, made clear: the glyph has no pixels left.
    write_image(
        &stage.root.join("atlas/block_0.qoi"),
        5,
        4,
        &solid(20, [0; 4]),
    );
    assert_eq!(stage.server.reload("atlas/block_0.qoi"), 1);
    assert_eq!(draw(&mut stage, &scene), [RED, RED, RED, CLEAR]);
    stage.finish();
    assert_eq!(draw(&mut stage, &scene), [RED, RED, CLEAR, CLEAR]);
}

#[test]
fn a_texture_reloaded_behind_its_handle_draws_its_new_texels() {
    let Some((mut stage, scene)) = scene("reload_texture") else {
        return;
    };
    assert_eq!(draw(&mut stage, &scene), [RED, RED, RED, CLEAR]);
    let old = stage.server.get(scene.sprite).map(Texture::id);
    // Of another size: the sprite's uv spans all of it either way.
    write_image(&stage.root.join("sprite.qoi"), 2, 2, &solid(4, BLUE));
    write_image(&stage.root.join("tiles.qoi"), 4, 4, &solid(16, BLUE));
    assert_eq!(stage.server.reload("sprite.qoi"), 1);
    assert_eq!(stage.server.reload("tiles.qoi"), 1);
    // Until a finish makes them, the old texels are drawn.
    assert_eq!(draw(&mut stage, &scene), [RED, RED, RED, CLEAR]);

    stage.finish();
    assert_eq!(draw(&mut stage, &scene), [BLUE, BLUE, RED, CLEAR]);
    let texture = *stage.server.get(scene.sprite).expect("the new texture");
    assert_eq!(stage.server.state(scene.sprite), Some(LoadState::Loaded));
    assert_eq!(texture.size(), UVec2::new(2, 2));
    assert_ne!(Some(texture.id()), old);
    let swapped: Vec<Handle<Texture>> = stage.server.drain_swapped::<Texture>().collect();
    assert_eq!(swapped.len(), 2);
    assert!(swapped.contains(&scene.sprite));
    // The old texture is gone from the device.
    let texel = IRect::new(IVec2::ZERO, IVec2::ONE);
    assert_eq!(
        stage
            .device
            .update_texture(old.expect("the old texture"), texel, &[0, 0, 0, 255]),
        Err(GfxError::StaleHandle(ResourceKind::Texture))
    );

    // A reload that fails keeps the texels it had.
    fs::write(stage.root.join("sprite.qoi"), b"not an image").expect("write the file");
    assert_eq!(stage.server.reload("sprite.qoi"), 1);
    stage.finish();
    assert_eq!(draw(&mut stage, &scene), [BLUE, BLUE, RED, CLEAR]);
    assert_eq!(
        stage.server.get(scene.sprite).map(Texture::id),
        Some(texture.id())
    );
}
