//! Pixel-perfect views: the camera's snapped origin, the integer upscale
//! with its letterbox, and the mapping between screen and world.

use nadir_core::{IRect, IVec2, UVec2, Vec2};

use crate::Camera2D;

/// Rounds to the nearest whole number, halves up: each step of a value
/// that moves by one moves the result by one, even across zero.
pub(crate) fn snap(v: Vec2) -> Vec2 {
    (v + Vec2::splat(0.5)).floor()
}

/// The zoom of pixel-perfect rendering: `zoom` rounded to a whole number,
/// at least 1. (A NaN is 1.)
pub(crate) fn pixel_zoom(zoom: f32) -> f32 {
    (zoom + 0.5).floor().max(1.0)
}

/// Where the virtual image lands in an output, scaled up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Upscale {
    /// Output pixels per virtual pixel: the largest whole number that fits,
    /// at least 1.
    pub factor: u32,
    /// The image's place in the output, in pixels from its top left. It is
    /// centred, with the odd pixel of a bar on the right or at the bottom.
    /// It reaches outside an output smaller than the virtual resolution,
    /// whose image is then cropped.
    pub rect: IRect,
}

impl Upscale {
    pub(crate) fn new(virtual_size: UVec2, output: UVec2) -> Self {
        let factor = (output.x / virtual_size.x)
            .min(output.y / virtual_size.y)
            .max(1);
        // `factor` is at most what fits, or 1, so the products fit in i64.
        let side = |output: u32, virtual_side: u32| {
            let size = i64::from(virtual_side) * i64::from(factor);
            let min = (i64::from(output) - size).div_euclid(2);
            (min as i32, (min + size) as i32)
        };
        let (min_x, max_x) = side(output.x, virtual_size.x);
        let (min_y, max_y) = side(output.y, virtual_size.y);
        Self {
            factor,
            rect: IRect::new(IVec2::new(min_x, min_y), IVec2::new(max_x, max_y)),
        }
    }
}

/// How a camera maps the world to the virtual image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct View {
    /// The virtual pixel at the image's top-left corner, as a world point
    /// times the zoom: a whole number.
    origin: Vec2,
    zoom: f32,
    size: Vec2,
}

impl View {
    pub(crate) fn new(camera: &Camera2D, virtual_size: UVec2) -> Self {
        let zoom = pixel_zoom(camera.zoom);
        let size = virtual_size.as_vec2();
        // The top-left corner snaps, not the centre, so that an odd
        // virtual size stays on the pixel grid.
        let origin = snap((camera.position + camera.shake) * zoom - size * 0.5);
        Self { origin, zoom, size }
    }

    pub(crate) fn zoom(&self) -> f32 {
        self.zoom
    }

    /// The `Camera` uniform of the `sprite` shader: clip = world * xy + zw.
    pub(crate) fn uniform(&self) -> [f32; 4] {
        let scale = Vec2::new(
            2.0 * self.zoom / self.size.x,
            -2.0 * self.zoom / self.size.y,
        );
        [
            scale.x,
            scale.y,
            -2.0 * self.origin.x / self.size.x - 1.0,
            1.0 + 2.0 * self.origin.y / self.size.y,
        ]
    }

    /// A world point in virtual pixels from the image's top left.
    pub(crate) fn project(&self, world: Vec2) -> Vec2 {
        world * self.zoom - self.origin
    }

    /// A point in virtual pixels as a world point.
    pub(crate) fn unproject(&self, virtual_point: Vec2) -> Vec2 {
        (virtual_point + self.origin) / self.zoom
    }
}

/// An output point in pixels from its top left, in virtual pixels.
pub(crate) fn screen_to_virtual(upscale: &Upscale, screen: Vec2) -> Vec2 {
    (screen - upscale.rect.min.as_vec2()) / upscale.factor as f32
}

/// A point in virtual pixels in output pixels.
pub(crate) fn virtual_to_screen(upscale: &Upscale, virtual_point: Vec2) -> Vec2 {
    virtual_point * upscale.factor as f32 + upscale.rect.min.as_vec2()
}

#[cfg(test)]
mod tests {
    use nadir_core::testing::check;

    use super::*;

    #[test]
    fn snapping_steps_by_one_across_zero() {
        let snapped: Vec<f32> = [-1.5f32, -0.5, 0.5, 1.5]
            .into_iter()
            .map(|x| snap(Vec2::new(x, 0.0)).x)
            .collect();
        assert_eq!(snapped, [-1.0, 0.0, 1.0, 2.0]);
    }

    #[test]
    fn an_odd_view_moves_one_pixel_for_each_pixel_the_camera_moves() {
        let size = UVec2::new(63, 35);
        let origins: Vec<f32> = (29..35)
            .map(|x| {
                let camera = Camera2D {
                    position: Vec2::new(x as f32, 0.0),
                    ..Camera2D::default()
                };
                View::new(&camera, size).origin.x
            })
            .collect();
        assert_eq!(origins, [-2.0, -1.0, 0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn zoom_is_a_whole_number_in_pixel_perfect_mode() {
        let zooms = [0.0, 0.3, 1.0, 1.4, 1.5, 2.6, f32::NAN, -3.0];
        assert_eq!(
            zooms.map(pixel_zoom),
            [1.0, 1.0, 1.0, 1.0, 2.0, 3.0, 1.0, 1.0]
        );
    }

    #[test]
    fn the_image_is_the_largest_whole_multiple_that_fits_and_centred() {
        let virtual_size = UVec2::new(64, 36);
        let cases = [
            // An exact fit: no bars.
            ((320, 180), 5, (0, 0, 320, 180)),
            // Wide enough for 4, high enough for 5.
            ((300, 200), 4, (22, 28, 278, 172)),
            // High enough for 4 only; bars of 38 and 39, and of 1 and 2.
            ((333, 147), 4, (38, 1, 294, 145)),
            // Smaller than the virtual image: cropped, centred.
            ((51, 31), 1, (-7, -3, 57, 33)),
            ((0, 0), 1, (-32, -18, 32, 18)),
        ];
        for ((width, height), factor, (x0, y0, x1, y1)) in cases {
            let upscale = Upscale::new(virtual_size, UVec2::new(width, height));
            assert_eq!(upscale.factor, factor, "{width}x{height}");
            assert_eq!(
                upscale.rect,
                IRect::new(IVec2::new(x0, y0), IVec2::new(x1, y1)),
                "{width}x{height}"
            );
        }
    }

    #[test]
    fn the_uniform_maps_the_view_onto_clip_space() {
        let camera = Camera2D {
            position: Vec2::new(32.0, 18.0),
            ..Camera2D::default()
        };
        let view = View::new(&camera, UVec2::new(64, 36));
        let [sx, sy, ox, oy] = view.uniform();
        let clip = |world: Vec2| Vec2::new(world.x * sx + ox, world.y * sy + oy);
        assert_eq!(clip(Vec2::new(0.0, 0.0)), Vec2::new(-1.0, 1.0), "top left");
        assert_eq!(
            clip(Vec2::new(64.0, 36.0)),
            Vec2::new(1.0, -1.0),
            "bottom right"
        );
        assert_eq!(clip(Vec2::new(32.0, 18.0)), Vec2::ZERO);

        // At zoom 2, a world unit is two virtual pixels: the view spans 32
        // by 18 units around the camera.
        let zoomed = Camera2D {
            zoom: 2.0,
            ..camera
        };
        let [sx, sy, ox, oy] = View::new(&zoomed, UVec2::new(64, 36)).uniform();
        let clip = |world: Vec2| Vec2::new(world.x * sx + ox, world.y * sy + oy);
        assert_eq!(clip(Vec2::new(16.0, 9.0)), Vec2::new(-1.0, 1.0));
        assert_eq!(clip(Vec2::new(48.0, 27.0)), Vec2::new(1.0, -1.0));
    }

    #[test]
    fn screen_and_world_map_back_and_forth() {
        check(
            "screen to world and back",
            5_000,
            |rng| {
                let pick = |rng: &mut nadir_core::Pcg32, range: u32| rng.range(0..range) as f32;
                let camera = Camera2D {
                    position: Vec2::new(pick(rng, 2000) - 1000.0, pick(rng, 2000) - 1000.0) * 0.25,
                    zoom: 1.0 + pick(rng, 3),
                    shake: Vec2::new(pick(rng, 5) - 2.0, 0.0),
                };
                let output = UVec2::new(1 + rng.range(0..800u32), 1 + rng.range(0..600u32));
                let screen = Vec2::new(pick(rng, 800), pick(rng, 600));
                (camera, output, screen)
            },
            |&(camera, output, screen)| {
                let upscale = Upscale::new(UVec2::new(64, 36), output);
                let view = View::new(&camera, UVec2::new(64, 36));
                let world = view.unproject(screen_to_virtual(&upscale, screen));
                let back = virtual_to_screen(&upscale, view.project(world));
                // A hundredth of a pixel: f32 at a few hundred pixels.
                if (back - screen).length() < 1e-2 {
                    Ok(())
                } else {
                    Err(format!("{screen:?} -> {world:?} -> {back:?}"))
                }
            },
        );
    }
}
