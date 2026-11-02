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

//! Sprite instances, packed as `f32`s for the instance buffer.
//!
//! `nadir-render` denies `unsafe`, so it cannot make its own structs `Pod`:
//! instances go into a `Vec<f32>` instead (`docs/subsystems/render.md`,
//! "Instance data").

use crate::SpriteDraw;

/// The `f32`s of one sprite instance, in the order of the `sprite`
/// shader's instance attributes (locations 0 to 5):
///
/// | Floats | Location | What |
/// | :- | :- | :- |
/// | 0, 1 | 0 | position: the pivot in the world |
/// | 2, 3 | 1 | size |
/// | 4, 5 | 2 | pivot, normalized, mirrored with the flip: `1 - x` for `flip.x` |
/// | 6 to 9 | 3 | uv rect: u0, v0, u1, v1, with the flip applied by swapping |
/// | 10 to 13 | 4 | color, linear |
/// | 14 | 5 | rotation, radians, clockwise |
pub const SPRITE_INSTANCE_FLOATS: usize = 15;

/// Adds the instance of `sprite`.
///
/// - `placeholder`: it shows its texture's placeholder, so all of it.
/// - `snap_zoom`: in pixel-perfect mode, the virtual pixels per world unit.
///   An unrotated sprite then has its top-left corner moved to the nearest
///   whole virtual pixel, so that its texels land on whole pixels on every
///   GPU.
pub(crate) fn push_instance(
    out: &mut Vec<f32>,
    sprite: &SpriteDraw,
    placeholder: bool,
    snap_zoom: Option<f32>,
) {
    let (mut u0, mut v0, mut u1, mut v1) = if placeholder {
        (0.0, 0.0, 1.0, 1.0)
    } else {
        (
            sprite.uv.min.x,
            sprite.uv.min.y,
            sprite.uv.max.x,
            sprite.uv.max.y,
        )
    };
    if sprite.flip.x {
        (u0, u1) = (u1, u0);
    }
    if sprite.flip.y {
        (v0, v1) = (v1, v0);
    }
    // The quad mirrors about the pivot, as its texture does in it.
    let mut pivot = sprite.pivot;
    if sprite.flip.x {
        pivot.x = 1.0 - pivot.x;
    }
    if sprite.flip.y {
        pivot.y = 1.0 - pivot.y;
    }
    // To 1/256 of a world unit: a normalized pivot rounds in `f32`
    // (7/13 × 13 is 7.0000005), which would snap a corner on a half-pixel
    // tie to the pixel below.
    let offset = (pivot * sprite.size * 256.0).round() * (1.0 / 256.0);
    let position = match snap_zoom {
        Some(zoom) if sprite.rotation == 0.0 => {
            let corner = sprite.position - offset;
            crate::view::snap(corner * zoom) * (1.0 / zoom) + offset
        }
        _ => sprite.position,
    };
    let color = sprite.color.to_linear();
    out.extend_from_slice(&[
        position.x,
        position.y,
        sprite.size.x,
        sprite.size.y,
        pivot.x,
        pivot.y,
        u0,
        v0,
        u1,
        v1,
        color.r,
        color.g,
        color.b,
        color.a,
        sprite.rotation,
    ]);
}

#[cfg(test)]
mod tests {
    use nadir_assets::Handle;
    use nadir_core::{Color, GenIndex, Rect, Vec2};

    use super::*;
    use crate::Flip;

    fn sprite() -> SpriteDraw {
        let handle = Handle::new(GenIndex::new(0, std::num::NonZeroU32::MIN));
        let mut sprite = SpriteDraw::new(handle, Vec2::new(100.0, 50.0), Vec2::new(12.0, 20.0));
        sprite.uv = Rect::new(Vec2::new(0.25, 0.5), Vec2::new(0.75, 1.0));
        sprite
    }

    fn pack(sprite: &SpriteDraw, placeholder: bool, snap_zoom: Option<f32>) -> Vec<f32> {
        let mut out = vec![9.0];
        push_instance(&mut out, sprite, placeholder, snap_zoom);
        assert_eq!(out.len(), 1 + SPRITE_INSTANCE_FLOATS, "appended");
        out.split_off(1)
    }

    #[test]
    fn an_instance_is_packed_in_attribute_order() {
        let mut sprite = sprite();
        sprite.rotation = 0.5;
        sprite.color = Color::rgba(255, 0, 0, 51);
        let floats = pack(&sprite, false, None);
        assert_eq!(
            floats,
            [
                100.0, 50.0, 12.0, 20.0, 0.5, 1.0, 0.25, 0.5, 0.75, 1.0, 1.0, 0.0, 0.0, 0.2, 0.5
            ]
        );
    }

    #[test]
    fn the_tint_is_converted_to_linear_light() {
        let mut sprite = sprite();
        sprite.color = Color::rgba(128, 0, 255, 128);
        let tint = &pack(&sprite, false, None)[10..14];
        // sRGB 128 is 0.2158605 linear; alpha is not converted.
        assert!((tint[0] - 0.215_860_5).abs() < 1e-6, "{tint:?}");
        assert_eq!(tint[1..3], [0.0, 1.0]);
        assert!((tint[3] - 128.0 / 255.0).abs() < 1e-6, "{tint:?}");
    }

    #[test]
    fn a_flip_swaps_the_uv_rect() {
        let mut sprite = sprite();
        sprite.flip = Flip { x: true, y: false };
        assert_eq!(pack(&sprite, false, None)[6..10], [0.75, 0.5, 0.25, 1.0]);
        sprite.flip = Flip { x: true, y: true };
        assert_eq!(pack(&sprite, false, None)[6..10], [0.75, 1.0, 0.25, 0.5]);
    }

    #[test]
    fn a_flip_mirrors_the_quad_about_its_pivot() {
        // A trimmed frame 10 wide with its pivot 4 pixels in, at x = 100:
        // it covers 96 to 106, and mirrored 94 to 104.
        let mut sprite = sprite();
        sprite.size = Vec2::new(10.0, 20.0);
        sprite.pivot = Vec2::new(0.4, 1.0);
        assert_eq!(pack(&sprite, false, None)[4..6], [0.4, 1.0]);
        sprite.flip.x = true;
        assert_eq!(pack(&sprite, false, None)[4..6], [0.6, 1.0]);
        // Its feet stay at y = 50: mirrored top to bottom, it hangs below
        // them.
        sprite.flip = Flip { x: false, y: true };
        assert_eq!(pack(&sprite, false, None)[4..6], [0.4, 0.0]);
        // A pivot in the middle mirrors in place.
        sprite.pivot = Vec2::new(0.5, 0.5);
        sprite.flip = Flip { x: true, y: true };
        assert_eq!(pack(&sprite, false, None)[4..6], [0.5, 0.5]);
    }

    #[test]
    fn pixel_perfect_sprites_snap_their_corner_as_their_pixels_would() {
        // A frame `width` wide whose pivot is `pixels` in, at x = 0.5, a
        // tie near the origin, where `f32` keeps the normalized pivot's
        // rounding: its corner, mirrored or not, goes up to the next whole
        // pixel. Each case snaps a pixel low with the pivot's offset left
        // unrounded, as the check below shows.
        let cases = [
            (7.0, 13.0, false),
            (6.0, 13.0, true),
            (5.0, 6.0, true),
            (9.0, 10.0, true),
            (10.0, 12.0, true),
            (13.0, 14.0, true),
        ];
        for (pixels, width, flipped) in cases {
            let mut sprite = sprite();
            sprite.size = Vec2::new(width, 20.0);
            sprite.pivot = Vec2::new(pixels / width, 1.0);
            sprite.position = Vec2::new(0.5, 50.0);
            sprite.flip.x = flipped;
            let floats = pack(&sprite, false, Some(1.0));
            let from_left = if flipped { width - pixels } else { pixels };
            let expected = (0.5 - from_left + 0.5_f32).floor();
            let corner = floats[0] - floats[4] * floats[2];
            assert!(
                (corner - expected).abs() < 1e-3,
                "{pixels}/{width}, flipped {flipped}: corner {corner}, not {expected}"
            );
            let unrounded = (0.5 - floats[4] * floats[2] + 0.5_f32).floor();
            assert_ne!(
                unrounded, expected,
                "{pixels}/{width}: the case needs the rounding"
            );
        }
    }

    #[test]
    fn a_placeholder_shows_all_of_itself() {
        let mut sprite = sprite();
        assert_eq!(pack(&sprite, true, None)[6..10], [0.0, 0.0, 1.0, 1.0]);
        sprite.flip.y = true;
        assert_eq!(pack(&sprite, true, None)[6..10], [0.0, 1.0, 1.0, 0.0]);
    }

    #[test]
    fn pixel_perfect_sprites_have_their_corner_on_a_whole_pixel() {
        // 13 wide, centred on x = 100: its corner is at 93.5, and moves to
        // 94 (ties go up), so its feet are at 100.5.
        let mut sprite = sprite();
        sprite.size = Vec2::new(13.0, 20.0);
        sprite.position = Vec2::new(100.0, 50.25);
        assert_eq!(pack(&sprite, false, Some(1.0))[0..2], [100.5, 50.0]);
        // At zoom 2, a world unit is two pixels: 93.5 is a whole pixel, and
        // 30.25 is 60.5 pixels, which goes up to 61.
        assert_eq!(pack(&sprite, false, Some(2.0))[0..2], [100.0, 50.5]);
        // Rotated sprites are not moved, nor are those of smooth mode.
        sprite.rotation = 0.1;
        assert_eq!(pack(&sprite, false, Some(1.0))[0..2], [100.0, 50.25]);
        sprite.rotation = 0.0;
        assert_eq!(pack(&sprite, false, None)[0..2], [100.0, 50.25]);
    }
}
