//! Rectangle packing: MaxRects with the best short-side fit (Jukka
//! Jylänki, "A Thousand Ways to Pack the Bin", 2010), and the pages it packs
//! into.

/// A rectangle, in whole pixels from the top-left corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Rect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) w: u32,
    pub(crate) h: u32,
}

impl Rect {
    pub(crate) const fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    fn right(&self) -> u32 {
        self.x + self.w
    }

    fn bottom(&self) -> u32 {
        self.y + self.h
    }

    fn overlaps(&self, other: &Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    fn contains(&self, other: &Rect) -> bool {
        self.x <= other.x
            && self.y <= other.y
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }
}

/// One bin: the free space left in a page, as the maximal free rectangles,
/// which may overlap each other.
struct MaxRects {
    free: Vec<Rect>,
}

impl MaxRects {
    fn new(width: u32, height: u32) -> Self {
        Self {
            free: vec![Rect::new(0, 0, width, height)],
        }
    }

    /// Places a rectangle of `w` × `h`, and returns its corner, or `None`
    /// if it does not fit.
    ///
    /// It takes the free rectangle that leaves the shortest side over, then
    /// the shortest long side, then the topmost, then the leftmost.
    fn insert(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let (_, _, y, x) = self
            .free
            .iter()
            .filter(|free| free.w >= w && free.h >= h)
            .map(|free| {
                let (dw, dh) = (free.w - w, free.h - h);
                (dw.min(dh), dw.max(dh), free.y, free.x)
            })
            .min()?;
        self.take(Rect::new(x, y, w, h));
        Some((x, y))
    }

    /// Takes `used` out of every free rectangle it overlaps, keeping the
    /// parts left, top, right and below it, then drops each free rectangle
    /// that another holds.
    ///
    /// Only the new parts can be held: no free rectangle held another
    /// before, and one that `used` did not touch cannot lie inside a part
    /// of one it did, since that one would have held it. So only the parts
    /// are checked, which keeps a group of thousands of frames fast.
    fn take(&mut self, used: Rect) {
        let mut parts = Vec::new();
        self.free.retain(|free| {
            if !free.overlaps(&used) {
                return true;
            }
            if used.x > free.x {
                parts.push(Rect::new(free.x, free.y, used.x - free.x, free.h));
            }
            if used.right() < free.right() {
                parts.push(Rect::new(
                    used.right(),
                    free.y,
                    free.right() - used.right(),
                    free.h,
                ));
            }
            if used.y > free.y {
                parts.push(Rect::new(free.x, free.y, free.w, used.y - free.y));
            }
            if used.bottom() < free.bottom() {
                parts.push(Rect::new(
                    free.x,
                    used.bottom(),
                    free.w,
                    free.bottom() - used.bottom(),
                ));
            }
            false
        });
        // Of two equal rectangles, the first is kept.
        let mut kept = Vec::with_capacity(parts.len());
        for (index, part) in parts.iter().enumerate() {
            let held = self.free.iter().any(|free| free.contains(part))
                || parts.iter().enumerate().any(|(other, by)| {
                    other != index && by.contains(part) && (by != part || other < index)
                });
            if !held {
                kept.push(*part);
            }
        }
        self.free.extend(kept);
    }
}

/// Where the items went: each item's page and corner, and each page's side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Packing {
    /// For each item, in the order given: its page and its top-left corner.
    pub(crate) places: Vec<(usize, u32, u32)>,
    /// Each page's width and height: a square.
    pub(crate) sides: Vec<u32>,
}

/// The pages' sides: the powers of two from 64 to 4096.
pub(crate) const SIDES: [u32; 7] = [64, 128, 256, 512, 1024, 2048, 4096];

/// Packs items of the sizes given, in that order, into square pages of a
/// side from [`SIDES`], each a bin `slack` wider and taller than its page,
/// so that each item's own trailing `slack` may fall off its edge.
///
/// All of them go into one page if one of [`SIDES`] holds them: the
/// smallest that does. Otherwise each goes into the first page of the
/// largest side it fits, a new one if none, and each page is then shrunk
/// to the smallest side that holds its items, packed again in their order.
///
/// # Errors
///
/// The index of the first item too big for the largest page.
pub(crate) fn pack(sizes: &[(u32, u32)], slack: u32) -> Result<Packing, usize> {
    let largest = SIDES[SIDES.len() - 1];
    if let Some(index) = sizes
        .iter()
        .position(|&(w, h)| w > largest + slack || h > largest + slack)
    {
        return Err(index);
    }
    if sizes.is_empty() {
        return Ok(Packing {
            places: Vec::new(),
            sides: Vec::new(),
        });
    }
    if let Some((side, corners)) = smallest_page(sizes, slack) {
        return Ok(Packing {
            places: corners.into_iter().map(|(x, y)| (0, x, y)).collect(),
            sides: vec![side],
        });
    }
    // Too many for one page.
    let mut pages: Vec<MaxRects> = Vec::new();
    let mut page_of = Vec::with_capacity(sizes.len());
    for &(w, h) in sizes {
        let page = match pages
            .iter_mut()
            .position(|page| page.insert(w, h).is_some())
        {
            Some(page) => page,
            None => {
                let mut page = MaxRects::new(largest + slack, largest + slack);
                page.insert(w, h).ok_or(page_of.len())?;
                pages.push(page);
                pages.len() - 1
            }
        };
        page_of.push(page);
    }
    let mut places = vec![(0, 0, 0); sizes.len()];
    let mut sides = Vec::with_capacity(pages.len());
    for page in 0..pages.len() {
        let items: Vec<usize> = (0..sizes.len()).filter(|&i| page_of[i] == page).collect();
        let own: Vec<(u32, u32)> = items.iter().map(|&i| sizes[i]).collect();
        // They fit the largest page once, in this order, so they do again.
        let (side, corners) = smallest_page(&own, slack).ok_or(items[0])?;
        for (&item, (x, y)) in items.iter().zip(corners) {
            places[item] = (page, x, y);
        }
        sides.push(side);
    }
    Ok(Packing { places, sides })
}

/// The smallest of [`SIDES`] whose page holds every item, in order, and
/// their corners in it.
fn smallest_page(sizes: &[(u32, u32)], slack: u32) -> Option<(u32, Vec<(u32, u32)>)> {
    SIDES.iter().find_map(|&side| {
        let mut bin = MaxRects::new(side + slack, side + slack);
        let corners: Option<Vec<(u32, u32)>> =
            sizes.iter().map(|&(w, h)| bin.insert(w, h)).collect();
        corners.map(|corners| (side, corners))
    })
}

#[cfg(test)]
mod tests {
    use nadir_core::testing::check;

    use super::*;

    /// Checks that `packing` puts every item inside its page's bin, apart
    /// from every other.
    fn assert_sound(sizes: &[(u32, u32)], slack: u32, packing: &Packing) -> Result<(), String> {
        if packing.places.len() != sizes.len() {
            return Err(format!(
                "{} places for {} items",
                packing.places.len(),
                sizes.len()
            ));
        }
        let rects: Vec<(usize, Rect)> = packing
            .places
            .iter()
            .zip(sizes)
            .map(|(&(page, x, y), &(w, h))| (page, Rect::new(x, y, w, h)))
            .collect();
        for (index, &(page, rect)) in rects.iter().enumerate() {
            let side = *packing.sides.get(page).ok_or(format!(
                "item {index} is on page {page}, which is not there"
            ))?;
            if rect.right() > side + slack || rect.bottom() > side + slack {
                return Err(format!("item {index}, {rect:?}, leaves its page of {side}"));
            }
            for (other, &(other_page, other_rect)) in rects.iter().enumerate().skip(index + 1) {
                if page == other_page && rect.w > 0 && rect.h > 0 && rect.overlaps(&other_rect) {
                    return Err(format!("items {index} and {other} overlap"));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn one_page_of_the_smallest_side_that_holds_them() {
        let sizes = [(40, 40), (40, 40), (40, 40), (40, 40)];
        let packing = pack(&sizes, 0).expect("they fit");
        assert_eq!(packing.sides, [128]);
        assert_eq!(assert_sound(&sizes, 0, &packing), Ok(()));
        // Four of 32 fill a page of 64 exactly.
        let packing = pack(&[(32, 32); 4], 0).expect("they fit");
        assert_eq!(packing.sides, [64]);
        assert_eq!(
            packing.places,
            [(0, 0, 0), (0, 32, 0), (0, 0, 32), (0, 32, 32)]
        );
    }

    #[test]
    fn the_slack_may_fall_off_the_edge() {
        // 62 pixels and 2 of slack each: two in a row fill a page of 128,
        // the second's slack past its edge.
        let packing = pack(&[(64, 64), (64, 64)], 2).expect("they fit");
        assert_eq!(packing.sides, [128]);
        assert_eq!(packing.places, [(0, 0, 0), (0, 64, 0)]);
        // One item of the largest side and its slack fits.
        assert!(pack(&[(4098, 4098)], 2).is_ok());
        assert_eq!(pack(&[(16, 16), (4099, 10)], 2), Err(1));
    }

    #[test]
    fn too_many_for_one_page_fill_several_and_each_is_shrunk() {
        let sizes = [(3000, 3000), (3000, 3000), (100, 100)];
        let packing = pack(&sizes, 0).expect("they fit");
        assert_eq!(packing.sides, [4096, 4096]);
        assert_eq!(assert_sound(&sizes, 0, &packing), Ok(()));
        // The small one goes beside the first, in its page.
        assert_eq!(packing.places[2].0, 0);
        // A page with only a small item left is shrunk to fit it.
        let packing = pack(&[(4096, 4096), (100, 100)], 0).expect("they fit");
        assert_eq!(packing.sides, [4096, 128]);
        assert_eq!(packing.places[1], (1, 0, 0));
    }

    /// The free rectangles none of which holds another, as a check of
    /// [`MaxRects::take`]'s shortcut.
    fn none_holds_another(free: &[Rect]) -> bool {
        free.iter().enumerate().all(|(index, rect)| {
            free.iter()
                .enumerate()
                .all(|(other, by)| other == index || !by.contains(rect))
        })
    }

    #[test]
    fn after_each_place_no_free_rectangle_holds_another() {
        check(
            "MaxRects keeps no free rectangle inside another",
            200,
            |rng| {
                let count = rng.range(1..120u32) as usize;
                (0..count)
                    .map(|_| (rng.range(1..60u32), rng.range(1..60u32)))
                    .collect::<Vec<_>>()
            },
            |sizes| {
                let mut bin = MaxRects::new(256, 256);
                for &(w, h) in sizes {
                    bin.insert(w, h);
                    if !none_holds_another(&bin.free) {
                        return Err(format!("after {w} × {h}: {:?}", bin.free));
                    }
                }
                Ok(())
            },
        );
    }

    #[test]
    fn nothing_packs_into_no_pages() {
        assert_eq!(
            pack(&[], 2),
            Ok(Packing {
                places: Vec::new(),
                sides: Vec::new()
            })
        );
    }

    #[test]
    fn the_same_items_pack_the_same_way() {
        let sizes: Vec<(u32, u32)> = (0..60).map(|i| (8 + i * 7 % 50, 6 + i * 11 % 40)).collect();
        assert_eq!(pack(&sizes, 2), pack(&sizes, 2));
    }

    #[test]
    fn any_items_pack_inside_their_pages_and_apart() {
        check(
            "packed items stay inside their pages and apart",
            300,
            |rng| {
                let count = rng.range(1..80u32) as usize;
                let big = rng.range(0..4u32) == 0;
                let slack = rng.range(0..4u32);
                let sizes: Vec<(u32, u32)> = (0..count)
                    .map(|_| {
                        let most: u32 = if big { 2500 } else { 300 };
                        (rng.range(1..most), rng.range(1..most))
                    })
                    .collect();
                (sizes, slack)
            },
            |(sizes, slack)| {
                let packing =
                    pack(sizes, *slack).map_err(|index| format!("item {index} did not fit"))?;
                assert_sound(sizes, *slack, &packing)
            },
        );
    }
}
