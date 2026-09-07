// SPDX-License-Identifier: MIT OR Apache-2.0
//! Drawing primitives, and the one place clipping happens.
//!
//! Every function here clips against the surface first and then emits spans
//! that are guaranteed in bounds. That is what rule 4.4 promises a [`Surface`]
//! implementation, and it is why the inner loops need no bounds checks and no
//! `unsafe` to avoid them — there is nothing left to check.
//!
//! # Why the clip is a separate step and not folded into each primitive
//!
//! Folding it in means every primitive repeats the same four comparisons and
//! gets a chance to get one of them wrong. Doing it once, in [`clip`], means a
//! bug in the clip is a bug in one place, and the primitives below it read as
//! the geometry they actually are.

use crate::{Color, Rect, Surface};

/// Intersect `rect` with the drawable area of `surface`.
///
/// Returns `None` when nothing of `rect` is on screen, which every primitive
/// treats as "draw nothing" rather than as an error: a widget scrolled off the
/// edge is normal, not exceptional.
#[must_use]
pub fn clip<S: Surface + ?Sized>(surface: &S, rect: Rect) -> Option<Rect> {
    let size = surface.size();
    let bounds = Rect::new(0, 0, size.w, size.h);
    rect.intersection(bounds).filter(|r| !r.is_empty())
}

/// Fill `rect` with a solid colour.
///
/// A fully transparent colour draws nothing at all rather than reading and
/// rewriting every pixel unchanged — on a non-cacheable framebuffer that read
/// is the expensive half of the operation.
pub fn fill_rect<S: Surface + ?Sized>(surface: &mut S, rect: Rect, color: Color) {
    if color.is_transparent() {
        return;
    }
    let Some(r) = clip(surface, rect) else {
        return;
    };
    for y in r.top()..r.bottom() {
        surface.fill_span(r.left(), y, r.size.w, color);
    }
}

/// Draw a one-pixel outline just inside `rect`.
///
/// The four edges are emitted as separate spans with the horizontal ones full
/// width and the vertical ones shortened, so no pixel is written twice. That
/// matters less for correctness than for a surface whose `fill_span` is a bus
/// transaction rather than a memory write.
pub fn stroke_rect<S: Surface + ?Sized>(surface: &mut S, rect: Rect, color: Color) {
    if color.is_transparent() || rect.is_empty() {
        return;
    }
    let w = rect.size.w;
    let h = rect.size.h;
    fill_rect(surface, Rect::new(rect.left(), rect.top(), w, 1), color);
    if h > 1 {
        fill_rect(
            surface,
            Rect::new(rect.left(), rect.bottom() - 1, w, 1),
            color,
        );
    }
    if h > 2 {
        let inner_h = h - 2;
        let y = rect.top() + 1;
        fill_rect(surface, Rect::new(rect.left(), y, 1, inner_h), color);
        if w > 1 {
            fill_rect(surface, Rect::new(rect.right() - 1, y, 1, inner_h), color);
        }
    }
}

/// Blit `src`, a `width`-pixel-wide RGBA image, with its top-left at `at`.
///
/// Rows are copied whole where the image is fully on screen and sliced where
/// it is not, so a partly off-screen image costs only the pixels that land.
///
/// `src` shorter than `width * height` draws only the rows it can supply,
/// which makes a truncated decode degrade to a partial image rather than to a
/// panic.
pub fn blit<S: Surface + ?Sized>(surface: &mut S, at: Rect, src: &[Color], width: u32) {
    if width == 0 {
        return;
    }
    let Some(r) = clip(surface, at) else {
        return;
    };

    // How far into the source the clip moved us. Both are non-negative because
    // `r` is the intersection, so it can only have shrunk from `at`.
    let skip_x = (r.left() - at.left()) as usize;
    let skip_y = (r.top() - at.top()) as usize;
    let stride = width as usize;

    for (n, y) in (r.top()..r.bottom()).enumerate() {
        let row_start = (skip_y + n) * stride + skip_x;
        let Some(row) = src.get(row_start..) else {
            return;
        };
        let take = (r.size.w as usize).min(row.len());
        if take == 0 {
            return;
        }
        surface.blit_span(r.left(), y, &row[..take]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PixelFormat, Size};
    use alloc::vec;
    use alloc::vec::Vec;

    /// Records every span it is handed, so a test can assert on what the
    /// rasteriser *emitted* rather than on the pixels that resulted. Bugs in
    /// clipping show up here directly; in a pixel buffer they have to be
    /// inferred.
    struct Recorder {
        size: Size,
        fills: Vec<(i32, i32, u32)>,
        blits: Vec<(i32, i32, usize)>,
    }

    impl Recorder {
        fn new(w: u32, h: u32) -> Self {
            Self {
                size: Size { w, h },
                fills: Vec::new(),
                blits: Vec::new(),
            }
        }
        /// Panics if anything landed outside the surface. This is the contract
        /// rule 4.4 makes to every backend, so it is worth asserting directly.
        fn assert_in_bounds(&self) {
            for &(x, y, n) in &self.fills {
                assert!(x >= 0 && y >= 0, "span at ({x},{y}) is negative");
                assert!(y < self.size.h as i32, "span row {y} past the surface");
                assert!(
                    x as i64 + n as i64 <= self.size.w as i64,
                    "span at ({x},{y}) runs {n} past the right edge"
                );
            }
            for &(x, y, n) in &self.blits {
                assert!(x >= 0 && y >= 0 && y < self.size.h as i32);
                assert!(x as i64 + n as i64 <= self.size.w as i64);
            }
        }
    }

    impl Surface for Recorder {
        fn size(&self) -> Size {
            self.size
        }
        fn format(&self) -> PixelFormat {
            PixelFormat::Bgrx8888
        }
        fn fill_span(&mut self, x: i32, y: i32, count: u32, _c: Color) {
            self.fills.push((x, y, count));
        }
        fn blit_span(&mut self, x: i32, y: i32, src: &[Color]) {
            self.blits.push((x, y, src.len()));
        }
        fn present(&mut self, _damage: Option<Rect>) {}
    }

    #[test]
    fn a_fill_emits_one_span_per_row() {
        let mut s = Recorder::new(100, 100);
        fill_rect(&mut s, Rect::new(10, 20, 5, 3), Color::WHITE);
        assert_eq!(s.fills, vec![(10, 20, 5), (10, 21, 5), (10, 22, 5)]);
        s.assert_in_bounds();
    }

    #[test]
    fn a_transparent_fill_touches_nothing() {
        // Reading and rewriting every pixel unchanged is the expensive half of
        // the operation on a non-cacheable framebuffer.
        let mut s = Recorder::new(100, 100);
        fill_rect(&mut s, Rect::new(0, 0, 50, 50), Color::TRANSPARENT);
        assert!(s.fills.is_empty());
    }

    #[test]
    fn a_fill_entirely_off_screen_draws_nothing() {
        let mut s = Recorder::new(100, 100);
        fill_rect(&mut s, Rect::new(-50, -50, 10, 10), Color::WHITE);
        fill_rect(&mut s, Rect::new(200, 200, 10, 10), Color::WHITE);
        assert!(s.fills.is_empty());
    }

    #[test]
    fn a_fill_straddling_an_edge_is_clipped_not_dropped() {
        let mut s = Recorder::new(100, 100);
        fill_rect(&mut s, Rect::new(-5, -5, 20, 20), Color::WHITE);
        // Starts at the origin, 15 wide and 15 tall: the on-screen remainder.
        assert_eq!(s.fills.first(), Some(&(0, 0, 15)));
        assert_eq!(s.fills.len(), 15);
        s.assert_in_bounds();
    }

    #[test]
    fn a_fill_larger_than_the_surface_covers_it_exactly_once() {
        let mut s = Recorder::new(8, 4);
        fill_rect(&mut s, Rect::new(-100, -100, 1000, 1000), Color::WHITE);
        assert_eq!(s.fills.len(), 4);
        assert!(s.fills.iter().all(|&(x, _, n)| x == 0 && n == 8));
        s.assert_in_bounds();
    }

    #[test]
    fn a_stroke_writes_no_pixel_twice() {
        let mut s = Recorder::new(100, 100);
        stroke_rect(&mut s, Rect::new(0, 0, 10, 10), Color::WHITE);
        let painted: usize = s.fills.iter().map(|&(_, _, n)| n as usize).sum();
        // Perimeter of a 10x10 box is 36, not 40: the corners belong to one
        // edge each.
        assert_eq!(painted, 36);
        s.assert_in_bounds();
    }

    #[test]
    fn a_one_pixel_high_stroke_is_a_single_line() {
        let mut s = Recorder::new(100, 100);
        stroke_rect(&mut s, Rect::new(0, 0, 10, 1), Color::WHITE);
        assert_eq!(s.fills, vec![(0, 0, 10)]);
    }

    #[test]
    fn a_blit_offsets_into_the_source_when_clipped() {
        let mut s = Recorder::new(100, 100);
        let img = vec![Color::WHITE; 10 * 10];
        blit(&mut s, Rect::new(-3, -2, 10, 10), &img, 10);
        // Seven columns and eight rows survive the clip.
        assert_eq!(s.blits.len(), 8);
        assert!(s.blits.iter().all(|&(x, _, n)| x == 0 && n == 7));
        s.assert_in_bounds();
    }

    #[test]
    fn a_truncated_image_draws_the_rows_it_has() {
        // A partial decode should degrade to a partial image, not a panic.
        let mut s = Recorder::new(100, 100);
        let img = vec![Color::WHITE; 25];
        blit(&mut s, Rect::new(0, 0, 10, 10), &img, 10);
        assert_eq!(s.blits.len(), 3);
        s.assert_in_bounds();
    }

    #[test]
    fn a_zero_width_image_draws_nothing() {
        let mut s = Recorder::new(100, 100);
        blit(&mut s, Rect::new(0, 0, 10, 10), &[Color::WHITE], 0);
        assert!(s.blits.is_empty());
    }
}
