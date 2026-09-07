// SPDX-License-Identifier: MIT OR Apache-2.0
//! Selection handles, and what dragging one does to a rectangle.
//!
//! Geometry only: it knows nothing about egui, the mouse, or the scene text.
//! That is what lets it be tested exhaustively without a window.

use copilot::{Point, Rect};

/// Which part of a selection box a drag has grabbed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handle {
    /// The whole widget: a drag moves it.
    Body,
    /// One of the eight edge or corner handles.
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

/// The handle under `point` for a selection at `rect`, if any.
///
/// Corners are tested before edges, and edges before the body, so the
/// smaller target wins where they overlap.
pub fn hit_handle(rect: Rect, point: Point, grab: i32) -> Option<Handle> {
    let left = rect.left();
    let top = rect.top();
    let right = rect.right();
    let bottom = rect.bottom();
    let w = rect.size.w as i32;
    let h = rect.size.h as i32;

    let mid_x = left + w / 2;
    let mid_y = top + h / 2;

    // Anchor points for each handle, in test order (corners first, then edges).
    let anchors: &[(Handle, i32, i32)] = &[
        (Handle::TopLeft, left, top),
        (Handle::TopRight, right, top),
        (Handle::BottomLeft, left, bottom),
        (Handle::BottomRight, right, bottom),
        (Handle::Top, mid_x, top),
        (Handle::Bottom, mid_x, bottom),
        (Handle::Left, left, mid_y),
        (Handle::Right, right, mid_y),
    ];

    for &(handle, hx, hy) in anchors {
        if point.x >= hx - grab
            && point.x <= hx + grab
            && point.y >= hy - grab
            && point.y <= hy + grab
        {
            return Some(handle);
        }
    }

    if rect.contains(point) {
        return Some(Handle::Body);
    }

    None
}

/// Apply a drag of (dx, dy) on `handle` to `rect`.
///
/// Returns the new rectangle. Width and height never go below 1.
pub fn resize(rect: Rect, handle: Handle, dx: i32, dy: i32) -> Rect {
    let mut x = rect.left();
    let mut y = rect.top();
    let mut w = rect.size.w as i32;
    let mut h = rect.size.h as i32;

    match handle {
        Handle::Body => {
            x = x.saturating_add(dx);
            y = y.saturating_add(dy);
        }
        Handle::TopLeft => {
            // Left edge moves by dx; top edge moves by dy.
            let new_w = w.saturating_sub(dx);
            let new_h = h.saturating_sub(dy);
            let clamped_w = new_w.max(1);
            let clamped_h = new_h.max(1);
            // Keep the right and bottom edges fixed.
            x = (x + w).saturating_sub(clamped_w);
            y = (y + h).saturating_sub(clamped_h);
            w = clamped_w;
            h = clamped_h;
        }
        Handle::Top => {
            // Top edge moves by dy.
            let new_h = h.saturating_sub(dy);
            let clamped_h = new_h.max(1);
            y = (y + h).saturating_sub(clamped_h);
            h = clamped_h;
        }
        Handle::TopRight => {
            // Right edge moves by dx; top edge moves by dy.
            let new_w = w.saturating_add(dx);
            let new_h = h.saturating_sub(dy);
            let clamped_w = new_w.max(1);
            let clamped_h = new_h.max(1);
            y = (y + h).saturating_sub(clamped_h);
            w = clamped_w;
            h = clamped_h;
        }
        Handle::Left => {
            // Left edge moves by dx.
            let new_w = w.saturating_sub(dx);
            let clamped_w = new_w.max(1);
            x = (x + w).saturating_sub(clamped_w);
            w = clamped_w;
        }
        Handle::Right => {
            // Right edge moves by dx.
            let new_w = w.saturating_add(dx);
            w = new_w.max(1);
        }
        Handle::BottomLeft => {
            // Left edge moves by dx; bottom edge moves by dy.
            let new_w = w.saturating_sub(dx);
            let new_h = h.saturating_add(dy);
            let clamped_w = new_w.max(1);
            let clamped_h = new_h.max(1);
            x = (x + w).saturating_sub(clamped_w);
            w = clamped_w;
            h = clamped_h;
        }
        Handle::Bottom => {
            // Bottom edge moves by dy.
            let new_h = h.saturating_add(dy);
            h = new_h.max(1);
        }
        Handle::BottomRight => {
            // Right edge moves by dx; bottom edge moves by dy.
            let new_w = w.saturating_add(dx);
            let new_h = h.saturating_add(dy);
            w = new_w.max(1);
            h = new_h.max(1);
        }
    }

    // Convert back to u32 for the constructor; sizes are guaranteed >= 1.
    Rect::new(x, y, w as u32, h as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: Rect = Rect::new(100, 100, 200, 100);

    fn at(x: i32, y: i32) -> Point {
        Point { x, y }
    }

    #[test]
    fn each_corner_is_grabbable_at_its_own_corner() {
        assert_eq!(hit_handle(R, at(100, 100), 4), Some(Handle::TopLeft));
        assert_eq!(hit_handle(R, at(300, 100), 4), Some(Handle::TopRight));
        assert_eq!(hit_handle(R, at(100, 200), 4), Some(Handle::BottomLeft));
        assert_eq!(hit_handle(R, at(300, 200), 4), Some(Handle::BottomRight));
    }

    #[test]
    fn each_edge_is_grabbable_at_its_midpoint() {
        assert_eq!(hit_handle(R, at(200, 100), 4), Some(Handle::Top));
        assert_eq!(hit_handle(R, at(200, 200), 4), Some(Handle::Bottom));
        assert_eq!(hit_handle(R, at(100, 150), 4), Some(Handle::Left));
        assert_eq!(hit_handle(R, at(300, 150), 4), Some(Handle::Right));
    }

    #[test]
    fn a_corner_wins_over_the_edge_it_shares_a_point_with() {
        // Where two targets overlap the smaller one must win, or a corner is
        // unreachable on a short edge.
        let narrow = Rect::new(0, 0, 8, 8);
        assert_eq!(hit_handle(narrow, at(0, 0), 4), Some(Handle::TopLeft));
    }

    #[test]
    fn the_middle_is_the_body() {
        assert_eq!(hit_handle(R, at(200, 150), 4), Some(Handle::Body));
    }

    #[test]
    fn outside_is_nothing() {
        assert_eq!(hit_handle(R, at(0, 0), 4), None);
        assert_eq!(hit_handle(R, at(500, 500), 4), None);
    }

    #[test]
    fn a_handle_has_slack_around_its_anchor() {
        // A person aiming at a one-pixel corner needs a target bigger than one
        // pixel; the grab radius is what makes the handles usable at all.
        assert_eq!(hit_handle(R, at(100 + 3, 100), 4), Some(Handle::TopLeft));
        assert_eq!(hit_handle(R, at(100 - 3, 100), 4), Some(Handle::TopLeft));
    }

    // --- resize ---

    #[test]
    fn the_body_moves_without_resizing() {
        let out = resize(R, Handle::Body, 10, -20);
        assert_eq!(out, Rect::new(110, 80, 200, 100));
    }

    #[test]
    fn a_right_drag_widens_and_leaves_the_left_edge_alone() {
        let out = resize(R, Handle::Right, 50, 0);
        assert_eq!(out, Rect::new(100, 100, 250, 100));
    }

    #[test]
    fn a_left_drag_moves_the_left_edge_and_leaves_the_right_alone() {
        let out = resize(R, Handle::Left, 50, 0);
        assert_eq!(out.right(), R.right(), "the far edge moved");
        assert_eq!(out, Rect::new(150, 100, 150, 100));
    }

    #[test]
    fn a_top_drag_moves_the_top_edge_and_leaves_the_bottom_alone() {
        let out = resize(R, Handle::Top, 0, 20);
        assert_eq!(out.bottom(), R.bottom(), "the far edge moved");
        assert_eq!(out, Rect::new(100, 120, 200, 80));
    }

    #[test]
    fn a_bottom_drag_grows_downward() {
        let out = resize(R, Handle::Bottom, 0, 30);
        assert_eq!(out, Rect::new(100, 100, 200, 130));
    }

    #[test]
    fn a_corner_drag_moves_both_axes() {
        let out = resize(R, Handle::BottomRight, 10, 20);
        assert_eq!(out, Rect::new(100, 100, 210, 120));
        let out = resize(R, Handle::TopLeft, 10, 20);
        assert_eq!(out, Rect::new(110, 120, 190, 80));
    }

    #[test]
    fn an_edge_only_handle_does_not_disturb_the_other_axis() {
        let out = resize(R, Handle::Top, 999, 10);
        assert_eq!(out.left(), R.left());
        assert_eq!(out.size.w, R.size.w);
        let out = resize(R, Handle::Left, 10, 999);
        assert_eq!(out.top(), R.top());
        assert_eq!(out.size.h, R.size.h);
    }

    #[test]
    fn a_size_never_falls_below_one() {
        for h in [Handle::Left, Handle::Right, Handle::Top, Handle::Bottom] {
            let out = resize(R, h, -9999, -9999);
            assert!(out.size.w >= 1, "{h:?} gave width {}", out.size.w);
            assert!(out.size.h >= 1, "{h:?} gave height {}", out.size.h);
        }
    }

    #[test]
    fn collapsing_from_the_left_stops_the_near_edge_not_the_far_one() {
        // Dragging the left handle past the right edge must pin the left edge
        // against it, not drag the whole widget away.
        let out = resize(R, Handle::Left, 9999, 0);
        assert_eq!(out.right(), R.right(), "the far edge moved");
        assert_eq!(out.size.w, 1);
    }

    #[test]
    fn collapsing_from_the_top_stops_the_near_edge_not_the_far_one() {
        let out = resize(R, Handle::Top, 0, 9999);
        assert_eq!(out.bottom(), R.bottom(), "the far edge moved");
        assert_eq!(out.size.h, 1);
    }

    #[test]
    fn an_extreme_drag_does_not_overflow() {
        for h in [
            Handle::Body,
            Handle::TopLeft,
            Handle::BottomRight,
            Handle::Left,
            Handle::Bottom,
        ] {
            let _ = resize(R, h, i32::MAX, i32::MAX);
            let _ = resize(R, h, i32::MIN, i32::MIN);
        }
    }
}
