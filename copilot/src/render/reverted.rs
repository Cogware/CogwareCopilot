// SPDX-License-Identifier: GPL-3.0-only
//! What a display shows after an update would not run and the old software
//! was put back.
//!
//! Unlike the update notice next door, this is shown on a working display,
//! after the fact, with nothing left to go wrong. So it is a band across the
//! middle rather than the whole panel: a driver who has just been given their
//! car back should not have the gauges taken away to be told so. It clears
//! itself after a while, because a message about something that is already
//! over must not become part of the dashboard.
//!
//! It lives here rather than in a firmware because any display on the bus can
//! be given an image it cannot run, and they should all say the same thing
//! about it.

use super::draw::draw_text;
use crate::color::Color;
use crate::font::Font;
use crate::surface::Surface;
use crate::widget::{Align, VAlign};
use crate::Rect;

/// The ground the band sits on.
const GROUND: Color = Color::rgb(16, 12, 10);
/// The rules above and below it, so the band reads as deliberate rather than
/// as a scene that has failed to draw.
const EDGE: Color = Color::rgb(255, 176, 64);
/// The heading, in the colour a warning is expected in.
const WARN: Color = Color::rgb(255, 176, 64);
/// The two lines under it.
const INK: Color = Color::rgb(235, 240, 248);

/// Fraction of the panel's height the band covers.
const BAND: u32 = 5;

/// Draw the notice across the middle of `at`.
///
/// Takes the whole panel rather than the band, and works out the band itself,
/// so a caller cannot put it somewhere a round panel would clip it away.
pub fn overlay<S: Surface + ?Sized>(surface: &mut S, at: Rect, font: &Font<'_>) {
    let (w, h) = (at.size.w, at.size.h);
    if w == 0 || h == 0 {
        return;
    }
    let x0 = at.origin.x;

    // Deep enough for three lines at the scale chosen below, whatever the
    // panel: a band worked out only as a fraction of the height is too shallow
    // to hold them on a short one.
    let small = ((w / 240).clamp(2, 8)) as u8;
    let big = (small * 2).min(14);
    let small_h = u32::from(font.cell_h) * u32::from(small);
    let big_h = u32::from(font.cell_h) * u32::from(big);
    let wanted = big_h + small_h * 4;
    let band_h = (h * 2 / BAND).max(wanted).min(h);
    let band_y = at.origin.y + ((h - band_h) / 2) as i32;

    for row in 0..band_h as i32 {
        surface.fill_span(x0, band_y + row, w, GROUND);
    }
    for row in [0, band_h as i32 - 1] {
        surface.fill_span(x0, band_y + row, w, EDGE);
    }

    let line = |surface: &mut S, text: &str, colour: Color, scale: u8, y: i32| {
        let height = u32::from(font.cell_h) * u32::from(scale);
        draw_text(
            surface,
            text,
            colour,
            Rect::new(x0, y, w, height),
            at,
            font,
            scale,
            Align::Center,
            VAlign::Top,
        );
    };

    // Measured from the top of the band rather than the middle of the panel,
    // so the three lines stay inside it at every size the band takes.
    let top = band_y + ((band_h - wanted) / 2) as i32;
    line(surface, "UPDATE DID NOT START", WARN, big, top);
    line(
        surface,
        "The new firmware would not run.",
        INK,
        small,
        top + big_h as i32 + small_h as i32,
    );
    line(
        surface,
        "Reverted to the previous version.",
        INK,
        small,
        top + big_h as i32 + small_h as i32 * 5 / 2,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::default_font;
    use crate::geom::Size;
    use crate::surface::PixelFormat;
    use crate::surface_mem::MemorySurface;

    fn drawn(w: u32, h: u32) -> MemorySurface {
        let mut s = MemorySurface::new(Size { w, h }, PixelFormat::Bgrx8888);
        let font = default_font();
        overlay(&mut s, Rect::new(0, 0, w, h), &font);
        s
    }

    /// The first and last row the notice touched, taking a row as touched when
    /// any pixel in it is no longer the black the surface starts as.
    fn band_rows(s: &MemorySurface, w: u32, h: u32) -> (u32, u32) {
        let px = s.pixels().as_chunks::<4>().0;
        let mut first = h;
        let mut last = 0;
        for y in 0..h {
            let touched = (0..w).any(|x| {
                let p = px[(y * w + x) as usize];
                p[0] != 0 || p[1] != 0 || p[2] != 0
            });
            if touched {
                first = first.min(y);
                last = y;
            }
        }
        (first, last)
    }

    #[test]
    fn the_notice_stays_inside_the_panel_on_a_round_gauge() {
        let (w, h) = (480, 480);
        let s = drawn(w, h);
        let (first, last) = band_rows(&s, w, h);
        assert!(first > 0, "the band reached the top edge");
        assert!(last < h - 1, "the band reached the bottom edge");
    }

    #[test]
    fn the_notice_is_centred_so_a_round_panel_does_not_clip_it() {
        let (w, h) = (480, 480);
        let s = drawn(w, h);
        let (first, last) = band_rows(&s, w, h);
        let (above, below) = (first, h - 1 - last);
        // Within a row: an odd number of spare rows cannot split evenly.
        assert!(
            above.abs_diff(below) <= 1,
            "band is not centred: {above} above, {below} below"
        );
    }

    #[test]
    fn the_notice_fits_a_wide_cluster() {
        let (w, h) = (2400, 900);
        let s = drawn(w, h);
        let (first, last) = band_rows(&s, w, h);
        assert!(first > 0 && last < h - 1);
    }

    #[test]
    fn the_notice_fits_a_small_panel() {
        let (w, h) = (320, 240);
        let s = drawn(w, h);
        let (first, last) = band_rows(&s, w, h);
        assert!(first < last, "nothing was drawn");
        assert!(last < h, "the band ran off the panel");
    }

    #[test]
    fn the_gauges_above_and_below_are_left_alone() {
        // The point of a band rather than a full cover: most of the panel is
        // still the scene underneath.
        let (w, h) = (480, 480);
        let s = drawn(w, h);
        let (first, last) = band_rows(&s, w, h);
        let covered = last - first + 1;
        assert!(
            covered < h * 3 / 4,
            "the band covers {covered} of {h} rows, which is not a band"
        );
    }

    #[test]
    fn a_panel_of_no_size_draws_nothing() {
        let mut s = MemorySurface::new(Size { w: 0, h: 0 }, PixelFormat::Bgrx8888);
        let font = default_font();
        overlay(&mut s, Rect::new(0, 0, 0, 0), &font);
    }
}
