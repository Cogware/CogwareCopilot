// SPDX-License-Identifier: GPL-3.0-only
//! Seven-segment digits, the way an instrument panel shows a number.
//!
//! Not a font. A bitmap glyph magnified is a picture of a digit; this is the
//! digit, drawn from the seven bars a real display has, so it scales to any
//! size with clean edges and can show its unlit segments -- which is most of
//! what makes a panel read as a panel rather than as text on a screen.

use crate::{Color, Rect, Surface};

use super::fill_rect;

/// Maps each printable character to the bitmask of segments it illuminates.
fn segments(ch: char) -> u8 {
    match ch {
        '0' => 0x3F,
        '1' => 0x06,
        '2' => 0x5B,
        '3' => 0x4F,
        '4' => 0x66,
        '5' => 0x6D,
        '6' => 0x7D,
        '7' => 0x07,
        '8' => 0x7F,
        '9' => 0x6F,
        '-' => 0x40,
        ' ' => 0x00,
        _ => 0x00,
    }
}

/// One cell of a readout: a digit, and whether its decimal point is lit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Cell {
    /// The character whose segments this cell lights.
    glyph: char,
    /// Whether the point after this digit is lit.
    point: bool,
}

/// The cells `text` draws as.
///
/// A `.` attaches to the digit before it; one with no digit before it gets a
/// blank cell to sit on, so `.5` is two cells rather than a point with
/// nowhere to go.
/// The most cells a readout is built from at once.
///
/// Not a limit on what a caller may pass: overflowing this does what
/// overflowing the field does, and keeps the low-order end.
const MAX_CELLS: usize = 16;

/// Lay `text` out into `out`, returning how many cells were written.
///
/// A `.` attaches to the digit before it, and one with no digit before it gets
/// a blank cell to sit on. Fills a caller's array rather than returning a
/// `Vec` because this runs once per readout per frame.
fn cells_into(text: &str, out: &mut [Cell; MAX_CELLS]) -> usize {
    let mut n: usize = 0;
    for c in text.chars() {
        // A point folds into the cell before it, the one case that does not
        // advance the count.
        if c == '.'
            && let Some(last) = n.checked_sub(1)
            && !out[last].point
        {
            out[last].point = true;
            continue;
        }
        let cell = if c == '.' {
            Cell {
                glyph: ' ',
                point: true,
            }
        } else {
            Cell {
                glyph: c,
                point: false,
            }
        };
        if n == MAX_CELLS {
            // Past the end, keep shifting: the low-order digits are the ones
            // that still move.
            out.rotate_left(1);
            out[MAX_CELLS - 1] = cell;
        } else {
            out[n] = cell;
            n += 1;
        }
    }
    n
}

/// Draw `text` as a seven-segment readout, clipped to the damage region.
///
/// `digits` sets the field width and zero sizes it to the text. A readout
/// showing a decimal point anywhere shows its unlit points on every cell.
#[allow(clippy::too_many_arguments)] // Each one is a distinct property of the readout.
pub fn seven_seg<S: Surface + ?Sized>(
    surface: &mut S,
    at: Rect,
    clip: Rect,
    text: &str,
    lit: Color,
    ghost: Color,
    thickness: u32,
    digits: u32,
) {
    if at.intersection(clip).is_none() || at.is_empty() {
        return;
    }

    // Right-aligned into the field, keeping the low-order digits when the
    // value has outgrown it: a number too big for its display is a scene that
    // needs a wider one, and the last digits are the ones that still move.
    let mut buf = [Cell {
        glyph: ' ',
        point: false,
    }; MAX_CELLS];
    let written = cells_into(text, &mut buf);
    let n = match digits {
        0 => written,
        d => d as usize,
    };
    if n == 0 {
        return;
    }
    // The low-order end, when the value has outgrown its field.
    let first = written.saturating_sub(n);
    let cells = &buf[first..written];
    let lead = n - cells.len();

    let cell_w = at.size.w as i32 / n as i32;
    let t = thickness.max(1) as i32;

    if cell_w - t < 3 * t + 2 || (at.size.h as i32) < 3 * t + 2 {
        return;
    }

    let ch = at.size.h as i32;
    let mid = (ch - t) / 2;
    // A bar's width of air between digits. Cells drawn edge to edge put one
    // digit's right-hand verticals against the next one's left-hand ones, and
    // "88" comes out as a single lattice rather than two eights. It is also
    // where the decimal point goes.
    let seg_w = cell_w - t;
    let any_point = cells.iter().any(|c| c.point);

    for i in 0..n {
        // The blank cells are the leading ones, so a short number sits at the
        // right-hand end of its field where a reader expects the units digit.
        let cell = i
            .checked_sub(lead)
            .and_then(|j| cells.get(j))
            .copied()
            .unwrap_or(Cell {
                glyph: ' ',
                point: false,
            });
        let mask = segments(cell.glyph);
        let x = (at.left() as i64 + i as i64 * cell_w as i64)
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let y = at.top();

        // The point, in the air to the right of the digit and on its baseline.
        if any_point {
            let col = if cell.point { lit } else { ghost };
            if !col.is_transparent() {
                let dot = Rect::new(x + seg_w, y + ch - t, t.max(0) as u32, t.max(0) as u32);
                if let Some(inter) = dot.intersection(clip)
                    && !inter.is_empty()
                {
                    fill_rect(surface, inter, col);
                }
            }
        }

        let segs: [(i32, i32, i32, i32); 7] = [
            (x + t, y, seg_w - 2 * t, t),
            (x + seg_w - t, y + t, t, mid - t),
            (x + seg_w - t, y + mid + t, t, ch - mid - 2 * t),
            (x + t, y + ch - t, seg_w - 2 * t, t),
            (x, y + mid + t, t, ch - mid - 2 * t),
            (x, y + t, t, mid - t),
            (x + t, y + mid, seg_w - 2 * t, t),
        ];

        for (bit, (sx, sy, sw, sh)) in segs.into_iter().enumerate() {
            let col = if mask & (1 << bit) != 0 { lit } else { ghost };
            if col.is_transparent() {
                continue;
            }
            let r = Rect::new(sx, sy, sw.max(0) as u32, sh.max(0) as u32);
            if let Some(inter) = r.intersection(clip)
                && !inter.is_empty()
            {
                fill_rect(surface, inter, col);
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

    fn lit_at(s: &MemorySurface, x: usize, y: usize) -> bool {
        let o = y * s.stride() + x * 4;
        s.pixels()[o..o + 4] != [0, 0, 0, 0]
    }

    fn count(s: &MemorySurface) -> usize {
        (0..64)
            .flat_map(|y| (0..64).map(move |x| (x, y)))
            .filter(|&(x, y)| lit_at(s, x, y))
            .count()
    }

    /// Draw one character across the whole surface and count its pixels.
    fn ink(ch: char) -> usize {
        let mut s = surf();
        let mut buf = [0u8; 4];
        seven_seg(
            &mut s,
            FULL,
            FULL,
            ch.encode_utf8(&mut buf),
            Color::WHITE,
            Color::TRANSPARENT,
            4,
            0,
        );
        count(&s)
    }

    // --- the segment table ---

    #[test]
    fn the_digits_light_the_segments_a_calculator_lights() {
        for (ch, want) in [
            ('0', 0x3F),
            ('1', 0x06),
            ('2', 0x5B),
            ('3', 0x4F),
            ('4', 0x66),
            ('5', 0x6D),
            ('6', 0x7D),
            ('7', 0x07),
            ('8', 0x7F),
            ('9', 0x6F),
        ] {
            assert_eq!(segments(ch), want, "{ch} lit the wrong segments");
        }
    }

    #[test]
    fn eight_lights_everything_and_a_space_lights_nothing() {
        assert_eq!(segments('8'), 0x7F);
        assert_eq!(segments(' '), 0x00);
        assert_eq!(segments('-'), 0x40, "a minus is the middle bar alone");
    }

    #[test]
    fn an_unknown_character_is_blank_rather_than_a_guess() {
        for ch in ['z', '@', 'Q', '\n', '\u{1F600}'] {
            assert_eq!(segments(ch), 0x00, "{ch:?} invented a shape");
        }
    }

    #[test]
    fn every_digit_is_a_distinct_shape() {
        // Two digits sharing a mask would be indistinguishable on a dashboard.
        let mut seen = alloc::vec::Vec::new();
        for ch in "0123456789".chars() {
            let m = segments(ch);
            assert!(!seen.contains(&m), "{ch} duplicates another digit");
            seen.push(m);
        }
    }

    // --- drawing ---

    #[test]
    fn eight_covers_more_than_any_other_digit() {
        let eight = ink('8');
        for ch in "0123456794-".chars() {
            assert!(eight > ink(ch), "{ch} drew at least as much as 8");
        }
    }

    #[test]
    fn a_one_is_the_thinnest_digit() {
        let one = ink('1');
        assert!(one > 0, "a one drew nothing");
        for ch in "023456890".chars() {
            assert!(one < ink(ch), "{ch} drew no more than a 1");
        }
    }

    #[test]
    fn a_space_draws_nothing_when_the_ghost_is_invisible() {
        assert_eq!(ink(' '), 0);
    }

    #[test]
    fn a_lit_segment_and_an_unlit_one_look_different() {
        // The trap: colours are written, not blended -- this crate's surfaces
        // have no alpha to blend against. A ghost given as a low-alpha version
        // of the lit colour lands as the lit colour, and every digit comes out
        // an eight.
        let mut s = surf();
        seven_seg(
            &mut s,
            FULL,
            FULL,
            "1",
            Color::WHITE,
            Color::rgb(40, 40, 40),
            4,
            0,
        );
        let mut seen = alloc::vec::Vec::new();
        for y in 0..64 {
            for x in 0..64 {
                let o = y * s.stride() + x * 4;
                let p: [u8; 4] = s.pixels()[o..o + 4].try_into().unwrap();
                if p != [0, 0, 0, 0] && !seen.contains(&p) {
                    seen.push(p);
                }
            }
        }
        assert_eq!(seen.len(), 2, "expected a lit and an unlit shade: {seen:?}");
    }

    #[test]
    fn a_ghost_draws_the_unlit_segments() {
        // An unlit readout should still read as a readout, the way a real
        // panel's dark segments catch the light.
        let mut bare = surf();
        let mut ghosted = surf();
        seven_seg(
            &mut bare,
            FULL,
            FULL,
            "1",
            Color::WHITE,
            Color::TRANSPARENT,
            4,
            0,
        );
        seven_seg(
            &mut ghosted,
            FULL,
            FULL,
            "1",
            Color::WHITE,
            Color::WHITE,
            4,
            0,
        );
        assert!(count(&ghosted) > count(&bare));
        // With every segment drawn, a 1 and an 8 cover the same ground.
        let mut eight = surf();
        seven_seg(
            &mut eight,
            FULL,
            FULL,
            "8",
            Color::WHITE,
            Color::WHITE,
            4,
            0,
        );
        assert_eq!(count(&ghosted), count(&eight));
    }

    #[test]
    fn digits_are_laid_out_left_to_right() {
        let mut s = surf();
        // A one lights only the right-hand verticals, so with two characters
        // the ink must fall in the right half of each cell.
        seven_seg(
            &mut s,
            FULL,
            FULL,
            " 1",
            Color::WHITE,
            Color::TRANSPARENT,
            3,
            0,
        );
        let left_half = (0..32).any(|x| (0..64).any(|y| lit_at(&s, x, y)));
        let right_half = (32..64).any(|x| (0..64).any(|y| lit_at(&s, x, y)));
        assert!(!left_half, "a leading space drew something");
        assert!(right_half, "the second cell drew nothing");
    }

    #[test]
    fn more_digits_means_narrower_cells() {
        let mut one = surf();
        let mut four = surf();
        seven_seg(
            &mut one,
            FULL,
            FULL,
            "8",
            Color::WHITE,
            Color::TRANSPARENT,
            3,
            0,
        );
        seven_seg(
            &mut four,
            FULL,
            FULL,
            "8888",
            Color::WHITE,
            Color::TRANSPARENT,
            3,
            0,
        );
        // Four digits in the same box: each is a quarter as wide, so the top
        // bar of each is much shorter than the single digit's.
        assert!(count(&four) < count(&one) * 4);
        assert!(count(&four) > 0);
    }

    #[test]
    fn a_thicker_segment_covers_more() {
        let mut thin = surf();
        let mut thick = surf();
        seven_seg(
            &mut thin,
            FULL,
            FULL,
            "8",
            Color::WHITE,
            Color::TRANSPARENT,
            2,
            0,
        );
        seven_seg(
            &mut thick,
            FULL,
            FULL,
            "8",
            Color::WHITE,
            Color::TRANSPARENT,
            6,
            0,
        );
        assert!(count(&thick) > count(&thin));
    }

    #[test]
    fn a_readout_stays_inside_its_box() {
        let mut s = surf();
        let at = Rect::new(10, 10, 40, 40);
        seven_seg(&mut s, at, FULL, "80", Color::WHITE, Color::WHITE, 3, 0);
        for y in 0..64 {
            for x in 0..64 {
                assert!(
                    !lit_at(&s, x, y) || (10..50).contains(&x) && (10..50).contains(&y),
                    "drew at ({x}, {y}), outside the box"
                );
            }
        }
    }

    #[test]
    fn a_readout_respects_the_clip() {
        let mut s = surf();
        seven_seg(
            &mut s,
            FULL,
            Rect::new(0, 0, 20, 20),
            "88",
            Color::WHITE,
            Color::WHITE,
            3,
            0,
        );
        for y in 0..64 {
            for x in 0..64 {
                assert!(
                    !lit_at(&s, x, y) || (x < 20 && y < 20),
                    "drew at ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn a_box_too_small_for_the_segments_draws_nothing() {
        // Better blank than a smear: three bars and two gaps do not fit, and
        // whatever came out would not be a digit.
        let mut s = surf();
        seven_seg(
            &mut s,
            Rect::new(0, 0, 6, 6),
            FULL,
            "8",
            Color::WHITE,
            Color::WHITE,
            4,
            0,
        );
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn an_empty_box_draws_nothing() {
        let mut s = surf();
        seven_seg(
            &mut s,
            Rect::new(5, 5, 0, 0),
            FULL,
            "8",
            Color::WHITE,
            Color::WHITE,
            2,
            0,
        );
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn empty_text_draws_nothing() {
        let mut s = surf();
        seven_seg(&mut s, FULL, FULL, "", Color::WHITE, Color::WHITE, 2, 0);
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn a_long_string_in_a_wide_box_does_not_panic() {
        let mut s = surf();
        let many = "8".repeat(500);
        seven_seg(
            &mut s,
            Rect::new(-1000, 0, u32::MAX / 2, 64),
            FULL,
            &many,
            Color::WHITE,
            Color::WHITE,
            2,
            0,
        );
        seven_seg(
            &mut s,
            Rect::new(i32::MAX - 8, 0, 64, 64),
            FULL,
            "88",
            Color::WHITE,
            Color::WHITE,
            2,
            0,
        );
    }

    // --- the decimal point ---

    /// The cells `text` lays out as, for the tests that care about layout
    /// rather than pixels.
    fn cells(text: &str) -> alloc::vec::Vec<Cell> {
        let mut buf = [Cell {
            glyph: ' ',
            point: false,
        }; MAX_CELLS];
        let n = cells_into(text, &mut buf);
        buf[..n].to_vec()
    }

    #[test]
    fn a_point_rides_on_a_digit_rather_than_taking_a_cell_of_its_own() {
        // The whole reason an AFR gauge fits: "14.7" is three cells, not four,
        // so the digits sit where a three-digit field puts them.
        let got = cells("14.7");
        assert_eq!(got.len(), 3);
        assert_eq!(
            got[1],
            Cell {
                glyph: '4',
                point: true
            }
        );
        assert_eq!(
            got[2],
            Cell {
                glyph: '7',
                point: false
            }
        );
    }

    #[test]
    fn a_point_with_no_digit_before_it_gets_a_blank_to_sit_on() {
        let got = cells(".5");
        assert_eq!(got.len(), 2);
        assert_eq!(
            got[0],
            Cell {
                glyph: ' ',
                point: true
            }
        );
        // And a second point in a row starts another cell rather than being
        // swallowed, so nonsense input stays visible as nonsense.
        assert_eq!(cells("8..").len(), 2);
    }

    #[test]
    fn a_value_longer_than_the_buffer_keeps_its_low_order_end() {
        // The same thing an overlong value does to a field it does not fit:
        // the digits that still move are the last ones.
        let long: alloc::string::String = (0..MAX_CELLS + 4)
            .map(|i| char::from(b'0' + (i % 10) as u8))
            .collect();
        let got = cells(&long);
        assert_eq!(got.len(), MAX_CELLS);
        let want: alloc::vec::Vec<char> = long.chars().skip(4).collect();
        let have: alloc::vec::Vec<char> = got.iter().map(|c| c.glyph).collect();
        assert_eq!(have, want);
    }

    #[test]
    fn a_point_draws_and_a_readout_without_one_draws_none() {
        let mut with = surf();
        let mut without = surf();
        seven_seg(
            &mut with,
            FULL,
            FULL,
            "8.8",
            Color::WHITE,
            Color::TRANSPARENT,
            3,
            0,
        );
        seven_seg(
            &mut without,
            FULL,
            FULL,
            "88",
            Color::WHITE,
            Color::TRANSPARENT,
            3,
            0,
        );
        assert!(
            count(&with) > count(&without),
            "the point drew nothing: {} vs {}",
            count(&with),
            count(&without)
        );
        // It sits on the baseline, in the air after the digit it belongs to.
        let bottom = (0..64).any(|x| lit_at(&with, x, 62));
        assert!(bottom, "the point is not on the baseline");
    }

    #[test]
    fn the_unlit_points_appear_only_on_a_readout_that_has_one() {
        // A display with decimal points has them whether or not they are lit;
        // one that never shows a fraction should not grow dark dots.
        let mut decimal = surf();
        let mut whole = surf();
        seven_seg(
            &mut decimal,
            FULL,
            FULL,
            "8.8",
            Color::WHITE,
            Color::WHITE,
            3,
            0,
        );
        seven_seg(
            &mut whole,
            FULL,
            FULL,
            "88",
            Color::WHITE,
            Color::WHITE,
            3,
            0,
        );
        assert!(count(&decimal) > count(&whole), "no ghosted points");

        let mut ghosted = surf();
        let mut lit = surf();
        seven_seg(
            &mut ghosted,
            FULL,
            FULL,
            "8.8",
            Color::WHITE,
            Color::WHITE,
            3,
            0,
        );
        seven_seg(
            &mut lit,
            FULL,
            FULL,
            "8.8.",
            Color::WHITE,
            Color::WHITE,
            3,
            0,
        );
        assert_eq!(
            count(&ghosted),
            count(&lit),
            "a ghosted point covers the same ground as a lit one"
        );
    }

    // --- a fixed field ---

    #[test]
    fn a_fixed_field_keeps_its_cells_wherever_the_number_is() {
        // The bug this fixes: without it, "9" and "188" are drawn at two
        // different cell widths in one box, and a speedometer rearranges
        // itself as it crosses a hundred.
        let mut one = surf();
        let mut three = surf();
        seven_seg(
            &mut one,
            FULL,
            FULL,
            "8",
            Color::WHITE,
            Color::TRANSPARENT,
            3,
            3,
        );
        seven_seg(
            &mut three,
            FULL,
            FULL,
            "888",
            Color::WHITE,
            Color::TRANSPARENT,
            3,
            3,
        );
        // One eight in a three-cell field covers a third of what three do.
        let (a, b) = (count(&one), count(&three));
        assert!(
            a > 0 && b > 2 * a,
            "{a} and {b} are not one and three cells"
        );
        // And the lone digit is at the right-hand end, where the units go.
        assert!(
            !(0..21).any(|x| (0..64).any(|y| lit_at(&one, x, y))),
            "a one-digit value did not sit at the right of its field"
        );
        assert!((43..64).any(|x| (0..64).any(|y| lit_at(&one, x, y))));
    }

    #[test]
    fn a_value_too_big_for_its_field_keeps_the_digits_that_still_move() {
        let mut wide = surf();
        let mut narrow = surf();
        seven_seg(
            &mut wide,
            FULL,
            FULL,
            "88",
            Color::WHITE,
            Color::TRANSPARENT,
            3,
            2,
        );
        seven_seg(
            &mut narrow,
            FULL,
            FULL,
            "188",
            Color::WHITE,
            Color::TRANSPARENT,
            3,
            2,
        );
        assert_eq!(
            count(&wide),
            count(&narrow),
            "188 in two cells should draw the last two digits"
        );
    }

    #[test]
    fn a_field_wider_than_the_text_is_blank_on_the_left_not_dark() {
        // The spare cells are unlit segments, not nothing: a four-digit panel
        // showing "42" still looks like a four-digit panel.
        let mut s = surf();
        seven_seg(
            &mut s,
            FULL,
            FULL,
            "42",
            Color::WHITE,
            Color::rgb(40, 40, 40),
            3,
            4,
        );
        let left = (0..16).any(|x| (0..64).any(|y| lit_at(&s, x, y)));
        assert!(left, "the leading cell drew no ghost");
    }

    #[test]
    fn a_zero_digits_field_still_sizes_itself_to_the_text() {
        // The default, and what every scene written before the field existed
        // relies on.
        let mut fixed = surf();
        let mut auto = surf();
        seven_seg(
            &mut fixed,
            FULL,
            FULL,
            "88",
            Color::WHITE,
            Color::TRANSPARENT,
            3,
            2,
        );
        seven_seg(
            &mut auto,
            FULL,
            FULL,
            "88",
            Color::WHITE,
            Color::TRANSPARENT,
            3,
            0,
        );
        assert_eq!(count(&fixed), count(&auto));
        // Empty text in a fixed field is a field of blanks, not nothing.
        let mut blank = surf();
        seven_seg(&mut blank, FULL, FULL, "", Color::WHITE, Color::WHITE, 3, 3);
        assert!(count(&blank) > 0, "an empty fixed field drew nothing");
    }

    #[test]
    fn a_zero_thickness_still_draws_something() {
        let mut s = surf();
        seven_seg(
            &mut s,
            FULL,
            FULL,
            "8",
            Color::WHITE,
            Color::TRANSPARENT,
            0,
            0,
        );
        assert!(count(&s) > 0, "a thickness of zero drew nothing at all");
    }
}
