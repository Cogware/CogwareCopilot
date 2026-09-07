// SPDX-License-Identifier: MIT OR Apache-2.0
//! Dial furniture: the pointer and the tick marks around it.
//!
//! Both take the same sweep description as [`super::draw`]'s arc -- degrees
//! clockwise from twelve o'clock -- so a needle, a scale and a ring stacked
//! in one node all agree about where a given value sits.

use crate::trig::{ONE, TURN, cos, sin};
use crate::{Color, Point, Rect, Surface};

use super::fill_rect;
use super::shape::{disc, line};

/// Draw a pointer from the centre of `at` out to the rim.
///
/// The hub goes on last. A line rasterised at an angle ends in a stair-step
/// that looks like a frayed thread at the pivot, where every needle in a
/// cluster converges and the eye is drawn; a disc painted over the top hides
/// all of them at once.
#[allow(clippy::too_many_arguments)] // Every one is a distinct dial property.
pub fn needle<S: Surface + ?Sized>(
    surface: &mut S,
    at: Rect,
    clip: Rect,
    start_deg: i32,
    end_deg: i32,
    value: f32,
    width: u32,
    color: Color,
    hub: u32,
    antialias: bool,
) {
    // Bail out before any trigonometry when the widget is wholly outside the
    // damage region or too small to contain a pivot, so we never issue a
    // rasterisation call that would be entirely clipped away.
    if at.intersection(clip).is_none() {
        return;
    }
    let w = at.size.w as i32;
    let h = at.size.h as i32;
    let radius = w.min(h) / 2;
    if radius <= 0 {
        return;
    }
    let cx = at.left() + w / 2;
    let cy = at.top() + h / 2;
    let to_brad = |deg: i32| -> i32 {
        ((deg as i64 * TURN as i64) / 360 - i64::from(TURN / 4))
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32
    };
    let start_brad = to_brad(start_deg);
    let end_brad = to_brad(end_deg);
    // The sweep is taken with saturating_sub so that a wildly out-of-range
    // end cannot wrap the fixed-point angle and send the needle to the wrong
    // side of the dial.
    let sweep = end_brad.saturating_sub(start_brad);
    // The fractional position is the only floating point in the module; it is
    // converted to an integer offset once and never touched again, keeping the
    // rest of the path in exact integer arithmetic.
    let offset = (value * sweep as f32) as i32;
    let angle = start_brad.saturating_add(offset);
    let dx = (i64::from(cos(angle)) * i64::from(radius)) / i64::from(ONE);
    let dy = (i64::from(sin(angle)) * i64::from(radius)) / i64::from(ONE);
    let tip = Point {
        x: (i64::from(cx) + dx).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        y: (i64::from(cy) + dy).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
    };
    if antialias {
        // From pixel centre to pixel centre, the same pixels the integer
        // path aims at, with the edges blended. The hub's half a pixel
        // matches the disc the integer path draws, which spans 2r + 1.
        let centre = (cx as f32 + 0.5, cy as f32 + 0.5);
        let end = (tip.x as f32 + 0.5, tip.y as f32 + 0.5);
        super::aa::line(surface, clip, centre, end, width.max(1) as f32, color);
        if hub > 0 {
            super::aa::disc(surface, clip, centre, hub as f32 + 0.5, color);
        }
        return;
    }
    line(surface, Point { x: cx, y: cy }, tip, width, color, clip);
    // The hub is drawn after the line so that its disc masks the ragged end of
    // the stroke at the pivot, giving a clean circular cap.
    if hub > 0 {
        disc(surface, Point { x: cx, y: cy }, hub as i32, color, clip);
    }
}

/// Draw evenly spaced tick marks around the rim of `at`.
///
/// Major ticks are longer and thicker rather than merely a different colour,
/// because a cluster is read at a glance and often in poor light: length
/// survives being seen out of the corner of an eye in a way that hue does not.
#[allow(clippy::too_many_arguments)] // Every one is a distinct dial property.
pub fn scale<S: Surface + ?Sized>(
    surface: &mut S,
    at: Rect,
    clip: Rect,
    start_deg: i32,
    end_deg: i32,
    ticks: u32,
    major_every: u32,
    length: u32,
    width: u32,
    color: Color,
    major_color: Color,
    antialias: bool,
) {
    // As with the needle, refuse to work when nothing is visible or the dial
    // is too small, and when there is nothing to draw.
    if at.intersection(clip).is_none() {
        return;
    }
    let w = at.size.w as i32;
    let h = at.size.h as i32;
    let radius = w.min(h) / 2;
    if radius <= 0 || ticks == 0 {
        return;
    }
    let cx = at.left() + w / 2;
    let cy = at.top() + h / 2;
    let to_brad = |deg: i32| -> i32 {
        ((deg as i64 * TURN as i64) / 360 - i64::from(TURN / 4))
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32
    };
    let start_brad = to_brad(start_deg);
    let end_brad = to_brad(end_deg);
    let sweep = end_brad.saturating_sub(start_brad);
    // A zero length or width would produce an invisible or degenerate stroke,
    // so both are floored to one pixel to guarantee a visible mark.
    let length = length.max(1);
    let width = width.max(1);
    for i in 0..ticks {
        // The per-tick angle is computed in i64 so that the product of the
        // sweep and the tick index cannot overflow for large dials.
        let angle = if ticks == 1 {
            start_brad
        } else {
            let t = i64::from(i);
            let denom = i64::from(ticks - 1);
            let step = (i64::from(sweep) * t) / denom;
            start_brad.saturating_add(step as i32)
        };
        let is_major = major_every > 0 && i % major_every == 0;
        // Major ticks are longer and thicker so the eye can pick them out
        // against the minor marks; the 3/2 factor is integer arithmetic.
        let tick_length = if is_major { length * 3 / 2 } else { length };
        let tick_width = if is_major { width + 1 } else { width };
        let tick_color = if is_major { major_color } else { color };
        let outer = point_at(cx, cy, angle, radius);
        // The inner distance is clamped at zero so an over-long tick stops at
        // the pivot instead of wrapping to the far side of the dial.
        let inner_dist = (radius - tick_length as i32).max(0);
        let inner = point_at(cx, cy, angle, inner_dist);
        if antialias {
            super::aa::line(
                surface,
                clip,
                (outer.x as f32 + 0.5, outer.y as f32 + 0.5),
                (inner.x as f32 + 0.5, inner.y as f32 + 0.5),
                tick_width as f32,
                tick_color,
            );
        } else {
            line(surface, outer, inner, tick_width, tick_color, clip);
        }
    }
}

fn point_at(cx: i32, cy: i32, angle: i32, dist: i32) -> Point {
    // The trigonometric products are taken in i64 and clamped back to i32 so
    // that an unusually large distance cannot overflow the coordinate space.
    let dx = (i64::from(cos(angle)) * i64::from(dist)) / i64::from(ONE);
    let dy = (i64::from(sin(angle)) * i64::from(dist)) / i64::from(ONE);
    Point {
        x: (i64::from(cx) + dx).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        y: (i64::from(cy) + dy).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
    }
}

/// Draws a ruler of tick marks along the near edge of a widget box, clipped to a damage region.
///
/// The function exists to give embedded UIs a deterministic, allocation-free way to render
/// measurement scales without relying on higher-level widget machinery.  Because the caller
/// supplies both the widget box and the damage region, the implementation can bail out
/// early when there is no visible overlap, avoiding any pixel writes that would be
/// immediately discarded by the compositor.
///
/// The tick count is bounded to a hard ceiling so that a misconfigured or hostile caller
/// cannot stall the frame for an unbounded number of iterations.  Each tick is drawn
/// inwards from the near edge and clipped against the damage region before being handed
/// to the surface, ensuring that no pixel is ever written outside the area the caller has
/// marked dirty.
#[allow(clippy::too_many_arguments)] // Every one is a distinct dial property.
pub fn ruler<S: Surface + ?Sized>(
    surface: &mut S,
    at: Rect,
    clip: Rect,
    ticks: u32,
    major_every: u32,
    length: u32,
    width: u32,
    color: Color,
    major_color: Color,
    vertical: bool,
) {
    // Bail out when the widget box is empty or does not overlap the damage region,
    // because no visible pixels could be produced in either case.
    if at.is_empty() || at.intersection(clip).is_none() || ticks == 0 {
        return;
    }

    // Clamp the thickness and length to at least one pixel so that a zero value does
    // not silently produce invisible marks, which would be confusing to the caller.
    let w = width.max(1) as i32;
    let len = length.max(1) as i32;

    // Bound the iteration count so that an absurd tick count cannot lock the frame up.
    let max_ticks = ticks.min(100000);

    // The span along the axis in which the ticks are spaced, used to compute positions.
    let span = if vertical {
        at.size.h as i64
    } else {
        at.size.w as i64
    };

    // The other dimension of the box, used to clamp tick length so a tick cannot spill
    // out of the widget box.
    let other_dim = if vertical {
        at.size.w as i32
    } else {
        at.size.h as i32
    };

    for i in 0..max_ticks {
        // Compute the position along the spacing axis, distributing ticks evenly across
        // the span.  When there is only one tick it is placed at the origin of the axis.
        let pos = if ticks == 1 {
            0
        } else {
            ((span - 1).max(0) * i as i64) / (ticks - 1) as i64
        };

        // Add the box's origin along the spacing axis, clamping to i32 range to avoid
        // overflow on extreme coordinates.
        let p = if vertical {
            (at.top() as i64 + pos).clamp(i32::MIN as i64, i32::MAX as i64) as i32
        } else {
            (at.left() as i64 + pos).clamp(i32::MIN as i64, i32::MAX as i64) as i32
        };

        // Determine whether this tick is major, which affects its length, thickness,
        // and colour.  A major tick is longer and thicker to make it visually distinct.
        let is_major = major_every > 0 && i % major_every == 0;
        let tick_len = if is_major {
            (len * 3 / 2).min(other_dim)
        } else {
            len.min(other_dim)
        };
        let thickness = if is_major { w + 1 } else { w };
        let tick_color = if is_major { major_color } else { color };

        // Skip the tick entirely when its colour is transparent, because drawing it
        // would have no visible effect and would waste a surface call.
        if tick_color.is_transparent() {
            continue;
        }

        // Build the tick rectangle inwards from the near edge.  For a horizontal ruler
        // the ticks hang down from the top edge; for a vertical ruler they extend to
        // the right from the left edge.
        let tick_rect = if vertical {
            Rect::new(at.left(), p, tick_len as u32, thickness as u32)
        } else {
            Rect::new(p, at.top(), thickness as u32, tick_len as u32)
        };

        // Against the widget's own box as well as the damage region. The last
        // mark sits on the far edge, so its thickness hangs past it -- and a
        // widget that paints outside its rectangle is one that cannot be
        // repainted from its own damage.
        let Some(clipped) = tick_rect
            .intersection(at)
            .and_then(|r| r.intersection(clip))
        else {
            continue;
        };
        if clipped.is_empty() {
            continue;
        }

        fill_rect(surface, clipped, tick_color);
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

    // ---- needle ----

    #[test]
    fn a_needle_at_the_top_points_up() {
        let mut s = surf();
        needle(&mut s, FULL, FULL, 0, 360, 0.0, 1, Color::WHITE, 0, false);
        // Twelve o'clock: the tip is above the pivot, on the pivot's column.
        assert!(lit(&s, 32, 4), "expected the tip near the top edge");
        assert!(!lit(&s, 32, 60), "nothing should be drawn below the pivot");
    }

    #[test]
    fn a_needle_at_a_quarter_sweep_points_right() {
        let mut s = surf();
        needle(&mut s, FULL, FULL, 0, 360, 0.25, 1, Color::WHITE, 0, false);
        assert!(lit(&s, 60, 32), "expected the tip near the right edge");
        assert!(!lit(&s, 4, 32), "nothing should be drawn left of the pivot");
    }

    #[test]
    fn a_needle_at_a_half_sweep_points_down() {
        let mut s = surf();
        needle(&mut s, FULL, FULL, 0, 360, 0.5, 1, Color::WHITE, 0, false);
        assert!(lit(&s, 32, 60), "expected the tip near the bottom edge");
        assert!(!lit(&s, 32, 4), "nothing should be drawn above the pivot");
    }

    #[test]
    fn a_needle_sweeps_the_other_way_when_end_is_less_than_start() {
        let mut s = surf();
        needle(&mut s, FULL, FULL, 0, -360, 0.25, 1, Color::WHITE, 0, false);
        assert!(lit(&s, 4, 32), "an anticlockwise quarter sweep points left");
    }

    #[test]
    fn a_needle_reaches_the_rim() {
        let mut s = surf();
        needle(&mut s, FULL, FULL, 0, 360, 0.25, 1, Color::WHITE, 0, false);
        // Radius is 32, pivot at x=32, so the tip lands at or just inside x=63.
        assert!(
            (60..64).any(|x| lit(&s, x, 32)),
            "the needle should run all the way out to the rim"
        );
    }

    #[test]
    fn a_needle_starts_at_the_pivot() {
        let mut s = surf();
        needle(&mut s, FULL, FULL, 0, 360, 0.25, 1, Color::WHITE, 0, false);
        assert!(lit(&s, 33, 32), "the needle should start at the pivot");
    }

    #[test]
    fn a_hub_fills_the_middle() {
        let mut bare = surf();
        let mut hubbed = surf();
        needle(
            &mut bare,
            FULL,
            FULL,
            0,
            360,
            0.25,
            1,
            Color::WHITE,
            0,
            false,
        );
        needle(
            &mut hubbed,
            FULL,
            FULL,
            0,
            360,
            0.25,
            1,
            Color::WHITE,
            6,
            false,
        );
        assert!(count(&hubbed) > count(&bare), "the hub adds pixels");
        assert!(
            lit(&hubbed, 32, 27),
            "the hub covers pixels above the pivot"
        );
        assert!(lit(&hubbed, 27, 32), "and to the left of it");
    }

    #[test]
    fn a_needle_respects_the_clip() {
        let mut s = surf();
        // A clip well away from the needle's quadrant.
        needle(
            &mut s,
            FULL,
            Rect::new(0, 0, 8, 8),
            0,
            360,
            0.25,
            1,
            Color::WHITE,
            8,
            false,
        );
        for y in 0..64 {
            for x in 0..64 {
                assert!(
                    !lit(&s, x, y) || (x < 8 && y < 8),
                    "drew at ({x}, {y}), outside the clip"
                );
            }
        }
    }

    #[test]
    fn a_needle_in_an_empty_box_draws_nothing() {
        let mut s = surf();
        needle(
            &mut s,
            Rect::new(10, 10, 0, 0),
            FULL,
            0,
            360,
            0.5,
            1,
            Color::WHITE,
            4,
            false,
        );
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn a_needle_with_absurd_angles_does_not_panic() {
        let mut s = surf();
        needle(
            &mut s,
            FULL,
            FULL,
            i32::MIN,
            i32::MAX,
            0.5,
            3,
            Color::WHITE,
            4,
            false,
        );
        needle(
            &mut s,
            FULL,
            FULL,
            i32::MAX,
            i32::MIN,
            1.0,
            1,
            Color::WHITE,
            0,
            false,
        );
    }

    #[test]
    fn a_thick_needle_covers_more_than_a_thin_one() {
        let mut thin = surf();
        let mut thick = surf();
        needle(
            &mut thin,
            FULL,
            FULL,
            0,
            360,
            0.25,
            1,
            Color::WHITE,
            0,
            false,
        );
        needle(
            &mut thick,
            FULL,
            FULL,
            0,
            360,
            0.25,
            5,
            Color::WHITE,
            0,
            false,
        );
        assert!(count(&thick) > count(&thin));
    }

    // ---- scale ----

    #[test]
    fn a_scale_draws_its_ticks_at_the_rim() {
        let mut s = surf();
        // Five, not four: the ends are inclusive, so the last tick lands back
        // on the first and a full turn needs five to reach the quarters.
        scale(
            &mut s,
            FULL,
            FULL,
            0,
            360,
            5,
            0,
            6,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert!((0..6).any(|y| lit(&s, 32, y)), "a tick at twelve o'clock");
        assert!((58..64).any(|x| lit(&s, x, 32)), "a tick at three o'clock");
    }

    #[test]
    fn a_scale_leaves_the_middle_alone() {
        let mut s = surf();
        scale(
            &mut s,
            FULL,
            FULL,
            0,
            360,
            12,
            0,
            6,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert!(!lit(&s, 32, 32), "ticks are rim furniture, not a pivot");
    }

    #[test]
    fn more_ticks_means_more_pixels() {
        let mut few = surf();
        let mut many = surf();
        scale(
            &mut few,
            FULL,
            FULL,
            0,
            360,
            4,
            0,
            6,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        scale(
            &mut many,
            FULL,
            FULL,
            0,
            360,
            16,
            0,
            6,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert!(count(&many) > count(&few));
    }

    #[test]
    fn major_ticks_are_longer_than_minor_ones() {
        let mut plain = surf();
        let mut marked = surf();
        scale(
            &mut plain,
            FULL,
            FULL,
            0,
            360,
            12,
            0,
            6,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        scale(
            &mut marked,
            FULL,
            FULL,
            0,
            360,
            12,
            3,
            6,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert!(
            count(&marked) > count(&plain),
            "every third tick should grow"
        );
    }

    #[test]
    fn a_scale_with_no_ticks_draws_nothing() {
        let mut s = surf();
        scale(
            &mut s,
            FULL,
            FULL,
            0,
            360,
            0,
            2,
            6,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn a_scale_of_one_tick_draws_it_at_the_start() {
        let mut s = surf();
        scale(
            &mut s,
            FULL,
            FULL,
            0,
            360,
            1,
            0,
            8,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert!(
            (0..8).any(|y| lit(&s, 32, y)),
            "the sole tick sits at the start angle"
        );
        assert!(count(&s) > 0);
    }

    #[test]
    fn a_scale_respects_the_clip() {
        let mut s = surf();
        scale(
            &mut s,
            FULL,
            Rect::new(0, 0, 10, 10),
            0,
            360,
            24,
            2,
            8,
            2,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        for y in 0..64 {
            for x in 0..64 {
                assert!(
                    !lit(&s, x, y) || (x < 10 && y < 10),
                    "drew at ({x}, {y}), outside the clip"
                );
            }
        }
    }

    #[test]
    fn a_scale_in_an_empty_box_draws_nothing() {
        let mut s = surf();
        scale(
            &mut s,
            Rect::new(5, 5, 0, 0),
            FULL,
            0,
            360,
            8,
            2,
            4,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn a_tick_longer_than_the_radius_stops_at_the_pivot() {
        // It must not run out the far side and turn the dial into a starburst.
        let mut s = surf();
        scale(
            &mut s,
            FULL,
            FULL,
            0,
            360,
            4,
            0,
            200,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert!(lit(&s, 32, 32), "a full-length tick reaches the pivot");
        // The tick at twelve o'clock must not continue below the pivot.
        assert!(!lit(&s, 32, 50), "and must not cross to the far side");
    }

    #[test]
    fn a_scale_with_absurd_angles_does_not_panic() {
        let mut s = surf();
        scale(
            &mut s,
            FULL,
            FULL,
            i32::MIN,
            i32::MAX,
            32,
            4,
            6,
            2,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        scale(
            &mut s,
            FULL,
            FULL,
            i32::MAX,
            i32::MIN,
            1,
            1,
            6,
            2,
            Color::WHITE,
            Color::WHITE,
            false,
        );
    }

    #[test]
    fn a_scale_with_an_absurd_tick_count_terminates() {
        let mut s = surf();
        scale(
            &mut s,
            FULL,
            FULL,
            0,
            360,
            4096,
            64,
            4,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert!(count(&s) > 0);
    }
}

#[cfg(test)]
mod ruler_tests {
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

    /// Which columns have any ink, for a horizontal ruler.
    fn columns(s: &MemorySurface) -> alloc::vec::Vec<usize> {
        (0..64).filter(|&x| (0..64).any(|y| lit(s, x, y))).collect()
    }

    #[test]
    fn a_ruler_puts_a_mark_at_each_end() {
        let mut s = surf();
        ruler(
            &mut s,
            FULL,
            FULL,
            5,
            0,
            8,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        let cols = columns(&s);
        assert_eq!(cols.first(), Some(&0), "no mark at the start");
        assert!(
            cols.last().is_some_and(|&x| x >= 62),
            "no mark at the end: {cols:?}"
        );
    }

    #[test]
    fn the_marks_are_evenly_spaced() {
        let mut s = surf();
        ruler(
            &mut s,
            FULL,
            FULL,
            5,
            0,
            8,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        let cols = columns(&s);
        assert_eq!(cols.len(), 5, "expected five marks: {cols:?}");
        let gaps: alloc::vec::Vec<usize> = cols.windows(2).map(|w| w[1] - w[0]).collect();
        let first = gaps[0];
        assert!(
            gaps.iter().all(|g| g.abs_diff(first) <= 1),
            "uneven spacing: {gaps:?}"
        );
    }

    #[test]
    fn a_ruler_draws_inwards_from_its_near_edge() {
        let mut s = surf();
        ruler(
            &mut s,
            FULL,
            FULL,
            3,
            0,
            10,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert!(lit(&s, 0, 0), "the mark should start at the top edge");
        assert!(lit(&s, 0, 9), "and run ten pixels in");
        assert!(!lit(&s, 0, 40), "not the whole height");
    }

    #[test]
    fn a_vertical_ruler_runs_down_the_left_edge() {
        let mut s = surf();
        ruler(
            &mut s,
            FULL,
            FULL,
            3,
            0,
            10,
            1,
            Color::WHITE,
            Color::WHITE,
            true,
        );
        assert!(lit(&s, 0, 0), "the first mark should be at the top-left");
        assert!(lit(&s, 9, 0), "and run ten pixels across");
        assert!(!lit(&s, 40, 0), "not the whole width");
        let rows: alloc::vec::Vec<usize> = (0..64)
            .filter(|&y| (0..64).any(|x| lit(&s, x, y)))
            .collect();
        assert_eq!(rows.len(), 3, "expected three marks down: {rows:?}");
    }

    #[test]
    fn major_marks_are_longer_and_thicker() {
        let mut plain = surf();
        let mut marked = surf();
        ruler(
            &mut plain,
            FULL,
            FULL,
            11,
            0,
            6,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        ruler(
            &mut marked,
            FULL,
            FULL,
            11,
            5,
            6,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert!(count(&marked) > count(&plain), "the majors added nothing");
    }

    #[test]
    fn more_marks_means_more_ink() {
        let mut few = surf();
        let mut many = surf();
        ruler(
            &mut few,
            FULL,
            FULL,
            3,
            0,
            6,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        ruler(
            &mut many,
            FULL,
            FULL,
            9,
            0,
            6,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert!(count(&many) > count(&few));
    }

    #[test]
    fn a_single_mark_sits_at_the_start() {
        let mut s = surf();
        ruler(
            &mut s,
            FULL,
            FULL,
            1,
            0,
            8,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert_eq!(columns(&s), alloc::vec![0]);
    }

    #[test]
    fn no_marks_draws_nothing() {
        let mut s = surf();
        ruler(
            &mut s,
            FULL,
            FULL,
            0,
            2,
            8,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn a_transparent_colour_draws_nothing() {
        let mut s = surf();
        ruler(
            &mut s,
            FULL,
            FULL,
            9,
            0,
            8,
            1,
            Color::TRANSPARENT,
            Color::TRANSPARENT,
            false,
        );
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn a_mark_longer_than_the_box_stays_inside_it() {
        let mut s = surf();
        let at = Rect::new(10, 10, 20, 20);
        ruler(
            &mut s,
            at,
            FULL,
            5,
            2,
            500,
            2,
            Color::WHITE,
            Color::WHITE,
            false,
        );
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
    fn a_ruler_respects_the_clip() {
        let mut s = surf();
        ruler(
            &mut s,
            FULL,
            Rect::new(0, 0, 12, 12),
            17,
            4,
            10,
            2,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        for y in 0..64 {
            for x in 0..64 {
                assert!(!lit(&s, x, y) || (x < 12 && y < 12), "drew at ({x}, {y})");
            }
        }
    }

    #[test]
    fn an_empty_box_draws_nothing() {
        let mut s = surf();
        ruler(
            &mut s,
            Rect::new(5, 5, 0, 0),
            FULL,
            9,
            2,
            6,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert_eq!(count(&s), 0);
    }

    #[test]
    fn an_absurd_mark_count_terminates() {
        let mut s = surf();
        ruler(
            &mut s,
            FULL,
            FULL,
            u32::MAX,
            1000,
            6,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        assert!(count(&s) > 0);
    }

    #[test]
    fn a_box_at_the_edge_of_the_range_does_not_overflow() {
        let mut s = surf();
        ruler(
            &mut s,
            Rect::new(i32::MAX - 8, 0, 64, 64),
            FULL,
            9,
            2,
            6,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
        ruler(
            &mut s,
            Rect::new(i32::MIN / 2, 0, u32::MAX, 64),
            FULL,
            9,
            2,
            6,
            1,
            Color::WHITE,
            Color::WHITE,
            false,
        );
    }
}
