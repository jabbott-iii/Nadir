//! The order sprites are drawn in, and the batches they are drawn in.

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

/// One drawn sprite, in draw order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Drawn {
    /// Its index in the frame's sprites.
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
    /// (key, sprite index) of each drawn sprite.
    keys: Vec<(u64, u32)>,
    /// Each sprite's texture, by index; `None` when skipped.
    textures: Vec<Option<(K, bool)>>,
    pub(crate) order: Vec<Drawn>,
    pub(crate) batches: Vec<Batch<K>>,
    pub(crate) skipped: u32,
}

impl<K: Copy + Eq> Plan<K> {
    pub(crate) fn new() -> Self {
        Self {
            keys: Vec::new(),
            textures: Vec::new(),
            order: Vec::new(),
            batches: Vec::new(),
            skipped: 0,
        }
    }

    /// Plans `sprites`: sorts the ready ones by layer, depth and texture,
    /// keeping submission order among ties, and groups runs of one texture
    /// into batches.
    pub(crate) fn build(
        &mut self,
        sprites: &[SpriteDraw],
        mut resolve: impl FnMut(&SpriteDraw) -> Resolved<K>,
    ) {
        self.keys.clear();
        self.textures.clear();
        self.order.clear();
        self.batches.clear();
        self.skipped = 0;

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
            let texture_bits = sprite.texture.id().index() as u16;
            let key = sort_key(
                sprite.layer,
                sprite.sort,
                sprite.position.y,
                texture_bits,
                index as u8,
            );
            self.keys.push((key, index));
        }

        // The submission number breaks ties, not the 8 bits of `seq`, so
        // that tied sprites keep their order whatever comes before them.
        self.keys
            .sort_unstable_by_key(|&(key, index)| (key >> 8, index));

        for &(_, index) in &self.keys {
            let Some((texture, placeholder)) = self.textures[index as usize] else {
                continue;
            };
            let position = self.order.len() as u32;
            self.order.push(Drawn {
                sprite: index,
                placeholder,
            });
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
        self.keys.partition_point(|&(key, _)| key >> 24 < bound) as u32
    }
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

    #[test]
    fn a_plan_is_reused() {
        let mut plan = plan(&[sprite(0, 1.0), sprite(1, 2.0)]);
        plan.build(&[sprite(5, 0.0)], |sprite| {
            Resolved::Draw(sprite.texture.id().index())
        });
        assert_eq!(order(&plan), [0]);
        assert_eq!(batches(&plan), [(5, 0, 1)]);
        assert_eq!(plan.skipped, 0);
    }
}
