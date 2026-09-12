// SPDX-License-Identifier: GPL-3.0-only
//! Putting a widget somewhere exact in its parent.
//!
//! Dragging with snapping gets a widget onto a line it can see. This is for
//! the times that is not good enough: a centred readout has to be centred
//! because it was centred, not because it was dragged until the guide
//! appeared and then left alone. The difference shows the first time the
//! parent is resized.
//!
//! Geometry only, so it can be tested without a window.

use copilot::{Rect, Size};

/// Where to put a widget inside its parent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Placement {
    /// Against the parent's left edge.
    Left,
    /// Centred horizontally.
    CenterX,
    /// Against the parent's right edge.
    Right,
    /// Against the parent's top edge.
    Top,
    /// Centred vertically.
    CenterY,
    /// Against the parent's bottom edge.
    Bottom,
    /// Spanning the parent's full width.
    FillX,
    /// Spanning the parent's full height.
    FillY,
}

/// Place `rect` inside a parent of `parent` size.
///
/// The parent is a size rather than a rect because a child's rect is already
/// parent-relative: the parent's own position is not part of this sum, and
/// passing it in is an invitation to add it twice.
///
/// A widget wider than its parent centres to a negative left edge rather than
/// being clamped to zero. Clamping would silently turn "centre this" into
/// "put this at the left", which is a different instruction, and the author
/// asked for the first one.
#[must_use]
pub fn place(rect: Rect, parent: Size, how: Placement) -> Rect {
    let (pw, ph) = (parent.w as i32, parent.h as i32);
    let (w, h) = (rect.size.w as i32, rect.size.h as i32);
    let (mut x, mut y) = (rect.left(), rect.top());
    let (mut nw, mut nh) = (rect.size.w, rect.size.h);

    match how {
        Placement::Left => x = 0,
        Placement::CenterX => x = (pw - w) / 2,
        Placement::Right => x = pw - w,
        Placement::Top => y = 0,
        Placement::CenterY => y = (ph - h) / 2,
        Placement::Bottom => y = ph - h,
        Placement::FillX => {
            x = 0;
            nw = parent.w;
        }
        Placement::FillY => {
            y = 0;
            nh = parent.h;
        }
    }

    Rect::new(x, y, nw, nh)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 40x20 widget loose inside a 100x60 parent.
    const R: Rect = Rect::new(7, 3, 40, 20);
    const P: Size = Size { w: 100, h: 60 };

    #[test]
    fn centring_horizontally_leaves_equal_margins() {
        let out = place(R, P, Placement::CenterX);
        assert_eq!(out.left(), 30);
        assert_eq!(out.left(), P.w as i32 - out.right());
    }

    #[test]
    fn centring_vertically_leaves_equal_margins() {
        let out = place(R, P, Placement::CenterY);
        assert_eq!(out.top(), 20);
        assert_eq!(out.top(), P.h as i32 - out.bottom());
    }

    #[test]
    fn centring_on_one_axis_leaves_the_other_alone() {
        assert_eq!(place(R, P, Placement::CenterX).top(), R.top());
        assert_eq!(place(R, P, Placement::CenterY).left(), R.left());
    }

    #[test]
    fn placing_never_changes_the_size() {
        for how in [
            Placement::Left,
            Placement::CenterX,
            Placement::Right,
            Placement::Top,
            Placement::CenterY,
            Placement::Bottom,
        ] {
            assert_eq!(place(R, P, how).size, R.size, "{how:?} resized it");
        }
    }

    #[test]
    fn the_edges_land_flush() {
        assert_eq!(place(R, P, Placement::Left).left(), 0);
        assert_eq!(place(R, P, Placement::Top).top(), 0);
        assert_eq!(place(R, P, Placement::Right).right(), P.w as i32);
        assert_eq!(place(R, P, Placement::Bottom).bottom(), P.h as i32);
    }

    #[test]
    fn filling_spans_the_parent_and_starts_at_its_edge() {
        let out = place(R, P, Placement::FillX);
        assert_eq!((out.left(), out.size.w), (0, P.w));
        assert_eq!((out.top(), out.size.h), (R.top(), R.size.h), "y changed");
        let out = place(R, P, Placement::FillY);
        assert_eq!((out.top(), out.size.h), (0, P.h));
        assert_eq!((out.left(), out.size.w), (R.left(), R.size.w), "x changed");
    }

    #[test]
    fn centring_is_idempotent() {
        // Applying it twice must not creep, or repeated use walks a widget
        // off the panel one pixel at a time.
        let once = place(R, P, Placement::CenterX);
        assert_eq!(place(once, P, Placement::CenterX), once);
    }

    #[test]
    fn an_odd_margin_favours_the_left_by_one() {
        // 100 - 41 is 59, which cannot split evenly. The rule has to be
        // stated so it is the same every time rather than merely whatever
        // integer division happened to do.
        let odd = Rect::new(0, 0, 41, 20);
        let out = place(odd, P, Placement::CenterX);
        assert_eq!(out.left(), 29);
        assert_eq!(P.w as i32 - out.right(), 30);
    }

    #[test]
    fn a_widget_wider_than_its_parent_centres_to_a_negative_edge() {
        // Not clamped: clamping turns "centre this" into "put this at the
        // left", and the overflow would then be entirely on one side.
        let big = Rect::new(0, 0, 140, 20);
        let out = place(big, P, Placement::CenterX);
        assert_eq!(out.left(), -20);
        assert_eq!(out.right(), 120);
    }

    #[test]
    fn a_parent_with_no_area_does_not_panic() {
        let none = Size { w: 0, h: 0 };
        for how in [
            Placement::Left,
            Placement::CenterX,
            Placement::Right,
            Placement::Top,
            Placement::CenterY,
            Placement::Bottom,
            Placement::FillX,
            Placement::FillY,
        ] {
            let _ = place(R, none, how);
        }
    }

    #[test]
    fn an_enormous_parent_does_not_overflow() {
        let huge = Size {
            w: u32::MAX,
            h: u32::MAX,
        };
        for how in [Placement::CenterX, Placement::Right, Placement::FillX] {
            let _ = place(R, huge, how);
        }
    }
}
