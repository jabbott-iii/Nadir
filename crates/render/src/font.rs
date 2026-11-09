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

//! The font asset: a cooked font file, `atlas/<font>.font.kbj`, whose
//! glyphs are frames of a sprite of an atlas (ADR-0035, ADR-0054).

use nadir_assets::{
    Asset, AssetError, AssetPath, AssetServer, DecodeCtx, FinalizeCtx, Handle, Scope,
};
use nadir_codec::font;
use nadir_core::IVec2;

use crate::{Atlas, AtlasSprite, Texture};

/// How far from 0 a glyph's offsets and advance, a kerning amount and the
/// line height may be, in pixels: ±32767 (`i16::MAX`), far past any font,
/// so that a line's sums cannot overflow.
const METRIC: i32 = i16::MAX as i32;

/// A bitmap font the cook made from a BMFont: its metrics, its glyphs by
/// character, its kerning pairs, and the glyph a character it lacks draws.
/// Each glyph is a frame of a sprite of an atlas, which [`Font::atlas`] and
/// [`Font::sprite`] name.
///
/// It needs no GPU, so a game can make it before its window opens, and it
/// lays text out on its own ([`Font::layout`], [`Font::measure`]). To be
/// drawn, its atlas and the atlas's pages must be loaded too:
/// [`Font::load_now_in`] loads all three. It has no placeholder.
#[derive(Clone, Debug, PartialEq)]
pub struct Font {
    face: String,
    size: i32,
    line_height: u32,
    base: u32,
    atlas: String,
    sprite: String,
    /// By character, ascending.
    glyphs: Vec<(char, Glyph)>,
    /// By (first, second), ascending.
    kerning: Vec<(char, char, i32)>,
    fallback: Glyph,
    /// The highest frame a glyph names, the fallback's included.
    last_frame: u32,
}

/// Why an atlas does not fit a font: what [`Font::sprite_in`] finds, with
/// nothing allocated, so that a text can be skipped each frame for free.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Misfit {
    /// The atlas has no sprite of the font's name.
    NoSprite,
    /// The sprite has this many frames, fewer than the glyphs name.
    TooFewFrames(usize),
}

/// A glyph of a [`Font`]: the frame of its font's sprite that holds its
/// pixels, where it goes from the pen, and how far it moves the pen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Glyph {
    frame: Option<u32>,
    offset: IVec2,
    advance: i32,
}

impl Font {
    /// Loads the font file at `path`, then the atlas file it names, at once,
    /// and starts loading the atlas's pages, all in `scope`: what a font
    /// needs to be drawn. It blocks until the two files are read, as
    /// [`AssetServer::load_now_in`] does; the pages, textures, are made on
    /// the GPU by a later finish.
    ///
    /// `server.load::<Font>(path)` loads the font file alone: its texts are
    /// then not drawn until its atlas and pages are loaded too.
    ///
    /// # Errors
    ///
    /// Any [`AssetError`] of the font or its atlas, and
    /// [`AssetError::Invalid`] when the atlas has no sprite of the font's
    /// name, or that sprite has fewer frames than the font's glyphs name.
    pub fn load_now_in(
        server: &mut AssetServer,
        path: &str,
        scope: Scope,
        ctx: &mut FinalizeCtx<'_>,
    ) -> Result<Handle<Font>, AssetError> {
        let handle = server.load_now_in::<Font>(path, scope, ctx)?;
        let font = server.get(handle).ok_or_else(|| {
            AssetError::Invalid(format!("the font `{path}` is not in the asset server"))
        })?;
        let atlas_path = font.atlas.clone();
        let atlas = server.load_now_in::<Atlas>(&atlas_path, scope, ctx)?;
        let (Some(font), Some(atlas)) = (server.get(handle), server.get(atlas)) else {
            return Err(AssetError::Invalid(format!(
                "the font `{path}` or its atlas `{atlas_path}` is not in the asset server"
            )));
        };
        font.sprite_in(atlas).map_err(|misfit| {
            AssetError::Invalid(format!("the font `{path}`: {}", font.misfit(misfit)))
        })?;
        let pages: Vec<String> = atlas
            .pages()
            .iter()
            .map(|page| page.path().to_string())
            .collect();
        for page in pages {
            server.load_in::<Texture>(&page, scope);
        }
        Ok(handle)
    }

    /// Its face's name, as BMFont gave it: it may be empty.
    pub fn face(&self) -> &str {
        &self.face
    }

    /// Its size, as BMFont gave it: a negative size is BMFont's "match the
    /// characters' height".
    pub fn size(&self) -> i32 {
        self.size
    }

    /// The pixels from one line's top to the next's.
    pub fn line_height(&self) -> u32 {
        self.line_height
    }

    /// The pixels from a line's top to its baseline.
    pub fn base(&self) -> u32 {
        self.base
    }

    /// The path of its atlas file, such as `atlas/fonts.kbj`, among the
    /// cooked files.
    pub fn atlas(&self) -> &str {
        &self.atlas
    }

    /// The name of the sprite of its atlas whose frames are its glyphs, such
    /// as `fonts/ui`.
    pub fn sprite(&self) -> &str {
        &self.sprite
    }

    /// The glyph of `character`, if the font has one.
    pub fn glyph(&self, character: char) -> Option<Glyph> {
        let index = self
            .glyphs
            .binary_search_by_key(&character, |&(id, _)| id)
            .ok()?;
        self.glyphs.get(index).map(|&(_, glyph)| glyph)
    }

    /// The glyph a character the font lacks draws: BMFont's fallback glyph,
    /// if the font has one, else its `?`. It has a frame.
    pub fn fallback(&self) -> Glyph {
        self.fallback
    }

    /// The pixels added to `first`'s advance when `second` follows it: 0 for
    /// a pair the font does not kern.
    pub fn kerning(&self, first: char, second: char) -> i32 {
        self.kerning
            .binary_search_by_key(&(first, second), |&(a, b, _)| (a, b))
            .map_or(0, |index| self.kerning[index].2)
    }

    /// The sprite of `atlas` whose frames are the glyphs, if `atlas` has it
    /// and it has every frame the glyphs name; otherwise why not.
    pub(crate) fn sprite_in<'a>(&self, atlas: &'a Atlas) -> Result<&'a AtlasSprite, Misfit> {
        let sprite = atlas.sprite(&self.sprite).ok_or(Misfit::NoSprite)?;
        let frames = sprite.frames().len();
        if self.last_frame as usize >= frames {
            return Err(Misfit::TooFewFrames(frames));
        }
        Ok(sprite)
    }

    /// What `misfit` means for this font, to be logged.
    pub(crate) fn misfit(&self, misfit: Misfit) -> String {
        match misfit {
            Misfit::NoSprite => format!("its atlas has no sprite `{}`", self.sprite),
            Misfit::TooFewFrames(frames) => format!(
                "its glyphs name frame {} of the sprite `{}`, which has {frames}",
                self.last_frame, self.sprite
            ),
        }
    }
}

impl Glyph {
    /// Its frame, an index into its font's sprite's
    /// [`AtlasSprite::frames`]: `None` for a glyph with no pixels, such as
    /// a space.
    pub fn frame(&self) -> Option<usize> {
        self.frame.map(|frame| frame as usize)
    }

    /// Where its rectangle's top-left corner is from the pen, which is on
    /// the line's top: BMFont's `xoffset` and `yoffset`. Its frame is that
    /// rectangle trimmed: [`AtlasFrame::offset`](crate::AtlasFrame::offset)
    /// places its pixels in it.
    pub fn offset(&self) -> IVec2 {
        self.offset
    }

    /// How far it moves the pen to the right: BMFont's `xadvance`.
    pub fn advance(&self) -> i32 {
        self.advance
    }
}

impl Asset for Font {
    type Decoded = Font;
    const EXTENSIONS: &'static [&'static str] = &["kbj"];

    fn decode(bytes: &[u8], ctx: &DecodeCtx<'_>) -> Result<Font, AssetError> {
        let file = font::parse(bytes, ctx.limits()).map_err(AssetError::Decode)?;
        AssetPath::new(&file.atlas)
            .ok()
            .filter(|path| path.as_str() == file.atlas && path.extension() == Some("kbj"))
            .ok_or_else(|| {
                AssetError::Invalid(format!(
                    "its atlas, `{}`, is not the path of an atlas file",
                    file.atlas
                ))
            })?;
        let within = |value: i32| (-METRIC..=METRIC).contains(&value);
        let line_height = i32::try_from(file.line_height)
            .ok()
            .filter(|&height| within(height))
            .ok_or_else(|| {
                AssetError::Invalid(format!(
                    "its line height, {}, is past {METRIC} pixels",
                    file.line_height
                ))
            })?;
        let metrics = |what: String, x: i32, y: i32, advance: i32| {
            if within(x) && within(y) && within(advance) {
                Ok(())
            } else {
                Err(AssetError::Invalid(format!(
                    "{what}'s offset ({x}, {y}) or advance {advance} is past {METRIC} pixels"
                )))
            }
        };
        let mut last_frame = file.fallback.frame;
        let mut glyphs = Vec::with_capacity(file.glyphs.len());
        for glyph in &file.glyphs {
            metrics(
                format!("the glyph {:?} (U+{:04X})", glyph.id, glyph.id as u32),
                glyph.x,
                glyph.y,
                glyph.advance,
            )?;
            last_frame = last_frame.max(glyph.frame.unwrap_or(0));
            glyphs.push((
                glyph.id,
                Glyph {
                    frame: glyph.frame,
                    offset: IVec2::new(glyph.x, glyph.y),
                    advance: glyph.advance,
                },
            ));
        }
        let fallback = &file.fallback;
        metrics(
            "the fallback".to_string(),
            fallback.x,
            fallback.y,
            fallback.advance,
        )?;
        let mut kerning = Vec::with_capacity(file.kerning.len());
        for pair in &file.kerning {
            if !within(pair.amount) {
                return Err(AssetError::Invalid(format!(
                    "the kerning of {:?} and {:?}, {}, is past {METRIC} pixels",
                    pair.first, pair.second, pair.amount
                )));
            }
            kerning.push((pair.first, pair.second, pair.amount));
        }
        Ok(Font {
            face: file.face,
            size: file.size,
            line_height: line_height as u32,
            base: file.base,
            atlas: file.atlas,
            sprite: file.sprite,
            glyphs,
            kerning,
            fallback: Glyph {
                frame: Some(fallback.frame),
                offset: IVec2::new(fallback.x, fallback.y),
                advance: fallback.advance,
            },
            last_frame,
        })
    }

    fn finalize(decoded: Font, _: &mut FinalizeCtx<'_>) -> Result<Self, AssetError> {
        Ok(decoded)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use nadir_codec::{Limits, json, kbj};

    /// The bytes of the KBJ file `text` describes, in JSON.
    pub(crate) fn kbj(text: &str) -> Vec<u8> {
        let value = json::parse(text, Limits::default()).expect("the test's JSON");
        kbj::encode(&value, Limits::default()).expect("encodable")
    }

    /// The font `text` describes, in JSON.
    pub(crate) fn decoded(text: &str) -> Result<Font, AssetError> {
        Font::decode(
            &kbj(text),
            &DecodeCtx::new("atlas/fonts/tiny.font.kbj", Limits::default()),
        )
    }

    /// A font of glyphs 3 wide on lines 6 high, with a space, kerning and a
    /// fallback of its own. It kerns B, which it lacks, as BMFont may.
    pub(crate) const FONT: &str = r#"{ "version": 1,
        "face": "Tiny", "size": -6, "line_height": 6, "base": 5,
        "atlas": "atlas/fonts.kbj", "sprite": "fonts/tiny",
        "glyphs": [
          { "id": 32, "x": 0, "y": 0, "advance": 2 },
          { "id": 65, "frame": 0, "x": 0, "y": 1, "advance": 4 },
          { "id": 86, "frame": 1, "x": 0, "y": 1, "advance": 4 },
          { "id": 105, "frame": 2, "x": 1, "y": 1, "advance": 3 } ],
        "kerning": [ [65, 66, -1], [65, 86, -1], [66, 86, -1], [86, 65, -1] ],
        "fallback": { "frame": 3, "x": 0, "y": 0, "advance": 4 } }"#;

    #[test]
    fn its_glyphs_and_kerning_are_found_by_character() {
        let font = decoded(FONT).expect("a font");
        assert_eq!(
            (font.face(), font.size(), font.line_height(), font.base()),
            ("Tiny", -6, 6, 5)
        );
        assert_eq!(
            (font.atlas(), font.sprite()),
            ("atlas/fonts.kbj", "fonts/tiny")
        );
        let a = font.glyph('A').expect("A");
        assert_eq!(
            (a.frame(), a.offset(), a.advance()),
            (Some(0), IVec2::new(0, 1), 4)
        );
        let space = font.glyph(' ').expect("a space");
        assert_eq!((space.frame(), space.advance()), (None, 2));
        assert_eq!(font.glyph('i').expect("i").offset(), IVec2::new(1, 1));
        assert!(font.glyph('B').is_none());
        assert!(font.glyph('\0').is_none());
        assert_eq!(font.fallback().frame(), Some(3));
        assert_eq!(font.kerning('A', 'V'), -1);
        assert_eq!(font.kerning('A', 'B'), -1, "a character it lacks");
        assert_eq!(font.kerning('V', 'A'), -1);
        assert_eq!(font.kerning('A', 'A'), 0);
        assert_eq!(font.kerning('V', 'i'), 0);
        assert_eq!(font.last_frame, 3);
    }

    #[test]
    fn an_atlas_that_is_not_an_atlas_file_s_path_is_refused() {
        for path in [
            "atlas/../fonts.kbj",
            "atlas/fonts.qoi",
            "Atlas/fonts.kbj",
            "/atlas/fonts.kbj",
        ] {
            let text = FONT.replacen("atlas/fonts.kbj", path, 1);
            assert_eq!(
                decoded(&text),
                Err(AssetError::Invalid(format!(
                    "its atlas, `{path}`, is not the path of an atlas file"
                ))),
                "{path}"
            );
        }
        // And the file itself is checked by the codec.
        let text = FONT.replacen(r#""version": 1"#, r#""version": 2"#, 1);
        assert!(matches!(
            decoded(&text),
            Err(AssetError::Decode(nadir_codec::CodecError::Font { ref at, .. })) if at == "version"
        ));
    }

    #[test]
    fn metrics_past_32767_either_way_are_refused_and_up_to_it_taken() {
        let cases = [
            (r#""advance": 4 },"#, r#""advance": 32768 },"#),
            (r#""x": 1, "y": 1"#, r#""x": -32768, "y": 1"#),
            (
                r#""x": 0, "y": 0, "advance": 4 }"#,
                r#""x": 0, "y": 40000, "advance": 4 }"#,
            ),
            ("[65, 86, -1]", "[65, 86, -32768]"),
            (r#""line_height": 6"#, r#""line_height": 32768"#),
        ];
        for (from, to) in cases {
            let text = FONT.replacen(from, to, 1);
            assert_ne!(text, FONT, "{from}");
            assert!(
                matches!(decoded(&text), Err(AssetError::Invalid(_))),
                "{to}: {:?}",
                decoded(&text)
            );
            let edge = text.replace("32768", "32767").replace("40000", "-32767");
            assert!(decoded(&edge).is_ok(), "{edge}");
        }
    }

    /// An atlas file whose sprite `fonts/tiny` has `frames` frames.
    pub(crate) fn atlas(frames: usize) -> Atlas {
        let frame =
            r#"{ "page": 0, "x": 0, "y": 0, "w": 3, "h": 4, "ox": 0, "oy": 0, "sw": 3, "sh": 4 }"#;
        let frames = vec![frame; frames].join(", ");
        let text = format!(
            r#"{{ "version": 2,
                "pages": [ {{ "path": "atlas/fonts_0.qoi", "w": 16, "h": 8 }} ],
                "sprites": [ {{ "name": "fonts/tiny", "frames": [ {frames} ], "pivot": [0.0, 0.0] }} ] }}"#
        );
        Atlas::decode(
            &kbj(&text),
            &DecodeCtx::new("atlas/fonts.kbj", Limits::default()),
        )
        .expect("an atlas")
    }

    #[test]
    fn the_atlas_must_have_its_sprite_with_every_frame_its_glyphs_name() {
        let font = decoded(FONT).expect("a font");
        assert!(font.sprite_in(&atlas(4)).is_ok());
        assert_eq!(font.sprite_in(&atlas(3)), Err(Misfit::TooFewFrames(3)));
        assert_eq!(
            font.misfit(Misfit::TooFewFrames(3)),
            "its glyphs name frame 3 of the sprite `fonts/tiny`, which has 3"
        );
        let other = decoded(&FONT.replacen("fonts/tiny", "fonts/big", 1)).expect("a font");
        assert_eq!(other.sprite_in(&atlas(4)), Err(Misfit::NoSprite));
        assert_eq!(
            other.misfit(Misfit::NoSprite),
            "its atlas has no sprite `fonts/big`"
        );
    }

    #[test]
    fn it_is_made_with_nothing_in_its_context() {
        let font = decoded(FONT).expect("a font");
        assert_eq!(
            Font::finalize(font.clone(), &mut FinalizeCtx::new()),
            Ok(font)
        );
    }
}
