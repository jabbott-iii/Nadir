//! What the game asks the renderer to draw in a frame.

use nadir_assets::Handle;
use nadir_core::{Color, Rect, Vec2};

use crate::Texture;

/// The view of the world: what is at the centre of the screen, and how
/// large.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera2D {
    /// The world point at the centre of the view.
    pub position: Vec2,
    /// Virtual pixels per world unit. Pixel-perfect rendering rounds it to
    /// a whole number, at least 1.
    pub zoom: f32,
    /// An offset added to the position, for screen shake.
    pub shake: Vec2,
}

impl Default for Camera2D {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            zoom: 1.0,
            shake: Vec2::ZERO,
        }
    }
}

/// How a sprite is ordered among the others of its layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SortMode {
    /// By its pivot's y: lower on screen is drawn later, in front.
    #[default]
    YSort,
    /// At a fixed depth, for ground decals and the like. `Fixed(0)` is
    /// behind every y-sorted sprite of its layer, and `Fixed(u32::MAX)` in
    /// front of every one.
    Fixed(u32),
}

/// Which way a sprite's texture is mirrored, in place.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Flip {
    /// Left to right.
    pub x: bool,
    /// Top to bottom.
    pub y: bool,
}

/// One sprite to draw in the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteDraw {
    /// The texture, usually an atlas page.
    pub texture: Handle<Texture>,
    /// The region of the texture, normalized: (0, 0) to (1, 1) is all of
    /// it. A texture that failed to load shows all of its placeholder
    /// instead.
    pub uv: Rect,
    /// Where the pivot is in the world.
    pub position: Vec2,
    /// The size in world units.
    pub size: Vec2,
    /// The point of the sprite at `position`, normalized: (0.5, 1) is the
    /// middle of its bottom edge, its feet.
    pub pivot: Vec2,
    /// Radians, clockwise on screen, about the pivot.
    pub rotation: f32,
    /// A tint, sRGB, multiplied with the texture in linear light.
    pub color: Color,
    /// Higher layers are drawn in front of lower ones.
    pub layer: u8,
    /// How it is ordered within its layer.
    pub sort: SortMode,
    /// Which way its texture is mirrored.
    pub flip: Flip,
}

impl SpriteDraw {
    /// A sprite of all of `texture`, `size` large, with its feet at
    /// `position`: untinted, unrotated, y-sorted in layer 0.
    pub fn new(texture: Handle<Texture>, position: Vec2, size: Vec2) -> Self {
        Self {
            texture,
            uv: Rect::new(Vec2::ZERO, Vec2::ONE),
            position,
            size,
            pivot: Vec2::new(0.5, 1.0),
            rotation: 0.0,
            color: Color::WHITE,
            layer: 0,
            sort: SortMode::YSort,
            flip: Flip::default(),
        }
    }
}

/// Everything the renderer draws in a frame. Keep one and clear it each
/// frame, so that its lists keep their capacity.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderFrame {
    /// The view of the world.
    pub camera: Camera2D,
    /// The background, and the color of the bars around an upscaled image.
    pub clear: Color,
    /// The sprites of the world, in any order.
    pub sprites: Vec<SpriteDraw>,
}

impl RenderFrame {
    /// An empty frame with a black background.
    pub fn new() -> Self {
        Self {
            camera: Camera2D::default(),
            clear: Color::BLACK,
            sprites: Vec::new(),
        }
    }

    /// Empties the lists for the next frame, keeping their capacity.
    pub fn clear(&mut self) {
        self.sprites.clear();
    }
}

impl Default for RenderFrame {
    fn default() -> Self {
        Self::new()
    }
}
