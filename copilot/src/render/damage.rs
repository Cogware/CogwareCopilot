// SPDX-License-Identifier: GPL-3.0-only
//! Tracking which parts of the screen actually changed.
//!
//! Repainting only what moved is what makes the toolkit fast on a machine with
//! no GPU, where a non-cacheable framebuffer makes every pixel a trip to DRAM.
//! This keeps at most [`MAX_RECTS`] rectangles and, when a new one will not
//! fit, merges the two whose union wastes fewest pixels: bounded overdraw and
//! no allocation, rather than the region algebra a widget tree does not need.
//! The rectangles may overlap, which only costs those pixels being painted
//! twice; a caller that cannot tolerate that should repaint [`Damage::bounds`].

use crate::Rect;

/// How many rectangles are tracked before merging begins.
///
/// Sixteen covers the instrument-cluster case — a handful of gauges plus a
/// telltale strip — without the merge path ever running. It is a tuning
/// constant, not a limit on what callers may mark.
///
/// It was eight while a moving widget marked its whole rectangle, when a
/// frame could not produce many distinct boxes. Now that an arc and a needle
/// each mark only the wedge they swept, a triple-buffered display repainting
/// three frames of history has three small boxes per instrument rather than
/// one big one — and merging those back together would hand back exactly the
/// area the wedges were computed to avoid.
pub const MAX_RECTS: usize = 16;

/// The set of rectangles that must be repainted this frame.
#[derive(Clone, Debug, Default)]
pub struct Damage {
    rects: [Rect; MAX_RECTS],
    len: usize,
}

impl Damage {
    /// An empty damage set: nothing needs repainting.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            rects: [Rect::ZERO; MAX_RECTS],
            len: 0,
        }
    }

    /// Whether anything at all needs repainting.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The rectangles to repaint. May overlap; see the module docs.
    #[must_use]
    pub fn rects(&self) -> &[Rect] {
        &self.rects[..self.len]
    }

    /// Forget all damage. Called after a frame has been presented.
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// The single rectangle covering everything damaged, if anything is.
    ///
    /// This is what gets handed to [`crate::Surface::present`], because a
    /// backend that flips pages or pushes a window wants one box, not eight.
    #[must_use]
    pub fn bounds(&self) -> Option<Rect> {
        self.rects()
            .iter()
            .copied()
            .reduce(|acc, r| acc.union(r))
            .filter(|r| !r.is_empty())
    }

    /// Mark `rect` as needing repaint.
    ///
    /// Empty rectangles are ignored: a widget that resized to nothing has no
    /// pixels to repaint, and admitting it would make [`bounds`] grow towards
    /// a corner for no reason.
    ///
    /// [`bounds`]: Self::bounds
    pub fn add(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }

        // Absorbing into an existing rectangle first keeps the common case —
        // one widget marked repeatedly in a frame — from ever growing the set.
        for existing in &mut self.rects[..self.len] {
            if existing.contains_rect(rect) {
                return;
            }
            if rect.contains_rect(*existing) {
                *existing = rect;
                return;
            }
        }

        if self.len < MAX_RECTS {
            self.rects[self.len] = rect;
            self.len += 1;
            return;
        }

        // Full. Merge whichever pair wastes the fewest pixels, counting the
        // candidate as one of the pair, then take the freed slot. Choosing by
        // wasted area rather than by union area matters: two large adjacent
        // boxes should merge before two small distant ones, and only wasted
        // area says so.
        let mut best = (0usize, usize::MAX);
        for i in 0..self.len {
            let waste = merge_waste(self.rects[i], rect);
            if waste < best.1 {
                best = (i, waste);
            }
        }
        let mut best_pair: Option<(usize, usize, usize)> = None;
        for i in 0..self.len {
            for j in (i + 1)..self.len {
                let waste = merge_waste(self.rects[i], self.rects[j]);
                if best_pair.is_none_or(|(_, _, w)| waste < w) {
                    best_pair = Some((i, j, waste));
                }
            }
        }

        match best_pair {
            // Merging two existing boxes is cheaper than absorbing the new one,
            // so do that and store the newcomer in the slot it frees.
            Some((i, j, waste)) if waste < best.1 => {
                self.rects[i] = self.rects[i].union(self.rects[j]);
                self.rects[j] = self.rects[self.len - 1];
                self.rects[self.len - 1] = rect;
            }
            _ => self.rects[best.0] = self.rects[best.0].union(rect),
        }
    }
}

/// Pixels a union would cover that neither input did.
///
/// Saturates rather than wrapping: coordinates are `i32` and a pathological
/// pair can exceed `usize` on a 32-bit target, where wrapping would make an
/// enormous merge look free and starve every sensible one.
fn merge_waste(a: Rect, b: Rect) -> usize {
    let union = a.union(b).area();
    let covered = a.area().saturating_add(b.area());
    union.saturating_sub(covered.saturating_sub(a.intersection(b).map_or(0, |r| r.area())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: i32, y: i32, w: u32, h: u32) -> Rect {
        Rect::new(x, y, w, h)
    }

    #[test]
    fn empty_rects_are_ignored() {
        let mut d = Damage::new();
        d.add(r(10, 10, 0, 5));
        d.add(r(10, 10, 5, 0));
        assert!(d.is_empty());
        assert_eq!(d.bounds(), None);
    }

    #[test]
    fn repeated_identical_marks_do_not_grow_the_set() {
        let mut d = Damage::new();
        for _ in 0..100 {
            d.add(r(4, 4, 10, 10));
        }
        assert_eq!(d.rects().len(), 1);
    }

    #[test]
    fn a_contained_rect_is_absorbed() {
        let mut d = Damage::new();
        d.add(r(0, 0, 100, 100));
        d.add(r(10, 10, 5, 5));
        assert_eq!(d.rects(), &[r(0, 0, 100, 100)]);
    }

    #[test]
    fn a_containing_rect_replaces_the_smaller_one() {
        let mut d = Damage::new();
        d.add(r(10, 10, 5, 5));
        d.add(r(0, 0, 100, 100));
        assert_eq!(d.rects(), &[r(0, 0, 100, 100)]);
    }

    #[test]
    fn the_set_never_exceeds_its_bound() {
        let mut d = Damage::new();
        for i in 0..(MAX_RECTS as i32 * 4) {
            d.add(r(i * 50, i * 50, 10, 10));
        }
        assert!(d.rects().len() <= MAX_RECTS);
    }

    #[test]
    fn merging_never_loses_coverage() {
        // The invariant that actually matters: whatever was marked must still
        // be inside the bounds after any amount of merging, or a frame keeps a
        // stale fragment and that looks like memory corruption.
        let mut d = Damage::new();
        let marks: alloc::vec::Vec<Rect> = (0..40)
            .map(|i| r(i * 37 % 900, i * 53 % 500, 12, 9))
            .collect();
        for m in &marks {
            d.add(*m);
        }
        let bounds = d.bounds().expect("40 non-empty marks cannot vanish");
        for m in &marks {
            assert!(
                bounds.contains_rect(*m),
                "{m:?} escaped the damage bounds {bounds:?}"
            );
        }
    }

    #[test]
    fn bounds_is_the_union_of_everything() {
        let mut d = Damage::new();
        d.add(r(10, 20, 5, 5));
        d.add(r(100, 200, 5, 5));
        assert_eq!(d.bounds(), Some(r(10, 20, 95, 185)));
    }

    #[test]
    fn clear_forgets_everything() {
        let mut d = Damage::new();
        d.add(r(1, 1, 2, 2));
        d.clear();
        assert!(d.is_empty());
        assert_eq!(d.bounds(), None);
    }
}
