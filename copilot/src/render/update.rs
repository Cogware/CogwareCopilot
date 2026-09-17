// SPDX-License-Identifier: GPL-3.0-only
//! What a display shows while the bus is writing to it.
//!
//! A transfer is slow enough that a panel left showing its scene looks frozen,
//! and a screen switched off part way through a write is the one thing the bus
//! cannot put right afterwards. So the scene is replaced outright rather than
//! dimmed: a dark panel that says why is unmistakably deliberate.
//!
//! It lives here rather than in a firmware because every display on the bus
//! has to say the same thing — the flags it is drawn from are broadcast to all
//! of them at once.

use alloc::format;

use super::draw::draw_text;
use crate::color::Color;
use crate::font::Font;
use crate::surface::Surface;
use crate::widget::{Align, VAlign};
use crate::Rect;

/// The ground the notice sits on.
const GROUND: Color = Color::rgb(10, 12, 16);
/// The heading.
const INK: Color = Color::rgb(235, 240, 248);
/// The line that matters, in the colour a warning is expected in.
const WARN: Color = Color::rgb(255, 176, 64);
/// Behind the bar.
const TRACK: Color = Color::rgb(38, 44, 56);
/// The bar itself.
const FILL: Color = Color::rgb(110, 198, 255);
/// The edge around the bar, so an empty one is still visibly a bar.
const EDGE: Color = Color::rgb(96, 106, 126);

/// Draw the update notice over the whole of `at`.
///
/// `mine` says whether this display is the one being written; the others are
/// only waiting, but must not be switched off either, because the bus is
/// carrying the image past them. `percent` is what the master last broadcast.
pub fn overlay<S: Surface + ?Sized>(
    surface: &mut S,
    at: Rect,
    font: &Font<'_>,
    mine: bool,
    percent: u8,
) {
    let (w, h) = (at.size.w, at.size.h);
    if w == 0 || h == 0 {
        return;
    }
    let x0 = at.origin.x;
    let y0 = at.origin.y;

    for row in 0..h as i32 {
        surface.fill_span(x0, y0 + row, w, GROUND);
    }

    // The font is a fixed cell, so the only way one notice suits both a
    // 480-pixel round gauge and a 2400-pixel cluster is to scale it with the
    // panel rather than pick a size that is right for one of them.
    let small = ((w / 240).clamp(2, 8)) as u8;
    let big = (small * 2).min(14);
    let small_h = i32::from(font.cell_h) * i32::from(small);
    let big_h = i32::from(font.cell_h) * i32::from(big);
    let middle = y0 + h as i32 / 2;

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

    line(surface, "UPDATING", INK, big, middle - big_h - small_h * 3);
    line(
        surface,
        "Do not unplug or power off, keep ign on",
        WARN,
        small,
        middle - small_h * 2,
    );

    let bar_w = (w * 3 / 5).max(8);
    let bar_h = small_h.max(6);
    let bar_x = x0 + (w as i32 - bar_w as i32) / 2;
    let bar_y = middle;
    for row in 0..bar_h {
        surface.fill_span(bar_x, bar_y + row, bar_w, TRACK);
    }
    for row in [0, bar_h - 1] {
        surface.fill_span(bar_x, bar_y + row, bar_w, EDGE);
    }
    // Clamped rather than trusted: the percentage arrives off the bus.
    let done = bar_w * u32::from(percent.min(100)) / 100;
    for row in 1..bar_h - 1 {
        surface.fill_span(bar_x, bar_y + row, done, FILL);
    }

    line(
        surface,
        &format!("{}%", percent.min(100)),
        INK,
        small,
        bar_y + bar_h + small_h,
    );
    line(
        surface,
        if mine {
            "writing to this display"
        } else {
            "the network is busy updating another display"
        },
        INK,
        small,
        bar_y + bar_h + small_h * 3,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::default_font;
    use crate::geom::Size;
    use crate::surface::PixelFormat;
    use crate::surface_mem::MemorySurface;

    fn drawn(w: u32, h: u32, mine: bool, percent: u8) -> MemorySurface {
        let mut s = MemorySurface::new(Size { w, h }, PixelFormat::Bgrx8888);
        let font = default_font();
        overlay(&mut s, Rect::new(0, 0, w, h), &font, mine, percent);
        s
    }

    /// Every pixel of a given colour, as packed by `Bgrx8888`.
    fn count(s: &MemorySurface, c: Color) -> usize {
        s.pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[2] == c.r && p[1] == c.g && p[0] == c.b)
            .count()
    }

    #[test]
    fn the_notice_covers_the_panel_it_is_given() {
        // Nothing of the scene underneath survives, which is the point: a
        // half-covered scene reads as a glitch rather than as a warning.
        let (w, h) = (480u32, 480u32);
        let mut s = MemorySurface::new(Size { w, h }, PixelFormat::Bgrx8888);
        let loud = Color::rgb(255, 0, 255);
        for y in 0..h as i32 {
            s.fill_span(0, y, w, loud);
        }
        let font = default_font();
        overlay(&mut s, Rect::new(0, 0, w, h), &font, true, 50);
        assert_eq!(count(&s, loud), 0, "the scene showed through");
    }

    #[test]
    fn the_bar_grows_with_the_percentage() {
        let (w, h) = (480, 480);
        let empty = count(&drawn(w, h, true, 0), FILL);
        let half = count(&drawn(w, h, true, 50), FILL);
        let full = count(&drawn(w, h, true, 100), FILL);
        assert_eq!(empty, 0, "an unstarted transfer draws no fill");
        assert!(half > 0 && full > half, "empty {empty}, half {half}, full {full}");
    }

    #[test]
    fn a_percentage_off_the_bus_cannot_overrun_the_bar() {
        // It arrives on the wire, so 200 is a frame this node did not write.
        let (w, h) = (480, 480);
        assert_eq!(
            count(&drawn(w, h, true, 200), FILL),
            count(&drawn(w, h, true, 100), FILL)
        );
    }

    #[test]
    fn it_draws_on_a_cluster_as_well_as_a_round_gauge() {
        // The two panels on this bus differ by a factor of five in width, and
        // a notice sized for one is unreadable or clipped on the other.
        for (w, h) in [(480u32, 480u32), (2400, 900), (320, 240)] {
            let s = drawn(w, h, false, 40);
            assert!(count(&s, INK) > 0, "no text at {w}x{h}");
            assert!(count(&s, WARN) > 0, "no warning at {w}x{h}");
            assert!(count(&s, FILL) > 0, "no bar at {w}x{h}");
        }
    }

    #[test]
    fn the_display_being_written_says_something_different() {
        let (w, h) = (480, 480);
        assert_ne!(
            count(&drawn(w, h, true, 40), INK),
            count(&drawn(w, h, false, 40), INK)
        );
    }
}
