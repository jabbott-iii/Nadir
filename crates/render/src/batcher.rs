//! [`SpriteBatcher`]: the work on the CPU of drawing sprites.

use crate::instance::push_instance;
use crate::plan::{Batch, Plan, Resolved};
use crate::view::pixel_zoom;
use crate::{Camera2D, FrameStats, SpriteDraw};

/// Sorts a frame's sprites, groups them into batches by texture, and packs
/// the instance of each one drawn: what the [`Renderer`](crate::Renderer)
/// does on the CPU before it uploads the instances and draws. It is public
/// so that this work can be measured without a GPU.
///
/// It is generic over the texture `K`: the renderer's is its GPU texture,
/// and a benchmark can use any number. Its lists are reused from one build
/// to the next, so a steady frame allocates nothing.
#[derive(Debug)]
pub struct SpriteBatcher<K> {
    plan: Plan<K>,
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
        mut texture: impl FnMut(&SpriteDraw) -> Option<(K, bool)>,
    ) {
        self.plan.build(sprites, |sprite| match texture(sprite) {
            None => Resolved::Skip,
            Some((texture, false)) => Resolved::Draw(texture),
            Some((texture, true)) => Resolved::Placeholder(texture),
        });
        let zoom = pixel_zoom(camera.zoom);
        self.packed.clear();
        for drawn in &self.plan.order {
            push_instance(
                &mut self.packed,
                &sprites[drawn.sprite as usize],
                drawn.placeholder,
                Some(zoom),
            );
        }
    }

    /// The instances of the sprites drawn, in draw order:
    /// [`SPRITE_INSTANCE_FLOATS`](crate::SPRITE_INSTANCE_FLOATS) floats
    /// each.
    pub fn instances(&self) -> &[f32] {
        &self.packed
    }

    /// What the last build drew. It knows nothing of tile layers: `tiles`
    /// and `chunks` are 0, and `batches` counts the batches before a tile
    /// layer drawn among the sprites splits one in two.
    pub fn stats(&self) -> FrameStats {
        // The plan numbers its sprites with a u32, so these fit.
        FrameStats {
            sprites: self.plan.order.len() as u32,
            skipped: self.plan.skipped,
            batches: self.plan.batches.len() as u32,
            tiles: 0,
            chunks: 0,
        }
    }

    /// The batches of the last build, in draw order.
    pub(crate) fn batches(&self) -> &[Batch<K>] {
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
                tiles: 0,
                chunks: 0,
            }
        );
        let order: Vec<u32> = batcher
            .batches()
            .iter()
            .map(|batch| batch.texture)
            .collect();
        assert_eq!(order, [0, 2, 0], "by depth: y 5, then 20, then 30");
        assert_eq!(batcher.instances().len(), 3 * SPRITE_INSTANCE_FLOATS);

        // A second build replaces the first.
        batcher.build(&sprites[..1], &Camera2D::default(), |_| Some((0, false)));
        assert_eq!(batcher.stats().sprites, 1);
        assert_eq!(batcher.instances().len(), SPRITE_INSTANCE_FLOATS);
    }
}
