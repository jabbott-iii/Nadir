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

//! F4's debug drawing (roadmap M7.6, ADR-0064): the overlays its window
//! ticks, drawn into the world as sprites of the debug UI's white texel, a
//! virtual pixel wide, in front of every sprite, as the inspector's outline
//! is (render's `DebugDrawList` is not built).

use nadir_assets::Handle;
use nadir_collision::{ColliderKind, Shape};
use nadir_core::{Color, IVec2, Rect, UVec2, Vec2};
use nadir_debug::Ui;
use nadir_ecs::World;
use nadir_render::{
    Camera2D, Font, RenderFrame, SortMode, SpriteDraw, TextDraw, Texture, TileLayer, TileLayers,
};

use crate::{Collider, Collisions, Parent, PrevTransform, Transform};

/// The solid colliders' outlines.
pub(crate) const SOLID: Color = Color::rgb(0, 220, 255);
/// The triggers' outlines.
pub(crate) const TRIGGER: Color = Color::rgb(255, 96, 255);
/// The collision grid's solid cells, filled.
pub(crate) const GRID: Color = Color::rgba(0, 220, 255, 56);
/// The spatial hash's cells and their counts.
pub(crate) const HASH: Color = Color::rgba(255, 255, 255, 160);
/// The tile layers' chunks.
pub(crate) const CHUNK: Color = Color::rgb(255, 160, 0);
/// The camera's cross and view.
pub(crate) const CAMERA: Color = Color::rgb(255, 64, 64);

/// How far each arm of the camera's cross reaches, in virtual pixels.
const CROSS: f32 = 6.0;

/// How many sides a circle is drawn with.
const SIDES: u32 = 24;

/// Which overlays draw, as the window's checkboxes leave them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Overlays {
    pub(crate) colliders: bool,
    pub(crate) triggers: bool,
    pub(crate) grid: bool,
    pub(crate) hash: bool,
    pub(crate) chunks: bool,
    pub(crate) camera: bool,
}

impl Overlays {
    /// The colliders, the triggers and the grid: what a game's collision is
    /// made of. The rest are the engine's insides.
    pub(crate) fn new() -> Self {
        Self {
            colliders: true,
            triggers: true,
            grid: true,
            hash: false,
            chunks: false,
            camera: false,
        }
    }

    /// Fills the window: a checkbox for each overlay, its name in its color.
    pub(crate) fn show(&mut self, ui: &mut Ui<'_>) {
        let rows: [(&str, Color, &mut bool); 6] = [
            ("Colliders##colliders", SOLID, &mut self.colliders),
            ("Triggers##triggers", TRIGGER, &mut self.triggers),
            ("Tile grid##grid", Color::rgb(0, 220, 255), &mut self.grid),
            (
                "Spatial hash##hash",
                Color::rgb(255, 255, 255),
                &mut self.hash,
            ),
            ("Tile chunks##chunks", CHUNK, &mut self.chunks),
            ("Camera##camera", CAMERA, &mut self.camera),
        ];
        for (text, color, on) in rows {
            let (name, id) = text.split_once("##").unwrap_or((text, text));
            ui.horizontal(|ui| {
                ui.with_id(id, |ui| {
                    ui.checkbox("##on", on);
                });
                ui.label_colored(color, name);
            });
        }
    }

    /// Draws the overlays ticked into `world`'s frame, as it is drawn: each
    /// entity `alpha` of the way from the last tick's place to this one's,
    /// in the view the frame's camera gives a virtual image of
    /// `virtual_resolution`. Only what touches the view is drawn. With
    /// `white`, the debug UI's white texel, and `font`, its font.
    pub(crate) fn draw(
        &self,
        world: &World,
        white: Handle<Texture>,
        font: Handle<Font>,
        virtual_resolution: UVec2,
        alpha: f32,
    ) {
        let mut frame = world.resource_mut::<RenderFrame>();
        let RenderFrame {
            camera,
            sprites,
            texts,
            tile_layers,
            ..
        } = &mut *frame;
        let (view, zoom) = view(camera, virtual_resolution);
        let mut pen = Pen {
            sprites,
            white,
            width: 1.0 / zoom,
        };

        if self.colliders || self.triggers {
            let transforms = world.read::<Transform>();
            let previous = world.read::<PrevTransform>();
            let parents = world.read::<Parent>();
            // Where the frame draws an entity: `alpha` of the way from the
            // last tick's place, as `nadir::extract_sprites` does.
            let drawn_at = |entity| {
                let current = transforms.get(entity)?.position;
                Some(
                    previous
                        .get(entity)
                        .map_or(current, |last| last.0.position.lerp(current, alpha)),
                )
            };
            for (entity, collider) in world.read::<Collider>().iter() {
                let collider = collider.0;
                let (on, color) = match collider.kind {
                    ColliderKind::Solid => (self.colliders, SOLID),
                    ColliderKind::Trigger => (self.triggers, TRIGGER),
                };
                if !on || !transforms.contains(entity) {
                    continue;
                }
                // A child where its root is drawn, with its offset, as its
                // sprite would be: its own place is the root's at the tick.
                let parented = if parents.contains(entity) {
                    crate::glue::root(&parents, entity)
                        .and_then(|(root, offset)| Some(drawn_at(root)? + offset))
                } else {
                    None
                };
                let Some(at) = parented.or_else(|| drawn_at(entity)) else {
                    continue;
                };
                if !touches(collider.bounds(at), view) {
                    continue;
                }
                let center = collider.center(at);
                match collider.shape {
                    Shape::Aabb { half } => {
                        pen.outline(Rect::new(center - half, center + half), color)
                    }
                    Shape::Circle { radius } => pen.circle(center, radius, color),
                }
            }
        }

        if self.grid || self.hash {
            let collisions = world.resource::<Collisions>();
            if self.grid {
                let grid = collisions.grid();
                if grid.width() > 0 && grid.height() > 0 && grid.cell_size() > 0.0 {
                    let last = IVec2::new(
                        i32::try_from(grid.width() - 1).unwrap_or(i32::MAX),
                        i32::try_from(grid.height() - 1).unwrap_or(i32::MAX),
                    );
                    let first = grid.cell_at(view.min).max(IVec2::ZERO);
                    let end = grid.cell_at(view.max).min(last);
                    for y in first.y..=end.y {
                        for x in first.x..=end.x {
                            let cell = IVec2::new(x, y);
                            if grid.is_solid(cell) {
                                pen.fill(grid.cell_rect(cell), GRID);
                            }
                        }
                    }
                }
            }
            if self.hash {
                collisions.world().hash_cells(|cell, count| {
                    if !touches(cell, view) {
                        return;
                    }
                    pen.outline(cell, HASH);
                    let mut draw = TextDraw::new(font, cell.min + Vec2::splat(2.0 / zoom));
                    draw.color = HASH;
                    draw.layer = u8::MAX;
                    draw.sort = SortMode::Fixed(u32::MAX);
                    draw.layout.markup = false;
                    texts.write(draw, format_args!("{count}"));
                });
            }
        }

        if self.chunks {
            let layers = world.resource::<TileLayers>();
            for drawn in tile_layers.iter() {
                let Some(layer) = layers.get(drawn.tiles) else {
                    continue;
                };
                chunks(layer, view, |rect| pen.outline(rect, CHUNK));
            }
        }

        if self.camera {
            let arm = CROSS / zoom;
            let at = camera.position;
            pen.line(at - Vec2::new(arm, 0.0), at + Vec2::new(arm, 0.0), CAMERA);
            pen.line(at - Vec2::new(0.0, arm), at + Vec2::new(0.0, arm), CAMERA);
            pen.outline(view, CAMERA);
        }
    }
}

/// The part of the world a camera shows in a virtual image of
/// `virtual_resolution`, and its zoom, as render's view makes them: the
/// zoom rounded to a whole number, at least 1, around the camera's
/// position and its shake.
pub(crate) fn view(camera: &Camera2D, virtual_resolution: UVec2) -> (Rect, f32) {
    let zoom = (camera.zoom + 0.5).floor().max(1.0);
    let zoom = if zoom.is_finite() { zoom } else { 1.0 };
    let half = virtual_resolution.max(UVec2::ONE).as_vec2() / (2.0 * zoom);
    let center = camera.position + camera.shake;
    (Rect::new(center - half, center + half), zoom)
}

/// Calls `each` with the rect of each chunk of `layer` that touches
/// `view`: [`TileLayer::CHUNK`] tiles a side, cut at the layer's edge.
pub(crate) fn chunks(layer: &TileLayer, view: Rect, mut each: impl FnMut(Rect)) {
    let tile = layer.tile_size();
    let side = TileLayer::CHUNK as f32 * tile;
    if !(side > 0.0 && side.is_finite()) {
        return;
    }
    let origin = layer.origin();
    let end = origin + layer.size().as_vec2() * tile;
    let count = layer.chunks();
    for y in 0..count.y {
        for x in 0..count.x {
            let min = origin + Vec2::new(x as f32, y as f32) * side;
            let rect = Rect::new(min, (min + Vec2::splat(side)).min(end));
            if touches(rect, view) {
                each(rect);
            }
        }
    }
}

/// Whether `a` and `b` touch or overlap, edges included: a box of no area
/// (a collider of no half width) is drawn too.
fn touches(a: Rect, b: Rect) -> bool {
    a.min.x <= b.max.x && a.max.x >= b.min.x && a.min.y <= b.max.y && a.max.y >= b.min.y
}

/// Draws lines a set width into a frame's sprites, of the white texel, in
/// front of every sprite of the top layer.
struct Pen<'a> {
    sprites: &'a mut Vec<SpriteDraw>,
    white: Handle<Texture>,
    /// In world units.
    width: f32,
}

impl Pen<'_> {
    /// `point` on the nearest corner of a virtual pixel, as render's view
    /// puts a sprite that is not turned: an edge drawn from snapped corners
    /// meets the next exactly.
    fn snap(&self, point: Vec2) -> Vec2 {
        let zoom = 1.0 / self.width;
        (point * zoom + Vec2::splat(0.5)).floor() / zoom
    }

    /// A sprite over `rect`, its corners on whole virtual pixels.
    fn fill(&mut self, rect: Rect, color: Color) {
        let (min, max) = (self.snap(rect.min), self.snap(rect.max));
        self.sprite(min, max - min, color);
    }

    /// A sprite of `size` with its top-left corner at `at`.
    fn sprite(&mut self, at: Vec2, size: Vec2, color: Color) {
        let mut draw = SpriteDraw::new(self.white, at, size);
        draw.pivot = Vec2::ZERO;
        draw.color = color;
        draw.layer = u8::MAX;
        draw.sort = SortMode::Fixed(u32::MAX);
        self.sprites.push(draw);
    }

    /// The edges of `rect`, inside it, its corners on whole virtual
    /// pixels: four sprites that meet and do not overlap, so a color that
    /// is not opaque stays even. A rect too thin for two edges is filled,
    /// a pixel at least each way.
    fn outline(&mut self, rect: Rect, color: Color) {
        let (min, max) = (rect.min.min(rect.max), rect.min.max(rect.max));
        let (min, max) = (self.snap(min), self.snap(max));
        let w = self.width;
        if max.x - min.x < 2.0 * w || max.y - min.y < 2.0 * w {
            self.sprite(min, (max - min).max(Vec2::splat(w)), color);
            return;
        }
        self.sprite(min, Vec2::new(max.x - min.x, w), color);
        self.sprite(
            Vec2::new(min.x, max.y - w),
            Vec2::new(max.x - min.x, w),
            color,
        );
        let side = Vec2::new(w, max.y - min.y - 2.0 * w);
        self.sprite(Vec2::new(min.x, min.y + w), side, color);
        self.sprite(Vec2::new(max.x - w, min.y + w), side, color);
    }

    /// A line from `from` to `to`, centered on it: a sprite turned about
    /// its start.
    fn line(&mut self, from: Vec2, to: Vec2, color: Color) {
        let along = to - from;
        let length = along.length();
        if !(length > 0.0 && length.is_finite()) {
            return;
        }
        let mut draw = SpriteDraw::new(self.white, from, Vec2::new(length, self.width));
        draw.pivot = Vec2::new(0.0, 0.5);
        draw.rotation = along.y.atan2(along.x);
        draw.color = color;
        draw.layer = u8::MAX;
        draw.sort = SortMode::Fixed(u32::MAX);
        self.sprites.push(draw);
    }

    /// A circle of [`SIDES`] lines.
    fn circle(&mut self, center: Vec2, radius: f32, color: Color) {
        let point = |side: u32| {
            let angle = side as f32 / SIDES as f32 * std::f32::consts::TAU;
            center + Vec2::new(angle.cos(), angle.sin()) * radius
        };
        for side in 0..SIDES {
            self.line(point(side), point(side + 1), color);
        }
    }
}
