// SPDX-License-Identifier: GPL-3.0-only
//! What a display shows while it waits on the server's radios, and what it
//! shows when the wait comes to nothing.
//!
//! The aerials belong to the server, which on this car is the board with no
//! screen and no buttons. So a display is where somebody asks for a radio, and
//! it is also the only place they can be told what happened -- including that
//! nothing did. A request that simply returns to the menu with the radio still
//! off is a car that has quietly refused and not said so.
//!
//! Hence two states and no third. While the server is thinking there is a
//! throbber, because the honest thing to show for "we do not know yet" is
//! movement without a figure: a bar would have to invent a percentage of
//! something that has no parts. When it goes wrong there are words saying
//! which of the ways it went wrong, because "it did not work" leaves a person
//! with nothing to do next, and "the radio would not start" and "the server
//! refused" call for quite different things.

use super::draw::draw_text;
use crate::color::Color;
use crate::font::Font;
use crate::surface::Surface;
use crate::trig::TURN;
use crate::widget::{Align, VAlign};
use crate::Rect;

/// The ground the notice sits on.
const GROUND: Color = Color::rgb(10, 12, 16);
/// The heading, and the rule round the band.
const INK: Color = Color::rgb(235, 240, 248);
/// The part of the throbber that is lit, and the rule while waiting.
const LIVE: Color = Color::rgb(110, 198, 255);
/// The ring the throbber runs round.
const TRACK: Color = Color::rgb(38, 44, 56);
/// A fault, and the rule round the band when there is one.
const WARN: Color = Color::rgb(255, 176, 64);

/// How much of the ring the throbber's lit part covers, in brads.
///
/// An eighth of a turn. Long enough to be seen moving at a glance, short
/// enough that it never looks like a ring that has simply been drawn wrong.
const SWEEP: i32 = TURN / 8;

/// Brads the throbber advances per tick.
///
/// A tick is one call, so this is per frame rather than per unit of time, and
/// a display drawing faster spins it faster. That is the right way round: the
/// throbber's job is to show the display is still alive, and one that crawled
/// on a busy panel would be saying the opposite.
const STEP: i32 = TURN / 48;

/// Where a request has got to, as far as drawing it is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step<'a> {
    /// Still waiting. `tick` advances the throbber; any counter that moves
    /// once a frame will do.
    Waiting {
        /// Frames since the wait began.
        tick: u32,
    },
    /// It came to nothing, and this says which way.
    Failed(&'a str),
}

/// Draw the notice across the middle of `at`.
///
/// `what` is what was asked for, in words a person would use -- "Turning on
/// Bluetooth", "Pairing" -- and is shown in both states, so a fault is still
/// attached to the thing that suffered it.
///
/// A band rather than the whole panel, for the same reason as the notice about
/// a put-back update: the car is working, and this is a conversation about one
/// setting rather than a reason to take the instruments away.
pub fn overlay<S: Surface + ?Sized>(surface: &mut S, at: Rect, font: &Font<'_>, what: &str, step: Step<'_>) {
    let (w, h) = (at.size.w, at.size.h);
    if w == 0 || h == 0 {
        return;
    }
    let x0 = at.origin.x;

    let small = ((w / 240).clamp(2, 8)) as u8;
    let small_h = u32::from(font.cell_h) * u32::from(small);
    let big = (small * 2).min(14);
    let big_h = u32::from(font.cell_h) * u32::from(big);

    // Deep enough for the heading, the throbber or the reason, and air round
    // both, whatever the panel.
    let wanted = big_h + small_h * 5;
    let band_h = (h * 2 / 5).max(wanted).min(h);
    let band_y = at.origin.y + ((h - band_h) / 2) as i32;
    let edge = match step {
        Step::Waiting { .. } => LIVE,
        Step::Failed(_) => WARN,
    };

    for row in 0..band_h as i32 {
        surface.fill_span(x0, band_y + row, w, GROUND);
    }
    for row in [0, band_h as i32 - 1] {
        surface.fill_span(x0, band_y + row, w, edge);
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

    let top = band_y + ((band_h - wanted) / 2) as i32;
    line(surface, what, INK, big, top);

    match step {
        Step::Waiting { tick } => {
            // Centred under the heading, sized from the text so it sits right
            // on a round gauge and on a cluster without either being special.
            let r = (small_h * 3 / 2) as f32;
            let cx = x0 as f32 + w as f32 / 2.0;
            let cy = (top + big_h as i32) as f32 + small_h as f32 * 2.0;
            let inner = r - (small_h as f32 / 3.0).max(2.0);
            super::aa::arc(surface, at, (cx, cy), inner, r, 0, TURN, TRACK, true);
            // Wrapped rather than left to grow: a tick that has been running
            // for an hour must land in the same place as one that has not.
            let from = ((tick as i32).wrapping_mul(STEP)).rem_euclid(TURN);
            super::aa::arc(surface, at, (cx, cy), inner, r, from, SWEEP, LIVE, true);
        }
        Step::Failed(why) => {
            line(
                surface,
                "could not",
                WARN,
                small,
                top + big_h as i32 + small_h as i32 / 2,
            );
            line(
                surface,
                why,
                INK,
                small,
                top + big_h as i32 + small_h as i32 * 2,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::default_font;
    use crate::geom::Size;
    use crate::surface::PixelFormat;
    use crate::surface_mem::MemorySurface;

    fn drawn(w: u32, h: u32, step: Step<'_>) -> MemorySurface {
        let mut s = MemorySurface::new(Size { w, h }, PixelFormat::Bgrx8888);
        let font = default_font();
        overlay(&mut s, Rect::new(0, 0, w, h), &font, "Pairing", step);
        s
    }

    /// A cheap digest of the whole surface.
    ///
    /// Compared instead of the pixels themselves so that a failure prints a
    /// number rather than several megabytes of buffer, which is unreadable
    /// and buries the assertion that failed.
    fn digest(s: &MemorySurface) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in s.pixels() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    fn painted(s: &MemorySurface) -> usize {
        s.pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] != 0 || p[1] != 0 || p[2] != 0)
            .count()
    }

    fn rows_touched(s: &MemorySurface, w: u32, h: u32) -> (u32, u32) {
        let px = s.pixels().as_chunks::<4>().0;
        let (mut first, mut last) = (h, 0);
        for y in 0..h {
            if (0..w).any(|x| {
                let p = px[(y * w + x) as usize];
                p[0] != 0 || p[1] != 0 || p[2] != 0
            }) {
                first = first.min(y);
                last = y;
            }
        }
        (first, last)
    }

    #[test]
    fn the_throbber_moves_between_ticks() {
        // The one thing a throbber has to do. A still one says the display has
        // stopped, which is the opposite of what it is there for.
        let a = drawn(480, 480, Step::Waiting { tick: 0 });
        let b = drawn(480, 480, Step::Waiting { tick: 12 });
        assert_ne!(digest(&a), digest(&b), "the throbber did not move");
    }

    #[test]
    fn the_throbber_comes_back_round_rather_than_running_off() {
        // A tick that has been counting for an hour must draw the same as one
        // that has just started its turn.
        //
        // The period is `TURN` ticks rather than `TURN / STEP`: `STEP` is a
        // truncating division, so a whole number of steps does not land on a
        // whole turn, but `TURN` more ticks always adds a whole multiple of
        // `TURN` brads and so lands on the same angle.
        let a = drawn(480, 480, Step::Waiting { tick: 3 });
        let b = drawn(480, 480, Step::Waiting { tick: 3 + TURN as u32 });
        assert_eq!(digest(&a), digest(&b), "the throbber did not wrap");
    }

    #[test]
    fn a_huge_tick_still_draws() {
        // `tick * STEP` overflows an i32 long before this, and a wrap that
        // panicked would take the display down for a spinner.
        let s = drawn(480, 480, Step::Waiting { tick: u32::MAX });
        assert!(painted(&s) > 0);
    }

    #[test]
    fn a_fault_says_which_way_it_failed() {
        let s = drawn(480, 480, Step::Failed("the radio would not start"));
        assert!(painted(&s) > 0);
        // And differs from the same panel still waiting, so the two states
        // cannot be confused by someone glancing at it.
        let waiting = drawn(480, 480, Step::Waiting { tick: 0 });
        assert_ne!(digest(&s), digest(&waiting));
    }

    #[test]
    fn the_band_stays_inside_a_round_gauge() {
        for step in [Step::Waiting { tick: 5 }, Step::Failed("the server refused")] {
            let (w, h) = (480, 480);
            let s = drawn(w, h, step);
            let (first, last) = rows_touched(&s, w, h);
            assert!(first > 0 && last < h - 1, "the band reached an edge");
            let covered = last - first + 1;
            assert!(covered < h * 3 / 4, "the band covers too much to be a band");
        }
    }

    #[test]
    fn it_fits_a_wide_cluster_and_a_small_panel() {
        for (w, h) in [(2400u32, 900u32), (320, 240)] {
            for step in [Step::Waiting { tick: 1 }, Step::Failed("no such radio")] {
                let s = drawn(w, h, step);
                let (first, last) = rows_touched(&s, w, h);
                assert!(first < last, "nothing was drawn at {w}x{h}");
                assert!(last < h, "the band ran off a {w}x{h} panel");
            }
        }
    }

    #[test]
    fn a_panel_of_no_size_draws_nothing() {
        let mut s = MemorySurface::new(Size { w: 0, h: 0 }, PixelFormat::Bgrx8888);
        let font = default_font();
        overlay(&mut s, Rect::new(0, 0, 0, 0), &font, "Pairing", Step::Waiting { tick: 0 });
    }
}
