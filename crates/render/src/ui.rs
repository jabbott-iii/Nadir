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

//! The UI: sprites and texts drawn on the output after the world is scaled
//! up into it, at the output's resolution, by layer and depth and then in
//! the order they are pushed (ADR-0055).

use std::fmt;

use nadir_core::{IRect, IVec2, Rect, UVec2, Vec2};

use crate::batcher::Pipe;
use crate::instance::{instance, pivot_offset, uv_and_pivot};
use crate::plan::GlyphQuad;
use crate::view::{Upscale, snap};
use crate::{SPRITE_INSTANCE_FLOATS, SortMode, SpriteDraw, TextDraw, TextList};

/// The smallest free scale: an eighth of an output pixel per UI unit.
const MIN_FREE_SCALE: f32 = 1.0 / 8.0;

/// The largest free scale: 64 output pixels per UI unit.
const MAX_FREE_SCALE: f32 = 64.0;

/// How the UI units of a [`UiView`] map onto the output.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UiScale {
    /// Pixel UI: a UI unit is this many pixels of the virtual image (0 is
    /// 1), scaled up with the world by the upscale's whole factor, from the
    /// image's top-left corner, and cut to the image: nothing is drawn on
    /// the bars around it. Corners snap to whole UI units, so its pixels
    /// line up as the world's do. The default, `Pixel(1)`, is the virtual
    /// image's own pixels.
    Pixel(u32),
    /// A free scale: output pixels per UI unit, from the output's top-left
    /// corner, over all of it, bars included: the settings' UI scale.
    ///
    /// The pixels are the output's, which are physical: where the desktop
    /// doubles its pixels, 2 is the size its 1 looks. Corners snap to whole
    /// output pixels. A scale that is not whole puts some texels on more
    /// pixels than others, and a texel's edge on the middle of a pixel,
    /// which GPUs may sample differently. Below 1/8 (0 and negatives too)
    /// is 1/8, above 64 is 64, and NaN is 1.
    Free(f32),
}

impl Default for UiScale {
    fn default() -> Self {
        Self::Pixel(1)
    }
}

impl UiScale {
    /// The scale drawn: a multiple of 0 is 1, and a free scale held
    /// between 1/8 and 64, NaN 1, so that views of one scale are equal.
    fn held(self) -> Self {
        match self {
            Self::Pixel(multiple) => Self::Pixel(multiple.max(1)),
            Self::Free(scale) if scale.is_nan() => Self::Free(1.0),
            Self::Free(scale) => Self::Free(scale.clamp(MIN_FREE_SCALE, MAX_FREE_SCALE)),
        }
    }
}

/// The scale and the clip of the UI's sprites and texts:
/// [`UiList::set_view`] sets the view of those pushed after it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UiView {
    scale: UiScale,
    clip: Option<Rect>,
}

impl UiView {
    /// A view at `scale`, cut only to its space's edges
    /// ([`UiSpace::rect`]). Its scale is the one drawn: `Pixel(0)` is
    /// `Pixel(1)`, and a free scale is held as [`UiScale::Free`] says.
    pub fn new(scale: UiScale) -> Self {
        Self {
            scale: scale.held(),
            clip: None,
        }
    }

    /// This view cut to `rect` too, in its UI units: its clip becomes the
    /// part of `rect` inside the clip it had, so that a clip inside a clip
    /// (a scrolled list in a window) is cut by both. A `rect` that does
    /// not overlap the clip, has no area, or has an edge that is NaN cuts
    /// everything: nothing in the view is drawn. Its edges may be
    /// infinite.
    pub fn clipped(self, rect: Rect) -> Self {
        let nan = rect.min.x.is_nan()
            || rect.min.y.is_nan()
            || rect.max.x.is_nan()
            || rect.max.y.is_nan();
        let inside = match self.clip {
            _ if nan => None,
            None => Some(rect).filter(|rect| !rect.is_empty()),
            Some(clip) => clip.intersection(rect),
        };
        Self {
            clip: Some(inside.unwrap_or(Rect::new(Vec2::ZERO, Vec2::ZERO))),
            ..self
        }
    }

    /// Its scale.
    pub fn scale(&self) -> UiScale {
        self.scale
    }

    /// Its clip, in its UI units: `None` when it is not clipped, and a rect
    /// with no area when it cuts everything.
    pub fn clip(&self) -> Option<Rect> {
        self.clip
    }
}

/// Where a [`UiScale`] puts the UI in an output: for drawing, and for
/// finding what is under the pointer, whose position
/// (`nadir_input::Input::pointer`) is in the output's pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiSpace {
    /// Output pixels per UI unit.
    pub scale: f32,
    /// The output pixel at UI point (0, 0), from the output's top left.
    pub origin: IVec2,
    /// The UI units from (0, 0) to the far corner of the space: the virtual
    /// resolution over the pixel UI's multiple, or the output's size over
    /// the free scale.
    pub size: Vec2,
    /// The output pixels the UI is drawn in, inside the output: the
    /// upscaled image for pixel UI, all of the output for a free scale.
    pub rect: IRect,
    /// Whether corners snap to whole UI units (pixel UI), or to whole
    /// output pixels.
    whole_units: bool,
}

impl UiSpace {
    /// Where `scale` puts the UI in an output of `output_size` pixels, when
    /// the world's virtual image is `virtual_resolution` (as
    /// [`Renderer::ui_space`](crate::Renderer::ui_space) does with its
    /// own).
    pub fn new(scale: UiScale, virtual_resolution: UVec2, output_size: UVec2) -> Self {
        let output = IRect::new(
            IVec2::ZERO,
            IVec2::new(
                i32::try_from(output_size.x).unwrap_or(i32::MAX),
                i32::try_from(output_size.y).unwrap_or(i32::MAX),
            ),
        );
        let inside = |rect: IRect| rect.intersection(output).unwrap_or_default();
        match scale.held() {
            UiScale::Pixel(multiple) => {
                let multiple = multiple as f32;
                let virtual_size = virtual_resolution.max(UVec2::ONE);
                let upscale = Upscale::new(virtual_size, output_size);
                Self {
                    scale: upscale.factor as f32 * multiple,
                    origin: upscale.rect.min,
                    size: virtual_size.as_vec2() / multiple,
                    rect: inside(upscale.rect),
                    whole_units: true,
                }
            }
            UiScale::Free(scale) => Self {
                scale,
                origin: IVec2::ZERO,
                size: output_size.as_vec2() / scale,
                rect: inside(output),
                whole_units: false,
            },
        }
    }

    /// The UI point at `screen`, in pixels from the output's top left: the
    /// pointer, in UI units.
    pub fn to_ui(&self, screen: Vec2) -> Vec2 {
        (screen - self.origin.as_vec2()) / self.scale
    }

    /// Where the UI point `ui` is on the output, in pixels from its top
    /// left. A corner there is drawn at the pixel it snaps to: for pixel
    /// UI, `ui` rounded to whole units, halves up; for a free scale, this
    /// point rounded to a whole pixel, halves up.
    pub fn to_screen(&self, ui: Vec2) -> Vec2 {
        self.origin.as_vec2() + ui * self.scale
    }

    /// The output pixel a corner at the UI point `ui` is drawn at: a whole
    /// number, or infinite.
    fn corner(&self, ui: Vec2) -> Vec2 {
        let origin = self.origin.as_vec2();
        if self.whole_units {
            origin + snap(ui) * self.scale
        } else {
            snap(origin + ui * self.scale)
        }
    }

    /// The output pixels a view whose clip is `clip` draws in, or `None`
    /// when it draws nowhere. Its edges snap as corners do.
    fn scissor(&self, clip: Option<Rect>) -> Option<IRect> {
        let Some(clip) = clip else {
            return Some(self.rect).filter(|rect| !rect.is_empty());
        };
        pixels(self.corner(clip.min), self.corner(clip.max)).intersection(self.rect)
    }
}

/// The pixels from `a` to `b`, corners in either order, whole numbers or
/// infinite: those past `i32` saturate.
fn pixels(a: Vec2, b: Vec2) -> IRect {
    let (min, max) = (a.min(b), a.max(b));
    IRect::new(
        IVec2::new(min.x as i32, min.y as i32),
        IVec2::new(max.x as i32, max.y as i32),
    )
}

/// What one entry of a [`UiList`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Sprite,
    Text,
}

/// One entry of a [`UiList`], in the order pushed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Item {
    kind: Kind,
    /// Its index among the list's sprites, or its texts.
    index: u32,
    /// Its view, in the list's views.
    view: u32,
}

/// One entry of a [`UiList`], as [`UiList::iter`] gives it.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UiDraw<'a> {
    /// A sprite: its position and size in UI units.
    Sprite(&'a SpriteDraw),
    /// A text, with its draw: its position in UI units, a UI unit a pixel
    /// of its font.
    Text(&'a TextDraw, &'a str),
}

/// The UI of a frame ([`RenderFrame::ui`](crate::RenderFrame::ui)): sprites
/// and texts in UI units, each drawn in the [`UiView`] set when it was
/// pushed, after the world, at the output's resolution.
///
/// **Order.** They are drawn by layer, then by depth, then in the order
/// they were pushed, sprites and texts alike: a window's panel, then its
/// labels, then the next window over them. Higher layers are in front of
/// lower ones; at one layer, a [`SortMode::Fixed`] depth orders them, and
/// [`SortMode::YSort`] is depth 0: nothing is sorted by its y on screen.
/// A text's glyphs keep their text's place, grouped by page.
///
/// **Views.** [`UiList::set_view`] sets the scale and the clip of what is
/// pushed after it. It holds until the next call, across the systems that
/// push, so a system that draws UI sets its own view first. The view
/// before the first call is [`UiView::default`]: pixel UI, unclipped. A
/// view changes nothing of the order: a clip only cuts.
///
/// **Cost.** Runs of one texture, one program and one view, in that order,
/// are one draw each: a panel then a label then a panel are three. A
/// quad wholly outside its view's clip, or with no pixels, is not drawn,
/// though a text is laid out whole. The list keeps its buffers when
/// cleared, so a frame that pushes what the last did allocates nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiList {
    items: Vec<Item>,
    sprites: Vec<SpriteDraw>,
    texts: TextList,
    /// Each text's place in `items`.
    text_items: Vec<u32>,
    views: Vec<UiView>,
}

impl UiList {
    /// An empty list, in the default view.
    pub fn new() -> Self {
        Self::default()
    }

    /// Draws what is pushed from now on in `view`, until the next call.
    pub fn set_view(&mut self, view: UiView) {
        // A view nothing was pushed in yet goes. The entries' views only
        // grow, so the last entry tells whether any is in the last view.
        let used = self
            .items
            .last()
            .is_some_and(|item| item.view as usize + 1 == self.views.len());
        if !used {
            self.views.pop();
        }
        if self.views.last() != Some(&view) {
            self.views.push(view);
        }
    }

    /// The view of what is pushed now.
    pub fn view(&self) -> UiView {
        self.views.last().copied().unwrap_or_default()
    }

    /// Adds `sprite`, its position and size in UI units.
    pub fn sprite(&mut self, sprite: SpriteDraw) {
        let Some((index, view)) = self.place(self.sprites.len()) else {
            return;
        };
        self.sprites.push(sprite);
        self.items.push(Item {
            kind: Kind::Sprite,
            index,
            view,
        });
    }

    /// Adds `text`, drawn as `draw` says, its position in UI units.
    pub fn text(&mut self, draw: TextDraw, text: &str) {
        let before = self.texts.len();
        if let Some((index, view)) = self.place(before) {
            self.texts.push(draw, text);
            self.push_text(index, view);
        }
    }

    /// Adds the text `args` formats, as [`TextList::write`] does: with no
    /// allocation once the list has held as much. A text whose formatting
    /// fails is left out.
    pub fn write(&mut self, draw: TextDraw, args: fmt::Arguments<'_>) {
        let before = self.texts.len();
        if let Some((index, view)) = self.place(before) {
            self.texts.write(draw, args);
            if self.texts.len() > before {
                self.push_text(index, view);
            }
        }
    }

    /// How many sprites and texts there are.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Empties the list, keeping its capacity, and puts the view back to
    /// the default.
    pub fn clear(&mut self) {
        self.items.clear();
        self.sprites.clear();
        self.texts.clear();
        self.text_items.clear();
        self.views.clear();
    }

    /// The sprites and texts, in the order they were pushed, each with its
    /// view: for checking what a UI drew without drawing it.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (UiDraw<'_>, UiView)> + '_ {
        self.items.iter().map(|item| {
            let draw = match item.kind {
                Kind::Sprite => UiDraw::Sprite(&self.sprites[item.index as usize]),
                Kind::Text => {
                    let (draw, text) = self.texts.at(item.index as usize);
                    UiDraw::Text(draw, text)
                }
            };
            (draw, self.views[item.view as usize])
        })
    }

    /// The texts, for laying out.
    pub(crate) fn texts(&self) -> &TextList {
        &self.texts
    }

    /// The index of the next sprite or text, the `count`th of its kind, and
    /// its view; `None` from `u32::MAX` entries on, which are left out, so
    /// that every place and index is below `u32::MAX`.
    fn place(&mut self, count: usize) -> Option<(u32, u32)> {
        let below = |n: usize| u32::try_from(n).ok().filter(|&n| n < u32::MAX);
        below(self.items.len())?;
        let index = below(count)?;
        if self.views.is_empty() {
            self.views.push(UiView::default());
        }
        // There are no more views than entries, and one.
        Some((index, (self.views.len() - 1) as u32))
    }

    fn push_text(&mut self, index: u32, view: u32) {
        // `place` checked that the entries fit a u32.
        self.text_items.push(self.items.len() as u32);
        self.items.push(Item {
            kind: Kind::Text,
            index,
            view,
        });
    }
}

/// The depth of a sprite or text in the UI: nothing is sorted by its y.
fn depth(sort: SortMode) -> u32 {
    match sort {
        SortMode::Fixed(depth) => depth,
        SortMode::YSort => 0,
    }
}

/// A quad of the UI to draw: its texture and program, its view, whether it
/// is a glyph, and its instance in output pixels.
#[derive(Clone, Copy, Debug)]
struct Quad<K> {
    texture: (K, Pipe),
    view: u32,
    glyph: bool,
    instance: [f32; SPRITE_INSTANCE_FLOATS],
}

/// A run of the UI's quads drawn with one texture, one program and one
/// view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UiBatch<K> {
    pub(crate) texture: (K, Pipe),
    pub(crate) view: u32,
    /// Where the run starts in the packed instances, counted in instances.
    pub(crate) start: u32,
    pub(crate) count: u32,
}

/// The UI of a frame, planned and packed: what the renderer draws on the
/// output after the blit. Its lists are reused from frame to frame. `K`
/// is a texture: the renderer's GPU texture, or a test's number.
#[derive(Debug)]
pub(crate) struct UiPass<K> {
    /// Each view's space, and the pixels it draws in (`None`: nowhere).
    views: Vec<(UiSpace, Option<IRect>)>,
    /// (layer, depth, place, page, quad) of each quad drawn.
    keys: Vec<(u8, u32, u32, u32, u32)>,
    quads: Vec<Quad<K>>,
    pub(crate) batches: Vec<UiBatch<K>>,
    /// The instances, in draw order, in output pixels.
    pub(crate) packed: Vec<f32>,
    /// The sprites and glyphs drawn.
    pub(crate) sprites: u32,
    pub(crate) glyphs: u32,
    /// The sprites not drawn because their texture is not ready.
    pub(crate) skipped: u32,
}

impl<K: Copy + Eq> UiPass<K> {
    pub(crate) fn new() -> Self {
        Self {
            views: Vec::new(),
            keys: Vec::new(),
            quads: Vec::new(),
            batches: Vec::new(),
            packed: Vec::new(),
            sprites: 0,
            glyphs: 0,
            skipped: 0,
        }
    }

    /// Plans and packs `list`, whose texts' glyphs are `glyphs`, laid out
    /// from its texts, for an output of `output` pixels with the world's
    /// image of `virtual_size`. `texture` finds a sprite's texture as the
    /// world's does: `None` while it is not ready, else the texture and
    /// whether it shows that texture's placeholder.
    pub(crate) fn build(
        &mut self,
        list: &UiList,
        glyphs: &[GlyphQuad<(K, Pipe)>],
        virtual_size: UVec2,
        output: UVec2,
        mut texture: impl FnMut(&SpriteDraw) -> Option<(K, bool)>,
    ) {
        self.views.clear();
        self.keys.clear();
        self.quads.clear();
        self.batches.clear();
        self.packed.clear();
        self.sprites = 0;
        self.glyphs = 0;
        self.skipped = 0;

        for view in &list.views {
            let space = UiSpace::new(view.scale, virtual_size, output);
            self.views.push((space, space.scissor(view.clip)));
        }
        // The list holds fewer than u32::MAX entries.
        for (place, item) in (0u32..).zip(&list.items) {
            if item.kind != Kind::Sprite {
                continue;
            }
            let sprite = &list.sprites[item.index as usize];
            // What is not ready is skipped, cut or not, as a glyph is.
            let Some((texture, placeholder)) = texture(sprite) else {
                self.skipped += 1;
                continue;
            };
            let (space, Some(scissor)) = self.views[item.view as usize] else {
                continue;
            };
            let Some(instance) = pack_sprite(sprite, placeholder, &space, scissor) else {
                continue;
            };
            let key = (sprite.layer, depth(sprite.sort), place, 0);
            self.push(key, (texture, Pipe::Sprite), item.view, false, instance);
        }
        for glyph in glyphs {
            let Some(&place) = list.text_items.get(glyph.text as usize) else {
                continue;
            };
            let item = list.items[place as usize];
            let (draw, _) = list.texts.at(item.index as usize);
            let (space, Some(scissor)) = self.views[item.view as usize] else {
                continue;
            };
            let Some(instance) = pack_glyph(&glyph.sprite, glyph.placeholder, &space, scissor)
            else {
                continue;
            };
            let page = glyph.sprite.texture.id().index();
            let key = (draw.layer, depth(draw.sort), place, page);
            self.push(key, glyph.texture, item.view, true, instance);
        }

        self.keys.sort_unstable();
        for &(.., quad) in &self.keys {
            let quad = &self.quads[quad as usize];
            // Fewer quads than u32::MAX: one for each sprite or glyph.
            let position = (self.packed.len() / SPRITE_INSTANCE_FLOATS) as u32;
            self.packed.extend_from_slice(&quad.instance);
            if quad.glyph {
                self.glyphs += 1;
            } else {
                self.sprites += 1;
            }
            match self.batches.last_mut() {
                Some(batch) if batch.texture == quad.texture && batch.view == quad.view => {
                    batch.count += 1;
                }
                _ => self.batches.push(UiBatch {
                    texture: quad.texture,
                    view: quad.view,
                    start: position,
                    count: 1,
                }),
            }
        }
    }

    /// The pixels the `view`th view draws in: one that draws, as every
    /// batch's does.
    pub(crate) fn scissor(&self, view: u32) -> Option<IRect> {
        self.views
            .get(view as usize)
            .and_then(|&(_, scissor)| scissor)
    }

    fn push(
        &mut self,
        (layer, depth, place, page): (u8, u32, u32, u32),
        texture: (K, Pipe),
        view: u32,
        glyph: bool,
        instance: [f32; SPRITE_INSTANCE_FLOATS],
    ) {
        // A quad for each sprite or glyph, fewer than u32::MAX of them.
        let Some(index) = u32::try_from(self.quads.len())
            .ok()
            .filter(|&index| index < u32::MAX)
        else {
            return;
        };
        self.quads.push(Quad {
            texture,
            view,
            glyph,
            instance,
        });
        self.keys.push((layer, depth, place, page, index));
    }
}

/// The instance of a UI sprite in output pixels, or `None` when it has no
/// pixels or none inside `scissor`.
///
/// Unrotated, both of its corners snap ([`UiSpace::corner`]), so that two
/// sprites that share an edge in UI units share it on the output at any
/// scale, and its size is what is between them. Rotated, it is scaled and
/// not moved, and culled by the box around its corners.
fn pack_sprite(
    sprite: &SpriteDraw,
    placeholder: bool,
    space: &UiSpace,
    scissor: IRect,
) -> Option<[f32; SPRITE_INSTANCE_FLOATS]> {
    let (uv, pivot) = uv_and_pivot(sprite, placeholder);
    if sprite.rotation != 0.0 {
        let position = space.origin.as_vec2() + sprite.position * space.scale;
        let size = sprite.size * space.scale;
        // The corners as the shader turns them, clockwise about the pivot.
        let (sin, cos) = sprite.rotation.sin_cos();
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        for corner in [Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::ONE] {
            let local = (corner - pivot) * size;
            let turned = Vec2::new(local.x * cos - local.y * sin, local.x * sin + local.y * cos);
            min = min.min(position + turned);
            max = max.max(position + turned);
        }
        let finite = [min.x, min.y, max.x, max.y].iter().all(|v| v.is_finite());
        if !finite
            || pixels(min.floor(), max.ceil())
                .intersection(scissor)
                .is_none()
        {
            return None;
        }
        return Some(instance(position, size, pivot, uv, sprite, sprite.rotation));
    }
    let near = sprite.position - pivot_offset(pivot, sprite.size);
    let a = space.corner(near);
    let b = space.corner(near + sprite.size);
    pixels(a, b).intersection(scissor)?;
    Some(instance(a, b - a, Vec2::ZERO, uv, sprite, 0.0))
}

/// The instance of a glyph's quad in output pixels, or `None` when it has
/// no pixels inside `scissor`. Its corner snaps as a sprite's, and its size
/// is its own scaled, rounded to whole pixels for a free scale, so that a
/// character is as wide wherever it lands.
fn pack_glyph(
    sprite: &SpriteDraw,
    placeholder: bool,
    space: &UiSpace,
    scissor: IRect,
) -> Option<[f32; SPRITE_INSTANCE_FLOATS]> {
    let (uv, _) = uv_and_pivot(sprite, placeholder);
    let a = space.corner(sprite.position);
    let size = if space.whole_units {
        sprite.size * space.scale
    } else {
        snap(sprite.size * space.scale)
    };
    pixels(a, a + size).intersection(scissor)?;
    Some(instance(a, size, Vec2::ZERO, uv, sprite, 0.0))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use nadir_assets::Handle;
    use nadir_core::{Color, GenIndex};

    use super::*;
    use crate::Flip;

    fn handle<T>(index: u32) -> Handle<T> {
        Handle::new(GenIndex::new(index, NonZeroU32::MIN))
    }

    /// A sprite of the texture `texture`, its top-left corner at `(x, y)`,
    /// `w` × `h`.
    fn rect(texture: u32, x: f32, y: f32, w: f32, h: f32) -> SpriteDraw {
        let mut sprite = SpriteDraw::new(handle(texture), Vec2::new(x, y), Vec2::new(w, h));
        sprite.pivot = Vec2::ZERO;
        sprite
    }

    fn irect(x0: i32, y0: i32, x1: i32, y1: i32) -> IRect {
        IRect::new(IVec2::new(x0, y0), IVec2::new(x1, y1))
    }

    /// 64 × 36 into 151 × 91: a factor of 2, the image at (11, 9) to
    /// (139, 81), with bars of 11 and 12, and 9 and 10.
    const VIRTUAL: UVec2 = UVec2::new(64, 36);
    const OUTPUT: UVec2 = UVec2::new(151, 91);

    fn space(scale: UiScale) -> UiSpace {
        UiSpace::new(scale, VIRTUAL, OUTPUT)
    }

    #[test]
    fn pixel_ui_is_the_upscaled_image_and_a_free_scale_all_of_the_output() {
        let pixel = space(UiScale::Pixel(1));
        assert_eq!(
            (pixel.scale, pixel.origin, pixel.size, pixel.rect),
            (
                2.0,
                IVec2::new(11, 9),
                Vec2::new(64.0, 36.0),
                irect(11, 9, 139, 81)
            )
        );
        // Two virtual pixels a unit: half as many units, as large.
        let double = space(UiScale::Pixel(2));
        assert_eq!((double.scale, double.size), (4.0, Vec2::new(32.0, 18.0)));
        assert_eq!((double.origin, double.rect), (pixel.origin, pixel.rect));
        assert_eq!(space(UiScale::Pixel(0)), pixel, "0 is 1");
        assert_eq!(space(UiScale::default()), pixel);

        let free = space(UiScale::Free(1.5));
        assert_eq!(
            (free.scale, free.origin, free.rect),
            (1.5, IVec2::ZERO, irect(0, 0, 151, 91))
        );
        assert_eq!(free.size, Vec2::new(151.0 / 1.5, 91.0 / 1.5));

        // An output smaller than the image crops it: the pixel UI's origin
        // is off the output, and it draws on all of the output.
        let cropped = UiSpace::new(UiScale::Pixel(1), VIRTUAL, UVec2::new(50, 30));
        assert_eq!(
            (cropped.scale, cropped.origin, cropped.rect),
            (1.0, IVec2::new(-7, -3), irect(0, 0, 50, 30))
        );
        // No output, no pixels.
        for scale in [UiScale::Pixel(1), UiScale::Free(1.0)] {
            assert!(UiSpace::new(scale, VIRTUAL, UVec2::ZERO).rect.is_empty());
        }
    }

    #[test]
    fn a_free_scale_is_held_between_an_eighth_and_64() {
        let scales = [0.0, -2.0, 0.1, 1.0, 64.5, f32::INFINITY, f32::NAN];
        let held = scales.map(|scale| space(UiScale::Free(scale)).scale);
        assert_eq!(held, [0.125, 0.125, 0.125, 1.0, 64.0, 64.0, 1.0]);
    }

    #[test]
    fn the_pointer_maps_to_ui_units_and_back() {
        let pixel = space(UiScale::Pixel(1));
        assert_eq!(pixel.to_ui(Vec2::new(11.0, 9.0)), Vec2::ZERO);
        assert_eq!(pixel.to_ui(Vec2::new(12.5, 10.0)), Vec2::new(0.75, 0.5));
        assert_eq!(pixel.to_ui(Vec2::new(0.0, 0.0)), Vec2::new(-5.5, -4.5));
        assert_eq!(
            pixel.to_screen(Vec2::new(63.5, 36.0)),
            Vec2::new(138.0, 81.0)
        );
        let free = space(UiScale::Free(2.0));
        assert_eq!(free.to_ui(Vec2::new(11.0, 9.0)), Vec2::new(5.5, 4.5));
        assert_eq!(free.to_screen(Vec2::new(5.5, 4.5)), Vec2::new(11.0, 9.0));
    }

    #[test]
    fn a_clip_inside_a_clip_is_cut_by_both_and_one_outside_cuts_everything() {
        let window = Rect::new(Vec2::new(0.0, 0.0), Vec2::new(20.0, 20.0));
        let list = Rect::new(Vec2::new(10.0, 5.0), Vec2::new(40.0, 15.0));
        let view = UiView::default().clipped(window).clipped(list);
        assert_eq!(
            view.clip(),
            Some(Rect::new(Vec2::new(10.0, 5.0), Vec2::new(20.0, 15.0)))
        );
        assert_eq!(UiView::default().clip(), None);

        // Scrolled out of its window: nothing, never no clip.
        let away = Rect::new(Vec2::new(30.0, 0.0), Vec2::new(40.0, 10.0));
        let gone = UiView::default().clipped(window).clipped(away);
        let empty = gone.clip().expect("a clip");
        assert!(empty.is_empty());
        let nan = UiView::default().clipped(Rect::new(Vec2::new(f32::NAN, 0.0), Vec2::ONE));
        assert!(nan.clip().expect("a clip").is_empty());
        let flat = UiView::default().clipped(Rect::new(Vec2::ONE, Vec2::new(5.0, 1.0)));
        assert!(flat.clip().expect("a clip").is_empty());
        // Inside out: not the rect between its corners.
        let inverted = UiView::default().clipped(Rect::new(Vec2::splat(5.0), Vec2::splat(2.0)));
        assert!(inverted.clip().expect("a clip").is_empty());
        let pixel = space(UiScale::Pixel(1));
        for view in [gone, nan, flat, inverted] {
            assert_eq!(pixel.scissor(view.clip()), None);
        }
        assert_eq!(view.scale(), UiScale::Pixel(1));
    }

    #[test]
    fn a_clip_s_edges_snap_as_corners_do_and_stay_in_the_space() {
        let clip = Some(Rect::new(Vec2::new(0.4, 0.6), Vec2::new(10.5, 10.5)));
        // Pixel UI: to whole units, halves up, then 2 pixels a unit.
        assert_eq!(
            space(UiScale::Pixel(1)).scissor(clip),
            Some(irect(11, 11, 33, 31))
        );
        // A free scale: to whole pixels. 0.4 × 1.5 is 0.6, which is 1.
        assert_eq!(
            space(UiScale::Free(1.5)).scissor(clip),
            Some(irect(1, 1, 16, 16))
        );
        // Unclipped, or infinite: the space's own pixels.
        let pixel = space(UiScale::Pixel(1));
        assert_eq!(pixel.scissor(None), Some(pixel.rect));
        let all = Rect::new(Vec2::splat(f32::NEG_INFINITY), Vec2::splat(f32::INFINITY));
        assert_eq!(pixel.scissor(Some(all)), Some(pixel.rect));
        // Off the image: nowhere.
        let off = Rect::new(Vec2::new(-10.0, 0.0), Vec2::new(-1.0, 5.0));
        assert_eq!(pixel.scissor(Some(off)), None);
        assert_eq!(
            space(UiScale::Free(1.0)).scissor(Some(off)),
            None,
            "left of the output"
        );
    }

    #[test]
    fn a_list_keeps_its_order_across_sprites_and_texts_each_with_its_view() {
        let font = handle(9);
        let mut list = UiList::new();
        assert!(list.is_empty());
        assert_eq!(list.view(), UiView::default());
        list.sprite(rect(1, 0.0, 0.0, 4.0, 4.0));
        list.text(TextDraw::new(font, Vec2::ZERO), "a");
        let free = UiView::new(UiScale::Free(2.0));
        list.set_view(free);
        assert_eq!(list.view(), free);
        list.write(TextDraw::new(font, Vec2::ZERO), format_args!("b{}", 2));
        list.sprite(rect(2, 0.0, 0.0, 4.0, 4.0));
        let all: Vec<(String, UiView)> = list
            .iter()
            .map(|(draw, view)| {
                let what = match draw {
                    UiDraw::Sprite(sprite) => format!("sprite {}", sprite.texture.id().index()),
                    UiDraw::Text(_, text) => format!("text {text}"),
                };
                (what, view)
            })
            .collect();
        let pixel = UiView::default();
        assert_eq!(
            all,
            [
                ("sprite 1".to_string(), pixel),
                ("text a".to_string(), pixel),
                ("text b2".to_string(), free),
                ("sprite 2".to_string(), free),
            ]
        );
        assert_eq!(list.len(), 4);
    }

    #[test]
    fn a_view_set_twice_before_anything_is_pushed_is_one_view() {
        let mut list = UiList::new();
        list.set_view(UiView::new(UiScale::Free(3.0)));
        list.set_view(UiView::new(UiScale::Free(2.0)));
        list.sprite(rect(1, 0.0, 0.0, 1.0, 1.0));
        list.set_view(UiView::new(UiScale::Free(2.0)));
        list.sprite(rect(1, 0.0, 0.0, 1.0, 1.0));
        assert_eq!(list.views, [UiView::new(UiScale::Free(2.0))]);
        list.set_view(UiView::default());
        list.set_view(UiView::new(UiScale::Free(2.0)));
        list.sprite(rect(1, 0.0, 0.0, 1.0, 1.0));
        assert_eq!(list.views.len(), 1, "set back before anything used it");
    }

    #[test]
    fn a_view_s_scale_is_the_one_drawn_so_views_of_one_scale_are_equal() {
        assert_eq!(UiView::new(UiScale::Pixel(0)).scale(), UiScale::Pixel(1));
        assert_eq!(
            UiView::new(UiScale::Free(f32::NAN)).scale(),
            UiScale::Free(1.0)
        );
        assert_eq!(
            UiView::new(UiScale::Free(100.0)).scale(),
            UiScale::Free(64.0)
        );
        let mut list = UiList::new();
        for _ in 0..3 {
            list.set_view(UiView::new(UiScale::Free(f32::NAN)));
            list.sprite(rect(1, 0.0, 0.0, 1.0, 1.0));
        }
        assert_eq!(list.views.len(), 1);
        assert_eq!(build(&list, &[]).batches.len(), 1);
    }

    #[test]
    fn a_text_whose_formatting_fails_is_left_out_of_the_ui() {
        struct Fails;
        impl fmt::Display for Fails {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("half")?;
                Err(fmt::Error)
            }
        }
        let mut list = UiList::new();
        list.write(
            TextDraw::new(handle(1), Vec2::ZERO),
            format_args!("{Fails}"),
        );
        list.text(TextDraw::new(handle(1), Vec2::ZERO), "kept");
        assert_eq!(list.len(), 1);
        assert_eq!(list.text_items, [0]);
        assert!(matches!(
            list.iter().next(),
            Some((UiDraw::Text(_, "kept"), _))
        ));
    }

    #[test]
    fn a_cleared_list_keeps_its_buffers_and_goes_back_to_the_default_view() {
        let mut list = UiList::new();
        let fill = |list: &mut UiList| {
            list.set_view(UiView::new(UiScale::Free(2.0)));
            for i in 0..20 {
                list.sprite(rect(1, i as f32, 0.0, 1.0, 1.0));
                list.write(
                    TextDraw::new(handle(1), Vec2::ZERO),
                    format_args!("row {i}"),
                );
            }
        };
        fill(&mut list);
        let capacity = (
            list.items.capacity(),
            list.sprites.capacity(),
            list.text_items.capacity(),
        );
        list.clear();
        assert!(list.is_empty());
        assert_eq!(list.view(), UiView::default());
        fill(&mut list);
        assert_eq!(
            (
                list.items.capacity(),
                list.sprites.capacity(),
                list.text_items.capacity()
            ),
            capacity
        );
    }

    /// A glyph quad of the `text`th text, on the page `page`, at `(x, y)`.
    fn glyph(text: u32, page: u32, x: f32, y: f32) -> GlyphQuad<(u32, Pipe)> {
        GlyphQuad {
            sprite: rect(page, x, y, 3.0, 5.0),
            text,
            texture: (page, Pipe::Text),
            placeholder: false,
        }
    }

    /// Builds `list` and `glyphs` into 151 × 91, each sprite's texture its
    /// handle's index, ready.
    fn build(list: &UiList, glyphs: &[GlyphQuad<(u32, Pipe)>]) -> UiPass<u32> {
        let mut pass = UiPass::new();
        pass.build(list, glyphs, VIRTUAL, OUTPUT, |sprite| {
            Some((sprite.texture.id().index(), false))
        });
        pass
    }

    /// The instances' (position, size), in draw order.
    fn quads(pass: &UiPass<u32>) -> Vec<(Vec2, Vec2)> {
        pass.packed
            .chunks(SPRITE_INSTANCE_FLOATS)
            .map(|f| (Vec2::new(f[0], f[1]), Vec2::new(f[2], f[3])))
            .collect()
    }

    fn batches(pass: &UiPass<u32>) -> Vec<(u32, Pipe, u32, u32, u32)> {
        pass.batches
            .iter()
            .map(|b| (b.texture.0, b.texture.1, b.view, b.start, b.count))
            .collect()
    }

    #[test]
    fn windows_draw_in_the_order_pushed_sprites_and_texts_alike() {
        // Two windows that overlap, each a panel and its label, and a caret
        // after the second label: nothing of the first window shows
        // through the second.
        let font = handle(9);
        let mut list = UiList::new();
        list.sprite(rect(2, 0.0, 0.0, 20.0, 10.0));
        list.text(TextDraw::new(font, Vec2::new(1.0, 1.0)), "a");
        list.sprite(rect(2, 5.0, 5.0, 20.0, 10.0));
        list.text(TextDraw::new(font, Vec2::new(6.0, 6.0)), "b");
        list.sprite(rect(3, 9.0, 6.0, 1.0, 5.0));
        let glyphs = [glyph(0, 5, 1.0, 1.0), glyph(1, 5, 6.0, 6.0)];
        let pass = build(&list, &glyphs);
        assert_eq!(
            batches(&pass),
            [
                (2, Pipe::Sprite, 0, 0, 1),
                (5, Pipe::Text, 0, 1, 1),
                (2, Pipe::Sprite, 0, 2, 1),
                (5, Pipe::Text, 0, 3, 1),
                (3, Pipe::Sprite, 0, 4, 1),
            ]
        );
        assert_eq!((pass.sprites, pass.glyphs, pass.skipped), (3, 2, 0));
    }

    #[test]
    fn layers_and_fixed_depths_order_before_the_push_and_ysort_is_depth_0() {
        let mut list = UiList::new();
        let mut top = rect(1, 0.0, 0.0, 1.0, 1.0);
        top.layer = 1;
        list.sprite(top);
        let mut raised = rect(2, 0.0, 0.0, 1.0, 1.0);
        raised.sort = SortMode::Fixed(5);
        list.sprite(raised);
        // Y-sorted, by their feet at 30 and then 10: in the order pushed.
        let mut low = SpriteDraw::new(handle(3), Vec2::new(0.0, 30.0), Vec2::ONE);
        list.sprite(low);
        low.position.y = 10.0;
        low.texture = handle(4);
        list.sprite(low);
        let order: Vec<u32> = build(&list, &[])
            .batches
            .iter()
            .map(|b| b.texture.0)
            .collect();
        assert_eq!(order, [3, 4, 2, 1]);

        // A text's layer and depth are its draw's: pushed first, in front.
        let mut list = UiList::new();
        let mut label = TextDraw::new(handle(9), Vec2::ZERO);
        label.layer = 1;
        list.text(label, "a");
        let mut raised = TextDraw::new(handle(9), Vec2::ZERO);
        raised.sort = SortMode::Fixed(1);
        list.text(raised, "b");
        list.sprite(rect(2, 0.0, 0.0, 4.0, 4.0));
        let glyphs = [glyph(0, 5, 0.0, 0.0), glyph(1, 6, 0.0, 0.0)];
        let order: Vec<u32> = build(&list, &glyphs)
            .batches
            .iter()
            .map(|b| b.texture.0)
            .collect();
        assert_eq!(order, [2, 6, 5]);
    }

    #[test]
    fn a_text_s_glyphs_keep_its_place_grouped_by_page() {
        let font = handle(9);
        let mut list = UiList::new();
        list.text(TextDraw::new(font, Vec2::ZERO), "ab");
        list.sprite(rect(2, 0.0, 0.0, 4.0, 4.0));
        let mut raised = TextDraw::new(font, Vec2::ZERO);
        raised.layer = 1;
        list.text(raised, "c");
        // The first text's glyphs on pages 6, 5, 6: by page, in their order.
        let glyphs = [
            glyph(0, 6, 0.0, 0.0),
            glyph(0, 5, 4.0, 0.0),
            glyph(0, 6, 8.0, 0.0),
            glyph(1, 5, 0.0, 9.0),
        ];
        let pass = build(&list, &glyphs);
        assert_eq!(
            batches(&pass),
            [
                (5, Pipe::Text, 0, 0, 1),
                (6, Pipe::Text, 0, 1, 2),
                (2, Pipe::Sprite, 0, 3, 1),
                (5, Pipe::Text, 0, 4, 1),
            ]
        );
        let xs: Vec<f32> = quads(&pass)[1..3].iter().map(|(p, _)| p.x).collect();
        // At factor 2 from x 11: 0 and 8 units.
        assert_eq!(xs, [11.0, 27.0]);

        // Pages whose handles share their low 16 bits are two pages.
        let mut list = UiList::new();
        list.text(TextDraw::new(font, Vec2::ZERO), "abc");
        let far = 3 + (1 << 16);
        let glyphs = [
            glyph(0, 3, 0.0, 0.0),
            glyph(0, far, 4.0, 0.0),
            glyph(0, 3, 8.0, 0.0),
        ];
        assert_eq!(
            batches(&build(&list, &glyphs)),
            [(3, Pipe::Text, 0, 0, 2), (far, Pipe::Text, 0, 2, 1)]
        );
    }

    #[test]
    fn a_batch_is_one_texture_one_program_and_one_view() {
        let mut list = UiList::new();
        list.sprite(rect(1, 0.0, 0.0, 1.0, 1.0));
        list.sprite(rect(1, 2.0, 0.0, 1.0, 1.0));
        list.set_view(UiView::default().clipped(Rect::new(Vec2::ZERO, Vec2::splat(50.0))));
        list.sprite(rect(1, 4.0, 0.0, 1.0, 1.0));
        let pass = build(&list, &[]);
        assert_eq!(
            batches(&pass),
            [(1, Pipe::Sprite, 0, 0, 2), (1, Pipe::Sprite, 1, 2, 1)]
        );
        assert_eq!(pass.scissor(0), Some(irect(11, 9, 139, 81)));
        assert_eq!(pass.scissor(1), Some(irect(11, 9, 111, 81)));
    }

    #[test]
    fn what_is_outside_its_clip_or_has_no_pixels_is_not_drawn_nor_counted() {
        let font = handle(9);
        let mut list = UiList::new();
        list.set_view(UiView::default().clipped(Rect::new(Vec2::ZERO, Vec2::splat(10.0))));
        list.sprite(rect(1, 2.0, 2.0, 4.0, 4.0));
        list.sprite(rect(1, 12.0, 2.0, 4.0, 4.0));
        list.sprite(rect(1, 2.0, 2.0, 0.0, 4.0));
        // Half a unit: to whole units, nothing.
        list.sprite(rect(1, 3.6, 2.0, 0.3, 4.0));
        list.text(TextDraw::new(font, Vec2::ZERO), "ab");
        // Rotated: outside by the box around its corners, of no size, or
        // turned by NaN.
        let mut turned = rect(1, 30.0, 2.0, 4.0, 4.0);
        turned.rotation = 0.5;
        list.sprite(turned);
        turned.position = Vec2::new(4.0, 4.0);
        turned.size = Vec2::ZERO;
        list.sprite(turned);
        turned.size = Vec2::splat(4.0);
        turned.rotation = f32::NAN;
        list.sprite(turned);
        // Inside, turned: drawn.
        turned.rotation = 0.5;
        list.sprite(turned);
        // A view that cuts everything draws nothing of its own; what is not
        // ready there is skipped still, as its glyphs would be.
        list.set_view(UiView::default().clipped(Rect::new(Vec2::splat(50.0), Vec2::splat(40.0))));
        list.sprite(rect(1, 2.0, 2.0, 4.0, 4.0));
        list.sprite(rect(7, 2.0, 2.0, 4.0, 4.0));
        list.text(TextDraw::new(font, Vec2::ZERO), "c");
        let glyphs = [
            glyph(0, 5, 1.0, 1.0),
            glyph(0, 5, 11.0, 1.0),
            glyph(1, 5, 1.0, 1.0),
        ];
        let mut pass = UiPass::new();
        pass.build(&list, &glyphs, VIRTUAL, OUTPUT, |sprite| {
            match sprite.texture.id().index() {
                7 => None,
                texture => Some((texture, false)),
            }
        });
        assert_eq!((pass.sprites, pass.glyphs, pass.skipped), (2, 1, 1));
        assert_eq!(pass.packed.len(), 3 * SPRITE_INSTANCE_FLOATS);
        assert_eq!(pass.packed[2 * SPRITE_INSTANCE_FLOATS + 14], 0.5, "turned");
    }

    #[test]
    fn a_sprite_not_ready_is_skipped_and_a_failed_one_shows_its_placeholder() {
        let mut list = UiList::new();
        let mut sprite = rect(1, 0.0, 0.0, 4.0, 4.0);
        sprite.uv = Rect::new(Vec2::new(0.5, 0.5), Vec2::ONE);
        list.sprite(sprite);
        sprite.texture = handle(2);
        list.sprite(sprite);
        let mut pass = UiPass::new();
        pass.build(&list, &[], VIRTUAL, OUTPUT, |sprite| {
            match sprite.texture.id().index() {
                1 => None,
                texture => Some((texture, true)),
            }
        });
        assert_eq!((pass.sprites, pass.skipped), (1, 1));
        assert_eq!(pass.packed[6..10], [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(pass.batches[0].texture, (2, Pipe::Sprite));
    }

    /// Packs `sprite` in `space`, unclipped.
    fn pack(sprite: &SpriteDraw, space: &UiSpace) -> [f32; SPRITE_INSTANCE_FLOATS] {
        pack_sprite(sprite, false, space, space.rect).expect("drawn")
    }

    #[test]
    fn at_a_free_scale_sprites_that_share_an_edge_share_it_in_pixels() {
        // At 1.5, a title bar 3 high and the body below it: the bar's
        // bottom is 4.5 pixels down, and the body's top too, both 5.
        let free = space(UiScale::Free(1.5));
        let bar = pack(&rect(1, 0.0, 0.0, 3.0, 3.0), &free);
        let body = pack(&rect(1, 0.0, 3.0, 3.0, 3.0), &free);
        assert_eq!(bar[0..4], [0.0, 0.0, 5.0, 5.0]);
        assert_eq!(body[0..4], [0.0, 5.0, 5.0, 4.0]);
        // Placed by its corner: the pivot is 0, unrotated.
        assert_eq!(bar[4..6], [0.0, 0.0]);
        assert_eq!(bar[14], 0.0);
    }

    #[test]
    fn a_ui_sprite_snaps_by_its_pivot_rounded_and_mirrors_about_it() {
        // 13 wide, its pivot 7 pixels in at x 7.5: its corner at 0.5 goes
        // up to 1, which an unrounded 7/13 × 13 would take down to 0.
        let pixel = space(UiScale::Pixel(1));
        let mut sprite = rect(1, 7.5, 0.0, 13.0, 2.0);
        sprite.pivot = Vec2::new(7.0 / 13.0, 0.0);
        let floats = pack(&sprite, &pixel);
        assert_eq!(floats[0..4], [11.0 + 2.0, 9.0, 26.0, 4.0]);
        // Mirrored: the pivot is 6 in from the left, the corner at 1.5,
        // which goes up to 2; the uv swapped.
        sprite.flip = Flip { x: true, y: false };
        sprite.uv = Rect::new(Vec2::new(0.25, 0.0), Vec2::new(0.75, 1.0));
        let floats = pack(&sprite, &pixel);
        assert_eq!(floats[0..4], [11.0 + 4.0, 9.0, 26.0, 4.0]);
        assert_eq!(floats[6..10], [0.75, 0.0, 0.25, 1.0]);
        // Two virtual pixels a unit: 2 units of 4 pixels, from 11.
        let mut small = rect(1, 2.25, 1.0, 1.0, 1.0);
        small.color = Color::rgb(255, 0, 0);
        let floats = pack(&small, &space(UiScale::Pixel(2)));
        assert_eq!(floats[0..4], [19.0, 13.0, 4.0, 4.0]);
        assert_eq!(floats[10..14], [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn a_rotated_ui_sprite_is_scaled_about_its_pivot_and_not_moved() {
        let mut sprite = rect(1, 2.25, 1.0, 4.0, 2.0);
        sprite.pivot = Vec2::new(0.5, 0.5);
        sprite.rotation = 0.5;
        let floats = pack(&sprite, &space(UiScale::Pixel(1)));
        assert_eq!(floats[0..6], [15.5, 11.0, 8.0, 4.0, 0.5, 0.5]);
        assert_eq!(floats[14], 0.5);
    }

    #[test]
    fn a_glyph_is_as_wide_wherever_it_lands() {
        // 3 × 5 at 1.5 is 4.5 × 7.5, which is 5 × 8 at x 0 and at x 1; a
        // sprite there would be 5, then 4.
        let free = space(UiScale::Free(1.5));
        let widths: Vec<[f32; 4]> = [0.0, 1.0]
            .map(|x| {
                let floats =
                    pack_glyph(&rect(1, x, 0.0, 3.0, 5.0), false, &free, free.rect).expect("drawn");
                [floats[0], floats[1], floats[2], floats[3]]
            })
            .to_vec();
        assert_eq!(widths, [[0.0, 0.0, 5.0, 8.0], [2.0, 0.0, 5.0, 8.0]]);
        let sprite = pack(&rect(1, 1.0, 0.0, 3.0, 5.0), &free);
        assert_eq!(sprite[2], 4.0);
        // Pixel UI: whole units, so whole pixels.
        let pixel = space(UiScale::Pixel(1));
        let mut glyph = rect(1, 1.0, 2.0, 3.0, 5.0);
        glyph.uv = Rect::new(Vec2::new(0.25, 0.5), Vec2::new(0.5, 1.0));
        let floats = pack_glyph(&glyph, false, &pixel, pixel.rect).expect("drawn");
        assert_eq!(floats[0..4], [13.0, 13.0, 6.0, 10.0]);
        assert_eq!(floats[6..10], [0.25, 0.5, 0.5, 1.0]);
        // On a page that failed: all of its placeholder.
        let floats = pack_glyph(&glyph, true, &pixel, pixel.rect).expect("drawn");
        assert_eq!(floats[6..10], [0.0, 0.0, 1.0, 1.0]);
    }
}
