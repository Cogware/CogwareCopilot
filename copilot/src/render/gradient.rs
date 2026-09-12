// SPDX-License-Identifier: GPL-3.0-only
//! A fill that changes colour across the widget.
//!
//! One span per row (or column) rather than a per-pixel blend: the axis a
//! gradient runs along is the only one its colour varies on, so every pixel in
//! the perpendicular direction shares a value and can be written in one go.
//! That keeps the cost the same as a plain fill plus one interpolation per
//! line, which is what makes it affordable on a Pi with no blitter.

use crate::{Color, Rect, Surface};

/// Blend `a` towards `b` by `num/den`, staying in eight bits per channel.
///
/// Integer throughout: the core crate has no floats to reach for, and a
/// gradient down a 1080-pixel panel needs a thousand of these per frame.
fn mix(a: Color, b: Color, num: i32, den: i32) -> Color {
    let den = den.max(1);
    let num = num.clamp(0, den);
    let lerp = |x: u8, y: u8| -> u8 {
        let x = i32::from(x);
        let y = i32::from(y);
        // Rounded rather than truncated: over a long ramp, truncation biases
        // every step towards the starting colour and the far end never quite
        // arrives.
        ((x * (den - num) + y * num + den / 2) / den) as u8
    };
    Color {
        r: lerp(a.r, b.r),
        g: lerp(a.g, b.g),
        b: lerp(a.b, b.b),
        a: lerp(a.a, b.a),
    }
}

/// Fill `at` with a ramp from `from` to `to`, clipped to `clip`.
///
/// `vertical` runs the ramp top to bottom; otherwise it runs left to right.
pub fn gradient<S: Surface + ?Sized>(
    surface: &mut S,
    at: Rect,
    clip: Rect,
    from: Color,
    to: Color,
    vertical: bool,
) {
    // Against the surface as well as the damage rectangle: the spans below go
    // straight to `fill_span`, so this is the only place they are clipped.
    let Some(area) = at
        .intersection(clip)
        .and_then(|a| crate::render::clip(surface, a))
    else {
        return;
    };
    if area.is_empty() {
        return;
    }
    // Both rectangles: `at` fixes the ramp, `area` fixes what may be painted.
    if surface.draw_gradient(at, area, from, to, vertical) {
        return;
    }

    // The ramp is measured across the whole widget, not across the damaged
    // part of it. A partial repaint has to produce the same pixels as a full
    // one, and a gradient recomputed over the damage rectangle would restart
    // its ramp at the edge of whatever happened to be dirty.
    if vertical {
        let span = (at.size.h as i32 - 1).max(1);
        for y in area.top()..area.bottom() {
            let c = mix(from, to, y - at.top(), span);
            if !c.is_transparent() {
                surface.fill_span(area.left(), y, area.size.w, c);
            }
        }
    } else {
        let span = (at.size.w as i32 - 1).max(1);
        for x in area.left()..area.right() {
            let c = mix(from, to, x - at.left(), span);
            if !c.is_transparent() {
                for y in area.top()..area.bottom() {
                    surface.fill_span(x, y, 1, c);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemorySurface, PixelFormat, Size};

    const FULL: Rect = Rect::new(0, 0, 32, 32);

    fn surf() -> MemorySurface {
        MemorySurface::new(Size { w: 32, h: 32 }, PixelFormat::Bgrx8888)
    }

    /// The (b, g, r) of one pixel.
    fn px(s: &MemorySurface, x: usize, y: usize) -> [u8; 3] {
        let o = y * s.stride() + x * 4;
        [s.pixels()[o], s.pixels()[o + 1], s.pixels()[o + 2]]
    }

    const BLACK: Color = Color::rgb(0, 0, 0);
    const WHITE: Color = Color::rgb(255, 255, 255);

    #[test]
    fn a_vertical_ramp_starts_and_ends_on_its_colours() {
        let mut s = surf();
        gradient(&mut s, FULL, FULL, BLACK, WHITE, true);
        assert_eq!(px(&s, 16, 0), [0, 0, 0], "the top is not the start colour");
        assert_eq!(px(&s, 16, 31), [255, 255, 255], "the bottom is not the end");
    }

    #[test]
    fn a_horizontal_ramp_starts_and_ends_on_its_colours() {
        let mut s = surf();
        gradient(&mut s, FULL, FULL, BLACK, WHITE, false);
        assert_eq!(px(&s, 0, 16), [0, 0, 0]);
        assert_eq!(px(&s, 31, 16), [255, 255, 255]);
    }

    #[test]
    fn a_ramp_only_varies_along_its_own_axis() {
        let mut s = surf();
        gradient(&mut s, FULL, FULL, BLACK, WHITE, true);
        for x in 0..32 {
            assert_eq!(px(&s, x, 7), px(&s, 0, 7), "row 7 varied across x");
        }
    }

    #[test]
    fn a_ramp_never_goes_backwards() {
        let mut s = surf();
        gradient(&mut s, FULL, FULL, BLACK, WHITE, true);
        for y in 1..32 {
            assert!(
                px(&s, 0, y)[0] >= px(&s, 0, y - 1)[0],
                "row {y} is darker than the one above it"
            );
        }
    }

    #[test]
    fn a_ramp_reaches_the_middle_at_the_middle() {
        // Rounded rather than truncated: over a long ramp truncation biases
        // every step towards the start and the far end never quite arrives.
        let mut s = surf();
        gradient(&mut s, FULL, FULL, BLACK, WHITE, true);
        let mid = px(&s, 0, 16)[0];
        assert!((110..=145).contains(&mid), "the midpoint was {mid}");
    }

    #[test]
    fn a_partial_repaint_matches_a_full_one() {
        // The ramp must be measured across the widget, not across the damage.
        let mut full = surf();
        gradient(&mut full, FULL, FULL, BLACK, WHITE, true);
        let mut partial = surf();
        for band in [
            Rect::new(0, 0, 32, 7),
            Rect::new(0, 7, 32, 9),
            Rect::new(0, 16, 32, 16),
        ] {
            gradient(&mut partial, FULL, band, BLACK, WHITE, true);
        }
        assert_eq!(partial.pixels(), full.pixels());
    }

    #[test]
    fn a_ramp_stays_inside_its_box() {
        let mut s = surf();
        let at = Rect::new(8, 8, 10, 10);
        gradient(&mut s, at, FULL, WHITE, WHITE, true);
        for y in 0..32 {
            for x in 0..32 {
                let inside = (8..18).contains(&x) && (8..18).contains(&y);
                assert_eq!(px(&s, x, y) != [0, 0, 0], inside, "({x}, {y}) is wrong");
            }
        }
    }

    #[test]
    fn a_one_pixel_ramp_does_not_divide_by_zero() {
        let mut s = surf();
        gradient(&mut s, Rect::new(0, 0, 1, 1), FULL, BLACK, WHITE, true);
        gradient(&mut s, Rect::new(2, 2, 1, 1), FULL, BLACK, WHITE, false);
    }

    #[test]
    fn an_empty_box_draws_nothing() {
        let mut s = surf();
        gradient(&mut s, Rect::new(4, 4, 0, 0), FULL, WHITE, WHITE, true);
        gradient(
            &mut s,
            Rect::new(4, 4, 8, 8),
            Rect::new(20, 20, 4, 4),
            WHITE,
            WHITE,
            true,
        );
        assert!((0..32).all(|y| (0..32).all(|x| px(&s, x, y) == [0, 0, 0])));
    }

    #[test]
    fn two_identical_colours_are_a_flat_fill() {
        let mut s = surf();
        let grey = Color::rgb(90, 90, 90);
        gradient(&mut s, FULL, FULL, grey, grey, true);
        for y in 0..32 {
            assert_eq!(px(&s, 5, y), [90, 90, 90], "row {y} drifted");
        }
    }

    #[test]
    fn a_transparent_end_leaves_that_end_unpainted() {
        let mut s = surf();
        gradient(&mut s, FULL, FULL, Color::TRANSPARENT, WHITE, true);
        assert_eq!(px(&s, 0, 0), [0, 0, 0], "the transparent end painted");
        assert_eq!(px(&s, 0, 31), [255, 255, 255]);
    }

    #[test]
    fn an_enormous_box_does_not_overflow() {
        let mut s = surf();
        gradient(
            &mut s,
            Rect::new(-100, -100, u32::MAX, u32::MAX),
            FULL,
            BLACK,
            WHITE,
            true,
        );
        gradient(
            &mut s,
            Rect::new(i32::MIN / 2, 0, u32::MAX, 32),
            FULL,
            BLACK,
            WHITE,
            false,
        );
    }
}
