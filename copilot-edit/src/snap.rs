// SPDX-License-Identifier: MIT OR Apache-2.0
//! Pulling a dragged widget onto the lines its neighbours already sit on.
//!
//! Geometry only, like [`crate::handle`]: no egui, no mouse, no scene text.
//! It adjusts the drag offset rather than the resulting rectangle, so that
//! [`crate::handle::resize`] stays the one place that knows which edge a
//! given handle moves.

use copilot::Rect;

use crate::handle::Handle;

/// The lines a drag snapped to, for drawing guides. `None` on an axis
/// means nothing was close enough on that axis.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Guides {
    pub x: Option<i32>,
    pub y: Option<i32>,
}

/// Adjusts a raw drag offset so that a relevant edge of the moving rect
/// lands exactly on a nearby target line, returning the corrected offset
/// and the guide lines that were used. The adjustment is chosen to be the
/// smallest nudge that achieves a snap, so the user's intended motion is
/// preserved as closely as possible while still giving the satisfying
/// "click" of alignment.
pub fn snap_delta(
    start: Rect,
    handle: Handle,
    dx: i32,
    dy: i32,
    targets: &[Rect],
    tol: i32,
) -> (i32, i32, Guides) {
    if tol <= 0 || targets.is_empty() {
        return (dx, dy, Guides::default());
    }

    let w_half = (start.size.w as i32) / 2;
    let h_half = (start.size.h as i32) / 2;

    // Build the list of moving lines on the x axis, in the order dictated
    // by the handle, so that tie-breaking follows the user's mental model
    // of which edge they are dragging.
    let (x_moving, x_count): ([i32; 3], usize) = match handle {
        Handle::Body => (
            [
                start.left().saturating_add(dx),
                start.left().saturating_add(w_half).saturating_add(dx),
                start.right().saturating_add(dx),
            ],
            3,
        ),
        Handle::Left | Handle::TopLeft | Handle::BottomLeft => {
            ([start.left().saturating_add(dx), 0, 0], 1)
        }
        Handle::Right | Handle::TopRight | Handle::BottomRight => {
            ([start.right().saturating_add(dx), 0, 0], 1)
        }
        Handle::Top | Handle::Bottom => ([0; 3], 0),
    };

    // Same for the y axis.
    let (y_moving, y_count): ([i32; 3], usize) = match handle {
        Handle::Body => (
            [
                start.top().saturating_add(dy),
                start.top().saturating_add(h_half).saturating_add(dy),
                start.bottom().saturating_add(dy),
            ],
            3,
        ),
        Handle::Top | Handle::TopLeft | Handle::TopRight => {
            ([start.top().saturating_add(dy), 0, 0], 1)
        }
        Handle::Bottom | Handle::BottomLeft | Handle::BottomRight => {
            ([start.bottom().saturating_add(dy), 0, 0], 1)
        }
        Handle::Left | Handle::Right => ([0; 3], 0),
    };

    // For each axis, scan every (moving line, target line) pair and keep
    // the one with the smallest absolute adjustment. Ties are broken by
    // first-found, which means the user's primary edge wins over a
    // coincidental secondary alignment.
    let mut best_x_adj: i32 = 0;
    let mut best_x_abs: i32 = tol.saturating_add(1);
    let mut best_x_target: Option<i32> = None;

    let mut best_y_adj: i32 = 0;
    let mut best_y_abs: i32 = tol.saturating_add(1);
    let mut best_y_target: Option<i32> = None;

    for target in targets {
        let t_left = target.left();
        let t_right = target.right();
        let t_top = target.top();
        let t_bottom = target.bottom();
        let tw_half = (target.size.w as i32) / 2;
        let th_half = (target.size.h as i32) / 2;

        // x target lines: left, centre, right
        let x_targets: [i32; 3] = [t_left, t_left.saturating_add(tw_half), t_right];

        // y target lines: top, centre, bottom
        let y_targets: [i32; 3] = [t_top, t_top.saturating_add(th_half), t_bottom];

        for &moving in &x_moving[..x_count] {
            for &xt in x_targets.iter() {
                let adj = xt.saturating_sub(moving);
                let abs = if adj < 0 { adj.saturating_neg() } else { adj };
                if abs <= tol && abs < best_x_abs {
                    best_x_abs = abs;
                    best_x_adj = adj;
                    best_x_target = Some(xt);
                }
            }
        }

        for &moving in &y_moving[..y_count] {
            for &yt in y_targets.iter() {
                let adj = yt.saturating_sub(moving);
                let abs = if adj < 0 { adj.saturating_neg() } else { adj };
                if abs <= tol && abs < best_y_abs {
                    best_y_abs = abs;
                    best_y_adj = adj;
                    best_y_target = Some(yt);
                }
            }
        }
    }

    let final_dx = if best_x_target.is_some() {
        dx.saturating_add(best_x_adj)
    } else {
        dx
    };
    let final_dy = if best_y_target.is_some() {
        dy.saturating_add(best_y_adj)
    } else {
        dy
    };

    (
        final_dx,
        final_dy,
        Guides {
            x: best_x_target,
            y: best_y_target,
        },
    )
}

/// Pull a drag onto a regular grid, when nothing nearer has claimed it.
///
/// Second to the neighbour snap rather than mixed with it, because the two
/// disagree on purpose: a grid says "every sixteen pixels" and a neighbour
/// says "level with that", and when a widget is being lined up with another
/// widget the grid's opinion is the one to drop. `already` says which axes the
/// neighbour snap has settled and must be left alone.
///
/// Returns the adjusted deltas and the axes this actually moved, so a caller
/// can tell a grid snap from no snap at all.
#[must_use]
pub fn snap_to_grid(
    start: Rect,
    handle: Handle,
    dx: i32,
    dy: i32,
    step: i32,
    already: Guides,
) -> (i32, i32) {
    if step <= 1 {
        return (dx, dy);
    }

    // The edge a grid should align is the one the hand is moving. A body drag
    // has no single edge, so its top-left is used: it is the corner a person
    // reads a widget's position from, and the one the inspector shows.
    let (mx, my) = match handle {
        Handle::Body | Handle::TopLeft | Handle::Top | Handle::Left => (start.left(), start.top()),
        Handle::TopRight | Handle::Right => (start.right(), start.top()),
        Handle::BottomLeft | Handle::Bottom => (start.left(), start.bottom()),
        Handle::BottomRight => (start.right(), start.bottom()),
    };

    let pull = |m: i32, d: i32| -> i32 {
        let at = m.saturating_add(d);
        // Rounded to the nearest line rather than floored, or every drag would
        // drift towards the origin by up to a step.
        let half = step / 2;
        let down = at.div_euclid(step).saturating_mul(step);
        let rest = at.sub_euclid_rem(step);
        let landed = if rest >= half {
            down.saturating_add(step)
        } else {
            down
        };
        d.saturating_add(landed.saturating_sub(at))
    };

    let out_x = if already.x.is_some() || matches!(handle, Handle::Top | Handle::Bottom) {
        dx
    } else {
        pull(mx, dx)
    };
    let out_y = if already.y.is_some() || matches!(handle, Handle::Left | Handle::Right) {
        dy
    } else {
        pull(my, dy)
    };
    (out_x, out_y)
}

/// `rem_euclid` on an `i32`, spelled out so a negative coordinate rounds the
/// same way a positive one does.
trait EuclidRem {
    fn sub_euclid_rem(self, step: i32) -> i32;
}

impl EuclidRem for i32 {
    fn sub_euclid_rem(self, step: i32) -> i32 {
        self.rem_euclid(step.max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rect being dragged: 100 wide, 40 tall, at (200, 100).
    const R: Rect = Rect::new(200, 100, 100, 40);

    /// A target whose left edge sits 3 to the right of R's.
    const NEAR_LEFT: Rect = Rect::new(203, 500, 50, 10);

    #[test]
    fn nothing_within_reach_leaves_the_drag_alone() {
        let far = [Rect::new(9000, 9000, 10, 10)];
        let (dx, dy, g) = snap_delta(R, Handle::Body, 7, -3, &far, 8);
        assert_eq!((dx, dy), (7, -3));
        assert_eq!(g, Guides::default());
    }

    #[test]
    fn no_targets_leaves_the_drag_alone() {
        let (dx, dy, g) = snap_delta(R, Handle::Body, 7, -3, &[], 8);
        assert_eq!((dx, dy), (7, -3));
        assert_eq!(g, Guides::default());
    }

    #[test]
    fn a_zero_tolerance_disables_snapping() {
        let (dx, dy, g) = snap_delta(R, Handle::Body, 1, 1, &[NEAR_LEFT], 0);
        assert_eq!((dx, dy), (1, 1));
        assert_eq!(g, Guides::default());
        let (dx, dy, g) = snap_delta(R, Handle::Body, 1, 1, &[NEAR_LEFT], -5);
        assert_eq!((dx, dy), (1, 1));
        assert_eq!(g, Guides::default());
    }

    #[test]
    fn a_body_drag_pulls_its_left_edge_onto_a_nearby_line() {
        // R.left() is 200; dragging by 1 puts it at 201, and the target's
        // left edge at 203 is 2 away, inside a tolerance of 8.
        let (dx, _, g) = snap_delta(R, Handle::Body, 1, 0, &[NEAR_LEFT], 8);
        assert_eq!(dx, 3, "the left edge should land on 203");
        assert_eq!(g.x, Some(203));
    }

    #[test]
    fn a_snap_reports_the_line_it_used() {
        let (_, dy, g) = snap_delta(R, Handle::Body, 0, 2, &[Rect::new(0, 104, 10, 10)], 8);
        assert_eq!(g.y, Some(104), "the top edge lands on the target's top");
        assert_eq!(dy, 4);
    }

    #[test]
    fn a_body_drag_can_centre_against_a_target() {
        // R is 100 wide, so its centre line starts at 250. The target's own
        // centre is 254, four away, while both its edges are far outside the
        // tolerance -- otherwise the edges tie with the centre and win on
        // first-found, and the test would not be about centring at all.
        let target = Rect::new(234, 0, 40, 10); // left 234, centre 254, right 274
        let (dx, _, g) = snap_delta(R, Handle::Body, 0, 0, &[target], 8);
        assert_eq!(dx, 4, "the centres should meet");
        assert_eq!(g.x, Some(254));
    }

    #[test]
    fn the_nearest_line_wins() {
        // Two candidates in range: 203 (2 away after a drag of 1) and 209
        // (8 away). The closer one must win.
        let targets = [Rect::new(209, 0, 10, 10), NEAR_LEFT];
        let (dx, _, g) = snap_delta(R, Handle::Body, 1, 0, &targets, 16);
        assert_eq!(g.x, Some(203));
        assert_eq!(dx, 3);
    }

    #[test]
    fn a_left_handle_snaps_its_left_edge_only() {
        let (dx, _, g) = snap_delta(R, Handle::Left, 1, 0, &[NEAR_LEFT], 8);
        assert_eq!((dx, g.x), (3, Some(203)));
        // Its right edge is at 300; a target there must not attract it.
        let (dx, _, g) = snap_delta(R, Handle::Left, 0, 0, &[Rect::new(302, 0, 10, 10)], 8);
        assert_eq!((dx, g.x), (0, None), "the right edge is not moving");
    }

    #[test]
    fn a_right_handle_snaps_its_right_edge_only() {
        // R.right() is 300; a target left edge at 302 is 2 away.
        let (dx, _, g) = snap_delta(R, Handle::Right, 0, 0, &[Rect::new(302, 0, 10, 10)], 8);
        assert_eq!((dx, g.x), (2, Some(302)));
        let (dx, _, g) = snap_delta(R, Handle::Right, 0, 0, &[NEAR_LEFT], 8);
        assert_eq!((dx, g.x), (0, None), "the left edge is not moving");
    }

    #[test]
    fn a_top_handle_snaps_its_top_edge_and_ignores_x() {
        let targets = [Rect::new(203, 103, 10, 10)];
        let (dx, dy, g) = snap_delta(R, Handle::Top, 5, 0, &targets, 8);
        assert_eq!((dy, g.y), (3, Some(103)), "the top edge should snap");
        assert_eq!((dx, g.x), (5, None), "a top drag must not move sideways");
    }

    #[test]
    fn a_bottom_handle_snaps_its_bottom_edge() {
        // R.bottom() is 140; a target top at 143 is 3 away.
        let (_, dy, g) = snap_delta(R, Handle::Bottom, 0, 0, &[Rect::new(0, 143, 10, 10)], 8);
        assert_eq!((dy, g.y), (3, Some(143)));
    }

    #[test]
    fn a_corner_snaps_on_both_axes() {
        let targets = [Rect::new(203, 103, 10, 10)];
        let (dx, dy, g) = snap_delta(R, Handle::TopLeft, 0, 0, &targets, 8);
        assert_eq!((dx, g.x), (3, Some(203)), "left edge");
        assert_eq!((dy, g.y), (3, Some(103)), "top edge");
    }

    #[test]
    fn the_far_corner_snaps_its_own_two_edges() {
        // BottomRight moves the right (300) and bottom (140) edges.
        let targets = [Rect::new(303, 0, 10, 10), Rect::new(0, 142, 10, 10)];
        let (dx, dy, g) = snap_delta(R, Handle::BottomRight, 0, 0, &targets, 8);
        assert_eq!((dx, g.x), (3, Some(303)));
        assert_eq!((dy, g.y), (2, Some(142)));
    }

    #[test]
    fn an_edge_handle_snaps_nothing_on_its_idle_axis() {
        for h in [Handle::Left, Handle::Right] {
            let (_, dy, g) = snap_delta(R, h, 0, 4, &[Rect::new(0, 103, 10, 10)], 8);
            assert_eq!((dy, g.y), (4, None), "{h:?} moved on y");
        }
        for h in [Handle::Top, Handle::Bottom] {
            let (dx, _, g) = snap_delta(R, h, 4, 0, &[Rect::new(203, 0, 10, 10)], 8);
            assert_eq!((dx, g.x), (4, None), "{h:?} moved on x");
        }
    }

    #[test]
    fn a_target_right_edge_is_a_candidate_too() {
        // The target's right edge is at 202, two from R's left.
        let (dx, _, g) = snap_delta(R, Handle::Body, 0, 0, &[Rect::new(190, 0, 12, 10)], 8);
        assert_eq!((dx, g.x), (2, Some(202)));
    }

    #[test]
    fn a_line_exactly_at_the_tolerance_still_snaps() {
        let (dx, _, g) = snap_delta(R, Handle::Body, 0, 0, &[Rect::new(208, 0, 10, 10)], 8);
        assert_eq!((dx, g.x), (8, Some(208)));
    }

    #[test]
    fn a_line_one_past_the_tolerance_does_not() {
        let (dx, _, g) = snap_delta(R, Handle::Body, 0, 0, &[Rect::new(209, 0, 10, 10)], 8);
        assert_eq!((dx, g.x), (0, None));
    }

    #[test]
    fn an_already_aligned_edge_stays_put() {
        let (dx, dy, g) = snap_delta(R, Handle::Body, 0, 0, &[Rect::new(200, 100, 10, 10)], 8);
        assert_eq!((dx, dy), (0, 0), "an exact match must not nudge anything");
        assert_eq!((g.x, g.y), (Some(200), Some(100)));
    }

    #[test]
    fn the_two_axes_are_decided_independently() {
        // Nothing in range on x, something in range on y.
        let (dx, dy, g) = snap_delta(R, Handle::Body, 40, 1, &[Rect::new(9000, 103, 10, 10)], 8);
        assert_eq!((dx, g.x), (40, None));
        assert_eq!((dy, g.y), (3, Some(103)));
    }

    #[test]
    fn an_extreme_drag_does_not_overflow() {
        let targets = [
            Rect::new(i32::MAX - 4, i32::MAX - 4, 8, 8),
            Rect::new(i32::MIN + 1, i32::MIN + 1, 8, 8),
        ];
        for h in [
            Handle::Body,
            Handle::TopLeft,
            Handle::BottomRight,
            Handle::Left,
            Handle::Bottom,
        ] {
            let _ = snap_delta(R, h, i32::MAX, i32::MAX, &targets, 8);
            let _ = snap_delta(R, h, i32::MIN, i32::MIN, &targets, 8);
            let _ = snap_delta(R, h, 0, 0, &targets, i32::MAX);
        }
    }

    #[test]
    fn an_extreme_rect_does_not_overflow() {
        let huge = Rect::new(i32::MAX - 10, i32::MAX - 10, u32::MAX, u32::MAX);
        let _ = snap_delta(huge, Handle::Body, 5, 5, &[R], 8);
        let _ = snap_delta(huge, Handle::BottomRight, -5, -5, &[R], 8);
    }

    // --- the grid ---

    #[test]
    fn a_grid_step_of_one_or_less_changes_nothing() {
        for step in [-4, 0, 1] {
            let (dx, dy) = snap_to_grid(R, Handle::Body, 7, 5, step, Guides::default());
            assert_eq!((dx, dy), (7, 5), "a step of {step} moved the drag");
        }
    }

    #[test]
    fn a_body_drag_lands_its_top_left_on_a_line() {
        // R is at (200, 100), which is already on a grid of 10. A drag of 3
        // has to come back to the line rather than sit between two.
        let (dx, dy) = snap_to_grid(R, Handle::Body, 3, 3, 10, Guides::default());
        assert_eq!((200 + dx) % 10, 0, "x landed off the grid");
        assert_eq!((100 + dy) % 10, 0, "y landed off the grid");
    }

    #[test]
    fn a_grid_rounds_to_the_nearest_line_not_the_one_below() {
        // Flooring would drag every widget towards the origin by up to a step.
        let (dx, _) = snap_to_grid(R, Handle::Body, 7, 0, 10, Guides::default());
        assert_eq!(200 + dx, 210, "rounded down instead of up");
        let (dx, _) = snap_to_grid(R, Handle::Body, 3, 0, 10, Guides::default());
        assert_eq!(200 + dx, 200, "rounded up instead of down");
    }

    #[test]
    fn a_negative_coordinate_rounds_the_same_way() {
        let far = Rect::new(-37, -37, 10, 10);
        let (dx, dy) = snap_to_grid(far, Handle::Body, 0, 0, 10, Guides::default());
        assert_eq!(-37 + dx, -40);
        assert_eq!(-37 + dy, -40);
    }

    #[test]
    fn a_right_handle_snaps_the_right_edge_to_the_grid() {
        // R.right() is 300. Dragging it by 4 should land on 300, not 304.
        let (dx, _) = snap_to_grid(R, Handle::Right, 4, 0, 10, Guides::default());
        assert_eq!(300 + dx, 300);
        let (dx, _) = snap_to_grid(R, Handle::Right, 7, 0, 10, Guides::default());
        assert_eq!(300 + dx, 310);
    }

    #[test]
    fn a_bottom_handle_snaps_the_bottom_edge() {
        // R.bottom() is 140.
        let (_, dy) = snap_to_grid(R, Handle::Bottom, 3, 0, 10, Guides::default());
        assert_eq!(140 + dy, 140);
    }

    #[test]
    fn an_edge_handle_leaves_its_idle_axis_alone() {
        let (dx, _) = snap_to_grid(R, Handle::Top, 7, 0, 10, Guides::default());
        assert_eq!(dx, 7, "a top drag moved sideways");
        let (_, dy) = snap_to_grid(R, Handle::Left, 0, 7, 10, Guides::default());
        assert_eq!(dy, 7, "a left drag moved vertically");
    }

    #[test]
    fn a_neighbour_snap_wins_over_the_grid() {
        // The two disagree on purpose: when a widget is being lined up with
        // another widget, the grid's opinion is the one to drop.
        let settled = Guides {
            x: Some(203),
            y: None,
        };
        let (dx, dy) = snap_to_grid(R, Handle::Body, 3, 3, 10, settled);
        assert_eq!(dx, 3, "the grid overrode a neighbour on x");
        assert_eq!((100 + dy) % 10, 0, "y should still have snapped");
    }

    #[test]
    fn an_extreme_drag_on_a_grid_does_not_overflow() {
        for h in [Handle::Body, Handle::BottomRight, Handle::Top, Handle::Left] {
            let _ = snap_to_grid(R, h, i32::MAX, i32::MAX, 16, Guides::default());
            let _ = snap_to_grid(R, h, i32::MIN, i32::MIN, 16, Guides::default());
            let _ = snap_to_grid(R, h, 0, 0, i32::MAX, Guides::default());
        }
        let huge = Rect::new(i32::MAX - 4, i32::MIN + 4, 8, 8);
        let _ = snap_to_grid(huge, Handle::BottomRight, 5, -5, 16, Guides::default());
    }
}
