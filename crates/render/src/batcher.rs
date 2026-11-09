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

//! [`SpriteBatcher`]: the work on the CPU of drawing sprites.

use crate::instance::push_instance;
use crate::plan::{Batch, GlyphQuad, Plan, Resolved};
use crate::view::pixel_zoom;
use crate::{Camera2D, FrameStats, SpriteDraw};

/// The program a batch draws with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Pipe {
    /// The `sprite` program: sprites, and the glyphs of
    /// [`GlyphMode::Rgba`](crate::GlyphMode::Rgba) texts and of pages that
    /// show their placeholder.
    Sprite,
    /// The `text` program: glyphs whose alpha is a mask.
    Text,
}

/// Sorts a frame's sprites, groups them into batches by texture, and packs
/// the instance of each one drawn: what the [`Renderer`](crate::Renderer)
/// does on the CPU before it uploads the instances and draws. It is public
/// so that this work can be measured without a GPU. The renderer gives it
/// the frame's texts' glyphs too, laid out, which it plans with the
/// sprites.
///
/// It is generic over the texture `K`: the renderer's is its GPU texture,
/// and a benchmark can use any number. Its lists are reused from one build
/// to the next, so a steady frame allocates nothing.
#[derive(Debug)]
pub struct SpriteBatcher<K> {
    plan: Plan<(K, Pipe)>,
    packed: Vec<f32>,
}

impl<K: Copy + Eq> SpriteBatcher<K> {
    /// A batcher that has built nothing yet.
    pub fn new() -> Self {
        Self {
            plan: Plan::new(),
            packed: Vec::new(),
        }
    }

    /// Sorts, batches and packs `sprites` as `camera` shows them, in
    /// pixel-perfect mode.
    ///
    /// `texture` gives each sprite's texture: `None` while it is not ready,
    /// and the sprite is not drawn. Otherwise it gives the texture, and
    /// whether the sprite shows that texture's placeholder, all of it
    /// whatever its `uv`, as a sprite whose texture failed to load does.
    pub fn build(
        &mut self,
        sprites: &[SpriteDraw],
        camera: &Camera2D,
        texture: impl FnMut(&SpriteDraw) -> Option<(K, bool)>,
    ) {
        self.build_with(sprites, &[], camera, texture);
    }

    /// Builds `sprites` and the texts' `glyphs`, whose textures and
    /// programs are found, as [`SpriteBatcher::build`] builds sprites: a
    /// batch is one texture and one program.
    pub(crate) fn build_with(
        &mut self,
        sprites: &[SpriteDraw],
        glyphs: &[GlyphQuad<(K, Pipe)>],
        camera: &Camera2D,
        mut texture: impl FnMut(&SpriteDraw) -> Option<(K, bool)>,
    ) {
        self.plan
            .build_with(sprites, glyphs, |sprite| match texture(sprite) {
                None => Resolved::Skip,
                Some((texture, false)) => Resolved::Draw((texture, Pipe::Sprite)),
                Some((texture, true)) => Resolved::Placeholder((texture, Pipe::Sprite)),
            });
        let zoom = pixel_zoom(camera.zoom);
        self.packed.clear();
        for drawn in &self.plan.order {
            let index = drawn.sprite as usize;
            let sprite = match sprites.get(index) {
                Some(sprite) => sprite,
                None => &glyphs[index - sprites.len()].sprite,
            };
            push_instance(&mut self.packed, sprite, drawn.placeholder, Some(zoom));
        }
    }

    /// The instances of the sprites and glyphs drawn, in draw order:
    /// [`SPRITE_INSTANCE_FLOATS`](crate::SPRITE_INSTANCE_FLOATS) floats
    /// each.
    pub fn instances(&self) -> &[f32] {
        &self.packed
    }

    /// What the last build drew. It knows nothing of tile layers or of
    /// texts: `tiles`, `chunks`, `texts` and `skipped_texts` are 0, and
    /// `batches` counts the batches before a tile layer drawn among the
    /// sprites splits one in two.
    pub fn stats(&self) -> FrameStats {
        // The plan numbers its sprites and glyphs with a u32, so these fit.
        FrameStats {
            sprites: self.plan.order.len() as u32 - self.plan.glyphs,
            skipped: self.plan.skipped,
            batches: self.plan.batches.len() as u32,
            tiles: 0,
            chunks: 0,
            texts: 0,
            glyphs: self.plan.glyphs,
            skipped_texts: 0,
        }
    }

    /// The batches of the last build, in draw order.
    pub(crate) fn batches(&self) -> &[Batch<(K, Pipe)>] {
        &self.plan.batches
    }

    /// Where tiles of `layer` and `depth` go among the sprites drawn: see
    /// [`Plan::position`].
    pub(crate) fn position(&self, layer: u8, depth: u32) -> u32 {
        self.plan.position(layer, depth)
    }
}

impl<K: Copy + Eq> Default for SpriteBatcher<K> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use nadir_assets::Handle;
    use nadir_core::{GenIndex, Vec2};

    use super::*;
    use crate::SPRITE_INSTANCE_FLOATS;

    fn sprite(texture: u32, y: f32) -> SpriteDraw {
        let id = GenIndex::new(texture, std::num::NonZeroU32::MIN);
        SpriteDraw::new(Handle::new(id), Vec2::new(10.0, y), Vec2::splat(8.0))
    }

    #[test]
    fn it_skips_what_is_not_ready_and_packs_the_rest_in_draw_order() {
        // Texture 0 is ready, 1 is loading, 2 failed and shows its
        // placeholder.
        let sprites = [
            sprite(0, 30.0),
            sprite(1, 10.0),
            sprite(2, 20.0),
            sprite(0, 5.0),
        ];
        let mut batcher = SpriteBatcher::new();
        batcher.build(&sprites, &Camera2D::default(), |sprite| {
            match sprite.texture.id().index() {
                1 => None,
                texture => Some((texture, texture == 2)),
            }
        });
        assert_eq!(
            batcher.stats(),
            FrameStats {
                sprites: 3,
                skipped: 1,
                batches: 3,
                ..FrameStats::default()
            }
        );
        let order: Vec<u32> = batcher
            .batches()
            .iter()
            .map(|batch| batch.texture.0)
            .collect();
        assert_eq!(order, [0, 2, 0], "by depth: y 5, then 20, then 30");
        // Every sprite draws with the sprite program, its placeholder too.
        assert!(
            batcher
                .batches()
                .iter()
                .all(|batch| batch.texture.1 == Pipe::Sprite)
        );
        assert_eq!(batcher.instances().len(), 3 * SPRITE_INSTANCE_FLOATS);

        // A second build replaces the first.
        batcher.build(&sprites[..1], &Camera2D::default(), |_| Some((0, false)));
        assert_eq!(batcher.stats().sprites, 1);
        assert_eq!(batcher.instances().len(), SPRITE_INSTANCE_FLOATS);
    }

    #[test]
    fn glyphs_are_packed_after_sprites_at_a_tie_and_split_batches_by_program() {
        let mut panel = sprite(0, 5.0);
        panel.sort = crate::SortMode::Fixed(3);
        let mut quad = panel;
        quad.position = Vec2::new(40.0, 7.0);
        quad.pivot = Vec2::ZERO;
        let glyph = |pipe| GlyphQuad {
            sprite: quad,
            text: 0,
            texture: (0, pipe),
            placeholder: false,
        };
        let glyphs = [glyph(Pipe::Text), glyph(Pipe::Sprite)];
        let mut batcher = SpriteBatcher::new();
        batcher.build_with(&[panel], &glyphs, &Camera2D::default(), |_| {
            Some((0, false))
        });
        let stats = batcher.stats();
        assert_eq!((stats.sprites, stats.glyphs, stats.batches), (1, 2, 3));
        let batches: Vec<(u32, Pipe)> = batcher.batches().iter().map(|b| b.texture).collect();
        assert_eq!(
            batches,
            [(0, Pipe::Sprite), (0, Pipe::Text), (0, Pipe::Sprite)]
        );
        // The glyph's instance is its own quad's: its corner at (40, 7).
        let second = &batcher.instances()[SPRITE_INSTANCE_FLOATS..];
        assert_eq!(second[..2], [40.0, 7.0]);
    }
}
