// SPDX-License-Identifier: GPL-3.0-only
//! Finding the widget under a point.
//!
//! Used by the editor to turn a click on the preview into a selection, and
//! available to any consumer that wants touch input. It walks the tree in
//! reverse paint order, because the widget a person means is the one they can
//! see -- the topmost, not the first.

use crate::Point;

use super::{NodeId, ROOT, Tree};

/// The topmost widget whose rectangle contains `point`, in screen coordinates.
///
/// Returns `None` if nothing is hit. ROOT itself is never returned - it is
/// the whole canvas and hitting it means "nothing was clicked".
pub fn hit_test(tree: &Tree, point: Point) -> Option<NodeId> {
    hit_test_inner(tree, ROOT, point, 0)
}

fn hit_test_inner(tree: &Tree, id: NodeId, point: Point, depth: u32) -> Option<NodeId> {
    // Bounded recursion prevents stack overflow on corrupt/cyclic trees.
    if depth > 64 {
        return None;
    }

    let node = tree.get(id)?;

    if !node.visible {
        return None;
    }

    let abs_rect = tree.absolute_rect(id)?;
    if abs_rect.is_empty() {
        return None;
    }

    // Children are drawn in order, so later children are on top.
    // Children are checked before the parent because a child is on top of its parent.
    for &child_id in node.children.iter().rev() {
        if let Some(hit) = hit_test_inner(tree, child_id, point, depth + 1) {
            return Some(hit);
        }
    }

    if id == ROOT {
        return None;
    }

    if abs_rect.contains(point) {
        Some(id)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::{Kind, Node};
    use crate::{Color, Rect};
    use alloc::vec::Vec;

    fn panel(x: i32, y: i32, w: u32, h: u32) -> Node {
        Node {
            rect: Rect::new(x, y, w, h),
            kind: Kind::Panel {
                background: Color::WHITE,
            },
            visible: true,
            antialias: None,
            name: None,
            children: Vec::new(),
            parent: None,
        }
    }

    fn at(x: i32, y: i32) -> Point {
        Point { x, y }
    }

    #[test]
    fn nothing_is_hit_in_an_empty_tree() {
        let t = Tree::new(Rect::new(0, 0, 100, 100));
        assert_eq!(hit_test(&t, at(50, 50)), None);
    }

    #[test]
    fn the_root_is_never_returned() {
        // Hitting the canvas means "nothing was clicked", not "you selected
        // the whole scene".
        let t = Tree::new(Rect::new(0, 0, 100, 100));
        assert_eq!(hit_test(&t, at(1, 1)), None);
    }

    #[test]
    fn a_widget_under_the_point_is_found() {
        let mut t = Tree::new(Rect::new(0, 0, 100, 100));
        let id = t.push(ROOT, panel(10, 10, 20, 20)).unwrap();
        assert_eq!(hit_test(&t, at(15, 15)), Some(id));
    }

    #[test]
    fn a_point_outside_every_widget_hits_nothing() {
        let mut t = Tree::new(Rect::new(0, 0, 100, 100));
        t.push(ROOT, panel(10, 10, 20, 20)).unwrap();
        assert_eq!(hit_test(&t, at(90, 90)), None);
    }

    #[test]
    fn the_topmost_of_two_overlapping_widgets_wins() {
        // Later siblings paint over earlier ones, so the later one is what a
        // person can see and therefore what they meant to click.
        let mut t = Tree::new(Rect::new(0, 0, 100, 100));
        t.push(ROOT, panel(0, 0, 50, 50)).unwrap();
        let top = t.push(ROOT, panel(0, 0, 50, 50)).unwrap();
        assert_eq!(hit_test(&t, at(10, 10)), Some(top));
    }

    #[test]
    fn a_child_is_hit_before_its_parent() {
        let mut t = Tree::new(Rect::new(0, 0, 100, 100));
        let parent = t.push(ROOT, panel(0, 0, 80, 80)).unwrap();
        let child = t.push(parent, panel(10, 10, 20, 20)).unwrap();
        assert_eq!(hit_test(&t, at(15, 15)), Some(child));
        assert_eq!(hit_test(&t, at(70, 70)), Some(parent), "outside the child");
    }

    #[test]
    fn child_coordinates_are_resolved_to_the_screen() {
        // node.rect is parent-relative; testing against it directly would hit
        // the wrong place for every nested widget.
        let mut t = Tree::new(Rect::new(0, 0, 100, 100));
        let parent = t.push(ROOT, panel(50, 50, 40, 40)).unwrap();
        let child = t.push(parent, panel(0, 0, 10, 10)).unwrap();
        assert_eq!(hit_test(&t, at(55, 55)), Some(child));
        assert_eq!(
            hit_test(&t, at(5, 5)),
            None,
            "not at the child's own coords"
        );
    }

    #[test]
    fn an_invisible_widget_and_its_children_are_skipped() {
        let mut t = Tree::new(Rect::new(0, 0, 100, 100));
        let hidden = t.push(ROOT, panel(0, 0, 50, 50)).unwrap();
        t.push(hidden, panel(0, 0, 10, 10)).unwrap();
        t.set_visible(hidden, false).unwrap();
        assert_eq!(hit_test(&t, at(5, 5)), None, "a hidden subtree was hit");
    }

    #[test]
    fn an_empty_widget_is_never_hit() {
        let mut t = Tree::new(Rect::new(0, 0, 100, 100));
        t.push(ROOT, panel(10, 10, 0, 20)).unwrap();
        assert_eq!(hit_test(&t, at(10, 15)), None);
    }

    #[test]
    fn edges_are_inclusive_at_the_top_left_and_exclusive_at_the_bottom_right() {
        // The same convention Rect::contains uses everywhere else; a different
        // one here would make a click on a border select the wrong widget.
        let mut t = Tree::new(Rect::new(0, 0, 100, 100));
        let id = t.push(ROOT, panel(10, 10, 10, 10)).unwrap();
        assert_eq!(hit_test(&t, at(10, 10)), Some(id));
        assert_eq!(hit_test(&t, at(19, 19)), Some(id));
        assert_eq!(hit_test(&t, at(20, 20)), None);
    }

    #[test]
    fn a_very_deep_tree_terminates() {
        let mut t = Tree::new(Rect::new(0, 0, 200, 200));
        let mut p = ROOT;
        for _ in 0..200 {
            p = t.push(p, panel(0, 0, 100, 100)).unwrap();
        }
        let _ = hit_test(&t, at(5, 5));
    }
}
