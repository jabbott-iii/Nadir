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

//! The atlas asset: a cooked atlas file, `atlas/<group>.kbj`, which says
//! where each sprite's frames are on the atlas's pages (ADR-0033), and the
//! clips an Aseprite export's tags make of them (ADR-0035).

use std::time::Duration;

use nadir_assets::{
    Asset, AssetError, AssetPath, AssetServer, DecodeCtx, FinalizeCtx, Handle, Scope,
};
use nadir_codec::atlas::{self, SpriteKind};
use nadir_core::{Rect, UVec2, Vec2};

use crate::{Texture, TileSet};

/// An atlas the cook made: its pages, and its sprites by name, each with
/// its frames on the pages and its clips. A game loads it, then its pages as
/// the [`Texture`]s they are ([`Atlas::load_pages`]).
///
/// It needs no GPU, so [`Asset::finalize`] needs nothing in its context,
/// and a game can make it before its window opens, with
/// [`AssetServer::load_now`](nadir_assets::AssetServer::load_now). It has
/// no placeholder: a sprite that is not there has no region to stand in
/// for.
#[derive(Clone, Debug, PartialEq)]
pub struct Atlas {
    pages: Vec<AtlasPage>,
    sprites: Vec<AtlasSprite>,
}

/// A page of an atlas: a texture of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtlasPage {
    path: String,
    size: UVec2,
}

/// A sprite of an atlas: an image, a sheet of frames, an Aseprite export,
/// or a tile set.
#[derive(Clone, Debug, PartialEq)]
pub struct AtlasSprite {
    name: String,
    frames: Vec<AtlasFrame>,
    pivot: Option<Vec2>,
    tile_size: Option<UVec2>,
    clips: Vec<AtlasClip>,
}

/// A clip of a sprite, from a tag of its Aseprite export: frames of the
/// sprite shown one after another, looping or once. The facade makes a
/// `Clip` of each (`Clips::add_atlas`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtlasClip {
    name: String,
    looping: bool,
    frames: Vec<AtlasClipFrame>,
}

/// One step of an [`AtlasClip`]: a frame of its sprite, shown for a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasClipFrame {
    frame: usize,
    duration: Duration,
}

/// Where a frame is on its page, trimmed to its pixels, and where it was in
/// its untrimmed cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtlasFrame {
    page: usize,
    uv: Rect,
    size: UVec2,
    offset: UVec2,
    source_size: UVec2,
}

impl Atlas {
    /// The pages: the packed pages, then the tile sets'.
    pub fn pages(&self) -> &[AtlasPage] {
        &self.pages
    }

    /// The page of `frame`.
    pub fn page(&self, frame: &AtlasFrame) -> Option<&AtlasPage> {
        self.pages.get(frame.page)
    }

    /// Loads every page with `server`, in `scope`, as a [`Texture`], and
    /// returns their handles in the order of [`Atlas::pages`], so that a
    /// frame's texture is `handles[frame.page()]`. Load the pages in the
    /// scope the atlas was loaded in, so that unloading it frees them too.
    pub fn load_pages(&self, server: &mut AssetServer, scope: Scope) -> Vec<Handle<Texture>> {
        self.pages
            .iter()
            .map(|page| server.load_in(&page.path, scope))
            .collect()
    }

    /// The sprites, in byte order of their names.
    pub fn sprites(&self) -> &[AtlasSprite] {
        &self.sprites
    }

    /// The sprite named `name`: the image's path in the source assets
    /// without `.png`, such as `sprites/person`.
    pub fn sprite(&self, name: &str) -> Option<&AtlasSprite> {
        let index = self
            .sprites
            .binary_search_by(|sprite| sprite.name.as_bytes().cmp(name.as_bytes()))
            .ok()?;
        self.sprites.get(index)
    }
}

impl AtlasPage {
    /// Its path among the cooked files, such as `atlas/sprites_0.qoi`, to
    /// load it by.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Its size in pixels.
    pub fn size(&self) -> UVec2 {
        self.size
    }
}

impl AtlasSprite {
    /// Its name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its frames, in the sheet's order: one at least.
    pub fn frames(&self) -> &[AtlasFrame] {
        &self.frames
    }

    /// Frame `index`.
    pub fn frame(&self, index: usize) -> Option<&AtlasFrame> {
        self.frames.get(index)
    }

    /// Its pivot, in its untrimmed frame's pixels, from (0, 0) at the
    /// top-left corner: `None` for a tile set.
    pub fn pivot(&self) -> Option<Vec2> {
        self.pivot
    }

    /// A tile's width and height in pixels, if it is a tile set.
    pub fn tile_size(&self) -> Option<UVec2> {
        self.tile_size
    }

    /// Its clips, in byte order of their names: none for a tile set, or for
    /// a sprite that is not an Aseprite export with tags.
    pub fn clips(&self) -> &[AtlasClip] {
        &self.clips
    }

    /// Its clip named `name`, the tag's name, such as `walk`.
    pub fn clip(&self, name: &str) -> Option<&AtlasClip> {
        let index = self
            .clips
            .binary_search_by(|clip| clip.name.as_bytes().cmp(name.as_bytes()))
            .ok()?;
        self.clips.get(index)
    }

    /// It as a tile set of `texture`, which must be its page, if it is one.
    pub fn tile_set(&self, texture: Handle<Texture>) -> Option<TileSet> {
        let tile = self.tile_size?;
        let size = self.frames.first()?.source_size;
        Some(TileSet {
            texture,
            columns: u16::try_from(size.x / tile.x).ok()?,
            rows: u16::try_from(size.y / tile.y).ok()?,
        })
    }
}

impl AtlasClip {
    /// Its name within its sprite: not empty, with no `/`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether it starts again at its end; otherwise it stops on its last
    /// frame.
    pub fn is_looping(&self) -> bool {
        self.looping
    }

    /// What it shows, in order: one at least.
    pub fn frames(&self) -> &[AtlasClipFrame] {
        &self.frames
    }
}

impl AtlasClipFrame {
    /// The frame, an index into its sprite's [`AtlasSprite::frames`].
    pub fn frame(&self) -> usize {
        self.frame
    }

    /// How long it shows: a whole number of milliseconds, 1 or more.
    pub fn duration(&self) -> Duration {
        self.duration
    }
}

impl AtlasFrame {
    /// Its page, an index into [`Atlas::pages`].
    pub fn page(&self) -> usize {
        self.page
    }

    /// Its region of its page, normalized: (0, 0) to (1, 1) is all of it.
    /// For an empty frame, an empty region.
    pub fn uv(&self) -> Rect {
        self.uv
    }

    /// Its size once trimmed, in pixels: (0, 0) for an empty frame.
    pub fn size(&self) -> UVec2 {
        self.size
    }

    /// How far its top-left corner is from its untrimmed cell's.
    pub fn offset(&self) -> UVec2 {
        self.offset
    }

    /// Its untrimmed cell's size, in pixels.
    pub fn source_size(&self) -> UVec2 {
        self.source_size
    }

    /// Whether it has no pixels: the cook trimmed it to nothing.
    pub fn is_empty(&self) -> bool {
        self.size == UVec2::ZERO
    }

    /// `pivot`, in the untrimmed cell's pixels, as a point of this frame's
    /// trimmed quad, normalized: (0, 0) its top-left corner, (1, 1) its
    /// bottom-right. A quad of [`AtlasFrame::size`] with that pivot at an
    /// entity's position covers the pixels the untrimmed cell would. For an
    /// empty frame, `pivot` over the cell's size.
    pub fn pivot_in(&self, pivot: Vec2) -> Vec2 {
        if self.is_empty() {
            return pivot / self.source_size.as_vec2();
        }
        (pivot - self.offset.as_vec2()) / self.size.as_vec2()
    }
}

impl Asset for Atlas {
    type Decoded = Atlas;
    const EXTENSIONS: &'static [&'static str] = &["kbj"];

    fn decode(bytes: &[u8], ctx: &DecodeCtx<'_>) -> Result<Atlas, AssetError> {
        let file = atlas::parse(bytes, ctx.limits()).map_err(AssetError::Decode)?;
        let mut pages = Vec::with_capacity(file.pages.len());
        for (index, page) in file.pages.into_iter().enumerate() {
            let path = AssetPath::new(&page.path)
                .ok()
                .filter(|path| path.as_str() == page.path && path.extension() == Some("qoi"))
                .ok_or_else(|| {
                    AssetError::Invalid(format!(
                        "page {index}, `{}`, is not the path of a QOI texture",
                        page.path
                    ))
                })?;
            pages.push(AtlasPage {
                path: path.as_str().to_string(),
                size: UVec2::new(page.width, page.height),
            });
        }
        let sprites = file
            .sprites
            .into_iter()
            .map(|sprite| AtlasSprite {
                name: sprite.name,
                frames: sprite
                    .frames
                    .iter()
                    .map(|frame| AtlasFrame::new(frame, &pages))
                    .collect(),
                pivot: match sprite.kind {
                    SpriteKind::Pivot([x, y]) => Some(Vec2::new(x, y)),
                    _ => None,
                },
                tile_size: match sprite.kind {
                    SpriteKind::Tiles([w, h]) => Some(UVec2::new(w, h)),
                    _ => None,
                },
                clips: sprite
                    .clips
                    .into_iter()
                    .map(|clip| AtlasClip {
                        name: clip.name,
                        looping: clip.looping,
                        frames: clip
                            .frames
                            .iter()
                            .map(|step| AtlasClipFrame {
                                frame: step.frame as usize,
                                duration: Duration::from_millis(u64::from(step.millis)),
                            })
                            .collect(),
                    })
                    .collect(),
            })
            .collect();
        Ok(Atlas { pages, sprites })
    }

    fn finalize(decoded: Atlas, _: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        Ok(decoded)
    }
}

impl AtlasFrame {
    /// A frame of the file, on `pages`, which the parser has checked it
    /// lies on.
    fn new(frame: &atlas::Frame, pages: &[AtlasPage]) -> Self {
        let page = frame.page as usize;
        let page_size = pages
            .get(page)
            .map_or(Vec2::ONE, |page| page.size.as_vec2());
        let corner = Vec2::new(frame.x as f32, frame.y as f32);
        let size = UVec2::new(frame.width, frame.height);
        Self {
            page,
            uv: Rect::new(corner / page_size, (corner + size.as_vec2()) / page_size),
            size,
            offset: UVec2::new(frame.offset_x, frame.offset_y),
            source_size: UVec2::new(frame.source_width, frame.source_height),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nadir_codec::{Limits, json, kbj};

    /// The atlas `text` describes, in JSON.
    fn decoded(text: &str) -> Result<Atlas, AssetError> {
        let value = json::parse(text, Limits::default()).expect("the test's JSON");
        let bytes = kbj::encode(&value, Limits::default()).expect("encodable");
        Atlas::decode(
            &bytes,
            &DecodeCtx::new("atlas/sprites.kbj", Limits::default()),
        )
    }

    const ATLAS: &str = r#"{ "version": 2,
        "pages": [ { "path": "atlas/sprites_0.qoi", "w": 128, "h": 128 },
                   { "path": "atlas/sprites/tiles.tiles.qoi", "w": 128, "h": 64 } ],
        "sprites": [
          { "name": "sprites/person",
            "frames": [ { "page": 0, "x": 33, "y": 85, "w": 10, "h": 20, "ox": 1, "oy": 0, "sw": 12, "sh": 20 },
                        { "page": 0, "x": 0, "y": 0, "w": 0, "h": 0, "ox": 0, "oy": 0, "sw": 12, "sh": 20 } ],
            "pivot": [6.0, 20.0],
            "clips": [ { "name": "blink", "loop": false, "frames": [[0, 250], [1, 40], [0, 1000]] },
                       { "name": "stand", "loop": true, "frames": [[0, 100]] } ] },
          { "name": "sprites/tiles",
            "frames": [ { "page": 1, "x": 0, "y": 0, "w": 128, "h": 64, "ox": 0, "oy": 0, "sw": 128, "sh": 64 } ],
            "tiles": [16, 16] },
          { "name": "sprites/tree",
            "frames": [ { "page": 0, "x": 1, "y": 1, "w": 24, "h": 44, "ox": 0, "oy": 4, "sw": 24, "sh": 48 } ],
            "pivot": [12.0, 48.0] } ] }"#;

    #[test]
    fn sprites_are_found_by_name_with_their_regions() {
        let atlas = decoded(ATLAS).expect("an atlas");
        assert_eq!(atlas.pages().len(), 2);
        assert_eq!(atlas.pages()[1].path(), "atlas/sprites/tiles.tiles.qoi");
        assert_eq!(atlas.pages()[1].size(), UVec2::new(128, 64));
        assert!(atlas.sprite("sprites/rock").is_none());
        assert!(atlas.sprite("sprites").is_none());

        let person = atlas.sprite("sprites/person").expect("the person");
        assert_eq!(person.name(), "sprites/person");
        assert_eq!(person.pivot(), Some(Vec2::new(6.0, 20.0)));
        assert_eq!(person.tile_size(), None);
        let frame = person.frame(0).expect("a frame");
        assert_eq!(frame.page(), 0);
        assert_eq!(
            frame.uv(),
            Rect::new(
                Vec2::new(33.0 / 128.0, 85.0 / 128.0),
                Vec2::new(43.0 / 128.0, 105.0 / 128.0)
            )
        );
        assert_eq!(
            (frame.size(), frame.offset(), frame.source_size()),
            (UVec2::new(10, 20), UVec2::new(1, 0), UVec2::new(12, 20))
        );
        assert_eq!(
            atlas.page(frame).map(AtlasPage::path),
            Some("atlas/sprites_0.qoi")
        );
        assert!(!frame.is_empty());
        assert!(person.frame(1).expect("a frame").is_empty());
        assert!(person.frame(2).is_none());
    }

    #[test]
    fn a_trimmed_frame_s_pivot_keeps_its_pixels_where_the_cell_had_them() {
        let atlas = decoded(ATLAS).expect("an atlas");
        let tree = atlas.sprite("sprites/tree").expect("the tree");
        let frame = tree.frames()[0];
        // Trimmed 4 pixels from the top: the feet stay at the bottom.
        let pivot = frame.pivot_in(tree.pivot().expect("a pivot"));
        assert_eq!(pivot, Vec2::new(0.5, 1.0));
        // Placed so, the quad's top-left corner is the cell's moved by the
        // offset.
        let feet = Vec2::new(100.0, 200.0);
        let corner = feet - pivot * frame.size().as_vec2();
        let cell = feet - Vec2::new(12.0, 48.0);
        assert_eq!(corner, cell + frame.offset().as_vec2());

        let person = atlas.sprite("sprites/person").expect("the person");
        let pivot = person.pivot().expect("a pivot");
        assert_eq!(person.frames()[0].pivot_in(pivot), Vec2::new(0.5, 1.0));
        // An empty frame: over its cell.
        assert_eq!(person.frames()[1].pivot_in(pivot), Vec2::new(0.5, 1.0));
    }

    #[test]
    fn a_tile_set_gives_its_grid() {
        let atlas = decoded(ATLAS).expect("an atlas");
        let tiles = atlas.sprite("sprites/tiles").expect("the tiles");
        assert_eq!(tiles.tile_size(), Some(UVec2::new(16, 16)));
        assert_eq!(tiles.pivot(), None);
        let texture = Handle::new(nadir_core::GenIndex::new(0, std::num::NonZeroU32::MIN));
        let set = tiles.tile_set(texture).expect("a tile set");
        assert_eq!((set.texture, set.columns, set.rows), (texture, 8, 4));
        let tree = atlas.sprite("sprites/tree").expect("the tree");
        assert_eq!(tree.tile_set(texture), None);
    }

    #[test]
    fn a_page_whose_path_is_not_a_qoi_asset_path_is_refused() {
        for path in [
            "atlas/../page.qoi",
            "atlas/page.png",
            "Atlas/page.qoi",
            "/atlas/page.qoi",
        ] {
            let text = ATLAS.replacen("atlas/sprites_0.qoi", path, 1);
            assert_eq!(
                decoded(&text),
                Err(AssetError::Invalid(format!(
                    "page 0, `{path}`, is not the path of a QOI texture"
                ))),
                "{path}"
            );
        }
        // And the file itself is checked by the codec: version 1, which had
        // no clips, and later ones are refused.
        for version in [1, 9] {
            let text = ATLAS.replacen(r#""version": 2"#, &format!(r#""version": {version}"#), 1);
            assert!(text.contains(&format!(r#""version": {version}"#)));
            assert!(
                matches!(
                    decoded(&text),
                    Err(AssetError::Decode(nadir_codec::CodecError::Atlas { ref at, .. })) if at == "version"
                ),
                "version {version}"
            );
        }
    }

    #[test]
    fn a_sprite_s_clips_are_found_by_name_with_their_frames_and_times() {
        let atlas = decoded(ATLAS).expect("an atlas");
        let person = atlas.sprite("sprites/person").expect("the person");
        let names: Vec<&str> = person.clips().iter().map(AtlasClip::name).collect();
        assert_eq!(names, ["blink", "stand"]);
        let blink = person.clip("blink").expect("a clip");
        assert!(!blink.is_looping());
        let steps: Vec<(usize, Duration)> = blink
            .frames()
            .iter()
            .map(|step| (step.frame(), step.duration()))
            .collect();
        assert_eq!(
            steps,
            [
                (0, Duration::from_millis(250)),
                (1, Duration::from_millis(40)),
                (0, Duration::from_secs(1)),
            ]
        );
        assert!(person.clip("stand").expect("a clip").is_looping());
        assert!(person.clip("walk").is_none());
        assert!(person.clip("").is_none());
        // Sprites with none.
        assert!(
            atlas
                .sprite("sprites/tree")
                .expect("the tree")
                .clips()
                .is_empty()
        );
        assert!(
            atlas
                .sprite("sprites/tiles")
                .expect("the tiles")
                .clips()
                .is_empty()
        );
    }

    #[test]
    fn its_pages_load_as_textures_in_the_scope_given() {
        let atlas = decoded(ATLAS).expect("an atlas");
        let mut server = AssetServer::new("no-such-folder", nadir_assets::OnFailure::Nothing);
        let pages = atlas.load_pages(&mut server, Scope::Level);
        assert_eq!(pages.len(), 2);
        assert_eq!(server.path(pages[0]), Some("atlas/sprites_0.qoi"));
        assert_eq!(server.path(pages[1]), Some("atlas/sprites/tiles.tiles.qoi"));
        // In that scope alone: unloading it frees them.
        assert_eq!(server.unload_scope(Scope::Level), 2);
        assert_eq!(server.state(pages[0]), None);
    }

    #[test]
    fn it_is_made_with_nothing_in_its_context() {
        let atlas = decoded(ATLAS).expect("an atlas");
        let made = Atlas::finalize(atlas.clone(), &mut FinalizeCtx::new());
        assert_eq!(made, Ok(atlas));
    }
}
