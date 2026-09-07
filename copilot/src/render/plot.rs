// SPDX-License-Identifier: MIT OR Apache-2.0
//! Plotted series: a polyline and the chart built on it.
//!
//! Both take their coordinates as fractions of the widget's box rather than
//! pixels. A tachometer's power curve is the same curve whether it is drawn
//! across a 320-pixel panel or a 1080-pixel one, and a scene file that said
//! pixels would have to be rewritten for every display it ran on.

use crate::{Color, Point, Rect, Surface};

use super::fill_rect;
use super::shape::line;

/// Draws a sequence of connected line segments, mapping normalised
/// coordinates into the widget's box so that the shape scales with its
/// allocation.  The `closed` flag closes the loop when there are enough
/// vertices to form a polygon, avoiding a degenerate self-overlap on
/// two-point inputs.
#[allow(clippy::too_many_arguments)] // A stroke is its points, width, colour, closure and finish.
pub fn polyline<S: Surface + ?Sized>(
    surface: &mut S,
    at: Rect,
    clip: Rect,
    points: &[(f32, f32)],
    width: u32,
    color: Color,
    closed: bool,
    antialias: bool,
) {
    if at.is_empty() || at.intersection(clip).is_none() {
        return;
    }
    if points.len() < 2 {
        return;
    }
    let segment = |surface: &mut S, a: Point, b: Point| {
        if antialias {
            let c = |p: Point| (p.x as f32 + 0.5, p.y as f32 + 0.5);
            super::aa::line(surface, clip, c(a), c(b), width.max(1) as f32, color);
        } else {
            line(surface, a, b, width, color, clip);
        }
    };

    // Mapped on the fly rather than collected first: the core crate does not
    // allocate, and a segment needs only its own two endpoints.
    for pair in points.windows(2) {
        let a = map(at, pair[0].0, pair[0].1);
        let b = map(at, pair[1].0, pair[1].1);
        segment(surface, a, b);
    }
    // Two points closed would be the same segment drawn back over itself.
    if closed && points.len() >= 3 {
        let last = points[points.len() - 1];
        let first = points[0];
        segment(surface, map(at, last.0, last.1), map(at, first.0, first.1));
    }
}

/// Renders a filled area chart with a crisp stroke along the top edge.
/// The fill is painted first so that the stroke remains unobscured,
/// preserving visual clarity at the boundary between the two regions.
#[allow(clippy::too_many_arguments)] // A series, a stroke, a fill and a finish.
pub fn chart<S: Surface + ?Sized>(
    surface: &mut S,
    at: Rect,
    clip: Rect,
    values: &[f32],
    width: u32,
    stroke: Color,
    fill: Color,
    antialias: bool,
) {
    if at.is_empty() || at.intersection(clip).is_none() {
        return;
    }
    if values.is_empty() {
        return;
    }

    let w = at.size.w as i32;
    let n = values.len();

    // Fill the area beneath the curve, column by column, so that the
    // shape follows the interpolated value at every pixel rather than
    // stepping between discrete sample points.
    if !fill.is_transparent() {
        let left = at.left();
        let right = at.right();
        let bottom = at.bottom();

        for x in left..right {
            // Determine the fractional position of this column within
            // the widget's width, then map it to a position along the
            // series for linear interpolation.
            let col_frac = if w > 1 {
                ((x - left) as f32) / (w - 1) as f32
            } else {
                0.0
            };

            // Map the column fraction to a position in the value array.
            let pos = if n == 1 {
                0.0
            } else {
                col_frac * (n - 1) as f32
            };

            let idx = pos as usize;
            let frac = pos - idx as f32;

            let v0 = values[idx.min(n - 1)];
            let v1 = if idx + 1 < n { values[idx + 1] } else { v0 };
            let interp = v0 + (v1 - v0) * frac;

            // Map the interpolated value to a y pixel; 0.0 is the bottom,
            // 1.0 is the top, so we invert the fraction.
            // Inverted: a value of 1.0 belongs at the top of the box, but the
            // mapping counts downwards from it.
            if antialias {
                // The column's top lands at a fraction of a pixel, so the
                // filled area follows the curve rather than a staircase of
                // it.
                let h = (at.size.h as i32 - 1).max(0) as f32;
                let top = at.top() as f32 + (1.0 - clamp_frac(interp)) * h;
                super::aa::frac_rect(
                    surface,
                    clip,
                    x as f32,
                    top,
                    x as f32 + 1.0,
                    bottom as f32,
                    fill,
                    true,
                );
                continue;
            }
            let y = map(at, 0.0, 1.0 - clamp_frac(interp)).y;

            let col_rect = Rect::new(x, y, 1, (bottom - y).max(0) as u32);
            if let Some(inter) = col_rect.intersection(clip)
                && !inter.is_empty()
            {
                fill_rect(surface, inter, fill);
            }
        }
    }

    // Draw the stroke on top so the line edge stays crisp against the
    // fill, which would otherwise bleed over the stroke's upper pixels.
    if !stroke.is_transparent() && n >= 2 {
        let mut prev: Option<Point> = None;
        // `n >= 2` above, so the divisor is never zero.
        for (i, &v) in values.iter().enumerate() {
            let pt = map(at, i as f32 / (n - 1) as f32, 1.0 - clamp_frac(v));

            if let Some(p) = prev {
                if antialias {
                    let c = |p: Point| (p.x as f32 + 0.5, p.y as f32 + 0.5);
                    super::aa::line(surface, clip, c(p), c(pt), width.max(1) as f32, stroke);
                } else {
                    line(surface, p, pt, width, stroke, clip);
                }
            }
            prev = Some(pt);
        }
    }
}

/// Turn a pair of normalised fractions into a pixel inside `at`.
///
/// One helper rather than the arithmetic written out at each call site,
/// because the clamp is the load-bearing part: without it a scene file's
/// stray value scatters pixels across the whole framebuffer, and a clamp
/// forgotten in one of three places is a bug nobody sees until it ships.
fn map(at: Rect, fx: f32, fy: f32) -> Point {
    // `w - 1` rather than `w`: a fraction of 1.0 means the last pixel inside
    // the box, not the first one past its exclusive right edge.
    let w = (at.size.w as i32 - 1).max(0) as f32;
    let h = (at.size.h as i32 - 1).max(0) as f32;
    Point {
        x: at.left() + (clamp_frac(fx) * w) as i32,
        y: at.top() + (clamp_frac(fy) * h) as i32,
    }
}

/// Clamp a fraction to 0.0..=1.0, treating NaN as 0.0.
///
/// NaN compares false against everything, so an unguarded `clamp` would pass
/// it straight through into a cast whose result is not worth predicting.
fn clamp_frac(t: f32) -> f32 {
    if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) }
}

/// A hard cap on the number of edge crossings a single scanline can record.
///
/// This crate renders without allocating, so a fixed-size stack buffer is the
/// only option. A dashboard shape that produces more than 64 crossings on one
/// scanline is not a shape anyone drew on purpose; beyond this bound the
/// remaining crossings are silently dropped rather than risking a heap
/// allocation or a panic.
const MAX_CROSSINGS: usize = 64;

/// How many sub-rows a row is cut into for antialiasing a polygon, and how
/// finely each sub-row's coverage is measured across a pixel.
const SUBROWS: u32 = 4;
const ACROSS: u32 = 16;

/// The x positions where the polygon's edges cross the horizontal line at
/// `yc`, sorted, written into `out`. Returns how many there are.
///
/// The half-open rule -- an edge counts when one end is at or above the line
/// and the other strictly below -- makes each vertex count for exactly one of
/// its two edges, which is what keeps the even-odd rule right at a vertex.
fn crossings(
    vertices: impl Fn(usize) -> (f32, f32),
    n: usize,
    yc: f32,
    out: &mut [f32; MAX_CROSSINGS],
) -> usize {
    let mut count = 0;
    for i in 0..n {
        let (ax, ay) = vertices(i);
        let (bx, by) = vertices((i + 1) % n);
        // A horizontal edge never crosses a horizontal line and would
        // divide by zero below.
        if ay == by {
            continue;
        }
        if (ay <= yc && by > yc) || (by <= yc && ay > yc) {
            let x = ax + (yc - ay) / (by - ay) * (bx - ax);
            // Past the buffer, crossings are dropped rather than allocated
            // for: a shape with more than sixty-four on one row is not a
            // shape anyone drew on purpose.
            if count < MAX_CROSSINGS {
                out[count] = x;
                count += 1;
            }
        }
    }
    // Insertion sort, because `sort` needs `std`.
    for i in 1..count {
        let key = out[i];
        let mut j = i;
        while j > 0 && out[j - 1] > key {
            out[j] = out[j - 1];
            j -= 1;
        }
        out[j] = key;
    }
    count
}

/// Fill a polygon defined by normalised vertices, clipped to the damage
/// region, using the even-odd rule.
///
/// The scanline algorithm walks each row of the intersection between the
/// widget's box and the clip region, collecting the x-coordinates where the
/// polygon's edges cross the centre of that row. Crossings are sorted and
/// consumed in pairs to determine the filled spans.
///
/// Antialiased, each row is cut into [`SUBROWS`] and every sub-row's spans
/// are measured against each pixel they touch, so a pixel's coverage is the
/// mean of four exact horizontal overlaps. Exact across and sampled down,
/// because a span already knows exactly where it starts and stops.
pub fn polygon<S: Surface + ?Sized>(
    surface: &mut S,
    at: Rect,
    clip: Rect,
    points: &[(f32, f32)],
    color: Color,
    antialias: bool,
) {
    // A transparent colour, fewer than three vertices, or no overlap between
    // the widget and the clip region means there is nothing to draw.
    if color.is_transparent() || points.len() < 3 {
        return;
    }
    let Some(area) = at.intersection(clip) else {
        return;
    };
    if area.is_empty() {
        return;
    }
    let n = points.len();

    if antialias {
        // Vertices at the same places the aliased path puts them, but not
        // rounded to pixels on the way.
        let w = (at.size.w as i32 - 1).max(0) as f32;
        let h = (at.size.h as i32 - 1).max(0) as f32;
        let vertex = |i: usize| {
            let (fx, fy) = points[i];
            (
                at.left() as f32 + clamp_frac(fx) * w,
                at.top() as f32 + clamp_frac(fy) * h,
            )
        };
        // Coverage is gathered a window of columns at a time, into a buffer
        // on the stack: this crate does not allocate, and a whole row of a
        // large panel would not fit.
        const WINDOW: usize = 256;
        let full = (SUBROWS * ACROSS) as f32;
        for y in area.top()..area.bottom() {
            let mut left = area.left();
            while left < area.right() {
                let right = (left as i64 + WINDOW as i64).min(area.right() as i64) as i32;
                let mut cov = [0u16; WINDOW];
                for s in 0..SUBROWS {
                    let yc = y as f32 + (2 * s + 1) as f32 / (2 * SUBROWS) as f32;
                    let mut xs = [0.0f32; MAX_CROSSINGS];
                    let count = crossings(vertex, n, yc, &mut xs);
                    for pair in xs[..count].as_chunks::<2>().0 {
                        let (xa, xb) = (pair[0], pair[1]);
                        let first = super::aa::floor_i(xa).max(left);
                        let last = super::aa::ceil_i(xb).min(right);
                        for x in first..last {
                            let inside = (xb.min(x as f32 + 1.0) - xa.max(x as f32)).max(0.0);
                            cov[(x - left) as usize] += (inside * ACROSS as f32 + 0.5) as u16;
                        }
                    }
                }
                let mut run = super::aa::Run::new(surface, y);
                for (i, &c) in cov[..(right - left) as usize].iter().enumerate() {
                    if c == 0 {
                        run.flush();
                    } else {
                        run.push(
                            left + i as i32,
                            super::aa::at_coverage(color, c as f32 / full),
                        );
                    }
                }
                run.flush();
                left = right;
            }
        }
        return;
    }

    let vertex = |i: usize| {
        let p = map(at, points[i].0, points[i].1);
        (p.x as f32, p.y as f32)
    };
    // Walk each scanline from the top of the intersection to the row just
    // before the bottom (exclusive edge), so every pixel row is covered.
    for y in area.top()..area.bottom() {
        let mut xs = [0.0f32; MAX_CROSSINGS];
        let count = crossings(vertex, n, y as f32 + 0.5, &mut xs);
        // Consume the sorted crossings in pairs: each pair bounds one span
        // that is inside the polygon under the even-odd rule.
        for pair in xs[..count].as_chunks::<2>().0 {
            // A pixel is in the span when its centre is: the first column
            // whose centre is at or past the crossing, up to the first
            // whose centre is past the far one.
            let x0 = super::aa::ceil_i(pair[0] - 0.5);
            let x1 = super::aa::ceil_i(pair[1] - 0.5);
            if x1 <= x0 {
                continue;
            }
            let span = Rect::new(x0, y, (x1 - x0) as u32, 1);
            if let Some(inter) = span.intersection(area)
                && !inter.is_empty()
            {
                fill_rect(surface, inter, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemorySurface, PixelFormat, Size};

    const FULL: Rect = Rect::new(0, 0, 64, 64);

    fn surf() -> MemorySurface {
        MemorySurface::new(Size { w: 64, h: 64 }, PixelFormat::Bgrx8888)
    }

    fn lit(s: &MemorySurface, x: usize, y: usize) -> bool {
        let o = y * s.stride() + x * 4;
        s.pixels()[o..o + 4] != [0, 0, 0, 0]
    }

    fn count(s: &MemorySurface) -> usize {
        (0..64)
            .flat_map(|y| (0..64).map(move |x| (x, y)))
            .filter(|&(x, y)| lit(s, x, y))
            .count()
    }

    /// The topmost lit row in a column, if any.
    fn top_of(s: &MemorySurface, x: usize) -> Option<usize> {
        (0..64).find(|&y| lit(s, x, y))
    }

    // ---- polyline ----

    #[test]
    fn a_polyline_spans_its_box_corner_to_corner() {
        let mut s = surf();
        polyline(
            &mut s,
            FULL,
            FULL,
            &[(0.0, 0.0), (1.0, 1.0)],
            1,
            Color::WHITE,
            false,
            false,
        );
        assert!(lit(&s, 0, 0), "starts at the top-left");
        assert!(lit(&s, 63, 63), "ends at the bottom-right");
    }

    #[test]
    fn a_polyline_is_normalised_to_its_box() {
        // The same points in a smaller box must land inside that box.
        let mut s = surf();
        let at = Rect::new(10, 10, 20, 20);
        polyline(
            &mut s,
            at,
            FULL,
            &[(0.0, 0.0), (1.0, 1.0)],
            1,
            Color::WHITE,
            false,
            false,
        );
        assert!(lit(&s, 10, 10));
        assert!(lit(&s, 29, 29));
        for y in 0..64 {
            for x in 0..64 {
                assert!(
                    !lit(&s, x, y) || (10..30).contains(&x) && (10..30).contains(&y),
                    "drew at ({x}, {y}), outside the box"
                );
            }
        }
    }

    #[test]
    fn a_polyline_joins_every_segment() {
        let mut s = surf();
        polyline(
            &mut s,
            FULL,
            FULL,
            &[(0.0, 1.0), (0.5, 0.0), (1.0, 1.0)],
            1,
            Color::WHITE,
            false,
            false,
        );
        assert!(lit(&s, 0, 63), "first point");
        assert!(
            top_of(&s, 31).is_some_and(|y| y < 4),
            "the peak in the middle"
        );
        assert!(lit(&s, 63, 63), "last point");
    }

    #[test]
    fn a_closed_polyline_draws_more_than_an_open_one() {
        let pts = [(0.1, 0.1), (0.9, 0.1), (0.5, 0.9)];
        let mut open = surf();
        let mut shut = surf();
        polyline(&mut open, FULL, FULL, &pts, 1, Color::WHITE, false, false);
        polyline(&mut shut, FULL, FULL, &pts, 1, Color::WHITE, true, false);
        assert!(
            count(&shut) > count(&open),
            "the closing segment adds pixels"
        );
    }

    #[test]
    fn a_closed_two_point_polyline_does_not_double_draw() {
        let pts = [(0.1, 0.1), (0.9, 0.9)];
        let mut open = surf();
        let mut shut = surf();
        polyline(&mut open, FULL, FULL, &pts, 1, Color::WHITE, false, false);
        polyline(&mut shut, FULL, FULL, &pts, 1, Color::WHITE, true, false);
        assert_eq!(
            count(&shut),
            count(&open),
            "a line back over itself adds nothing"
        );
    }

    #[test]
    fn a_polyline_of_fewer_than_two_points_draws_nothing() {
        let mut s = surf();
        polyline(&mut s, FULL, FULL, &[], 1, Color::WHITE, true, false);
        polyline(
            &mut s,
            FULL,
            FULL,
            &[(0.5, 0.5)],
            1,
            Color::WHITE,
            true,
            false,
        );
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn a_polyline_respects_the_clip() {
        let mut s = surf();
        polyline(
            &mut s,
            FULL,
            Rect::new(0, 0, 12, 12),
            &[(0.0, 0.0), (1.0, 1.0)],
            3,
            Color::WHITE,
            false,
            false,
        );
        for y in 0..64 {
            for x in 0..64 {
                assert!(!lit(&s, x, y) || (x < 12 && y < 12), "drew at ({x}, {y})");
            }
        }
    }

    #[test]
    fn a_polyline_in_an_empty_box_draws_nothing() {
        let mut s = surf();
        polyline(
            &mut s,
            Rect::new(5, 5, 0, 0),
            FULL,
            &[(0.0, 0.0), (1.0, 1.0)],
            1,
            Color::WHITE,
            false,
            false,
        );
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn out_of_range_and_nan_points_are_clamped_into_the_box() {
        let mut s = surf();
        let at = Rect::new(8, 8, 16, 16);
        polyline(
            &mut s,
            at,
            FULL,
            &[(-9.0, 40.0), (f32::NAN, f32::NAN), (2.0, -2.0)],
            1,
            Color::WHITE,
            true,
            false,
        );
        for y in 0..64 {
            for x in 0..64 {
                assert!(
                    !lit(&s, x, y) || (8..24).contains(&x) && (8..24).contains(&y),
                    "drew at ({x}, {y}), outside the box"
                );
            }
        }
    }

    // ---- chart ----

    #[test]
    fn a_flat_chart_draws_a_level_line() {
        let mut s = surf();
        chart(
            &mut s,
            FULL,
            FULL,
            &[0.5; 8],
            1,
            Color::WHITE,
            Color::TRANSPARENT,
            false,
        );
        let tops: alloc::vec::Vec<_> = (0..64).filter_map(|x| top_of(&s, x)).collect();
        assert!(tops.len() > 50, "the line should cross most columns");
        let first = tops[0];
        assert!(
            tops.iter().all(|&t| t.abs_diff(first) <= 1),
            "a constant series must not wander: {tops:?}"
        );
    }

    #[test]
    fn a_rising_chart_climbs_from_left_to_right() {
        let mut s = surf();
        chart(
            &mut s,
            FULL,
            FULL,
            &[0.0, 0.25, 0.5, 0.75, 1.0],
            1,
            Color::WHITE,
            Color::TRANSPARENT,
            false,
        );
        let left = top_of(&s, 2).expect("a left-hand column");
        let right = top_of(&s, 61).expect("a right-hand column");
        assert!(right < left, "a rising series draws higher on the right");
    }

    #[test]
    fn a_chart_value_of_one_reaches_the_top() {
        let mut s = surf();
        chart(
            &mut s,
            FULL,
            FULL,
            &[1.0, 1.0],
            1,
            Color::WHITE,
            Color::TRANSPARENT,
            false,
        );
        assert!((0..2).any(|y| lit(&s, 32, y)), "1.0 sits on the top edge");
    }

    #[test]
    fn a_chart_value_of_zero_sits_on_the_floor() {
        let mut s = surf();
        chart(
            &mut s,
            FULL,
            FULL,
            &[0.0, 0.0],
            1,
            Color::WHITE,
            Color::TRANSPARENT,
            false,
        );
        assert!(lit(&s, 32, 63), "0.0 sits on the bottom edge");
        assert!(!lit(&s, 32, 0));
    }

    #[test]
    fn a_filled_chart_paints_the_area_beneath_the_line() {
        let mut stroked = surf();
        let mut filled = surf();
        chart(
            &mut stroked,
            FULL,
            FULL,
            &[0.5; 4],
            1,
            Color::WHITE,
            Color::TRANSPARENT,
            false,
        );
        chart(
            &mut filled,
            FULL,
            FULL,
            &[0.5; 4],
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert!(
            count(&filled) > count(&stroked) * 4,
            "the fill dwarfs the line"
        );
        assert!(lit(&filled, 32, 62), "filled right down to the floor");
        assert!(!lit(&filled, 32, 2), "and not above the series");
    }

    #[test]
    fn a_chart_with_no_values_draws_nothing() {
        let mut s = surf();
        chart(
            &mut s,
            FULL,
            FULL,
            &[],
            2,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn a_chart_of_one_value_still_fills() {
        let mut s = surf();
        chart(
            &mut s,
            FULL,
            FULL,
            &[0.5],
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert!(count(&s) > 0, "one value is a flat area, not nothing");
    }

    #[test]
    fn a_chart_respects_the_clip() {
        let mut s = surf();
        chart(
            &mut s,
            FULL,
            Rect::new(0, 0, 16, 16),
            &[0.2, 0.9, 0.4, 0.7],
            2,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        for y in 0..64 {
            for x in 0..64 {
                assert!(!lit(&s, x, y) || (x < 16 && y < 16), "drew at ({x}, {y})");
            }
        }
    }

    #[test]
    fn a_chart_stays_inside_its_box() {
        let mut s = surf();
        let at = Rect::new(20, 20, 24, 24);
        chart(
            &mut s,
            at,
            FULL,
            &[0.0, 1.0, 0.0, 1.0],
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        for y in 0..64 {
            for x in 0..64 {
                assert!(
                    !lit(&s, x, y) || (20..44).contains(&x) && (20..44).contains(&y),
                    "drew at ({x}, {y}), outside the box"
                );
            }
        }
    }

    #[test]
    fn out_of_range_and_nan_values_are_clamped() {
        let mut s = surf();
        let at = Rect::new(8, 8, 20, 20);
        chart(
            &mut s,
            at,
            FULL,
            &[f32::NAN, 40.0, -3.0, f32::INFINITY],
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        for y in 0..64 {
            for x in 0..64 {
                assert!(
                    !lit(&s, x, y) || (8..28).contains(&x) && (8..28).contains(&y),
                    "drew at ({x}, {y}), outside the box"
                );
            }
        }
    }

    #[test]
    fn a_transparent_stroke_leaves_only_the_fill() {
        let mut s = surf();
        chart(
            &mut s,
            FULL,
            FULL,
            &[0.5; 4],
            1,
            Color::TRANSPARENT,
            Color::WHITE,
            false,
        );
        assert!(count(&s) > 0, "the fill still paints");
        assert!(!lit(&s, 32, 2), "and nothing above the series");
    }

    #[test]
    fn a_chart_with_a_long_series_terminates() {
        let mut s = surf();
        let many: alloc::vec::Vec<f32> = (0..4096).map(|i| (i % 17) as f32 / 16.0).collect();
        chart(
            &mut s,
            FULL,
            FULL,
            &many,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert!(count(&s) > 0);
    }
}

#[cfg(test)]
mod poly_tests {
    use super::*;
    use crate::{MemorySurface, PixelFormat, Size};

    const FULL: Rect = Rect::new(0, 0, 64, 64);

    fn surf() -> MemorySurface {
        MemorySurface::new(Size { w: 64, h: 64 }, PixelFormat::Bgrx8888)
    }

    fn lit(s: &MemorySurface, x: usize, y: usize) -> bool {
        let o = y * s.stride() + x * 4;
        s.pixels()[o..o + 4] != [0, 0, 0, 0]
    }

    fn count(s: &MemorySurface) -> usize {
        (0..64)
            .flat_map(|y| (0..64).map(move |x| (x, y)))
            .filter(|&(x, y)| lit(s, x, y))
            .count()
    }

    /// The whole box, as a square polygon.
    const SQUARE: [(f32, f32); 4] = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];

    #[test]
    fn a_square_polygon_fills_its_box() {
        let mut s = surf();
        polygon(&mut s, FULL, FULL, &SQUARE, Color::WHITE, false);
        // Allowing the boundary row and column: a scanline fill is inclusive
        // at one edge and exclusive at the other, and which is which is not
        // what this test is about.
        assert!(count(&s) > 60 * 60, "only filled {} pixels", count(&s));
        assert!(lit(&s, 32, 32), "the middle is empty");
    }

    #[test]
    fn a_triangle_is_solid_below_its_apex_and_empty_beside_it() {
        // The turn-signal shape: apex at the top middle, base along the bottom.
        let tri = [(0.5, 0.0), (1.0, 1.0), (0.0, 1.0)];
        let mut s = surf();
        polygon(&mut s, FULL, FULL, &tri, Color::WHITE, false);
        assert!(lit(&s, 32, 60), "the base should be filled");
        assert!(lit(&s, 32, 10), "under the apex should be filled");
        assert!(
            !lit(&s, 2, 2),
            "the top-left corner is outside the triangle"
        );
        assert!(!lit(&s, 61, 2), "the top-right corner is outside it");
        assert!(lit(&s, 2, 62), "the bottom-left corner is inside it");
    }

    #[test]
    fn a_triangle_covers_about_half_the_box() {
        let tri = [(0.5, 0.0), (1.0, 1.0), (0.0, 1.0)];
        let mut s = surf();
        polygon(&mut s, FULL, FULL, &tri, Color::WHITE, false);
        let n = count(&s);
        assert!((1400..2400).contains(&n), "a half of 4096 is not {n}");
    }

    #[test]
    fn a_left_arrow_points_left() {
        // The Z31's turn signal: apex on the left, base on the right.
        let arrow = [(0.0, 0.5), (1.0, 0.0), (1.0, 1.0)];
        let mut s = surf();
        polygon(&mut s, FULL, FULL, &arrow, Color::WHITE, false);
        // The apex row, not the box's middle row: a fraction of 0.5 across 64
        // pixels lands on 31, and by row 32 the shape has already narrowed.
        assert!(
            (28..36).any(|y| lit(&s, 1, y)),
            "the point should reach the left edge"
        );
        assert!(!lit(&s, 2, 2), "and nothing above it");
        assert!(!lit(&s, 2, 61), "or below it");
        assert!(lit(&s, 61, 2), "the base spans the right edge");
        assert!(lit(&s, 61, 61));
    }

    #[test]
    fn winding_order_does_not_change_the_shape() {
        let tri = [(0.5, 0.0), (1.0, 1.0), (0.0, 1.0)];
        let reversed = [(0.0, 1.0), (1.0, 1.0), (0.5, 0.0)];
        let mut a = surf();
        let mut b = surf();
        polygon(&mut a, FULL, FULL, &tri, Color::WHITE, false);
        polygon(&mut b, FULL, FULL, &reversed, Color::WHITE, false);
        assert_eq!(a.pixels(), b.pixels());
    }

    #[test]
    fn a_concave_shape_keeps_its_notch() {
        // An arrowhead with a notched tail: the notch must stay empty, which
        // is the whole reason for the even-odd rule.
        let chevron = [(0.5, 0.0), (1.0, 1.0), (0.5, 0.7), (0.0, 1.0)];
        let mut s = surf();
        polygon(&mut s, FULL, FULL, &chevron, Color::WHITE, false);
        assert!(lit(&s, 32, 20), "the body should be filled");
        assert!(!lit(&s, 32, 62), "the notch should be empty");
        // The tails are thin this far down; ask whether they exist at all
        // rather than guessing how wide they are.
        assert!(
            (0..12).any(|x| lit(&s, x, 60)),
            "the left tail should be filled"
        );
        assert!(
            (52..64).any(|x| lit(&s, x, 60)),
            "the right tail should be filled"
        );
    }

    #[test]
    fn fewer_than_three_points_draws_nothing() {
        let mut s = surf();
        polygon(&mut s, FULL, FULL, &[], Color::WHITE, false);
        polygon(&mut s, FULL, FULL, &[(0.0, 0.0)], Color::WHITE, false);
        polygon(
            &mut s,
            FULL,
            FULL,
            &[(0.0, 0.0), (1.0, 1.0)],
            Color::WHITE,
            false,
        );
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn a_transparent_colour_draws_nothing() {
        let mut s = surf();
        polygon(&mut s, FULL, FULL, &SQUARE, Color::TRANSPARENT, false);
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn a_polygon_stays_inside_its_box() {
        let mut s = surf();
        let at = Rect::new(10, 10, 20, 20);
        polygon(&mut s, at, FULL, &SQUARE, Color::WHITE, false);
        for y in 0..64 {
            for x in 0..64 {
                assert!(
                    !lit(&s, x, y) || (10..30).contains(&x) && (10..30).contains(&y),
                    "drew at ({x}, {y}), outside the box"
                );
            }
        }
    }

    #[test]
    fn a_polygon_respects_the_clip() {
        let mut s = surf();
        polygon(
            &mut s,
            FULL,
            Rect::new(0, 0, 16, 16),
            &SQUARE,
            Color::WHITE,
            false,
        );
        for y in 0..64 {
            for x in 0..64 {
                assert!(!lit(&s, x, y) || (x < 16 && y < 16), "drew at ({x}, {y})");
            }
        }
    }

    #[test]
    fn a_partial_repaint_matches_a_full_one() {
        let tri = [(0.5, 0.0), (1.0, 1.0), (0.0, 1.0)];
        let mut full = surf();
        polygon(&mut full, FULL, FULL, &tri, Color::WHITE, false);
        let mut partial = surf();
        for band in [
            Rect::new(0, 0, 64, 21),
            Rect::new(0, 21, 64, 22),
            Rect::new(0, 43, 64, 21),
        ] {
            polygon(&mut partial, FULL, band, &tri, Color::WHITE, false);
        }
        assert_eq!(partial.pixels(), full.pixels());
    }

    #[test]
    fn an_empty_box_draws_nothing() {
        let mut s = surf();
        polygon(
            &mut s,
            Rect::new(5, 5, 0, 0),
            FULL,
            &SQUARE,
            Color::WHITE,
            false,
        );
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn a_degenerate_polygon_draws_nothing_and_does_not_panic() {
        let mut s = surf();
        // All three points on one horizontal line: no area to fill.
        polygon(
            &mut s,
            FULL,
            FULL,
            &[(0.0, 0.5), (0.5, 0.5), (1.0, 0.5)],
            Color::WHITE,
            false,
        );
        // All three the same point.
        polygon(
            &mut s,
            FULL,
            FULL,
            &[(0.3, 0.3), (0.3, 0.3), (0.3, 0.3)],
            Color::WHITE,
            false,
        );
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn out_of_range_and_nan_vertices_are_clamped_into_the_box() {
        let mut s = surf();
        let at = Rect::new(8, 8, 16, 16);
        polygon(
            &mut s,
            at,
            FULL,
            &[(-9.0, 40.0), (f32::NAN, 2.0), (5.0, -3.0), (0.5, f32::NAN)],
            Color::WHITE,
            false,
        );
        for y in 0..64 {
            for x in 0..64 {
                assert!(
                    !lit(&s, x, y) || (8..24).contains(&x) && (8..24).contains(&y),
                    "drew at ({x}, {y}), outside the box"
                );
            }
        }
    }

    #[test]
    fn a_polygon_with_more_vertices_than_the_crossing_bound_terminates() {
        let mut s = surf();
        // A star-ish shape with far more crossings than the fixed array holds.
        let many: alloc::vec::Vec<(f32, f32)> = (0..400)
            .map(|i| {
                let t = i as f32 / 400.0;
                if i % 2 == 0 { (t, 0.1) } else { (t, 0.9) }
            })
            .collect();
        polygon(&mut s, FULL, FULL, &many, Color::WHITE, false);
        assert!(count(&s) > 0, "drew nothing at all");
    }
}
