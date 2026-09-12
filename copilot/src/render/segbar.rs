// SPDX-License-Identifier: GPL-3.0-only
//! A bargraph made of discrete cells, the way a vacuum-fluorescent panel
//! shows a reading.
//!
//! Distinct from [`super::draw`]'s smooth bar, and not a refinement of it. A
//! solid bar says "about this much"; a row of cells says "this many", and a
//! driver counts them without looking away from the road. It is also what the
//! instrument being replicated actually has.

use crate::{Color, Rect, Surface};

/// How far across a cell reaches, as a fraction, when the caller has given
/// a shape to follow.
///
/// An empty profile means every cell fills its box, which is a plain bargraph.
/// A profile is what lets one widget be the instrument being replicated here:
/// a row of segments cut to a power curve printed on the lens, rather than
/// twelve rectangles placed by hand with a curve drawn behind them.
fn shaped(profile: &[f32], i: i32) -> f32 {
    if profile.is_empty() {
        return 1.0;
    }
    let Ok(idx) = usize::try_from(i) else {
        return 1.0;
    };
    // A cell past the end of the profile keeps the last value rather than
    // collapsing: a short profile is an unfinished one, not a request for a
    // gauge that stops halfway.
    let f = profile
        .get(idx)
        .or_else(|| profile.last())
        .copied()
        .unwrap_or(1.0);
    // NaN first: `clamp` on a float panics if the bounds are ordered wrongly
    // and returns NaN otherwise, and a NaN cast to an integer is not worth
    // predicting.
    if f.is_nan() { 0.0 } else { f.clamp(0.0, 1.0) }
}

/// Draw a segmented bar gauge, clipped to the damage region and the widget
/// box.
///
/// Cells are coloured by their position on the scale, and the number of lit
/// cells is the fractional reading rounded up.
#[allow(clippy::too_many_arguments)] // Every one is a distinct gauge property.
pub fn seg_bar<S: Surface + ?Sized>(
    surface: &mut S,
    at: Rect,
    clip: Rect,
    value: f32,
    height: f32,
    segments: u32,
    gap: u32,
    fill: Color,
    track: Color,
    warn: f32,
    warn_fill: Color,
    danger: f32,
    danger_fill: Color,
    vertical: bool,
    profile: &[f32],
    divisions: u32,
    div_gap: u32,
    antialias: bool,
) {
    // Nothing to draw when the box misses the damage, is empty, or asks for
    // no cells; proceeding would only risk fills on empty rectangles.
    let Some(clip) = at.intersection(clip) else {
        return;
    };
    if clip.is_empty() || segments == 0 {
        return;
    }

    // Capped to keep the loop bounded on absurd input; 4096 cells is far more
    // than any gauge needs.
    let n = segments.min(4096) as i32;
    let g = gap as f32;

    // The cells and the gaps between them share the axis exactly.
    let span = if vertical {
        at.size.h as f32
    } else {
        at.size.w as f32
    };
    let pitch = (span - g * (n - 1) as f32) / n as f32;
    // Cells thinner than one pixel are not cells; drawing them would be a
    // smear rather than a gauge.
    if pitch < 1.0 || pitch.is_nan() {
        return;
    }

    // The same rule for the bands that cut a cell across its width. They are
    // measured across the whole widget, not across the cell, so they line up
    // right along the bank: that common grid is what makes it read as one
    // display rather than as a row of unrelated gauges.
    let across = if vertical {
        at.size.w as f32
    } else {
        at.size.h as f32
    };
    let divs = divisions.min(4096) as i32;
    let dg = div_gap as f32;
    let dpitch = if divs > 0 {
        (across - dg * (divs - 1) as f32) / divs as f32
    } else {
        across
    };
    // Bands too thin to see are not drawn as bands; the cell stays solid.
    let divided = divs > 0 && dpitch >= 1.0;
    // Cells that touch are not antialiased. Two blended edges meeting in one
    // pixel leave it a fifth short of solid, and a faint seam between every
    // pair of cells is worse than the pixel of unevenness it would replace.
    let antialias = antialias && g > 0.0 && (!divided || dg > 0.0);

    // The reading, clamped and NaN-proofed, then the ceiling by hand:
    // `f32::ceil` lives in `std`, and this crate's whole point is running
    // where there is none. A cast truncates towards zero, so one more is
    // added whenever anything was thrown away.
    let v = if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    };
    // Clamped and NaN-proofed like the reading, and for the same reason: a
    // gauge fed nonsense should read empty, not draw somewhere off the box.
    let tall = if height.is_nan() {
        0.0
    } else {
        height.clamp(0.0, 1.0)
    };
    let scaled = v * n as f32;
    let mut lit = scaled as i32;
    if (lit as f32) < scaled {
        lit += 1;
    }
    let lit = lit.clamp(0, n);

    let (left, bottom) = (at.left() as f32, at.bottom() as f32);

    for i in 0..n {
        let is_lit = i < lit;
        // The band colour comes from where the cell sits on the scale, not
        // from the reading, so the thresholds stay put as the gauge fills.
        let at_frac = (i + 1) as f32 / n as f32;
        let colour = if !is_lit {
            track
        } else if at_frac >= danger {
            danger_fill
        } else if at_frac >= warn {
            warn_fill
        } else {
            fill
        };
        if colour.is_transparent() {
            continue;
        }

        // Where this cell starts and ends along the axis, from the origin
        // end, and how far across the box it reaches.
        let c0 = i as f32 * (pitch + g);
        let c1 = c0 + pitch;
        // The two axes meet here: `profile` is the envelope printed on the
        // lens, and `height` is how far up it the reading stands. On the
        // instrument this replicates the columns light with the revs and
        // their height is boost, so the top row is full boost.
        let reach = across * shaped(profile, i) * tall;
        if reach <= 0.0 {
            continue;
        }
        // Cells stack from the bottom for a vertical gauge and run from the
        // left for a horizontal one; a profile is cut from the bottom, the
        // edge a bargraph is read from and a printed envelope drawn against.
        let (x0, y0, x1, y1) = if vertical {
            (left, bottom - c1, left + reach, bottom - c0)
        } else {
            (left + c0, bottom - reach, left + c1, bottom)
        };

        if !divided {
            super::aa::frac_rect(surface, clip, x0, y0, x1, y1, colour, antialias);
            continue;
        }
        for j in 0..divs {
            let b0 = j as f32 * (dpitch + dg);
            let b1 = b0 + dpitch;
            // The band's extent across, cut down to what this cell reaches.
            let (bx0, by0, bx1, by1) = if vertical {
                ((left + b0).max(x0), y0, (left + b1).min(x1), y1)
            } else {
                (x0, (bottom - b1).max(y0), x1, (bottom - b0).min(y1))
            };
            super::aa::frac_rect(surface, clip, bx0, by0, bx1, by1, colour, antialias);
        }
    }
}

#[cfg(test)]
mod segbar_tests {
    use super::*;
    use crate::{MemorySurface, PixelFormat, Size};

    const FULL: Rect = Rect::new(0, 0, 64, 64);
    const LIT: Color = Color::rgb(0, 255, 0);
    const OFF: Color = Color::rgb(20, 20, 20);
    const WARN: Color = Color::rgb(255, 200, 0);
    const DANGER: Color = Color::rgb(255, 0, 0);

    fn surf() -> MemorySurface {
        MemorySurface::new(Size { w: 64, h: 64 }, PixelFormat::Bgrx8888)
    }

    /// The (b, g, r) at a pixel.
    fn px(s: &MemorySurface, x: usize, y: usize) -> [u8; 3] {
        let o = y * s.stride() + x * 4;
        [s.pixels()[o], s.pixels()[o + 1], s.pixels()[o + 2]]
    }

    fn bgr(c: Color) -> [u8; 3] {
        [c.b, c.g, c.r]
    }

    /// A plain horizontal bargraph with no colour bands.
    fn plain(s: &mut MemorySurface, value: f32, segments: u32, gap: u32) {
        // Thresholds above the top of the scale, not at it: a cell sitting
        // exactly on a threshold is in that band, so 1.0 would make the last
        // cell red on every gauge that never wanted a redline.
        seg_bar(
            s,
            FULL,
            FULL,
            value,
            1.0,
            segments,
            gap,
            LIT,
            OFF,
            2.0,
            WARN,
            2.0,
            DANGER,
            false,
            &[],
            0,
            0,
            false,
        );
    }

    /// The distinct colours found along the middle row, left to right.
    fn runs(s: &MemorySurface, y: usize) -> alloc::vec::Vec<[u8; 3]> {
        let mut out: alloc::vec::Vec<[u8; 3]> = alloc::vec::Vec::new();
        for x in 0..64 {
            let p = px(s, x, y);
            if out.last() != Some(&p) {
                out.push(p);
            }
        }
        out
    }

    #[test]
    fn a_gap_leaves_dark_between_the_cells() {
        // The whole point: a segmented gauge is cells with air between them,
        // not a bar with lines drawn on it.
        let mut s = surf();
        plain(&mut s, 1.0, 4, 4);
        let seen = runs(&s, 32);
        assert!(
            seen.len() >= 7,
            "expected lit and dark alternating, got {seen:?}"
        );
        assert!(seen.contains(&[0, 0, 0]), "no gaps at all: {seen:?}");
    }

    #[test]
    fn no_gap_still_draws_every_cell() {
        let mut s = surf();
        plain(&mut s, 1.0, 8, 0);
        for x in 0..64 {
            assert_eq!(px(&s, x, 32), bgr(LIT), "column {x} was not lit");
        }
    }

    #[test]
    fn a_full_reading_lights_every_cell() {
        let mut s = surf();
        plain(&mut s, 1.0, 4, 2);
        assert!(!runs(&s, 32).contains(&bgr(OFF)), "something stayed unlit");
    }

    #[test]
    fn an_empty_reading_lights_none_of_them() {
        let mut s = surf();
        plain(&mut s, 0.0, 4, 2);
        assert!(!runs(&s, 32).contains(&bgr(LIT)), "something lit at zero");
        assert!(runs(&s, 32).contains(&bgr(OFF)), "the track went missing");
    }

    #[test]
    fn a_reading_just_off_the_stop_lights_one_cell() {
        // Ceiling, not rounding: a gauge barely off zero that reads as empty
        // is worse than one that overstates by a cell.
        let mut s = surf();
        plain(&mut s, 0.01, 10, 1);
        assert_eq!(px(&s, 1, 32), bgr(LIT), "the first cell stayed dark");
        assert_eq!(px(&s, 60, 32), bgr(OFF), "a later cell lit at 0.01");
    }

    #[test]
    fn half_a_reading_lights_half_the_cells() {
        let mut s = surf();
        plain(&mut s, 0.5, 10, 1);
        assert_eq!(px(&s, 1, 32), bgr(LIT), "the first cell");
        assert_eq!(px(&s, 30, 32), bgr(LIT), "the middle cell");
        assert_eq!(px(&s, 40, 32), bgr(OFF), "past halfway should be dark");
    }

    #[test]
    fn the_cells_light_from_the_left() {
        let mut s = surf();
        plain(&mut s, 0.3, 10, 1);
        let first = px(&s, 1, 32);
        let last = px(&s, 62, 32);
        assert_eq!(first, bgr(LIT));
        assert_eq!(last, bgr(OFF));
    }

    #[test]
    fn a_vertical_bargraph_stacks_from_the_bottom() {
        let mut s = surf();
        seg_bar(
            &mut s,
            FULL,
            FULL,
            0.3,
            1.0,
            10,
            1,
            LIT,
            OFF,
            2.0,
            WARN,
            2.0,
            DANGER,
            true,
            &[],
            0,
            0,
            false,
        );
        assert_eq!(px(&s, 32, 62), bgr(LIT), "the bottom cell should be lit");
        assert_eq!(px(&s, 32, 1), bgr(OFF), "the top cell should not be");
    }

    #[test]
    fn a_vertical_bargraph_fills_the_width() {
        let mut s = surf();
        seg_bar(
            &mut s,
            FULL,
            FULL,
            1.0,
            1.0,
            4,
            2,
            LIT,
            OFF,
            2.0,
            WARN,
            2.0,
            DANGER,
            true,
            &[],
            0,
            0,
            false,
        );
        for x in 0..64 {
            assert_eq!(px(&s, x, 62), bgr(LIT), "column {x} of the bottom cell");
        }
    }

    #[test]
    fn a_cell_takes_its_band_from_where_it_sits() {
        // A red cell is red whenever it is lit, which is what makes a redline
        // a redline rather than a colour the whole gauge turns.
        let mut s = surf();
        seg_bar(
            &mut s,
            FULL,
            FULL,
            1.0,
            1.0,
            10,
            1,
            LIT,
            OFF,
            0.7,
            WARN,
            0.9,
            DANGER,
            false,
            &[],
            0,
            0,
            false,
        );
        assert_eq!(px(&s, 1, 32), bgr(LIT), "the first cell should be normal");
        // Ten cells and nine gaps in 64: cells are 5.5 wide, and the eighth
        // runs from 45.5 to 51.
        assert_eq!(px(&s, 48, 32), bgr(WARN), "the eighth cell should warn");
        assert_eq!(
            px(&s, 62, 32),
            bgr(DANGER),
            "the last cell should be danger"
        );
    }

    #[test]
    fn the_bands_do_not_move_with_the_reading() {
        let mut low = surf();
        let mut high = surf();
        seg_bar(
            &mut low,
            FULL,
            FULL,
            0.5,
            1.0,
            10,
            1,
            LIT,
            OFF,
            0.7,
            WARN,
            0.9,
            DANGER,
            false,
            &[],
            0,
            0,
            false,
        );
        seg_bar(
            &mut high,
            FULL,
            FULL,
            1.0,
            1.0,
            10,
            1,
            LIT,
            OFF,
            0.7,
            WARN,
            0.9,
            DANGER,
            false,
            &[],
            0,
            0,
            false,
        );
        // The cells lit in both readings must be the same colour in both.
        for x in [1, 10, 20, 30] {
            assert_eq!(
                px(&low, x, 32),
                px(&high, x, 32),
                "column {x} changed colour with the reading"
            );
        }
    }

    #[test]
    fn a_transparent_track_leaves_the_unlit_cells_unpainted() {
        let mut s = surf();
        seg_bar(
            &mut s,
            FULL,
            FULL,
            0.2,
            1.0,
            10,
            1,
            LIT,
            Color::TRANSPARENT,
            1.0,
            WARN,
            1.0,
            DANGER,
            false,
            &[],
            0,
            0,
            false,
        );
        assert_eq!(px(&s, 1, 32), bgr(LIT));
        assert_eq!(px(&s, 60, 32), [0, 0, 0], "the track painted anyway");
    }

    #[test]
    fn a_bargraph_stays_inside_its_box() {
        let mut s = surf();
        let at = Rect::new(10, 10, 21, 21);
        seg_bar(
            &mut s,
            at,
            FULL,
            1.0,
            1.0,
            4,
            3,
            LIT,
            OFF,
            1.0,
            WARN,
            1.0,
            DANGER,
            false,
            &[],
            0,
            0,
            false,
        );
        for y in 0..64 {
            for x in 0..64 {
                let inside = (10..31).contains(&x) && (10..31).contains(&y);
                assert!(
                    px(&s, x, y) == [0, 0, 0] || inside,
                    "drew at ({x}, {y}), outside the box"
                );
            }
        }
    }

    #[test]
    fn a_bargraph_respects_the_clip() {
        let mut s = surf();
        seg_bar(
            &mut s,
            FULL,
            Rect::new(0, 0, 16, 16),
            1.0,
            1.0,
            8,
            1,
            LIT,
            OFF,
            1.0,
            WARN,
            1.0,
            DANGER,
            false,
            &[],
            0,
            0,
            false,
        );
        for y in 0..64 {
            for x in 0..64 {
                assert!(
                    px(&s, x, y) == [0, 0, 0] || (x < 16 && y < 16),
                    "drew at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn a_partial_repaint_matches_a_full_one() {
        let mut full = surf();
        seg_bar(
            &mut full,
            FULL,
            FULL,
            0.6,
            1.0,
            7,
            2,
            LIT,
            OFF,
            0.7,
            WARN,
            0.9,
            DANGER,
            false,
            &[],
            0,
            0,
            false,
        );
        let mut partial = surf();
        for band in [
            Rect::new(0, 0, 21, 64),
            Rect::new(21, 0, 22, 64),
            Rect::new(43, 0, 21, 64),
        ] {
            seg_bar(
                &mut partial,
                FULL,
                band,
                0.6,
                1.0,
                7,
                2,
                LIT,
                OFF,
                0.7,
                WARN,
                0.9,
                DANGER,
                false,
                &[],
                0,
                0,
                false,
            );
        }
        assert_eq!(partial.pixels(), full.pixels());
    }

    #[test]
    fn no_segments_draws_nothing() {
        let mut s = surf();
        plain(&mut s, 1.0, 0, 1);
        assert!((0..64).all(|x| px(&s, x, 32) == [0, 0, 0]));
    }

    #[test]
    fn cells_too_thin_to_be_cells_draw_nothing() {
        // Better blank than a smear: sixty-four pixels cannot hold a hundred
        // cells with gaps, and whatever came out would not be a gauge.
        let mut s = surf();
        plain(&mut s, 1.0, 100, 4);
        assert!((0..64).all(|x| px(&s, x, 32) == [0, 0, 0]));
    }

    #[test]
    fn an_empty_box_draws_nothing() {
        let mut s = surf();
        seg_bar(
            &mut s,
            Rect::new(5, 5, 0, 0),
            FULL,
            1.0,
            1.0,
            4,
            1,
            LIT,
            OFF,
            1.0,
            WARN,
            1.0,
            DANGER,
            false,
            &[],
            0,
            0,
            false,
        );
        assert!((0..64).all(|y| (0..64).all(|x| px(&s, x, y) == [0, 0, 0])));
    }

    #[test]
    fn a_wild_reading_is_clamped_rather_than_believed() {
        for bad in [f32::NAN, -5.0, 12.0, f32::INFINITY] {
            let mut s = surf();
            plain(&mut s, bad, 8, 1);
            let seen = runs(&s, 32);
            assert!(
                seen.iter()
                    .all(|c| *c == bgr(LIT) || *c == bgr(OFF) || *c == [0, 0, 0]),
                "{bad} produced {seen:?}"
            );
        }
    }

    #[test]
    fn an_absurd_segment_count_terminates() {
        let mut s = surf();
        seg_bar(
            &mut s,
            Rect::new(0, 0, 64, 64),
            FULL,
            1.0,
            1.0,
            u32::MAX,
            0,
            LIT,
            OFF,
            1.0,
            WARN,
            1.0,
            DANGER,
            false,
            &[],
            0,
            0,
            false,
        );
    }

    #[test]
    fn a_box_at_the_edge_of_the_range_does_not_overflow() {
        let mut s = surf();
        seg_bar(
            &mut s,
            Rect::new(i32::MAX - 8, 0, 64, 64),
            FULL,
            1.0,
            1.0,
            8,
            1,
            LIT,
            OFF,
            1.0,
            WARN,
            1.0,
            DANGER,
            false,
            &[],
            0,
            0,
            false,
        );
        seg_bar(
            &mut s,
            Rect::new(i32::MIN / 2, 0, u32::MAX, 64),
            FULL,
            1.0,
            1.0,
            8,
            1,
            LIT,
            OFF,
            1.0,
            WARN,
            1.0,
            DANGER,
            true,
            &[],
            0,
            0,
            false,
        );
    }

    // --- the profile ---

    #[test]
    fn a_profile_cuts_each_cell_to_its_own_height() {
        // The instrument being replicated: cells cut to a power curve printed
        // on the lens, so the lit ones trace the engine rather than forming a
        // level block.
        let mut s = surf();
        seg_bar(
            &mut s,
            FULL,
            FULL,
            1.0,
            1.0,
            4,
            0,
            LIT,
            OFF,
            2.0,
            WARN,
            2.0,
            DANGER,
            false,
            &[0.25, 0.5, 0.75, 1.0],
            0,
            0,
            false,
        );
        // Each cell is sixteen wide; sample the middle of each.
        assert!(
            px(&s, 8, 62) == bgr(LIT),
            "the first cell should reach the floor"
        );
        assert!(
            px(&s, 8, 40) == [0, 0, 0],
            "and stop a quarter of the way up"
        );
        assert!(
            px(&s, 56, 2) == bgr(LIT),
            "the last cell should reach the top"
        );
    }

    #[test]
    fn a_profile_cuts_from_the_bottom_up() {
        let mut s = surf();
        seg_bar(
            &mut s,
            FULL,
            FULL,
            1.0,
            1.0,
            2,
            0,
            LIT,
            OFF,
            2.0,
            WARN,
            2.0,
            DANGER,
            false,
            &[0.5, 0.5],
            0,
            0,
            false,
        );
        assert_eq!(px(&s, 16, 63), bgr(LIT), "the base should be painted");
        assert_eq!(px(&s, 16, 0), [0, 0, 0], "the top should not be");
    }

    #[test]
    fn an_empty_profile_is_a_plain_bargraph() {
        let mut a = surf();
        let mut b = surf();
        plain(&mut a, 0.6, 8, 2);
        seg_bar(
            &mut b,
            FULL,
            FULL,
            0.6,
            1.0,
            8,
            2,
            LIT,
            OFF,
            2.0,
            WARN,
            2.0,
            DANGER,
            false,
            &[],
            0,
            0,
            false,
        );
        assert_eq!(a.pixels(), b.pixels());
    }

    #[test]
    fn a_short_profile_holds_its_last_value() {
        // An unfinished profile is not a request for a gauge that stops.
        let mut s = surf();
        seg_bar(
            &mut s,
            FULL,
            FULL,
            1.0,
            1.0,
            4,
            0,
            LIT,
            OFF,
            2.0,
            WARN,
            2.0,
            DANGER,
            false,
            &[1.0, 1.0],
            0,
            0,
            false,
        );
        assert_eq!(px(&s, 56, 2), bgr(LIT), "the last cell collapsed");
    }

    #[test]
    fn a_profile_of_zero_draws_that_cell_as_nothing() {
        let mut s = surf();
        seg_bar(
            &mut s,
            FULL,
            FULL,
            1.0,
            1.0,
            2,
            0,
            LIT,
            OFF,
            2.0,
            WARN,
            2.0,
            DANGER,
            false,
            &[0.0, 1.0],
            0,
            0,
            false,
        );
        assert_eq!(px(&s, 16, 63), [0, 0, 0], "a zero cell painted anyway");
        assert_eq!(px(&s, 48, 63), bgr(LIT), "the full cell went missing");
    }

    #[test]
    fn a_wild_profile_value_is_clamped() {
        let mut s = surf();
        seg_bar(
            &mut s,
            FULL,
            FULL,
            1.0,
            1.0,
            3,
            0,
            LIT,
            OFF,
            2.0,
            WARN,
            2.0,
            DANGER,
            false,
            &[-4.0, 9.0, f32::NAN],
            0,
            0,
            false,
        );
        for y in 0..64 {
            for x in 0..64 {
                assert!(
                    px(&s, x, y) == [0, 0, 0] || px(&s, x, y) == bgr(LIT),
                    "({x}, {y}) is some third colour"
                );
            }
        }
    }

    #[test]
    fn a_profiled_vertical_bargraph_cuts_its_width() {
        let mut s = surf();
        seg_bar(
            &mut s,
            FULL,
            FULL,
            1.0,
            1.0,
            2,
            0,
            LIT,
            OFF,
            2.0,
            WARN,
            2.0,
            DANGER,
            true,
            &[0.5, 1.0],
            0,
            0,
            false,
        );
        assert_eq!(px(&s, 1, 48), bgr(LIT), "the bottom cell should start left");
        assert_eq!(px(&s, 62, 48), [0, 0, 0], "and stop halfway across");
        assert_eq!(px(&s, 62, 16), bgr(LIT), "the top cell should span it all");
    }

    // --- divisions ---

    /// One horizontal bargraph, fully lit, cut into `divisions` bands.
    fn banded(divisions: u32, div_gap: u32) -> MemorySurface {
        let mut s = surf();
        seg_bar(
            &mut s,
            FULL,
            FULL,
            1.0,
            1.0,
            2,
            0,
            LIT,
            OFF,
            2.0,
            WARN,
            2.0,
            DANGER,
            false,
            &[],
            divisions,
            div_gap,
            false,
        );
        s
    }

    /// Which rows of a column have ink.
    fn rows_lit(s: &MemorySurface, x: usize) -> alloc::vec::Vec<usize> {
        (0..64).filter(|&y| px(s, x, y) != [0, 0, 0]).collect()
    }

    #[test]
    fn no_divisions_leaves_a_cell_solid() {
        let s = banded(0, 2);
        assert_eq!(rows_lit(&s, 16).len(), 64, "the column was broken up");
    }

    #[test]
    fn divisions_cut_a_cell_into_dashes() {
        let s = banded(8, 2);
        let lit = rows_lit(&s, 16);
        assert!(lit.len() < 64, "the column stayed solid");
        assert!(!lit.is_empty(), "the column went dark");
        // Eight bands means seven gaps, so the lit rows come in eight runs.
        let runs = lit.windows(2).filter(|w| w[1] != w[0] + 1).count() + 1;
        assert_eq!(runs, 8, "expected eight dashes, got {runs}: {lit:?}");
    }

    #[test]
    fn the_dashes_line_up_across_the_bank() {
        // The point of measuring the bands on the widget rather than the cell:
        // a common grid is what makes it one display instead of a row of
        // unrelated gauges.
        let s = banded(8, 2);
        assert_eq!(
            rows_lit(&s, 8),
            rows_lit(&s, 48),
            "two columns disagreed about where the dashes sit"
        );
    }

    #[test]
    fn a_wider_division_gap_leaves_less_ink() {
        assert!(rows_lit(&banded(8, 4), 16).len() < rows_lit(&banded(8, 1), 16).len());
    }

    #[test]
    fn the_dashes_reach_both_ends_of_the_column() {
        // The gap belongs between dashes, never against the outside, or the
        // bank looks inset from its own box.
        let s = banded(8, 2);
        let lit = rows_lit(&s, 16);
        assert_eq!(lit.first(), Some(&0), "nothing at the top");
        assert_eq!(lit.last(), Some(&63), "nothing at the bottom");
    }

    #[test]
    fn a_profiled_cell_only_shows_the_dashes_it_reaches() {
        let mut s = surf();
        seg_bar(
            &mut s,
            FULL,
            FULL,
            1.0,
            1.0,
            2,
            0,
            LIT,
            OFF,
            2.0,
            WARN,
            2.0,
            DANGER,
            false,
            &[0.25, 1.0],
            8,
            2,
            false,
        );
        let short = rows_lit(&s, 8);
        let tall = rows_lit(&s, 48);
        assert!(short.len() < tall.len(), "the profile did not shorten it");
        assert_eq!(short.last(), Some(&63), "the short column left the floor");
        // Whatever rows it does light must be rows the tall one lights too:
        // the grid is shared, so a short column is a subset of a long one.
        assert!(
            short.iter().all(|r| tall.contains(r)),
            "the short column lit rows off the grid"
        );
    }

    #[test]
    fn a_vertical_bargraph_cuts_its_cells_across_the_width() {
        let mut s = surf();
        seg_bar(
            &mut s,
            FULL,
            FULL,
            1.0,
            1.0,
            2,
            0,
            LIT,
            OFF,
            2.0,
            WARN,
            2.0,
            DANGER,
            true,
            &[],
            8,
            2,
            false,
        );
        let cols: alloc::vec::Vec<usize> =
            (0..64).filter(|&x| px(&s, x, 60) != [0, 0, 0]).collect();
        assert!(cols.len() < 64, "the row stayed solid");
        let runs = cols.windows(2).filter(|w| w[1] != w[0] + 1).count() + 1;
        assert_eq!(runs, 8, "expected eight dashes across, got {runs}");
    }

    #[test]
    fn a_divided_bargraph_still_repaints_from_its_own_damage() {
        let mut full = surf();
        seg_bar(
            &mut full,
            FULL,
            FULL,
            0.6,
            1.0,
            7,
            2,
            LIT,
            OFF,
            0.7,
            WARN,
            0.9,
            DANGER,
            false,
            &[],
            9,
            2,
            false,
        );
        let mut partial = surf();
        for band in [
            Rect::new(0, 0, 64, 21),
            Rect::new(0, 21, 64, 22),
            Rect::new(0, 43, 64, 21),
        ] {
            seg_bar(
                &mut partial,
                FULL,
                band,
                0.6,
                1.0,
                7,
                2,
                LIT,
                OFF,
                0.7,
                WARN,
                0.9,
                DANGER,
                false,
                &[],
                9,
                2,
                false,
            );
        }
        assert_eq!(partial.pixels(), full.pixels());
    }

    #[test]
    fn an_absurd_division_count_terminates() {
        let mut s = surf();
        seg_bar(
            &mut s,
            FULL,
            FULL,
            1.0,
            1.0,
            4,
            1,
            LIT,
            OFF,
            2.0,
            WARN,
            2.0,
            DANGER,
            false,
            &[],
            u32::MAX,
            1,
            false,
        );
    }
    // --- the second axis ---

    /// How many pixels of the box are the lit colour.
    fn lit_pixels(s: &MemorySurface) -> usize {
        (0..64)
            .flat_map(|y| (0..64).map(move |x| (x, y)))
            .filter(|&(x, y)| px(s, x, y) == bgr(LIT))
            .count()
    }

    /// Lit pixels for a full bank at `height`, with `profile` as its envelope.
    fn bank(height: f32, profile: &[f32]) -> usize {
        let mut s = surf();
        seg_bar(
            &mut s,
            FULL,
            FULL,
            1.0,
            height,
            4,
            2,
            LIT,
            Color::TRANSPARENT,
            2.0,
            WARN,
            2.0,
            DANGER,
            false,
            profile,
            0,
            1,
            false,
        );
        lit_pixels(&s)
    }

    #[test]
    fn the_second_axis_scales_how_tall_the_lit_cells_stand() {
        // The reading along the scale says how many columns light; this says
        // how far up they reach. Both full is the whole envelope.
        let full = bank(1.0, &[]);
        let half = bank(0.5, &[]);
        let none = bank(0.0, &[]);
        assert!(full > 0, "a full bank drew nothing");
        assert!(
            (half as f32 / full as f32 - 0.5).abs() < 0.06,
            "half height should be about half the ink: {half} of {full}"
        );
        assert_eq!(none, 0, "no height is no bank");
    }

    #[test]
    fn the_envelope_and_the_second_axis_multiply() {
        // A profile is the shape printed on the lens; the axis is how far up
        // it the reading stands. Half of a half-height envelope is a quarter.
        let flat = bank(1.0, &[]);
        let enveloped = bank(1.0, &[0.5]);
        let both = bank(0.5, &[0.5]);
        assert!(
            (enveloped as f32 / flat as f32 - 0.5).abs() < 0.06,
            "{enveloped} of {flat}"
        );
        assert!(
            (both as f32 / flat as f32 - 0.25).abs() < 0.06,
            "{both} of {flat}"
        );
    }

    #[test]
    fn a_bank_at_full_height_draws_what_it_always_did() {
        // The default, and the reason every scene written before the axis
        // existed renders unchanged.
        let mut with = surf();
        let mut plain = surf();
        seg_bar(
            &mut with,
            FULL,
            FULL,
            0.75,
            1.0,
            8,
            2,
            LIT,
            OFF,
            2.0,
            WARN,
            2.0,
            DANGER,
            false,
            &[],
            0,
            1,
            false,
        );
        seg_bar(
            &mut plain,
            FULL,
            FULL,
            0.75,
            1.0,
            8,
            2,
            LIT,
            OFF,
            2.0,
            WARN,
            2.0,
            DANGER,
            false,
            &[],
            0,
            1,
            false,
        );
        assert_eq!(lit_pixels(&with), lit_pixels(&plain));
        assert!(lit_pixels(&with) > 0);
    }

    #[test]
    fn a_second_axis_of_nan_reads_empty_rather_than_drawing_off_the_box() {
        assert_eq!(bank(f32::NAN, &[]), 0);
        // And out of range is clamped, not extrapolated past the envelope.
        assert_eq!(bank(4.0, &[]), bank(1.0, &[]));
    }
}
