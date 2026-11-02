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

//! Pixels: trimming frames, and copying them into pages with their edges
//! extruded.

use crate::pack::Rect;

/// An RGBA8 image, rows from the top, alpha straight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pixels {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

impl Pixels {
    /// A transparent black image.
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            rgba: vec![0; width as usize * height as usize * 4],
        }
    }

    fn at(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * 4
    }

    fn alpha(&self, x: u32, y: u32) -> u8 {
        self.rgba[self.at(x, y) + 3]
    }

    /// The smallest part of `frame` that holds every pixel of it whose
    /// alpha is above 0, or `None` if it has none.
    pub(crate) fn trim(&self, frame: Rect) -> Option<Rect> {
        let rows = frame.y..frame.y + frame.h;
        let columns = frame.x..frame.x + frame.w;
        let opaque_row = |y: u32| columns.clone().any(|x| self.alpha(x, y) > 0);
        let opaque_column =
            |x: u32, top: u32, bottom: u32| (top..bottom).any(|y| self.alpha(x, y) > 0);
        let top = rows.clone().find(|&y| opaque_row(y))?;
        let bottom = rows.rev().find(|&y| opaque_row(y)).unwrap_or(top) + 1;
        let left = columns
            .clone()
            .find(|&x| opaque_column(x, top, bottom))
            .unwrap_or(frame.x);
        let right = columns
            .rev()
            .find(|&x| opaque_column(x, top, bottom))
            .unwrap_or(left)
            + 1;
        Some(Rect::new(left, top, right - left, bottom - top))
    }

    /// Copies `from`, a part of `source`, to `(x, y)` in this image, with its
    /// edge pixels repeated `extrude` pixels further out on each side, the
    /// corners too. The copy with its extrusion must fit.
    pub(crate) fn blit(&mut self, source: &Pixels, from: Rect, x: u32, y: u32, extrude: u32) {
        if from.w == 0 || from.h == 0 {
            return;
        }
        let (width, height) = (from.w + 2 * extrude, from.h + 2 * extrude);
        for dy in 0..height {
            let sy = from.y + dy.saturating_sub(extrude).min(from.h - 1);
            for dx in 0..width {
                let sx = from.x + dx.saturating_sub(extrude).min(from.w - 1);
                let (to, at) = (self.at(x + dx, y + dy), source.at(sx, sy));
                self.rgba[to..to + 4].copy_from_slice(&source.rgba[at..at + 4]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An image from rows of letters: `.` transparent, any other letter an
    /// opaque pixel whose red is the letter.
    fn image(rows: &[&str]) -> Pixels {
        let height = rows.len() as u32;
        let width = rows[0].len() as u32;
        let mut pixels = Pixels::new(width, height);
        for (y, row) in rows.iter().enumerate() {
            for (x, letter) in row.bytes().enumerate() {
                if letter != b'.' {
                    let at = pixels.at(x as u32, y as u32);
                    pixels.rgba[at..at + 4].copy_from_slice(&[letter, 0, 0, 255]);
                }
            }
        }
        pixels
    }

    fn letters(pixels: &Pixels) -> Vec<String> {
        (0..pixels.height)
            .map(|y| {
                (0..pixels.width)
                    .map(|x| {
                        let at = pixels.at(x, y);
                        if pixels.rgba[at + 3] == 0 {
                            '.'
                        } else {
                            char::from(pixels.rgba[at])
                        }
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn trimming_keeps_the_box_of_the_pixels_seen() {
        let sheet = image(&["......", ".a....", "....b.", "......"]);
        let whole = Rect::new(0, 0, 6, 4);
        assert_eq!(sheet.trim(whole), Some(Rect::new(1, 1, 4, 2)));
        // Within a frame of the sheet only.
        assert_eq!(
            sheet.trim(Rect::new(0, 0, 3, 4)),
            Some(Rect::new(1, 1, 1, 1))
        );
        assert_eq!(
            sheet.trim(Rect::new(3, 0, 3, 4)),
            Some(Rect::new(4, 2, 1, 1))
        );
        assert_eq!(sheet.trim(Rect::new(0, 3, 6, 1)), None);
        // A pixel of alpha 1 counts; one of 0 does not, whatever its color.
        let mut faint = Pixels::new(3, 3);
        let at = faint.at(2, 0);
        faint.rgba[at..at + 4].copy_from_slice(&[9, 9, 9, 0]);
        assert_eq!(faint.trim(Rect::new(0, 0, 3, 3)), None);
        let at = faint.at(1, 2);
        faint.rgba[at + 3] = 1;
        assert_eq!(
            faint.trim(Rect::new(0, 0, 3, 3)),
            Some(Rect::new(1, 2, 1, 1))
        );
        // A full frame stays whole.
        let full = image(&["ab", "cd"]);
        assert_eq!(
            full.trim(Rect::new(0, 0, 2, 2)),
            Some(Rect::new(0, 0, 2, 2))
        );
    }

    #[test]
    fn a_blit_repeats_the_edges_and_the_corners() {
        let source = image(&["....", ".ab.", ".cd.", "...."]);
        let mut page = Pixels::new(6, 6);
        page.blit(&source, Rect::new(1, 1, 2, 2), 1, 1, 1);
        assert_eq!(
            letters(&page),
            ["......", ".aabb.", ".aabb.", ".ccdd.", ".ccdd.", "......"]
        );
        let mut page = Pixels::new(4, 3);
        page.blit(&source, Rect::new(1, 1, 2, 1), 0, 0, 0);
        assert_eq!(letters(&page), ["ab..", "....", "...."]);
        // Nothing of an empty frame.
        page.blit(&source, Rect::new(0, 0, 0, 0), 0, 0, 1);
        assert_eq!(letters(&page), ["ab..", "....", "...."]);
    }
}
