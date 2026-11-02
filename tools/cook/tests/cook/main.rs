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

//! The cook on real folders: what it writes, when it cooks again, and what
//! it refuses.

mod aseprite;
mod common;
mod data;
mod fonts;
mod incremental;
mod output;
mod rules;
mod workspace;

use std::path::Path;

use common::{plain, png, write};

/// A source tree of three groups: `chars` (a sheet of two frames and an
/// empty image), `world` (the folder `props`, renamed by its
/// `atlas.jsonc`) and `tiles` (a tile set), and a file nothing cooks.
pub(crate) fn tree(root: &Path) {
    // Two frames of 8 × 8, each a 4 × 6 block with a transparent border.
    let mut sheet = vec![0u8; 16 * 8 * 4];
    for (left, color) in [(1, [200, 10, 10, 255]), (10, [10, 200, 10, 255])] {
        for y in 1..7 {
            for x in left..left + 4 {
                let at = (y * 16 + x) * 4;
                sheet[at..at + 4].copy_from_slice(&color);
            }
        }
    }
    write(root, "chars/hero.png", png(16, 8, &sheet, &[]));
    write(
        root,
        "chars/hero.sprite.jsonc",
        r#"{ "frames": [8, 8], "pivot": [4, 8] }"#,
    );
    write(root, "chars/ghost.png", png(8, 8, &[0; 8 * 8 * 4], &[]));
    write(root, "props/rock.png", plain(10, 6, [90, 90, 100, 255]));
    write(root, "props/atlas.jsonc", r#"{ "group": "world" }"#);
    let mut ground = Vec::new();
    for y in 0..16u8 {
        for x in 0..32u8 {
            ground.extend_from_slice(&[x * 8, y * 16, 50, 255]);
        }
    }
    write(root, "tiles/ground.png", png(32, 16, &ground, &[]));
    write(
        root,
        "tiles/ground.sprite.jsonc",
        r#"{ "tiles": [16, 16] }"#,
    );
    write(root, "readme.md", "Not an asset.");
}
