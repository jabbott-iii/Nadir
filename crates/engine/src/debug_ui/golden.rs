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

//! The facade's golden scene, which `cargo xtask golden` compares with
//! `tests/golden/<scene>.qoi` (`docs/04-quality.md`, ADR-0017): a fixed
//! layout of debug windows over the game's UI, drawn offscreen in the
//! engine's font, as render's scenes are drawn.
//!
//! When `NADIR_GOLDEN_DIR` names a directory, the scene writes its pixels
//! there as `<scene>.pam`; without it, it still draws and checks them.

use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use nadir_core::{Color, Key, MouseButton, Rect, UVec2, Vec2};
use nadir_debug::Ui;
use nadir_ecs::{FrameStage, World};
use nadir_gfx::GfxError;
use nadir_platform::Event;
use nadir_render::{RenderError, RenderFrame, SpriteDraw, Texture};

use super::font;
use crate::{App, Config, Error, Offscreen};

/// Names the directory the scene writes its pixels to.
const DIR_VAR: &str = "NADIR_GOLDEN_DIR";
/// Names a part of the renderer that the context must have.
const EXPECT_RENDERER: &str = "NADIR_EXPECT_GL_RENDERER";

const SIZE: UVec2 = UVec2::new(320, 180);
/// The world, scaled up twice into the output.
const VIRTUAL: UVec2 = UVec2::new(160, 90);
const CLEAR: [u8; 4] = [16, 64, 48, 255];
const GOLD: [u8; 4] = [255, 255, 0, 255];
/// The debug UI's default colors where they are opaque.
const TITLE: [u8; 4] = [48, 48, 72, 255];
const TITLE_FRONT: [u8; 4] = [64, 72, 128, 255];
const TEXT: [u8; 4] = [230, 230, 230, 255];
const WIDGET: [u8; 4] = [56, 56, 72, 255];
const ACCENT: [u8; 4] = [160, 200, 255, 255];

const TOOLS: Rect = Rect::new(Vec2::new(8.0, 8.0), Vec2::new(168.0, 132.0));
const STATS: Rect = Rect::new(Vec2::new(184.0, 16.0), Vec2::new(304.0, 80.0));

/// The scene's game: its UI a gold box, which the window `Tools` covers in
/// part, and two debug windows, `Tools` on F8 and `Stats` on F9.
fn app() -> App {
    App::new(Config {
        virtual_resolution: VIRTUAL,
        ..Config::default()
    })
    .add_frame_system(FrameStage::Ui, "game_ui", |world, _| {
        let white = world
            .resource_mut::<nadir_assets::AssetServer>()
            .load::<Texture>(font::WHITE);
        let mut frame = world.resource_mut::<RenderFrame>();
        frame.clear = Color::rgb(CLEAR[0], CLEAR[1], CLEAR[2]);
        // Pixel UI: two output pixels a unit, from (120, 80) to (200, 140).
        let mut sprite = SpriteDraw::new(white, Vec2::new(60.0, 40.0), Vec2::new(40.0, 30.0));
        sprite.pivot = Vec2::ZERO;
        sprite.color = Color::rgb(GOLD[0], GOLD[1], GOLD[2]);
        frame.ui.sprite(sprite);
    })
    .add_debug_window(Key::F8, "Tools", TOOLS, tools)
    .add_debug_window(Key::F9, "Stats", STATS, |_, ui| ui.label("hidden"))
}

/// A row of most kinds of widget: not the plot, whose lines are rotated
/// sprites, nor the drag, the colored label or the color edit.
fn tools(_: &World, ui: &mut Ui<'_>) {
    let mut paused = true;
    let mut speed = 0.5;
    let mut lives = 3;
    let mut name = String::from("hero");
    let mut mode = 1;
    ui.label("Nadir 0.3: ~{}[]|_!?");
    ui.checkbox("Paused", &mut paused);
    ui.slider_f32("Speed", &mut speed, 0.0..=1.0);
    ui.slider_i32("Lives", &mut lives, 0..=9);
    ui.text_input("Name", &mut name);
    ui.horizontal(|ui| {
        ui.button("Reset");
        ui.button("Step");
    });
    ui.separator();
    ui.combo("Mode", &mut mode, &["Walk", "Run"]);
    ui.tree_node("More", |ui| ui.label("inside"));
}

fn key(key: Key, pressed: bool) -> Event {
    Event::Key {
        key,
        pressed,
        repeat: false,
    }
}

/// `app()` drawn offscreen, or `None` where this system has no offscreen
/// context yet and no renderer is asked for.
fn offscreen() -> Option<Offscreen> {
    let expected = env::var(EXPECT_RENDERER).unwrap_or_default();
    match app().offscreen(SIZE) {
        Ok(offscreen) => Some(offscreen),
        Err(Error::Render(RenderError::Gfx(GfxError::NoOffscreenContext)))
            if expected.is_empty() =>
        {
            None
        }
        Err(error) => panic!("no offscreen OpenGL context: {error}"),
    }
}

/// The debug windows over the game's UI: `Tools`, every widget at rest,
/// behind and over a corner of the gold box; and `Stats`, collapsed and in
/// front.
#[test]
fn debug_window() {
    let Some(mut offscreen) = offscreen() else {
        return;
    };
    let frame = |offscreen: &mut Offscreen, events: &[Event]| {
        offscreen.frame(Duration::ZERO, events).expect("a frame")
    };
    // Shown: the font's page and the white texel are made after this
    // frame is drawn.
    frame(
        &mut offscreen,
        &[
            key(Key::F8, true),
            key(Key::F8, false),
            key(Key::F9, true),
            key(Key::F9, false),
        ],
    );
    // Stats collapsed by its box, left on its title bar, which brings it to
    // the front; then the pointer away from every window.
    let collapse = Vec2::new(STATS.min.x + 5.0, STATS.min.y + 7.0);
    frame(
        &mut offscreen,
        &[
            Event::MouseMoved(collapse),
            Event::MouseButton {
                button: MouseButton::Left,
                pressed: true,
            },
            Event::MouseButton {
                button: MouseButton::Left,
                pressed: false,
            },
        ],
    );
    frame(
        &mut offscreen,
        &[Event::MouseMoved(Vec2::new(300.0, 170.0))],
    );
    let bytes = frame(&mut offscreen, &[]);
    let pixels: Vec<[u8; 4]> = bytes.as_chunks::<4>().0.to_vec();
    save("debug_window", offscreen.renderer(), &pixels);

    let mut failed = Vec::new();
    let mut check = |(x, y): (u32, u32), expected: [u8; 4], what: &str| {
        let found = pixels[(y * SIZE.x + x) as usize];
        if !found.iter().zip(expected).all(|(&a, b)| a.abs_diff(b) <= 2) {
            failed.push(format!(
                "{what} at ({x}, {y}) is {found:?}, not {expected:?}"
            ));
        }
    };
    check((2, 2), CLEAR, "the world");
    check((300, 170), CLEAR, "the world");
    check((195, 135), GOLD, "the game's box");
    check((100, 10), TITLE, "the title bar of Tools, behind");
    check(
        (250, 18),
        TITLE_FRONT,
        "the title bar of Stats, collapsed in front",
    );
    check((250, 40), CLEAR, "the world below Stats");
    // The label's N, from (10, 27): the row's top at 24, 2 of padding and
    // the glyph a row down.
    check((10, 27), TEXT, "the label's N");
    check((17, 46), ACCENT, "the checkbox's mark");
    check((90, 95), WIDGET, "the text field");
    // The panel is see-through: over the gold box, it darkens it.
    let under = pixels[(100 * SIZE.x + 150) as usize];
    if !(under[0] < 128 && under[2] < 64 && under[0] > 32) {
        failed.push(format!("the panel over the box at (150, 100) is {under:?}"));
    }
    let renderer = offscreen.renderer().to_string();
    let expected = env::var(EXPECT_RENDERER).unwrap_or_default();
    if !renderer.contains(&expected) {
        failed.push(format!(
            "the renderer is `{renderer}`, and {EXPECT_RENDERER} asks for `{expected}`"
        ));
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Writes the pixels as `<name>.pam` (Netpbm PAM, RGBA8, from the top row)
/// in the `NADIR_GOLDEN_DIR` directory, if it is set, as render's scenes
/// do. The header names the renderer in a `# renderer` comment.
fn save(name: &str, renderer: &str, pixels: &[[u8; 4]]) {
    assert_eq!(pixels.len(), (SIZE.x * SIZE.y) as usize);
    let Some(dir) = env::var_os(DIR_VAR).filter(|dir| !dir.is_empty()) else {
        return;
    };
    // The header is one line of text a field: keep the name to printable
    // ASCII.
    let renderer: String = renderer
        .chars()
        .map(|c| {
            if c == ' ' || c.is_ascii_graphic() {
                c
            } else {
                '?'
            }
        })
        .collect();
    let mut bytes = format!(
        "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\n\
         # renderer {renderer}\nENDHDR\n",
        SIZE.x, SIZE.y
    )
    .into_bytes();
    bytes.extend_from_slice(pixels.as_flattened());
    let dir = Path::new(&dir);
    fs::create_dir_all(dir).expect("the golden directory");
    let path = dir.join(format!("{name}.pam"));
    // A file that is there already is another scene's, or a stale one:
    // xtask empties the directory before each run.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("cannot create `{}`: {error}", path.display()));
    file.write_all(&bytes).expect("the scene's pixels written");
}
