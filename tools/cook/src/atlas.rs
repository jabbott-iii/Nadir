//! One atlas group: its images cut into frames and trimmed, packed into
//! pages with padding and extrusion, and written as QOI pages and a KBJ of
//! where each frame went (`docs/subsystems/assets-and-codecs.md`, "Atlas
//! groups").

use nadir_codec::json::Value;
use nadir_codec::qoi::{self, Channels, ColorSpace, Header};
use nadir_codec::{Limits, png};

use crate::CookError;
use crate::config::{Layout, Sprite};
use crate::image::Pixels;
use crate::pack::{self, Rect};

/// The atlas file's version: the one the game's reader reads.
pub(crate) const VERSION: i64 = nadir_codec::atlas::VERSION;
/// The space between two frames in a page, in pixels.
pub(crate) const PADDING: u32 = 2;
/// How far each frame's edge pixels are repeated outward.
pub(crate) const EXTRUDE: u32 = 1;
/// The side of the page that a group of only empty frames gets.
const EMPTY_PAGE: u32 = pack::SIDES[0];
/// The widest and tallest tile set: the largest page.
const MAX_TILE_SET: u32 = pack::SIDES[pack::SIDES.len() - 1];

/// The color chunks that make the cook warn: they say the colors may not
/// be plain sRGB, as the cook takes them. `sRGB` itself, and text, do not.
const COLOR_CHUNKS: [&str; 6] = ["gAMA", "cHRM", "iCCP", "cICP", "mDCV", "cLLI"];

/// The limits a source image is decoded with: any width or height PNG
/// allows, since a sheet is cut up, and the default bytes.
fn source_limits() -> Limits {
    Limits {
        max_texture_dimension: 0x7fff_ffff,
        ..Limits::default()
    }
}

/// An image of the group, read from its file.
pub(crate) struct Member<'a> {
    /// Its path, lowercased: `sprites/person.png`.
    pub(crate) path: &'a str,
    /// The file's bytes.
    pub(crate) png: &'a [u8],
    /// What its sidecar says.
    pub(crate) sprite: Sprite,
}

/// What a group cooks to.
pub(crate) struct Cooked {
    /// The files, by path below the output folder, in order.
    pub(crate) files: Vec<(String, Vec<u8>)>,
    /// What to tell the user.
    pub(crate) warnings: Vec<String>,
    pub(crate) pages: usize,
    pub(crate) sprites: usize,
}

/// A frame of an image: its cell, and the part of it that is packed.
struct Frame {
    cell: Rect,
    /// `None` for a frame with no pixel to show.
    content: Option<Rect>,
    /// Where `content` went: its page, and its corner there.
    place: (usize, u32, u32),
}

/// An image decoded and cut.
struct Cut<'a> {
    member: &'a Member<'a>,
    pixels: Pixels,
    frames: Vec<Frame>,
    /// A tile set's tile width and height, and its page.
    tiles: Option<(u32, u32, usize)>,
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

/// Decodes and cuts one image, and says whether to warn about its colors.
fn cut<'a>(member: &'a Member<'a>, warnings: &mut Vec<String>) -> Result<Cut<'a>, CookError> {
    let path = member.path;
    let image = png::decode(member.png, source_limits()).map_err(|error| CookError::Decode {
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
    let (width, height) = (image.width, image.height);
    let pixels = Pixels {
        width,
        height,
        rgba: image.pixels,
    };
    let mut tiles = None;
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
        tiles = Some((w, h, 0));
    }
    let frames = cells(path, width, height, member.sprite.layout)?
        .into_iter()
        .map(|cell| Frame {
            cell,
            content: if member.sprite.trim {
                pixels.trim(cell)
            } else {
                Some(cell)
            },
            place: (0, 0, 0),
        })
        .collect();
    Ok(Cut {
        member,
        pixels,
        frames,
        tiles,
    })
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

/// The region's name: its image's path without `.png`.
fn name(path: &str) -> &str {
    path.strip_suffix(".png").unwrap_or(path)
}

/// Cooks the group `group` of `members`, in any order: what it writes
/// follows its sprites' names.
///
/// # Errors
///
/// [`CookError::Decode`] for a PNG the decoder refuses,
/// [`CookError::Layout`] for an image that does not fit its sidecar,
/// [`CookError::TooBig`] for a frame bigger than the largest page, and
/// [`CookError::Encode`] if an output cannot be written as QOI or KBJ, or
/// the game's atlas reader refuses the atlas file written.
pub(crate) fn cook(group: &str, members: &[Member<'_>]) -> Result<Cooked, CookError> {
    let mut warnings = Vec::new();
    // By name, whatever order the files were found in, so that the same
    // files always cook to the same bytes.
    let mut members: Vec<&Member<'_>> = members.iter().collect();
    members.sort_by(|a, b| name(a.path).cmp(name(b.path)));
    let mut cuts = members
        .into_iter()
        .map(|member| cut(member, &mut warnings))
        .collect::<Result<Vec<_>, _>>()?;

    // The frames to pack, tallest first.
    let mut items: Vec<(usize, usize)> = Vec::new();
    for (image, cut) in cuts.iter().enumerate() {
        if cut.tiles.is_none() {
            for (frame, data) in cut.frames.iter().enumerate() {
                if data.content.is_some() {
                    items.push((image, frame));
                }
            }
        }
    }
    let content = |cuts: &[Cut<'_>], (image, frame): (usize, usize)| {
        cuts[image].frames[frame]
            .content
            .unwrap_or(Rect::new(0, 0, 0, 0))
    };
    items.sort_by(|&a, &b| {
        let (ra, rb) = (content(&cuts, a), content(&cuts, b));
        rb.h.cmp(&ra.h)
            .then(rb.w.cmp(&ra.w))
            .then(name(cuts[a.0].member.path).cmp(name(cuts[b.0].member.path)))
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
        let (image, frame) = items[index];
        let rect = content(&cuts, items[index]);
        CookError::TooBig {
            path: cuts[image].member.path.to_string(),
            frame,
            size: (rect.w, rect.h),
        }
    })?;
    let mut sides = packing.sides.clone();
    let packed_frames = cuts.iter().any(|cut| cut.tiles.is_none());
    if sides.is_empty() && packed_frames {
        sides.push(EMPTY_PAGE);
    }
    for (&(image, frame), &place) in items.iter().zip(&packing.places) {
        cuts[image].frames[frame].place = place;
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
            let path = format!("atlas/{}.tiles.qoi", name(cut.member.path));
            cut.tiles = Some((w, h, pages.len()));
            pages.push((path, cut.pixels.clone()));
        }
    }
    for cut in &cuts {
        if cut.tiles.is_some() {
            continue;
        }
        for frame in &cut.frames {
            if let Some(rect) = frame.content {
                let (page, x, y) = frame.place;
                pages[page].1.blit(&cut.pixels, rect, x, y, EXTRUDE);
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
                    let cell = frame.cell;
                    let (page, x, y, rect) = match (frame.content, cut.tiles) {
                        (_, Some((_, _, page))) => (page, 0, 0, cell),
                        (Some(rect), None) => {
                            let (page, x, y) = frame.place;
                            (page, x + EXTRUDE, y + EXTRUDE, rect)
                        }
                        // Nothing to show: no pixels, from the cell's corner.
                        (None, None) => (0, 0, 0, Rect::new(cell.x, cell.y, 0, 0)),
                    };
                    object(vec![
                        ("page", Value::Int(page as i64)),
                        ("x", int(x)),
                        ("y", int(y)),
                        ("w", int(rect.w)),
                        ("h", int(rect.h)),
                        ("ox", int(rect.x - cell.x)),
                        ("oy", int(rect.y - cell.y)),
                        ("sw", int(cell.w)),
                        ("sh", int(cell.h)),
                    ])
                })
                .collect();
            let mut members = vec![
                ("name", Value::String(name(cut.member.path).to_string())),
                ("frames", Value::Array(frames)),
            ];
            match cut.tiles {
                Some((w, h, _)) => {
                    members.push(("tiles", Value::Array(vec![int(w), int(h)])));
                }
                None => {
                    let first = cut
                        .frames
                        .first()
                        .map_or(Rect::new(0, 0, 0, 0), |frame| frame.cell);
                    let (px, py) = cut
                        .member
                        .sprite
                        .pivot
                        .unwrap_or((f64::from(first.w) / 2.0, f64::from(first.h)));
                    members.push((
                        "pivot",
                        Value::Array(vec![Value::Float(px), Value::Float(py)]),
                    ));
                }
            }
            object(members)
        })
        .collect();
    let sprite_count = sprites.len();
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
    let bytes =
        nadir_codec::kbj::encode(&atlas, Limits::default()).map_err(|error| CookError::Encode {
            path: kbj.clone(),
            error,
        })?;
    // The game's reader takes it, or the cook refuses it: a pivot too large
    // for the game's numbers, say.
    nadir_codec::atlas::parse(&bytes, Limits::default()).map_err(|error| CookError::Encode {
        path: kbj.clone(),
        error,
    })?;
    let mut files = vec![(kbj, bytes)];
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
    })
}
