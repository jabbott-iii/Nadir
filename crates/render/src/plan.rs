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

//! The order sprites, and texts' glyphs, are drawn in, and the batches
//! they are drawn in.

use crate::SpriteDraw;
use crate::sort::sort_key;

/// What a sprite's texture is, for drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resolved<K> {
    /// Not ready yet: the sprite is not drawn.
    Skip,
    /// The texture.
    Draw(K),
    /// The placeholder of a texture that failed: all of it is drawn.
    Placeholder(K),
}

/// A run of sprites, consecutive in the order, drawn with one texture in
/// one instanced draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Batch<K> {
    pub(crate) texture: K,
    /// Where the run starts in [`Plan::order`], which is also where its
    /// instances start.
    pub(crate) start: u32,
    pub(crate) count: u32,
}

/// A glyph of a text, laid out, with its texture found: a sprite of its
/// text's layer, whose sort is its text's depth, `Fixed`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GlyphQuad<K> {
    pub(crate) sprite: SpriteDraw,
    /// Its text's place among the frame's texts.
    pub(crate) text: u32,
    pub(crate) texture: K,
    /// Whether it shows its texture's placeholder.
    pub(crate) placeholder: bool,
}

/// One drawn sprite or glyph, in draw order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Drawn {
    /// Its index among the frame's sprites, then the glyphs after them: a
    /// glyph's is the sprites' count plus its own.
    pub(crate) sprite: u32,
    /// Whether it shows its texture's placeholder.
    pub(crate) placeholder: bool,
}

/// The plan of a frame. Its lists are reused from frame to frame, so a
/// steady frame allocates nothing.
///
/// It is generic over the texture `K`, so that it can be tested and
/// measured without a GPU.
#[derive(Debug)]
pub(crate) struct Plan<K> {
    /// (key, group, index) of each drawn sprite and glyph: the group is 0
    /// for a sprite, and its text's place plus 1 for a glyph.
    keys: Vec<(u64, u32, u32)>,
    /// Each sprite's and glyph's texture, by index; `None` when skipped.
    textures: Vec<Option<(K, bool)>>,
    pub(crate) order: Vec<Drawn>,
    pub(crate) batches: Vec<Batch<K>>,
    /// The sprites skipped: not ready, or past `u32::MAX` of them.
    pub(crate) skipped: u32,
    /// The glyphs in `order`.
    pub(crate) glyphs: u32,
}

impl<K: Copy + Eq> Plan<K> {
    pub(crate) fn new() -> Self {
        Self {
            keys: Vec::new(),
            textures: Vec::new(),
            order: Vec::new(),
            batches: Vec::new(),
            skipped: 0,
            glyphs: 0,
        }
    }

    /// Plans `sprites`: sorts the ready ones by layer, depth and texture,
    /// keeping submission order among ties, and groups runs of one texture
    /// into batches.
    #[cfg(test)]
    pub(crate) fn build(
        &mut self,
        sprites: &[SpriteDraw],
        resolve: impl FnMut(&SpriteDraw) -> Resolved<K>,
    ) {
        self.build_with(sprites, &[], resolve);
    }

    /// Plans `sprites` and the texts' `glyphs`, whose textures are found:
    /// sorts them by layer and depth; at one depth, the sprites first, by
    /// texture then in submission order, then each text's glyphs in the
    /// order of the texts, by texture then in the order they were laid out.
    /// Runs of one texture `K` are batches.
    pub(crate) fn build_with(
        &mut self,
        sprites: &[SpriteDraw],
        glyphs: &[GlyphQuad<K>],
        mut resolve: impl FnMut(&SpriteDraw) -> Resolved<K>,
    ) {
        self.keys.clear();
        self.textures.clear();
        self.order.clear();
        self.batches.clear();
        self.skipped = 0;
        self.glyphs = 0;

        for (index, sprite) in sprites.iter().enumerate() {
            // A frame of more than u32::MAX sprites is far past any
            // instance buffer; such sprites are not drawn.
            let Ok(index) = u32::try_from(index) else {
                self.skipped = self.skipped.saturating_add(1);
                self.textures.push(None);
                continue;
            };
            let texture = match resolve(sprite) {
                Resolved::Skip => None,
                Resolved::Draw(texture) => Some((texture, false)),
                Resolved::Placeholder(texture) => Some((texture, true)),
            };
            self.textures.push(texture);
            if texture.is_none() {
                self.skipped += 1;
                continue;
            }
            self.keys.push((key_of(sprite, index), 0, index));
        }
        let first_glyph = self.textures.len();
        for (offset, glyph) in glyphs.iter().enumerate() {
            // As for sprites: past u32::MAX in all, they are not drawn.
            let Ok(index) = u32::try_from(first_glyph + offset) else {
                break;
            };
            self.textures.push(Some((glyph.texture, glyph.placeholder)));
            let group = glyph.text.saturating_add(1);
            self.keys.push((key_of(&glyph.sprite, index), group, index));
        }

        // The layer and depth first, then sprites before glyphs and each
        // text's glyphs together, then the texture. The submission number
        // breaks ties, not the 8 bits of `seq`, so that tied sprites keep
        // their order whatever comes before them.
        self.keys.sort_unstable_by_key(|&(key, group, index)| {
            (key >> 24, group, (key >> 8) & 0xFFFF, index)
        });

        for &(_, group, index) in &self.keys {
            let Some((texture, placeholder)) = self.textures[index as usize] else {
                continue;
            };
            let position = self.order.len() as u32;
            self.order.push(Drawn {
                sprite: index,
                placeholder,
            });
            if group > 0 {
                self.glyphs += 1;
            }
            match self.batches.last_mut() {
                Some(batch) if batch.texture == texture => batch.count += 1,
                _ => self.batches.push(Batch {
                    texture,
                    start: position,
                    count: 1,
                }),
            }
        }
    }

    /// Where tiles of `layer` and `depth` go in the order: before every
    /// drawn sprite of the same layer and depth or later, after every one
    /// before. The keys are sorted, so their layer and depth bits are too.
    pub(crate) fn position(&self, layer: u8, depth: u32) -> u32 {
        let bound = u64::from(layer) << 32 | u64::from(depth);
        // The plan numbers its sprites with a u32, so this fits.
        self.keys.partition_point(|&(key, _, _)| key >> 24 < bound) as u32
    }
}

/// The sort key of the sprite or glyph `sprite`, the `index`th.
fn key_of(sprite: &SpriteDraw, index: u32) -> u64 {
    let texture_bits = sprite.texture.id().index() as u16;
    sort_key(
        sprite.layer,
        sprite.sort,
        sprite.position.y,
        texture_bits,
        index as u8,
    )
}

#[cfg(test)]
mod tests {
    use nadir_assets::Handle;
    use nadir_core::{GenIndex, Vec2};

    use super::*;
    use crate::SortMode;

    /// A sprite whose texture handle is `texture`, with its feet at `y`.
    fn sprite(texture: u32, y: f32) -> SpriteDraw {
        let id = GenIndex::new(texture, std::num::NonZeroU32::MIN);
        SpriteDraw::new(Handle::new(id), Vec2::new(0.0, y), Vec2::ONE)
    }

    /// Plans with each sprite's handle index as its texture.
    fn plan(sprites: &[SpriteDraw]) -> Plan<u32> {
        let mut plan = Plan::new();
        plan.build(sprites, |sprite| {
            Resolved::Draw(sprite.texture.id().index())
        });
        plan
    }

    fn order(plan: &Plan<u32>) -> Vec<u32> {
        plan.order.iter().map(|drawn| drawn.sprite).collect()
    }

    fn batches(plan: &Plan<u32>) -> Vec<(u32, u32, u32)> {
        plan.batches
            .iter()
            .map(|batch| (batch.texture, batch.start, batch.count))
            .collect()
    }

    #[test]
    fn sprites_are_drawn_from_the_top_of_the_screen_down() {
        let plan = plan(&[sprite(0, 30.0), sprite(0, 10.0), sprite(0, 20.0)]);
        assert_eq!(order(&plan), [1, 2, 0]);
        assert_eq!(batches(&plan), [(0, 0, 3)]);
    }

    #[test]
    fn a_new_texture_starts_a_new_batch() {
        // A, B, A at three depths: three batches, as y-sorting interleaves
        // them.
        let plan = plan(&[sprite(1, 10.0), sprite(2, 20.0), sprite(1, 30.0)]);
        assert_eq!(batches(&plan), [(1, 0, 1), (2, 1, 1), (1, 2, 1)]);
        // At one depth, sprites group by texture.
        let plan = self::plan(&[sprite(1, 5.0), sprite(2, 5.0), sprite(1, 5.0)]);
        assert_eq!(batches(&plan), [(1, 0, 2), (2, 2, 1)]);
        assert_eq!(order(&plan), [0, 2, 1]);
    }

    #[test]
    fn layers_come_before_depth() {
        let mut ground = sprite(0, 100.0);
        ground.sort = SortMode::Fixed(0);
        let mut top = sprite(0, -100.0);
        top.layer = 2;
        let plan = plan(&[top, sprite(0, 0.0), ground]);
        assert_eq!(order(&plan), [2, 1, 0]);
    }

    #[test]
    fn ties_keep_their_submission_order_whatever_comes_first() {
        // Two decals at one depth, submitted at 255 and 256: their `seq`
        // bits are 255 and 0, yet the first stays behind.
        let mut sprites = vec![sprite(0, 0.0); 257];
        for sprite in &mut sprites {
            sprite.sort = SortMode::Fixed(7);
        }
        let plan = plan(&sprites);
        assert_eq!(order(&plan), (0..257).collect::<Vec<u32>>());
    }

    #[test]
    fn sprites_not_ready_are_skipped_and_placeholders_marked() {
        let sprites = [sprite(1, 10.0), sprite(2, 20.0), sprite(3, 30.0)];
        let mut plan = Plan::new();
        plan.build(&sprites, |sprite| match sprite.texture.id().index() {
            1 => Resolved::Skip,
            2 => Resolved::Placeholder(20),
            texture => Resolved::Draw(texture),
        });
        assert_eq!(plan.skipped, 1);
        assert_eq!(
            plan.order,
            [
                Drawn {
                    sprite: 1,
                    placeholder: true
                },
                Drawn {
                    sprite: 2,
                    placeholder: false
                },
            ]
        );
        assert_eq!(batches(&plan), [(20, 0, 1), (3, 1, 1)]);
    }

    #[test]
    fn tiles_go_before_the_sprites_of_their_layer_and_depth() {
        let mut ground = sprite(0, 100.0);
        ground.sort = SortMode::Fixed(0);
        let mut above = sprite(0, -100.0);
        above.layer = 2;
        let mut front = sprite(1, 50.0);
        front.sort = SortMode::Fixed(u32::MAX);
        // Drawn: ground, the y-sorted sprite, front, then above.
        let plan = plan(&[above, sprite(0, 0.0), ground, front]);
        assert_eq!(order(&plan), [2, 1, 3, 0]);
        assert_eq!(plan.position(0, 0), 0, "before the ground decal");
        assert_eq!(plan.position(0, 1), 1, "after it");
        assert_eq!(plan.position(0, u32::MAX), 2, "before front");
        assert_eq!(plan.position(1, 0), 3);
        assert_eq!(plan.position(2, 0), 3, "before the sprite above");
        assert_eq!(plan.position(3, 0), 4, "after everything");
    }

    /// A glyph of text `text`, on texture `texture`, at depth `depth` of
    /// layer 0.
    fn glyph(text: u32, texture: u32, depth: u32) -> GlyphQuad<u32> {
        let mut sprite = sprite(texture, 0.0);
        sprite.sort = SortMode::Fixed(depth);
        GlyphQuad {
            sprite,
            text,
            texture,
            placeholder: false,
        }
    }

    fn plan_with(sprites: &[SpriteDraw], glyphs: &[GlyphQuad<u32>]) -> Plan<u32> {
        let mut plan = Plan::new();
        plan.build_with(sprites, glyphs, |sprite| {
            Resolved::Draw(sprite.texture.id().index())
        });
        plan
    }

    #[test]
    fn at_a_tie_texts_go_after_sprites_whatever_their_textures() {
        // A panel on texture 5, and a text on its font's page, texture 1,
        // at one depth: the text is in front, though its texture's bits are
        // lower.
        let mut panel = sprite(5, 0.0);
        panel.sort = SortMode::Fixed(10);
        let plan = plan_with(&[panel], &[glyph(0, 1, 10), glyph(0, 1, 10)]);
        assert_eq!(order(&plan), [0, 1, 2]);
        assert_eq!(batches(&plan), [(5, 0, 1), (1, 1, 2)]);
        assert_eq!(plan.glyphs, 2);
        // A text at a lower depth goes behind.
        let plan = plan_with(&[panel], &[glyph(0, 1, 9)]);
        assert_eq!(order(&plan), [1, 0]);
    }

    #[test]
    fn texts_keep_their_order_and_a_text_s_glyphs_group_by_page() {
        // A shadow text (0) then its text (1), at one depth, on a font of
        // two pages, 3 and 2: each text's glyphs by page, the shadow's
        // first.
        let glyphs = [
            glyph(0, 3, 4),
            glyph(0, 2, 4),
            glyph(0, 3, 4),
            glyph(1, 3, 4),
            glyph(1, 2, 4),
        ];
        let plan = plan_with(&[], &glyphs);
        assert_eq!(order(&plan), [1, 0, 2, 4, 3]);
        assert_eq!(batches(&plan), [(2, 0, 1), (3, 1, 2), (2, 3, 1), (3, 4, 1)]);
    }

    #[test]
    fn tiles_go_before_the_glyphs_of_their_layer_and_depth() {
        let mut ground = sprite(0, 0.0);
        ground.sort = SortMode::Fixed(3);
        let plan = plan_with(&[ground], &[glyph(0, 1, 3), glyph(0, 1, 4)]);
        assert_eq!(order(&plan), [0, 1, 2]);
        assert_eq!(plan.position(0, 3), 0);
        assert_eq!(
            plan.position(0, 4),
            2,
            "after the sprite and the glyph at 3"
        );
        assert_eq!(plan.position(0, 5), 3);
    }

    #[test]
    fn a_glyph_s_texture_and_placeholder_are_its_own() {
        let mut shown = glyph(0, 7, 0);
        shown.placeholder = true;
        let mut plan = Plan::new();
        // The resolver is for sprites alone.
        plan.build_with(&[sprite(1, 0.0)], &[shown], |_| Resolved::Skip);
        assert_eq!(plan.skipped, 1);
        assert_eq!(
            plan.order,
            [Drawn {
                sprite: 1,
                placeholder: true
            }]
        );
        assert_eq!(batches(&plan), [(7, 0, 1)]);
    }

    #[test]
    fn a_plan_is_reused() {
        let mut plan = plan(&[sprite(0, 1.0), sprite(1, 2.0)]);
        plan.build(&[sprite(5, 0.0)], |sprite| {
            Resolved::Draw(sprite.texture.id().index())
        });
        assert_eq!(order(&plan), [0]);
        assert_eq!(batches(&plan), [(5, 0, 1)]);
        assert_eq!(plan.skipped, 0);
        let mut plan = plan_with(&[], &[glyph(0, 1, 0)]);
        plan.build(&[sprite(5, 0.0)], |sprite| {
            Resolved::Draw(sprite.texture.id().index())
        });
        assert_eq!(plan.glyphs, 0);
    }
}
