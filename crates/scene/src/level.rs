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

//! Levels: the cooked level file, `levels/<name>.klvl`, as an asset
//! (`docs/subsystems/scene.md`, "Levels"; ADR-0038). Its entities spawn
//! through their prefabs with [`PrefabLibrary::spawn_level`]; its tile
//! layers, grid and tall tiles are the facade's to draw and collide with
//! (roadmap M5.6).
//!
//! The file's types are [`nadir_codec::level`]'s, re-exported here.
//!
//! [`PrefabLibrary::spawn_level`]: crate::PrefabLibrary::spawn_level

use nadir_assets::{Asset, AssetError, DecodeCtx, FinalizeCtx};
pub use nadir_codec::level::{
    Animation, Bounds, Cell, EntityEntry, GridEntry, LayerEntry, LevelFile, TallEntry, TilesetEntry,
};

/// A level: its cooked file, `levels/<name>.klvl`, read with
/// [`nadir_codec::level::parse`].
#[derive(Clone, Debug, PartialEq)]
pub struct Level {
    file: LevelFile,
}

impl Level {
    /// The level file: its size, tile sets and layers, grid, tall tiles
    /// and entities.
    pub fn file(&self) -> &LevelFile {
        &self.file
    }
}

impl Asset for Level {
    type Decoded = LevelFile;
    const EXTENSIONS: &'static [&'static str] = &["klvl"];

    fn decode(bytes: &[u8], ctx: &DecodeCtx<'_>) -> Result<LevelFile, AssetError> {
        nadir_codec::level::parse(bytes, ctx.limits()).map_err(AssetError::Decode)
    }

    fn finalize(file: LevelFile, _ctx: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        Ok(Self { file })
    }
}
