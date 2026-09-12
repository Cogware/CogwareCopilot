// SPDX-License-Identifier: GPL-3.0-only
//! A ruled grid, the way a panel's glass is.
//!
//! Not a drawing aid. The instrument this crate was written for has a fine
//! grid etched across its face, and at night it is one of the things that
//! makes the panel look like itself rather than like a screen showing a
//! picture of it.
//!
//! Lines are placed from the widget's own origin rather than from the damage
//! rectangle, so a partial repaint puts them back exactly where the last one
//! had them.

use crate::{Color, Rect, Surface};

/// The most lines drawn on either axis.
///
/// A pitch of one on a large panel is thousands of lines and a grey wash; the
/// cap keeps a mistyped pitch from costing a frame rather than trying to
/// render something nobody can read anyway.
const MAX_LINES: i32 = 4096;

/// Rule `at` with lines every `pitch_x` across and `pitch_y` down.
///
/// A pitch of zero on an axis leaves that axis unruled, which is how a scene
/// asks for lines one way only.
pub fn grid<S: Surface + ?Sized>(
    surface: &mut S,
    at: Rect,
    clip: Rect,
    pitch_x: u32,
    pitch_y: u32,
    width: u32,
    color: Color,
) {
    let Some(area) = at.intersection(clip) else {
        return;
    };
    if area.is_empty() || color.is_transparent() {
        return;
    }
    let w = width.max(1) as i32;

    if pitch_x > 0 {
        let step = pitch_x as i32;
        let mut n = 0;
        let mut x = at.left();
        while x < at.right() && n < MAX_LINES {
            // Only the lines the damage rectangle actually covers are drawn,
            // but every line's position comes from `at`, so which ones are
            // dirty never changes where any of them sit.
            let line = Rect::new(x, area.top(), w as u32, area.size.h);
            if let Some(r) = line.intersection(area)
                && !r.is_empty()
            {
                surface_fill(surface, r, color);
            }
            x = x.saturating_add(step);
            n += 1;
        }
    }

    if pitch_y > 0 {
        let step = pitch_y as i32;
        let mut n = 0;
        let mut y = at.top();
        while y < at.bottom() && n < MAX_LINES {
            let line = Rect::new(area.left(), y, area.size.w, w as u32);
            if let Some(r) = line.intersection(area)
                && !r.is_empty()
            {
                surface_fill(surface, r, color);
            }
            y = y.saturating_add(step);
            n += 1;
        }
    }
}

/// [`super::fill_rect`] under a name that does not shadow the local `grid`.
fn surface_fill<S: Surface + ?Sized>(surface: &mut S, r: Rect, color: Color) {
    super::fill_rect(surface, r, color);
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

    #[test]
    fn a_grid_rules_both_ways() {
        let mut s = surf();
        grid(&mut s, FULL, FULL, 8, 8, 1, Color::WHITE);
        assert!(lit(&s, 0, 3), "no vertical line at the origin");
        assert!(lit(&s, 3, 0), "no horizontal line at the origin");
        assert!(lit(&s, 8, 3), "no vertical line at the first pitch");
        assert!(!lit(&s, 3, 3), "the space between lines was painted");
    }

    #[test]
    fn a_pitch_of_zero_leaves_that_axis_alone() {
        let mut s = surf();
        grid(&mut s, FULL, FULL, 8, 0, 1, Color::WHITE);
        assert!(lit(&s, 8, 30), "the vertical lines went missing");
        assert!(!lit(&s, 3, 8), "a horizontal line appeared anyway");
    }

    #[test]
    fn no_pitch_at_all_draws_nothing() {
        let mut s = surf();
        grid(&mut s, FULL, FULL, 0, 0, 1, Color::WHITE);
        assert!((0..64).all(|y| (0..64).all(|x| !lit(&s, x, y))));
    }

    #[test]
    fn a_wider_line_covers_more() {
        let mut thin = surf();
        let mut thick = surf();
        grid(&mut thin, FULL, FULL, 16, 0, 1, Color::WHITE);
        grid(&mut thick, FULL, FULL, 16, 0, 3, Color::WHITE);
        let count = |s: &MemorySurface| {
            (0..64)
                .flat_map(|y| (0..64).map(move |x| (x, y)))
                .filter(|&(x, y)| lit(s, x, y))
                .count()
        };
        assert!(count(&thick) > count(&thin));
    }

    #[test]
    fn the_lines_start_at_the_widget_not_the_screen() {
        let mut s = surf();
        grid(
            &mut s,
            Rect::new(10, 10, 32, 32),
            FULL,
            8,
            8,
            1,
            Color::WHITE,
        );
        assert!(lit(&s, 10, 20), "no line at the widget's own left edge");
        assert!(
            !lit(&s, 8, 20),
            "a line landed on the screen's grid instead"
        );
    }

    #[test]
    fn a_grid_stays_inside_its_box() {
        let mut s = surf();
        let at = Rect::new(10, 10, 20, 20);
        grid(&mut s, at, FULL, 4, 4, 2, Color::WHITE);
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
    fn a_partial_repaint_matches_a_full_one() {
        // The reason line positions come from `at` and not from the damage.
        let mut full = surf();
        grid(&mut full, FULL, FULL, 7, 5, 1, Color::WHITE);
        let mut partial = surf();
        for band in [
            Rect::new(0, 0, 64, 23),
            Rect::new(0, 23, 64, 18),
            Rect::new(0, 41, 64, 23),
        ] {
            grid(&mut partial, FULL, band, 7, 5, 1, Color::WHITE);
        }
        assert_eq!(partial.pixels(), full.pixels());
    }

    #[test]
    fn a_transparent_colour_draws_nothing() {
        let mut s = surf();
        grid(&mut s, FULL, FULL, 4, 4, 1, Color::TRANSPARENT);
        assert!((0..64).all(|y| (0..64).all(|x| !lit(&s, x, y))));
    }

    #[test]
    fn an_empty_box_draws_nothing() {
        let mut s = surf();
        grid(&mut s, Rect::new(5, 5, 0, 0), FULL, 4, 4, 1, Color::WHITE);
        assert!((0..64).all(|y| (0..64).all(|x| !lit(&s, x, y))));
    }

    #[test]
    fn a_pitch_of_one_on_an_enormous_box_terminates() {
        let mut s = surf();
        grid(
            &mut s,
            Rect::new(-1000, -1000, u32::MAX, u32::MAX),
            FULL,
            1,
            1,
            1,
            Color::WHITE,
        );
    }
}
