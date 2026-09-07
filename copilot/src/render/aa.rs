// SPDX-License-Identifier: MIT OR Apache-2.0
//! Antialiasing: shapes whose edge pixels are painted by how much of each
//! the shape covers.
//!
//! Everything else in this renderer writes whole pixels, which is what a
//! bitmap font and a solid panel want and what a bus-bound display can
//! afford. A needle at an odd angle, a ring, a curve and a cell edge that
//! falls between two pixels all read better with the edge blended by
//! coverage, and that is what this module does when a scene asks for it.
//!
//! Coverage is measured by sampling: a four-by-four grid of points inside
//! each pixel, each asked whether it is inside the shape. Sampling rather than
//! analytic area because it works the same way for every shape, and a shape
//! has to answer only one question -- is this point inside you -- to be
//! antialiased. The exception is the rectangle with fractional edges, whose
//! coverage is the product of two overlaps and is computed exactly.
//!
//! Blending needs the pixel underneath, which the surface contract does not
//! otherwise require. [`Surface::blend_span`] reads it back where the surface
//! can be read and falls back to whole pixels where it cannot, so a scene
//! asking for antialiasing on a write-only panel gets the aliased picture
//! rather than a wrong one.
//!
//! No `sqrt` and no `floor`: neither exists in `core` for `f32`. Distances
//! are compared squared, and the helpers below round by hand.

use crate::trig::{ONE, TURN, cos, isqrt, sin};
use crate::{Color, Rect, Surface};

use super::raster::{clip as clip_to_surface, fill_rect};

/// Samples per axis inside a pixel.
const SUB: i32 = 4;

/// Samples per pixel, which is full coverage.
const FULL: u32 = (SUB * SUB) as u32;

/// The largest integer not above `v`.
///
/// `as i32` truncates towards zero, which is a floor for positive values and
/// one too high for negative ones; a widget partly off the left edge has
/// negative coordinates and must round the same way as one in the middle.
#[must_use]
pub fn floor_i(v: f32) -> i32 {
    let t = v as i32;
    if (t as f32) > v { t - 1 } else { t }
}

/// The smallest integer not below `v`.
#[must_use]
pub fn ceil_i(v: f32) -> i32 {
    let t = v as i32;
    if (t as f32) < v { t + 1 } else { t }
}

/// `v` to the nearest integer, halves rounding up.
#[must_use]
pub fn round_i(v: f32) -> i32 {
    floor_i(v + 0.5)
}

/// Whether `a` is strictly greater than `b`, with NaN answering no.
///
/// Spelled out so that a NaN edge or radius, which fails every comparison,
/// reads as "nothing to draw" rather than as a negated test that a reader has
/// to think about.
fn above(a: f32, b: f32) -> bool {
    a.partial_cmp(&b) == Some(core::cmp::Ordering::Greater)
}

/// How much of `[a0, a1)` lies inside `[b0, b1)`.
fn overlap(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

/// `color` at a fraction `cov` of its own opacity.
#[must_use]
pub fn at_coverage(color: Color, cov: f32) -> Color {
    if cov >= 1.0 {
        return color;
    }
    let a = (f32::from(color.a) * cov.max(0.0) + 0.5) as u8;
    Color::rgba(color.r, color.g, color.b, a)
}

/// How many of the samples in pixel (`x`, `y`) `inside` accepts.
fn samples(x: i32, y: i32, inside: &impl Fn(f32, f32) -> bool) -> u32 {
    let mut n = 0;
    for j in 0..SUB {
        for i in 0..SUB {
            let px = x as f32 + (2 * i + 1) as f32 / (2 * SUB) as f32;
            let py = y as f32 + (2 * j + 1) as f32 / (2 * SUB) as f32;
            if inside(px, py) {
                n += 1;
            }
        }
    }
    n
}

/// Pixels queued for one row, blended in runs.
///
/// A run rather than a pixel at a time: on a memory-mapped framebuffer the
/// row is fetched once per call, and one call per pixel fetches it once per
/// pixel.
pub struct Run<'s, S: Surface + ?Sized> {
    surface: &'s mut S,
    y: i32,
    start: i32,
    buf: [Color; 128],
    len: usize,
}

impl<'s, S: Surface + ?Sized> Run<'s, S> {
    /// A run on row `y`, empty.
    pub fn new(surface: &'s mut S, y: i32) -> Self {
        Self {
            surface,
            y,
            start: 0,
            buf: [Color::TRANSPARENT; 128],
            len: 0,
        }
    }

    /// Queue `color` for pixel `x`, which must not be left of the last one.
    pub fn push(&mut self, x: i32, color: Color) {
        if self.len == self.buf.len() || (self.len > 0 && self.start + self.len as i32 != x) {
            self.flush();
        }
        if self.len == 0 {
            self.start = x;
        }
        self.buf[self.len] = color;
        self.len += 1;
    }

    /// Blend whatever is queued.
    pub fn flush(&mut self) {
        if self.len > 0 {
            self.surface
                .blend_span(self.start, self.y, &self.buf[..self.len]);
            self.len = 0;
        }
    }
}

/// Shade the pixels `x0..x1` of row `y` by how much of each `inside` covers,
/// staying inside `clip`, which must already be inside the surface.
pub fn shade_row<S: Surface + ?Sized>(
    surface: &mut S,
    clip: Rect,
    y: i32,
    x0: i32,
    x1: i32,
    color: Color,
    inside: &impl Fn(f32, f32) -> bool,
) {
    if y < clip.top() || y >= clip.bottom() {
        return;
    }
    let (x0, x1) = (x0.max(clip.left()), x1.min(clip.right()));
    let mut run = Run::new(surface, y);
    for x in x0..x1 {
        let n = samples(x, y, inside);
        if n == 0 {
            run.flush();
        } else {
            run.push(x, at_coverage(color, n as f32 / FULL as f32));
        }
    }
    run.flush();
}

/// Fill the rectangle from (`x0`, `y0`) to (`x1`, `y1`), edges fractional.
///
/// This is what makes a bar, a chart column and a bargraph cell land where
/// the arithmetic put them rather than on the nearest pixel: a cell that is
/// 3.2 pixels wide is drawn three pixels wide and a fifth of a fourth, and a
/// row of them looks even because it is. With `aa` off the edges are rounded
/// to the nearest pixel instead, which is the old picture.
#[allow(clippy::too_many_arguments)] // Two corners, a colour, a clip and a switch.
pub fn frac_rect<S: Surface + ?Sized>(
    surface: &mut S,
    clip: Rect,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    color: Color,
    aa: bool,
) {
    // Written this way round so a NaN edge fails the test and draws nothing.
    if color.is_transparent() || !above(x1, x0) || !above(y1, y0) {
        return;
    }
    let Some(clip) = clip_to_surface(surface, clip) else {
        return;
    };
    if !aa {
        // In i64: a box at the far end of the coordinate range has edges
        // whose difference does not fit an i32.
        let edge = |v: f32| i64::from(round_i(v));
        let (l, t, r, b) = (edge(x0), edge(y0), edge(x1), edge(y1));
        let w = (r - l).clamp(0, i64::from(u32::MAX)) as u32;
        let h = (b - t).clamp(0, i64::from(u32::MAX)) as u32;
        if w > 0
            && h > 0
            && let Some(v) = Rect::new(l as i32, t as i32, w, h).intersection(clip)
        {
            fill_rect(surface, v, color);
        }
        return;
    }
    let rows = floor_i(y0).max(clip.top())..ceil_i(y1).min(clip.bottom());
    let cols = floor_i(x0).max(clip.left())..ceil_i(x1).min(clip.right());
    if rows.is_empty() || cols.is_empty() {
        return;
    }
    // The columns wholly inside, which a full row fills as a span.
    let (li, ri) = (ceil_i(x0).max(cols.start), floor_i(x1).min(cols.end));
    for y in rows {
        let cy = overlap(y as f32, y as f32 + 1.0, y0, y1);
        if cy <= 0.0 {
            continue;
        }
        if cy < 1.0 {
            // A partial row: every pixel in it is an edge pixel.
            let mut run = Run::new(surface, y);
            for x in cols.clone() {
                let cx = overlap(x as f32, x as f32 + 1.0, x0, x1);
                if cx <= 0.0 {
                    run.flush();
                } else {
                    run.push(x, at_coverage(color, cx * cy));
                }
            }
            run.flush();
            continue;
        }
        if ri > li {
            fill_rect(surface, Rect::new(li, y, (ri - li) as u32, 1), color);
        }
        // The edge columns, each at most one pixel.
        for x in [cols.start, cols.end - 1] {
            if x >= li && x < ri {
                continue;
            }
            let cx = overlap(x as f32, x as f32 + 1.0, x0, x1);
            if cx > 0.0 {
                surface.blend_span(x, y, &[at_coverage(color, cx)]);
            }
        }
    }
}

/// Draw a line from `a` to `b`, `width` wide with round ends.
///
/// The band of rows the line can touch is walked, and on each row only the
/// stretch near the line is sampled, so a long thin diagonal costs its
/// length times its width rather than the area of its bounding box.
pub fn line<S: Surface + ?Sized>(
    surface: &mut S,
    clip: Rect,
    a: (f32, f32),
    b: (f32, f32),
    width: f32,
    color: Color,
) {
    if color.is_transparent() {
        return;
    }
    let Some(clip) = clip_to_surface(surface, clip) else {
        return;
    };
    let r = (width / 2.0).max(0.5);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let (min_x, max_x) = (a.0.min(b.0) - r, a.0.max(b.0) + r);
    let (min_y, max_y) = (a.1.min(b.1) - r, a.1.max(b.1) + r);
    let inside = |px: f32, py: f32| {
        let (ex, ey) = (px - a.0, py - a.1);
        let t = if len2 > 0.0 {
            ((ex * dx + ey * dy) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (qx, qy) = (ex - t * dx, ey - t * dy);
        qx * qx + qy * qy <= r * r
    };
    let rows = floor_i(min_y).max(clip.top())..ceil_i(max_y).min(clip.bottom());
    for y in rows {
        let yc = y as f32 + 0.5;
        // Where the line crosses this row, and how far either side of that
        // the band reaches. The reach grows as the line flattens; for a flat
        // line it is the whole segment, which the endpoints already bound.
        let (lo, hi) = if dy.abs() < 1e-3 {
            (min_x, max_x)
        } else {
            let xc = a.0 + (yc - a.1) * dx / dy;
            let hx = r * (1.0 + (dx / dy).abs()) + 1.0;
            ((xc - hx).max(min_x), (xc + hx).min(max_x))
        };
        shade_row(surface, clip, y, floor_i(lo), ceil_i(hi), color, &inside);
    }
}

/// Fill a disc of radius `r` around `c`.
pub fn disc<S: Surface + ?Sized>(surface: &mut S, clip: Rect, c: (f32, f32), r: f32, color: Color) {
    if color.is_transparent() || !above(r, 0.0) {
        return;
    }
    let Some(clip) = clip_to_surface(surface, clip) else {
        return;
    };
    let inside = |px: f32, py: f32| {
        let (vx, vy) = (px - c.0, py - c.1);
        vx * vx + vy * vy <= r * r
    };
    let rows = floor_i(c.1 - r).max(clip.top())..ceil_i(c.1 + r).min(clip.bottom());
    for y in rows {
        shade_row(
            surface,
            clip,
            y,
            floor_i(c.0 - r),
            ceil_i(c.0 + r),
            color,
            &inside,
        );
    }
}

/// Fill the part of the ring between `inner` and `outer` around `c` that
/// lies within `sweep` brads clockwise of `a0`.
///
/// The wedge test is two cross products, no angles: a point is past the
/// start ray and short of the end ray. A sweep beyond half a turn is the
/// complement of the smaller wedge, so that one is tested and inverted.
#[allow(clippy::too_many_arguments)] // A ring is a centre, two radii and a sweep.
pub fn arc<S: Surface + ?Sized>(
    surface: &mut S,
    clip: Rect,
    c: (f32, f32),
    inner: f32,
    outer: f32,
    a0: i32,
    sweep: i32,
    color: Color,
) {
    if color.is_transparent() || !above(outer, 0.0) || sweep == 0 {
        return;
    }
    let Some(clip) = clip_to_surface(surface, clip) else {
        return;
    };
    let inner = inner.clamp(0.0, outer);
    let ray = |brad: i32| (cos(brad) as f32 / ONE as f32, sin(brad) as f32 / ONE as f32);
    let start = ray(a0);
    let end = ray(a0.saturating_add(sweep));
    let full = sweep.unsigned_abs() >= TURN as u32;
    let reflex = sweep.unsigned_abs() > (TURN / 2) as u32;
    let sign = if sweep < 0 { -1.0 } else { 1.0 };
    let cross = |ax: f32, ay: f32, bx: f32, by: f32| ax * by - ay * bx;
    let inside = |px: f32, py: f32| {
        let (vx, vy) = (px - c.0, py - c.1);
        let d2 = vx * vx + vy * vy;
        if d2 > outer * outer || d2 < inner * inner {
            return false;
        }
        if full {
            return true;
        }
        let after_start = cross(start.0, start.1, vx, vy) * sign >= 0.0;
        let before_end = cross(vx, vy, end.0, end.1) * sign >= 0.0;
        if reflex {
            after_start || before_end
        } else {
            after_start && before_end
        }
    };
    let rows = floor_i(c.1 - outer).max(clip.top())..ceil_i(c.1 + outer).min(clip.bottom());
    for y in rows {
        // The stretch of this row within the outer circle, and the hole in
        // the middle of it inside the inner one, so the sampling only visits
        // the ring itself. Integer square roots on pixel distances: a pixel
        // either side is slack enough for the fraction thrown away.
        let dy = (y as f32 + 0.5 - c.1).abs();
        let reach = |radius: f32| -> Option<i32> {
            let d = radius - dy;
            if d <= 0.0 {
                return None;
            }
            let r2 = (radius * radius - dy * dy).min(i32::MAX as f32 / 2.0) as i32;
            Some(isqrt(r2))
        };
        let Some(half) = reach(outer) else { continue };
        let (x0, x1) = (floor_i(c.0) - half - 1, ceil_i(c.0) + half + 1);
        match reach(inner) {
            Some(hole) if hole > 1 => {
                let (h0, h1) = (floor_i(c.0) - hole + 1, ceil_i(c.0) + hole - 1);
                shade_row(surface, clip, y, x0, h0, color, &inside);
                shade_row(surface, clip, y, h1, x1, color, &inside);
            }
            _ => shade_row(surface, clip, y, x0, x1, color, &inside),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemorySurface, PixelFormat, Size};

    const W: Color = Color::WHITE;
    const AREA: Rect = Rect::new(0, 0, 32, 32);

    fn surf() -> MemorySurface {
        MemorySurface::new(Size { w: 32, h: 32 }, PixelFormat::Bgrx8888)
    }

    /// The red channel at a pixel, which for white on black is its coverage.
    fn level(s: &MemorySurface, x: usize, y: usize) -> u8 {
        s.pixels()[y * s.stride() + x * 4 + 2]
    }

    #[test]
    fn rounding_helpers_agree_with_the_mathematics_below_zero() {
        assert_eq!(floor_i(-0.5), -1);
        assert_eq!(floor_i(2.0), 2);
        assert_eq!(ceil_i(-0.5), 0);
        assert_eq!(ceil_i(2.0), 2);
        assert_eq!(ceil_i(2.1), 3);
        assert_eq!(round_i(-1.5), -1);
        assert_eq!(round_i(1.5), 2);
        assert_eq!(round_i(1.49), 1);
    }

    #[test]
    fn a_fractional_rect_paints_its_edges_by_coverage() {
        let mut s = surf();
        frac_rect(&mut s, AREA, 2.5, 2.0, 5.5, 4.0, W, true);
        assert_eq!(level(&s, 3, 2), 255, "the inside is solid");
        assert!(
            (120..=135).contains(&level(&s, 2, 2)),
            "left edge {}",
            level(&s, 2, 2)
        );
        assert!(
            (120..=135).contains(&level(&s, 5, 2)),
            "right edge {}",
            level(&s, 5, 2)
        );
        assert_eq!(level(&s, 1, 2), 0);
        assert_eq!(level(&s, 6, 2), 0);
        assert_eq!(level(&s, 3, 4), 0, "the row below is untouched");
    }

    #[test]
    fn a_fractional_rect_without_antialiasing_rounds_to_pixels() {
        let mut s = surf();
        frac_rect(&mut s, AREA, 2.5, 2.0, 5.4, 4.0, W, false);
        assert_eq!(level(&s, 2, 2), 0, "2.5 rounds up to 3");
        assert_eq!(level(&s, 3, 2), 255);
        assert_eq!(level(&s, 4, 2), 255);
        assert_eq!(level(&s, 5, 2), 0, "5.4 rounds down to 5");
    }

    #[test]
    fn two_abutting_fractional_rects_leave_a_faint_seam() {
        // The cell boundary at 4.3: the pixel takes 0.3 of one and 0.7 of
        // the other. Coverage composes as alpha, so the second blend covers
        // 0.7 of what the first left, and the seam pixel reaches 79% rather
        // than full. That is why a bargraph with no gap between its cells
        // keeps rounded edges: a seam is worse than a pixel of unevenness.
        let mut s = surf();
        frac_rect(&mut s, AREA, 1.0, 1.0, 4.3, 3.0, W, true);
        frac_rect(&mut s, AREA, 4.3, 1.0, 8.0, 3.0, W, true);
        let seam = level(&s, 4, 1);
        assert!((190..=210).contains(&seam), "seam pixel {seam}");
    }

    #[test]
    fn a_line_has_soft_edges_and_a_solid_core() {
        let mut s = surf();
        // Half a width of 2.5 either side of 16.5 reaches from 15.25 to
        // 17.75: row 16 is wholly inside and rows 15 and 17 are three
        // quarters covered.
        line(&mut s, AREA, (3.0, 16.5), (30.0, 16.5), 2.5, W);
        assert_eq!(level(&s, 16, 16), 255, "the core is solid");
        let edge = level(&s, 16, 15);
        assert!(edge > 0 && edge < 255, "edge {edge}");
        assert_eq!(level(&s, 16, 14), 0);
        assert_eq!(level(&s, 0, 16), 0, "before the start and its round cap");
    }

    #[test]
    fn a_diagonal_line_covers_its_corner_pixels_partly() {
        let mut s = surf();
        line(&mut s, AREA, (1.0, 1.0), (30.0, 30.0), 1.0, W);
        let on = level(&s, 15, 15);
        let beside = level(&s, 16, 15);
        assert!(on > 0, "nothing on the diagonal");
        assert!(beside > 0 && beside < on, "beside {beside}, on {on}");
    }

    #[test]
    fn a_disc_is_solid_inside_and_soft_at_the_rim() {
        let mut s = surf();
        // The centre sits on a pixel corner, so a radius of 5.5 cuts pixel
        // row 10 -- which spans 5 to 6 away -- down the middle.
        disc(&mut s, AREA, (16.0, 16.0), 5.5, W);
        assert_eq!(level(&s, 16, 16), 255);
        assert_eq!(level(&s, 16, 24), 0);
        let rim = level(&s, 16, 10);
        assert!(rim > 0 && rim < 255, "rim {rim}");
    }

    #[test]
    fn an_arc_stays_within_its_sweep() {
        let mut s = surf();
        // A quarter turn from twelve o'clock, clockwise: the top-right.
        let top = -(TURN / 4);
        arc(&mut s, AREA, (16.0, 16.0), 8.0, 12.0, top, TURN / 4, W);
        assert!(level(&s, 23, 9) > 0, "the top-right quadrant is empty");
        assert_eq!(level(&s, 9, 23), 0, "the bottom-left quadrant was painted");
        assert_eq!(level(&s, 16, 16), 0, "the hole was painted");
    }

    #[test]
    fn a_reflex_arc_is_the_complement_of_the_small_one() {
        let mut s = surf();
        let top = -(TURN / 4);
        arc(&mut s, AREA, (16.0, 16.0), 8.0, 12.0, top, TURN * 3 / 4, W);
        assert!(level(&s, 23, 9) > 0, "top-right");
        assert!(level(&s, 23, 23) > 0, "bottom-right");
        assert!(level(&s, 9, 23) > 0, "bottom-left");
        assert_eq!(level(&s, 9, 9), 0, "top-left is the part left out");
    }

    #[test]
    fn a_full_turn_is_a_whole_ring() {
        let mut s = surf();
        arc(&mut s, AREA, (16.0, 16.0), 8.0, 12.0, 0, TURN, W);
        for (x, y) in [(23, 9), (23, 23), (9, 23), (9, 9)] {
            assert!(level(&s, x, y) > 0, "({x}, {y}) is empty");
        }
    }

    #[test]
    fn nothing_lands_outside_the_clip() {
        let mut s = surf();
        let clip = Rect::new(0, 0, 16, 32);
        line(&mut s, AREA, (2.0, 16.0), (30.0, 16.0), 3.0, Color::BLACK);
        line(&mut s, clip, (2.0, 16.0), (30.0, 16.0), 3.0, W);
        disc(&mut s, clip, (16.0, 4.0), 6.0, W);
        frac_rect(&mut s, clip, 10.0, 24.5, 30.0, 28.5, W, true);
        for y in 0..32 {
            for x in 16..32 {
                assert_eq!(level(&s, x, y), 0, "({x}, {y}) was painted");
            }
        }
    }

    #[test]
    fn a_surface_that_cannot_be_read_gets_whole_pixels() {
        // The fallback in `blend_span`: half-covered or more is written
        // solid, less is left alone.
        struct WriteOnly(MemorySurface);
        impl Surface for WriteOnly {
            fn size(&self) -> Size {
                self.0.size()
            }
            fn format(&self) -> PixelFormat {
                self.0.format()
            }
            fn fill_span(&mut self, x: i32, y: i32, count: u32, color: Color) {
                self.0.fill_span(x, y, count, color);
            }
            fn blit_span(&mut self, x: i32, y: i32, src: &[Color]) {
                self.0.blit_span(x, y, src);
            }
            fn present(&mut self, _: Option<Rect>) {}
        }
        let mut s = WriteOnly(surf());
        frac_rect(&mut s, AREA, 2.0, 2.0, 5.75, 3.0, W, true);
        assert_eq!(level(&s.0, 4, 2), 255);
        assert_eq!(level(&s.0, 5, 2), 255, "three quarters covered is written");
        frac_rect(&mut s, AREA, 2.0, 5.0, 5.25, 6.0, W, true);
        assert_eq!(level(&s.0, 5, 5), 0, "a quarter covered is left alone");
    }
}
