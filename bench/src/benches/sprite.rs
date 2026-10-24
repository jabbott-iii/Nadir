//! Sprite benchmarks.

use std::hint::black_box;
use std::num::NonZeroU32;

use nadir_core::{Color, GenIndex, Handle, Pcg32, Vec2};
use nadir_render::{Camera2D, RenderFrame, SortMode, SpriteBatcher, SpriteDraw, Texture};

use crate::harness::Bencher;

/// How many people walk about in the benchmark frame.
const CROWD: usize = 10_000;

/// The benchmark frame's textures, by number: the atlas that the people,
/// the props and the player are drawn from, and the grass under them.
const ATLAS: u32 = 1;
const GRASS: u32 = 2;

/// `sprite_sort_batch`: the renderer's work on the CPU for one frame shaped
/// like the sandbox's stress scene (`--scene stress-sprites`): 192 grass
/// tiles under 10,000 people, 10 props and the player, 10,203 sprites in 2
/// batches.
///
/// It times what `docs/subsystems/render.md` budgets as sort and batch:
/// the sort keys, the sort, the batches, and packing each sprite's
/// instance, colors included. It leaves out the asset lookups, which need
/// textures on a GPU, and the upload.
pub(crate) fn sort_batch(bencher: &mut Bencher) {
    let frame = stress_frame(CROWD);
    let mut batcher = SpriteBatcher::new();
    bencher.iter(|| {
        batcher.build(black_box(&frame.sprites), &frame.camera, texture);
        batcher.stats().batches
    });
}

/// Every texture is ready.
fn texture(sprite: &SpriteDraw) -> Option<(u32, bool)> {
    Some((sprite.texture.id().index(), false))
}

/// A frame like the stress scene's, with `crowd` people placed at random
/// in the 480×270 view: the same every time.
fn stress_frame(crowd: usize) -> RenderFrame {
    let handle = |texture| Handle::<Texture>::new(GenIndex::new(texture, NonZeroU32::MIN));
    let mut rng = Pcg32::new(0x5370_7269, 0);
    let mut frame = RenderFrame::new();
    frame.camera = Camera2D {
        position: Vec2::new(240.0, 135.0),
        ..Camera2D::default()
    };
    for row in 0..12 {
        for column in 0..16 {
            let corner = Vec2::new(column as f32 * 32.0, row as f32 * 32.0);
            let mut tile = SpriteDraw::new(handle(GRASS), corner, Vec2::splat(32.0));
            tile.pivot = Vec2::ZERO;
            tile.sort = SortMode::Fixed(0);
            frame.sprites.push(tile);
        }
    }
    let on_layer_1 = |position: Vec2, size: Vec2| {
        let mut sprite = SpriteDraw::new(handle(ATLAS), position, size);
        sprite.layer = 1;
        sprite
    };
    for prop in 0..10 {
        let position = Vec2::new(40.0 + prop as f32 * 44.0, 60.0 + (prop % 3) as f32 * 90.0);
        frame
            .sprites
            .push(on_layer_1(position, Vec2::new(24.0, 48.0)));
    }
    let tints = [Color::rgb(255, 200, 200), Color::rgb(200, 220, 255)];
    for person in 0..crowd {
        let position = Vec2::new(6.0 + rng.next_f32() * 468.0, 20.0 + rng.next_f32() * 250.0);
        let mut sprite = on_layer_1(position, Vec2::new(12.0, 20.0));
        sprite.flip.x = rng.next_f32() < 0.5;
        // A quarter of them are tinted.
        if person % 4 == 0 {
            sprite.color = tints[person / 4 % 2];
        }
        frame.sprites.push(sprite);
    }
    frame
        .sprites
        .push(on_layer_1(Vec2::new(240.0, 160.0), Vec2::new(12.0, 20.0)));
    frame
}

#[cfg(test)]
mod tests {
    use nadir_render::FrameStats;

    use super::*;
    use crate::harness::Config;

    #[test]
    fn the_frame_is_the_stress_scenes_shape() {
        let frame = stress_frame(CROWD);
        let mut batcher = SpriteBatcher::new();
        batcher.build(&frame.sprites, &frame.camera, texture);
        assert_eq!(
            batcher.stats(),
            FrameStats {
                sprites: 10_203,
                skipped: 0,
                batches: 2
            }
        );
    }

    #[test]
    fn the_frame_is_the_same_every_time() {
        let (a, b) = (stress_frame(50), stress_frame(50));
        assert!(a.sprites.iter().zip(&b.sprites).all(|(a, b)| a == b));
    }

    #[test]
    fn the_benchmark_times_the_batcher() {
        let mut bencher = Bencher::new(Config::fast());
        sort_batch(&mut bencher);
        let measurement = bencher.finish().unwrap();
        assert!(measurement.median_ns() > 0.0);
    }
}
