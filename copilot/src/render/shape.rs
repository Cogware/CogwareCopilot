// SPDX-License-Identifier: GPL-3.0-only
//! Lines, circles and discs.
//!
//! All integer: no float maths anywhere, because the core crate has none to
//! reach for. Bresenham for lines, midpoint for circles, and a square root
//! per row for discs.
//!
//! Every one of these clips before it writes rather than relying on the
//! surface to reject what falls outside. A widget drawn during a partial
//! repaint has to stay inside its damage rectangle, which is a tighter
//! bound than the surface.

use crate::trig::isqrt;
use crate::{Color, Point, Rect, Surface};

use super::fill_rect;

/// The largest radius `circle` and `disc` will honour.
///
/// Two reasons, and both are hard limits rather than taste. `disc` squares the
/// radius, and 32767² is the last square that fits in an `i32`. And both walk
/// the radius one step at a time, so an unclamped `i32::MAX` is a two-billion
/// iteration loop that never finishes. Nothing this crate can draw on is
/// remotely this large, so a bigger radius is a caller's arithmetic slip and
/// clamping it keeps the frame rendering.
const MAX_RADIUS: i32 = 32_767;

/// Draw a `width`-pixel line from `a` to `b`.
///
/// Integer Bresenham, plotting a `width`-square at each step. A `width` below
/// one still draws a single-pixel line.
pub fn line<S: Surface + ?Sized>(
    surface: &mut S,
    a: Point,
    b: Point,
    width: u32,
    color: Color,
    clip: Rect,
) {
    // i64 throughout the error term: a caller may hand us coordinates a whole
    // i32 apart, and both the difference and the doubling below overflow i32
    // long before the line leaves the clip.
    let dx = (i64::from(b.x) - i64::from(a.x)).abs();
    let dy = -(i64::from(b.y) - i64::from(a.y)).abs();
    let sx = if a.x < b.x { 1 } else { -1 };
    let sy = if a.y < b.y { 1 } else { -1 };
    let mut err = dx + dy;
    let mut x = a.x;
    let mut y = a.y;
    let w = width.max(1);
    let half = (w / 2) as i32;

    // Bound iterations to guarantee termination on pathological input.
    for _ in 0..1_000_000 {
        // Plot a width×width square centred on (x, y), clipped.
        let r = Rect::new(x.saturating_sub(half), y.saturating_sub(half), w, w);
        if let Some(v) = r.intersection(clip) {
            fill_rect(surface, v, color);
        }

        if x == b.x && y == b.y {
            break;
        }

        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x = x.saturating_add(sx);
        }
        if e2 <= dx {
            err += dx;
            y = y.saturating_add(sy);
        }
    }
}

/// Draw the outline of a circle centred on `c` with radius `r`.
///
/// Integer midpoint circle, plotting a `width`-square at each of the eight
/// symmetric points. A radius of zero or less draws nothing.
pub fn circle<S: Surface + ?Sized>(
    surface: &mut S,
    c: Point,
    r: i32,
    width: u32,
    color: Color,
    clip: Rect,
) {
    if r <= 0 {
        return;
    }
    let r = r.min(MAX_RADIUS);

    let w = width.max(1);
    let half = (w / 2) as i32;

    let mut x = r;
    let mut y = 0;
    let mut err = 0;

    while x >= y {
        let points = [
            (c.x.saturating_add(x), c.y.saturating_add(y)),
            (c.x.saturating_add(y), c.y.saturating_add(x)),
            (c.x.saturating_sub(y), c.y.saturating_add(x)),
            (c.x.saturating_sub(x), c.y.saturating_add(y)),
            (c.x.saturating_sub(x), c.y.saturating_sub(y)),
            (c.x.saturating_sub(y), c.y.saturating_sub(x)),
            (c.x.saturating_add(y), c.y.saturating_sub(x)),
            (c.x.saturating_add(x), c.y.saturating_sub(y)),
        ];

        for (px, py) in points {
            let r = Rect::new(px.saturating_sub(half), py.saturating_sub(half), w, w);
            if let Some(v) = r.intersection(clip) {
                fill_rect(surface, v, color);
            }
        }

        y += 1;
        if err <= 0 {
            err += 2 * y + 1;
        } else {
            x -= 1;
            err += 2 * (y - x) + 1;
        }
    }
}

/// Fill a disc centred on `c` with radius `r`.
///
/// One span per row, the half-width from an integer square root. A radius of
/// zero or less draws nothing.
pub fn disc<S: Surface + ?Sized>(surface: &mut S, c: Point, r: i32, color: Color, clip: Rect) {
    if r <= 0 {
        return;
    }
    let r = r.min(MAX_RADIUS);

    let r2 = r * r;

    for dy in -r..=r {
        let half_w = isqrt(r2 - dy * dy);
        let row = Rect::new(
            c.x.saturating_sub(half_w),
            c.y.saturating_add(dy),
            (2 * half_w + 1) as u32,
            1,
        );
        if let Some(v) = row.intersection(clip) {
            fill_rect(surface, v, color);
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

    fn at(x: i32, y: i32) -> Point {
        Point { x, y }
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

    // --- line ---

    #[test]
    fn a_horizontal_line_connects_its_endpoints() {
        let mut s = surf();
        line(&mut s, at(10, 20), at(30, 20), 1, Color::WHITE, FULL);
        for x in 10..=30 {
            assert!(lit(&s, x, 20), "gap at {x}");
        }
        assert!(!lit(&s, 9, 20) && !lit(&s, 31, 20), "overshot");
    }

    #[test]
    fn a_vertical_line_connects_its_endpoints() {
        let mut s = surf();
        line(&mut s, at(20, 10), at(20, 30), 1, Color::WHITE, FULL);
        for y in 10..=30 {
            assert!(lit(&s, 20, y), "gap at {y}");
        }
    }

    #[test]
    fn a_diagonal_has_no_gaps() {
        // The property that separates a line from a dotted line: every step
        // must touch the previous one.
        let mut s = surf();
        line(&mut s, at(5, 5), at(40, 25), 1, Color::WHITE, FULL);
        let mut prev: Option<(usize, usize)> = None;
        for x in 5..=40 {
            let y = (5..=25).find(|&y| lit(&s, x, y));
            let y = y.unwrap_or_else(|| panic!("column {x} is empty"));
            if let Some((px, py)) = prev {
                assert!(
                    x.abs_diff(px) <= 1 && y.abs_diff(py) <= 1,
                    "jump from {px},{py} to {x},{y}"
                );
            }
            prev = Some((x, y));
        }
    }

    #[test]
    fn a_line_is_drawn_the_same_in_both_directions() {
        let mut a = surf();
        let mut b = surf();
        line(&mut a, at(3, 7), at(29, 41), 1, Color::WHITE, FULL);
        line(&mut b, at(29, 41), at(3, 7), 1, Color::WHITE, FULL);
        // Bresenham is not symmetric in general, but the pixel *count* must
        // match or one direction is dropping steps the other keeps.
        assert_eq!(count(&a), count(&b));
    }

    #[test]
    fn a_single_point_line_draws_something() {
        let mut s = surf();
        line(&mut s, at(10, 10), at(10, 10), 1, Color::WHITE, FULL);
        assert!(lit(&s, 10, 10));
    }

    #[test]
    fn a_thick_line_is_wider_than_a_thin_one() {
        let mut thin = surf();
        let mut thick = surf();
        line(&mut thin, at(10, 32), at(50, 32), 1, Color::WHITE, FULL);
        line(&mut thick, at(10, 32), at(50, 32), 5, Color::WHITE, FULL);
        assert!(count(&thick) > count(&thin) * 3);
    }

    #[test]
    fn a_line_outside_the_clip_draws_nothing() {
        let mut s = surf();
        line(
            &mut s,
            at(0, 0),
            at(60, 60),
            1,
            Color::WHITE,
            Rect::new(0, 40, 10, 5),
        );
        for y in 0..64 {
            for x in 0..64 {
                if lit(&s, x, y) {
                    assert!(
                        (40..45).contains(&y) && x < 10,
                        "ink escaped the clip at {x},{y}"
                    );
                }
            }
        }
    }

    #[test]
    fn an_enormous_line_terminates_and_does_not_panic() {
        let mut s = surf();
        line(
            &mut s,
            at(i32::MIN / 2, 0),
            at(i32::MAX / 2, 1),
            1,
            Color::WHITE,
            FULL,
        );
    }

    // --- circle ---

    #[test]
    fn a_circle_touches_its_four_cardinal_points() {
        let mut s = surf();
        circle(&mut s, at(32, 32), 10, 1, Color::WHITE, FULL);
        assert!(lit(&s, 42, 32), "east");
        assert!(lit(&s, 22, 32), "west");
        assert!(lit(&s, 32, 42), "south");
        assert!(lit(&s, 32, 22), "north");
    }

    #[test]
    fn a_circle_is_hollow() {
        let mut s = surf();
        circle(&mut s, at(32, 32), 12, 1, Color::WHITE, FULL);
        assert!(!lit(&s, 32, 32), "the centre should be clear");
    }

    #[test]
    fn a_circle_is_symmetric_about_both_axes() {
        let mut s = surf();
        circle(&mut s, at(32, 32), 14, 1, Color::WHITE, FULL);
        for d in 1..14usize {
            assert_eq!(lit(&s, 32 + d, 32), lit(&s, 32 - d, 32), "x at {d}");
            assert_eq!(lit(&s, 32, 32 + d), lit(&s, 32, 32 - d), "y at {d}");
        }
    }

    #[test]
    fn a_zero_or_negative_radius_draws_nothing() {
        let mut s = surf();
        circle(&mut s, at(32, 32), 0, 1, Color::WHITE, FULL);
        circle(&mut s, at(32, 32), -5, 1, Color::WHITE, FULL);
        assert_eq!(count(&s), 0);
    }

    // --- disc ---

    #[test]
    fn a_disc_is_solid() {
        let mut s = surf();
        disc(&mut s, at(32, 32), 10, Color::WHITE, FULL);
        assert!(lit(&s, 32, 32), "the centre must be filled");
        assert!(lit(&s, 38, 32));
        assert!(!lit(&s, 45, 32), "outside the radius");
    }

    #[test]
    fn a_disc_covers_roughly_pi_r_squared() {
        // A disc that is a square, or half a disc, both pass a "centre is
        // filled" check; the area is what pins the shape.
        let mut s = surf();
        let r = 15i32;
        disc(&mut s, at(32, 32), r, Color::WHITE, FULL);
        let want = (core::f64::consts::PI * (r * r) as f64) as usize;
        let got = count(&s);
        assert!(
            got > want * 9 / 10 && got < want * 11 / 10,
            "area {got}, expected about {want}"
        );
    }

    #[test]
    fn a_disc_respects_the_clip() {
        let mut s = surf();
        disc(
            &mut s,
            at(32, 32),
            20,
            Color::WHITE,
            Rect::new(32, 32, 8, 8),
        );
        for y in 0..64 {
            for x in 0..64 {
                if lit(&s, x, y) {
                    assert!(
                        (32..40).contains(&x) && (32..40).contains(&y),
                        "ink at {x},{y} escaped the clip"
                    );
                }
            }
        }
    }

    #[test]
    fn a_disc_at_the_edge_does_not_wrap() {
        // Clipping by wrapping rather than by intersection puts the left of
        // the disc on the right of the surface, which reads as corruption.
        let mut s = surf();
        disc(&mut s, at(0, 32), 10, Color::WHITE, FULL);
        for y in 0..64 {
            assert!(!lit(&s, 63, y), "wrapped onto the right edge at row {y}");
        }
    }

    #[test]
    fn an_enormous_circle_terminates_and_does_not_panic() {
        let mut s = surf();
        circle(&mut s, at(0, 0), i32::MAX, 1, Color::WHITE, FULL);
        circle(&mut s, at(-3, 40), i32::MAX / 2, 3, Color::WHITE, FULL);
    }

    #[test]
    fn an_enormous_disc_terminates_and_does_not_panic() {
        let mut s = surf();
        disc(&mut s, at(0, 0), i32::MAX, Color::WHITE, FULL);
        disc(&mut s, at(32, 32), i32::MAX / 2, Color::WHITE, FULL);
    }

    #[test]
    fn a_disc_larger_than_the_clip_fills_it_completely() {
        // The radius cap must not turn a covering disc into a partial one.
        let mut s = surf();
        disc(&mut s, at(32, 32), 30_000, Color::WHITE, FULL);
        assert_eq!(count(&s), 64 * 64);
    }
}
