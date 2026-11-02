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

//! One atlas group: its images, Aseprite exports and fonts cut into frames
//! and trimmed, packed into pages with padding and extrusion, and written
//! as QOI pages, a KBJ of where each frame went and the clips of the
//! exports' tags, and a file for each font (`docs/subsystems/assets-and-codecs.md`,
//! "Atlas groups").

use nadir_codec::aseprite::Export;
use nadir_codec::bmfont::{Font, Glyph};
use nadir_codec::json::Value;
use nadir_codec::qoi::{self, Channels, ColorSpace, Header};
use nadir_codec::{Limits, png};

use crate::CookError;
use crate::config::{Layout, Sprite};
use crate::image::Pixels;
use crate::pack::{self, Rect};

/// The atlas file's version: the one the game's reader reads.
pub(crate) const VERSION: i64 = nadir_codec::atlas::VERSION;
/// The version of a font's file.
pub(crate) const FONT_VERSION: i64 = 1;
/// The space between two frames in a page, in pixels.
pub(crate) const PADDING: u32 = 2;
/// How far each frame's edge pixels are repeated outward.
pub(crate) const EXTRUDE: u32 = 1;
/// The side of the page that a group of only empty frames gets.
const EMPTY_PAGE: u32 = pack::SIDES[0];
/// The widest and tallest tile set: the largest page.
const MAX_TILE_SET: u32 = pack::SIDES[pack::SIDES.len() - 1];
/// The character a font draws for one it lacks, when BMFont wrote no
/// fallback glyph: `?`.
const QUESTION_MARK: u32 = 0x3f;

/// The color chunks that make the cook warn: they say the colors may not
/// be plain sRGB, as the cook takes them. `sRGB` itself, and text, do not.
const COLOR_CHUNKS: [&str; 6] = ["gAMA", "cHRM", "iCCP", "cICP", "mDCV", "cLLI"];

/// The limits a source image is decoded with: any width or height PNG
/// allows, since a sheet is cut up, and the default bytes.
pub(crate) fn source_limits() -> Limits {
    Limits {
        max_texture_dimension: 0x7fff_ffff,
        ..Limits::default()
    }
}

/// A sprite of the group, from the files it was read from.
pub(crate) struct Member<'a> {
    /// Its name: its file's path, lowercased, without `.png`, `.json` or
    /// `.fnt`, such as `sprites/person`.
    pub(crate) name: &'a str,
    /// What it is cooked from.
    pub(crate) kind: Kind<'a>,
    /// What its sidecar says.
    pub(crate) sprite: Sprite,
}

/// What a sprite is cooked from: each file by its path, lowercased, and its
/// bytes.
pub(crate) enum Kind<'a> {
    /// A PNG, one frame or a sheet as its sidecar says.
    Image { png: (&'a str, &'a [u8]) },
    /// An Aseprite export, and its sheet.
    Export {
        path: &'a str,
        export: &'a Export,
        sheet: (&'a str, &'a [u8]),
    },
    /// A BMFont font, and its pages by id.
    Font {
        path: &'a str,
        font: &'a Font,
        pages: Vec<(&'a str, &'a [u8])>,
    },
}

impl Kind<'_> {
    /// The file to name in an error about the sprite.
    fn path(&self) -> &str {
        match self {
            Self::Image { png } => png.0,
            Self::Export { path, .. } | Self::Font { path, .. } => path,
        }
    }
}

/// What a group cooks to.
pub(crate) struct Cooked {
    /// The files, by path below the output folder, in order.
    pub(crate) files: Vec<(String, Vec<u8>)>,
    /// What to tell the user.
    pub(crate) warnings: Vec<String>,
    pub(crate) pages: usize,
    pub(crate) sprites: usize,
    pub(crate) clips: usize,
    pub(crate) fonts: usize,
}

/// A frame of a sprite: where its pixels are in one of its images, where
/// that rectangle sits in its untrimmed cell, and the part of it packed.
struct Frame {
    /// The image, an index into [`Cut::images`].
    image: usize,
    /// Its pixels in the image, before the cook's trim.
    region: Rect,
    /// Where `region`'s top-left corner is in the cell.
    origin: (u32, u32),
    /// The untrimmed cell's width and height.
    cell: (u32, u32),
    /// The part of `region` that is packed: `None` for a frame with no
    /// pixel to show.
    content: Option<Rect>,
    /// Where `content` went: its page, and its corner there.
    place: (usize, u32, u32),
}

impl Frame {
    /// A frame that is the whole of `cell` in image `image`, trimmed or not.
    fn whole(image: usize, cell: Rect, pixels: &Pixels, trim: bool) -> Self {
        Self {
            image,
            region: cell,
            origin: (0, 0),
            cell: (cell.w, cell.h),
            content: if trim { pixels.trim(cell) } else { Some(cell) },
            place: (0, 0, 0),
        }
    }
}

/// A sprite decoded and cut.
struct Cut<'a> {
    member: &'a Member<'a>,
    images: Vec<Pixels>,
    frames: Vec<Frame>,
    /// A tile set's tile width and height, and its page.
    tiles: Option<(u32, u32, usize)>,
    /// Its pivot, in its untrimmed cell's pixels, if not the default.
    pivot: Option<(f64, f64)>,
    /// An export's clips, by name.
    clips: Vec<Clip>,
    /// A font's file, once the frames of its glyphs are known.
    font: Option<Value>,
}

/// A clip of an export's tag.
struct Clip {
    name: String,
    looping: bool,
    /// Its frames, each with its milliseconds.
    steps: Vec<(u32, u32)>,
}

/// The cells of a `width` × `height` image, row by row.
fn cells(path: &str, width: u32, height: u32, layout: Layout) -> Result<Vec<Rect>, CookError> {
    let (w, h) = match layout {
        Layout::Whole | Layout::Tiles(..) => return Ok(vec![Rect::new(0, 0, width, height)]),
        Layout::Frames(w, h) => (w, h),
    };
    if !width.is_multiple_of(w) || !height.is_multiple_of(h) {
        return Err(CookError::Layout {
            path: path.to_string(),
            reason: format!(
                "a {width} × {height} image is not a whole number of frames of {w} × {h}"
            ),
        });
    }
    let (across, down) = (width / w, height / h);
    let count = u64::from(across) * u64::from(down);
    let most = Limits::default().max_entries;
    if count > most as u64 {
        return Err(CookError::Layout {
            path: path.to_string(),
            reason: format!("its {count} frames are more than {most}"),
        });
    }
    Ok((0..down)
        .flat_map(|row| (0..across).map(move |column| Rect::new(column * w, row * h, w, h)))
        .collect())
}

/// Decodes the PNG `path`, and warns if its colors may not be sRGB.
fn decode(path: &str, bytes: &[u8], warnings: &mut Vec<String>) -> Result<Pixels, CookError> {
    let image = png::decode(bytes, source_limits()).map_err(|error| CookError::Decode {
        path: path.to_string(),
        error,
    })?;
    let color: Vec<&str> = image
        .ignored
        .iter()
        .copied()
        .filter(|chunk| COLOR_CHUNKS.contains(chunk))
        .collect();
    if !color.is_empty() {
        warnings.push(format!(
            "`{path}` has {} chunks, which the cook ignores: it takes every PNG as sRGB",
            color.join(", ")
        ));
    }
    Ok(Pixels {
        width: image.width,
        height: image.height,
        rgba: image.pixels,
    })
}

/// Decodes and cuts one sprite.
fn cut<'a>(
    group: &str,
    member: &'a Member<'a>,
    warnings: &mut Vec<String>,
) -> Result<Cut<'a>, CookError> {
    let mut cut = Cut {
        member,
        images: Vec::new(),
        frames: Vec::new(),
        tiles: None,
        pivot: member.sprite.pivot,
        clips: Vec::new(),
        font: None,
    };
    match &member.kind {
        Kind::Image { png: (path, bytes) } => {
            let pixels = decode(path, bytes, warnings)?;
            let (width, height) = (pixels.width, pixels.height);
            if let Layout::Tiles(w, h) = member.sprite.layout {
                let fault = if !width.is_multiple_of(w) || !height.is_multiple_of(h) {
                    Some(format!(
                        "a {width} × {height} image is not a whole number of tiles of {w} × {h}"
                    ))
                } else if width > MAX_TILE_SET || height > MAX_TILE_SET {
                    Some(format!(
                        "a tile set must fit a page of {MAX_TILE_SET} × {MAX_TILE_SET}, and this is {width} × {height}"
                    ))
                } else {
                    None
                };
                if let Some(reason) = fault {
                    return Err(CookError::Layout {
                        path: path.to_string(),
                        reason,
                    });
                }
                cut.tiles = Some((w, h, 0));
            }
            cut.frames = cells(path, width, height, member.sprite.layout)?
                .into_iter()
                .map(|cell| Frame::whole(0, cell, &pixels, member.sprite.trim))
                .collect();
            cut.images.push(pixels);
        }
        Kind::Export {
            path,
            export,
            sheet: (sheet_path, bytes),
        } => {
            let sheet = decode(sheet_path, bytes, warnings)?;
            if (sheet.width, sheet.height) != (export.width, export.height) {
                return Err(CookError::Layout {
                    path: path.to_string(),
                    reason: format!(
                        "its sheet `{sheet_path}` is {} × {}, but the export says {} × {}: export them again together",
                        sheet.width, sheet.height, export.width, export.height
                    ),
                });
            }
            for frame in &export.frames {
                let region = Rect::new(frame.x, frame.y, frame.width, frame.height);
                cut.frames.push(Frame {
                    image: 0,
                    region,
                    origin: (frame.offset_x, frame.offset_y),
                    cell: (frame.source_width, frame.source_height),
                    content: if member.sprite.trim {
                        sheet.trim(region)
                    } else {
                        Some(region)
                    },
                    place: (0, 0, 0),
                });
            }
            let mut clips: Vec<Clip> = export
                .tags
                .iter()
                .map(|tag| {
                    let steps = tag
                        .frames
                        .iter()
                        .map(|&frame| {
                            let shown = export.frames.get(frame as usize);
                            (frame, shown.map_or(0, |shown| shown.duration))
                        })
                        .collect();
                    Clip {
                        name: tag.name.clone(),
                        looping: tag.looping,
                        steps,
                    }
                })
                .collect();
            clips.sort_by(|a, b| a.name.as_bytes().cmp(b.name.as_bytes()));
            cut.clips = clips;
            cut.images.push(sheet);
        }
        Kind::Font { path, font, pages } => {
            for &(page_path, bytes) in pages {
                let page = decode(page_path, bytes, warnings)?;
                font_page(path, font, page_path, &page)?;
                cut.images.push(page);
            }
            let (frames, value) = font_frames(group, member.name, path, font, &cut.images)?;
            cut.frames = frames;
            cut.font = Some(value);
            // The font file places the glyphs; the pivot is only the
            // reader's.
            cut.pivot = Some((0.0, 0.0));
        }
    }
    Ok(cut)
}

/// Checks a page of the font `path`: of the font's page size, and with the
/// glyphs in its alpha.
fn font_page(path: &str, font: &Font, page_path: &str, page: &Pixels) -> Result<(), CookError> {
    let fault = |reason: String| CookError::Layout {
        path: path.to_string(),
        reason,
    };
    if (page.width, page.height) != (font.scale_w, font.scale_h) {
        return Err(fault(format!(
            "its page `{page_path}` is {} × {}, not the font's {} × {}",
            page.width, page.height, font.scale_w, font.scale_h
        )));
    }
    if !matches!(font.channels[0], None | Some(0 | 2)) {
        return Err(fault(format!(
            "the alpha of its pages must hold the glyphs (alphaChnl 0 or 2), and it holds {}",
            font.channels[0].unwrap_or_default()
        )));
    }
    if page
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel[3] == 255)
    {
        return Err(fault(format!(
            "its page `{page_path}` has no transparent pixel: export 32-bit pages with the glyphs in alpha"
        )));
    }
    Ok(())
}

/// The frames of a font's glyphs, by id, then of its fallback, and its
/// file: each glyph with no pixel above alpha 0 has no frame.
fn font_frames(
    group: &str,
    name: &str,
    path: &str,
    font: &Font,
    pages: &[Pixels],
) -> Result<(Vec<Frame>, Value), CookError> {
    let mut frames = Vec::new();
    // A glyph's frame, if it has one, and its members but its id.
    let mut glyph = |glyph: &Glyph| -> (Option<usize>, Vec<(&'static str, Value)>) {
        let rect = Rect::new(glyph.x, glyph.y, glyph.width, glyph.height);
        let shown = pages
            .get(glyph.page as usize)
            .and_then(|page| page.trim(rect).map(|content| (page, content)));
        let frame = shown.map(|(_, content)| {
            frames.push(Frame {
                image: glyph.page as usize,
                region: rect,
                origin: (0, 0),
                cell: (rect.w, rect.h),
                content: Some(content),
                place: (0, 0, 0),
            });
            frames.len() - 1
        });
        let mut members = Vec::new();
        if let Some(frame) = frame {
            members.push(("frame", Value::Int(frame as i64)));
        }
        members.extend([
            ("x", Value::Int(i64::from(glyph.x_offset))),
            ("y", Value::Int(i64::from(glyph.y_offset))),
            ("advance", Value::Int(i64::from(glyph.x_advance))),
        ]);
        (frame, members)
    };
    let mut glyphs: Vec<&Glyph> = font.glyphs.iter().collect();
    glyphs.sort_by_key(|glyph| glyph.id);
    let mut entries = Vec::with_capacity(glyphs.len());
    let mut question = None;
    for one in &glyphs {
        let (frame, members) = glyph(one);
        if one.id == QUESTION_MARK {
            question = Some((frame, members.clone()));
        }
        let mut entry = vec![("id", Value::Int(i64::from(one.id)))];
        entry.extend(members);
        entries.push(object(entry));
    }
    let fallback = match &font.fallback {
        Some(fallback) => Some(glyph(fallback)),
        None => question,
    };
    let fallback = match fallback {
        Some((Some(_), members)) => object(members),
        Some((None, _)) => {
            return Err(CookError::Layout {
                path: path.to_string(),
                reason: "the glyph it draws for a character it lacks has no pixels".to_string(),
            });
        }
        None => {
            return Err(CookError::Layout {
                path: path.to_string(),
                reason: "it has no `?` and no fallback glyph to draw for a character it lacks"
                    .to_string(),
            });
        }
    };
    let mut kernings: Vec<_> = font.kernings.iter().collect();
    kernings.sort_by_key(|pair| (pair.first, pair.second));
    let kerning = kernings
        .iter()
        .map(|pair| {
            Value::Array(vec![
                Value::Int(i64::from(pair.first)),
                Value::Int(i64::from(pair.second)),
                Value::Int(i64::from(pair.amount)),
            ])
        })
        .collect();
    let file = object(vec![
        ("version", Value::Int(FONT_VERSION)),
        ("face", Value::String(font.face.clone())),
        ("size", Value::Int(i64::from(font.size))),
        ("line_height", int(font.line_height)),
        ("base", int(font.base)),
        ("atlas", Value::String(format!("atlas/{group}.kbj"))),
        ("sprite", Value::String(name.to_string())),
        ("glyphs", Value::Array(entries)),
        ("kerning", Value::Array(kerning)),
        ("fallback", fallback),
    ]);
    Ok((frames, file))
}

fn int(value: u32) -> Value {
    Value::Int(i64::from(value))
}

fn object(members: Vec<(&str, Value)>) -> Value {
    Value::Object(
        members
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

/// The path of the font file of the sprite `name`.
pub(crate) fn font_file(name: &str) -> String {
    format!("atlas/{name}.font.kbj")
}

/// Cooks the group `group` of `members`, in any order: what it writes
/// follows its sprites' names.
///
/// # Errors
///
/// [`CookError::Decode`] for a PNG the decoder refuses,
/// [`CookError::Layout`] for an image that does not fit its sidecar, a
/// sheet that does not fit its export, or a font whose pages or glyphs are
/// wrong, [`CookError::TooBig`] for a frame bigger than the largest page,
/// and [`CookError::Encode`] if an output cannot be written as QOI or KBJ,
/// or the game's atlas reader refuses the atlas file written.
pub(crate) fn cook(group: &str, members: &[Member<'_>]) -> Result<Cooked, CookError> {
    let mut warnings = Vec::new();
    // By name, whatever order the files were found in, so that the same
    // files always cook to the same bytes.
    let mut members: Vec<&Member<'_>> = members.iter().collect();
    members.sort_by(|a, b| a.name.cmp(b.name));
    let mut cuts = members
        .into_iter()
        .map(|member| cut(group, member, &mut warnings))
        .collect::<Result<Vec<_>, _>>()?;

    // The frames to pack, tallest first.
    let mut items: Vec<(usize, usize)> = Vec::new();
    for (sprite, cut) in cuts.iter().enumerate() {
        if cut.tiles.is_none() {
            for (frame, data) in cut.frames.iter().enumerate() {
                if data.content.is_some() {
                    items.push((sprite, frame));
                }
            }
        }
    }
    let content = |cuts: &[Cut<'_>], (sprite, frame): (usize, usize)| {
        cuts[sprite].frames[frame]
            .content
            .unwrap_or(Rect::new(0, 0, 0, 0))
    };
    items.sort_by(|&a, &b| {
        let (ra, rb) = (content(&cuts, a), content(&cuts, b));
        rb.h.cmp(&ra.h)
            .then(rb.w.cmp(&ra.w))
            .then(cuts[a.0].member.name.cmp(cuts[b.0].member.name))
            .then(a.1.cmp(&b.1))
    });
    let border = 2 * EXTRUDE + PADDING;
    let sizes: Vec<(u32, u32)> = items
        .iter()
        .map(|&item| {
            let rect = content(&cuts, item);
            (rect.w + border, rect.h + border)
        })
        .collect();
    let packing = pack::pack(&sizes, PADDING).map_err(|index| {
        let (sprite, frame) = items[index];
        let rect = content(&cuts, items[index]);
        CookError::TooBig {
            path: cuts[sprite].member.kind.path().to_string(),
            frame,
            size: (rect.w, rect.h),
        }
    })?;
    let mut sides = packing.sides.clone();
    let packed_frames = cuts.iter().any(|cut| cut.tiles.is_none());
    if sides.is_empty() && packed_frames {
        sides.push(EMPTY_PAGE);
    }
    for (&(sprite, frame), &place) in items.iter().zip(&packing.places) {
        cuts[sprite].frames[frame].place = place;
    }

    // The pages: packed ones first, then each tile set's, by name.
    let mut pages: Vec<(String, Pixels)> = sides
        .iter()
        .enumerate()
        .map(|(index, &side)| {
            (
                format!("atlas/{group}_{index}.qoi"),
                Pixels::new(side, side),
            )
        })
        .collect();
    for cut in cuts.iter_mut() {
        if let Some((w, h, _)) = cut.tiles {
            let path = format!("atlas/{}.tiles.qoi", cut.member.name);
            cut.tiles = Some((w, h, pages.len()));
            pages.push((path, cut.images[0].clone()));
        }
    }
    for cut in &cuts {
        if cut.tiles.is_some() {
            continue;
        }
        for frame in &cut.frames {
            if let Some(rect) = frame.content {
                let (page, x, y) = frame.place;
                pages[page]
                    .1
                    .blit(&cut.images[frame.image], rect, x, y, EXTRUDE);
            }
        }
    }

    // Where each frame went.
    let sprites: Vec<Value> = cuts
        .iter()
        .map(|cut| {
            let frames = cut
                .frames
                .iter()
                .map(|frame| {
                    let (page, x, y, rect, ox, oy) = match (frame.content, cut.tiles) {
                        (_, Some((_, _, page))) => (page, 0, 0, frame.region, 0, 0),
                        (Some(rect), None) => {
                            let (page, x, y) = frame.place;
                            (
                                page,
                                x + EXTRUDE,
                                y + EXTRUDE,
                                rect,
                                frame.origin.0 + (rect.x - frame.region.x),
                                frame.origin.1 + (rect.y - frame.region.y),
                            )
                        }
                        // Nothing to show: no pixels, page 0 at (0, 0).
                        (None, None) => (0, 0, 0, Rect::new(0, 0, 0, 0), 0, 0),
                    };
                    object(vec![
                        ("page", Value::Int(page as i64)),
                        ("x", int(x)),
                        ("y", int(y)),
                        ("w", int(rect.w)),
                        ("h", int(rect.h)),
                        ("ox", int(ox)),
                        ("oy", int(oy)),
                        ("sw", int(frame.cell.0)),
                        ("sh", int(frame.cell.1)),
                    ])
                })
                .collect();
            let mut members = vec![
                ("name", Value::String(cut.member.name.to_string())),
                ("frames", Value::Array(frames)),
            ];
            match cut.tiles {
                Some((w, h, _)) => {
                    members.push(("tiles", Value::Array(vec![int(w), int(h)])));
                }
                None => {
                    let first = cut.frames.first().map_or((0, 0), |frame| frame.cell);
                    let (px, py) = cut
                        .pivot
                        .unwrap_or((f64::from(first.0) / 2.0, f64::from(first.1)));
                    members.push((
                        "pivot",
                        Value::Array(vec![Value::Float(px), Value::Float(py)]),
                    ));
                }
            }
            if !cut.clips.is_empty() {
                let clips = cut
                    .clips
                    .iter()
                    .map(|clip| {
                        object(vec![
                            ("name", Value::String(clip.name.clone())),
                            ("loop", Value::Bool(clip.looping)),
                            (
                                "frames",
                                Value::Array(
                                    clip.steps
                                        .iter()
                                        .map(|&(frame, ms)| Value::Array(vec![int(frame), int(ms)]))
                                        .collect(),
                                ),
                            ),
                        ])
                    })
                    .collect();
                members.push(("clips", Value::Array(clips)));
            }
            object(members)
        })
        .collect();
    let sprite_count = sprites.len();
    let clip_count = cuts.iter().map(|cut| cut.clips.len()).sum();
    let atlas = object(vec![
        ("version", Value::Int(VERSION)),
        (
            "pages",
            Value::Array(
                pages
                    .iter()
                    .map(|(path, pixels)| {
                        object(vec![
                            ("path", Value::String(path.clone())),
                            ("w", int(pixels.width)),
                            ("h", int(pixels.height)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("sprites", Value::Array(sprites)),
    ]);

    let kbj = format!("atlas/{group}.kbj");
    let encode = |path: &str, value: &Value| {
        nadir_codec::kbj::encode(value, Limits::default()).map_err(|error| CookError::Encode {
            path: path.to_string(),
            error,
        })
    };
    let bytes = encode(&kbj, &atlas)?;
    // The game's reader takes it, or the cook refuses it: a pivot too large
    // for the game's numbers, say.
    nadir_codec::atlas::parse(&bytes, Limits::default()).map_err(|error| CookError::Encode {
        path: kbj.clone(),
        error,
    })?;
    let mut files = vec![(kbj, bytes)];
    let mut font_count = 0;
    for cut in &cuts {
        if let Some(font) = &cut.font {
            let path = font_file(cut.member.name);
            let bytes = encode(&path, font)?;
            files.push((path, bytes));
            font_count += 1;
        }
    }
    let page_count = pages.len();
    for (path, pixels) in pages {
        let header = Header {
            width: pixels.width,
            height: pixels.height,
            channels: Channels::Rgba,
            color_space: ColorSpace::Srgb,
        };
        let bytes = qoi::encode(&header, &pixels.rgba).map_err(|error| CookError::Encode {
            path: path.clone(),
            error,
        })?;
        files.push((path, bytes));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Cooked {
        files,
        warnings,
        pages: page_count,
        sprites: sprite_count,
        clips: clip_count,
        fonts: font_count,
    })
}
