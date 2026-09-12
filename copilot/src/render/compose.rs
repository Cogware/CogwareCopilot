// SPDX-License-Identifier: GPL-3.0-only
//! Painting a widget tree into a surface.
//!
//! The compositor walks the tree in document order, so a later sibling paints
//! over an earlier one, and draws each node once per damage rectangle it
//! intersects.
//!
//! # Why the walk is per damage rectangle rather than per node
//!
//! The alternative — visit each node once and intersect it against the whole
//! damage set — sounds cheaper and is wrong in a way that only shows up with
//! overlapping damage. A widget straddling two dirty rectangles has to be
//! drawn clipped to each of them separately; drawing it once against the union
//! would repaint the gap between them, and that gap may contain a widget that
//! was *not* dirty and is now half-erased.
//!
//! Trees here are small and the damage set is capped at
//! [`super::damage::MAX_RECTS`], so the repeated walk costs far less than the
//! bookkeeping needed to avoid it.

use crate::widget::{NodeId, ROOT, Tree};
use crate::{Rect, Surface};

use super::draw::draw_kind;
use super::{Damage, Resources};

/// Repaint everything in `tree` that `damage` says has changed.
///
/// The caller is expected to follow this with [`Surface::present`] and then
/// [`Tree::clear_damage`]; keeping those separate lets a double-buffered
/// backend decide for itself when a frame is finished.
pub fn compose<S: Surface + ?Sized>(
    surface: &mut S,
    tree: &Tree,
    damage: &Damage,
    res: Resources<'_>,
) {
    for clip in damage.rects() {
        paint(surface, tree, ROOT, Rect::ZERO, *clip, res, false);
    }
}

/// Repaint the whole tree, ignoring damage.
///
/// For the first frame, and for a backend that has just been handed a buffer
/// whose previous contents it cannot vouch for.
pub fn compose_all<S: Surface + ?Sized>(surface: &mut S, tree: &Tree, res: Resources<'_>) {
    let size = surface.size();
    paint(
        surface,
        tree,
        ROOT,
        Rect::ZERO,
        Rect::new(0, 0, size.w, size.h),
        res,
        false,
    );
}

/// Repaint the whole tree, moved by (`dx`, `dy`) and clipped to `clip`.
///
/// For a transition, which puts two trees in different places in one frame.
pub fn compose_moved<S: Surface + ?Sized>(
    surface: &mut S,
    tree: &Tree,
    res: Resources<'_>,
    dx: i32,
    dy: i32,
    clip: Rect,
) {
    paint(
        surface,
        tree,
        ROOT,
        Rect::new(dx, dy, 0, 0),
        clip,
        res,
        false,
    );
}

/// `inherited` is the antialiasing a node draws with when it has no setting
/// of its own: its parent's, all the way up to the document root, which the
/// builder gave the scene's.
#[allow(clippy::too_many_arguments)] // The walk carries its whole context.
fn paint<S: Surface + ?Sized>(
    surface: &mut S,
    tree: &Tree,
    id: NodeId,
    parent_origin: Rect,
    clip: Rect,
    res: Resources<'_>,
    inherited: bool,
) {
    let Some(node) = tree.get(id) else { return };
    if !node.visible {
        return;
    }

    let at = node
        .rect
        .translate(parent_origin.left(), parent_origin.top());
    let antialias = node.antialias.unwrap_or(inherited);

    // Deliberately no cull of subtrees outside `clip`: nothing confines a
    // child to its parent's rectangle, so a parent that misses the damage may
    // still have descendants inside it. draw_kind rejects the ones that miss.
    draw_kind(surface, &node.kind, at, clip, res, antialias);

    for child in &node.children {
        paint(surface, tree, *child, at, clip, res, antialias);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::asset::{AnimTable, ImageTable};
    use crate::font::{Font, default_font};
    use crate::widget::{Kind, Node};
    use crate::{Color, MemorySurface, PixelFormat, Size};
    use alloc::vec::Vec;

    /// Parsed once per call; the atlas is 683 bytes and parsing is a
    /// header read, so a test does not need to cache it.
    fn font() -> Font<'static> {
        default_font()
    }

    /// Empty tables plus the built-in font, which is what most tests need.
    fn res<'a>(images: &'a ImageTable, anims: &'a AnimTable, font: &'a Font<'a>) -> Resources<'a> {
        Resources {
            images,
            anims,
            font,
            menus: &[],
        }
    }

    fn surf(w: u32, h: u32) -> MemorySurface {
        MemorySurface::new(Size { w, h }, PixelFormat::Bgrx8888)
    }

    fn px(s: &MemorySurface, x: usize, y: usize) -> [u8; 4] {
        let o = y * s.stride() + x * 4;
        [
            s.pixels()[o],
            s.pixels()[o + 1],
            s.pixels()[o + 2],
            s.pixels()[o + 3],
        ]
    }

    fn is_set(s: &MemorySurface, x: usize, y: usize) -> bool {
        px(s, x, y) != [0, 0, 0, 0]
    }

    fn panel(x: i32, y: i32, w: u32, h: u32, c: Color) -> Node {
        Node {
            rect: Rect::new(x, y, w, h),
            kind: Kind::Panel { background: c },
            visible: true,
            antialias: None,
            name: None,
            children: Vec::new(),
            parent: None,
        }
    }

    #[test]
    fn compose_all_paints_the_whole_tree() {
        let mut t = Tree::new(Rect::new(0, 0, 10, 10));
        t.push(ROOT, panel(1, 1, 2, 2, Color::WHITE)).unwrap();
        let mut s = surf(10, 10);
        compose_all(
            &mut s,
            &t,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
        );
        assert!(is_set(&s, 1, 1) && is_set(&s, 2, 2));
        assert!(!is_set(&s, 0, 0) && !is_set(&s, 3, 3));
    }

    #[test]
    fn children_paint_over_earlier_siblings() {
        // Document order is paint order; that is the only stacking control the
        // scene format offers, so it has to be exact.
        let mut t = Tree::new(Rect::new(0, 0, 10, 10));
        t.push(ROOT, panel(0, 0, 4, 4, Color::rgb(255, 0, 0)))
            .unwrap();
        t.push(ROOT, panel(0, 0, 4, 4, Color::rgb(0, 0, 255)))
            .unwrap();
        let mut s = surf(10, 10);
        compose_all(
            &mut s,
            &t,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
        );
        assert_eq!(px(&s, 0, 0), [255, 0, 0, 255], "the later sibling wins");
    }

    #[test]
    fn a_child_is_positioned_relative_to_its_parent() {
        let mut t = Tree::new(Rect::new(0, 0, 20, 20));
        let outer = t
            .push(ROOT, panel(5, 5, 10, 10, Color::TRANSPARENT))
            .unwrap();
        t.push(outer, panel(1, 1, 2, 2, Color::WHITE)).unwrap();
        let mut s = surf(20, 20);
        compose_all(
            &mut s,
            &t,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
        );
        assert!(is_set(&s, 6, 6), "child should land at parent + offset");
        assert!(!is_set(&s, 1, 1));
    }

    #[test]
    fn an_invisible_node_and_its_subtree_are_skipped() {
        let mut t = Tree::new(Rect::new(0, 0, 10, 10));
        let hidden = t.push(ROOT, panel(0, 0, 8, 8, Color::WHITE)).unwrap();
        t.push(hidden, panel(0, 0, 2, 2, Color::WHITE)).unwrap();
        t.set_visible(hidden, false).unwrap();
        let mut s = surf(10, 10);
        compose_all(
            &mut s,
            &t,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
        );
        assert!(!is_set(&s, 0, 0), "a hidden subtree must not paint");
    }

    #[test]
    fn compose_repaints_only_the_damaged_region() {
        let mut t = Tree::new(Rect::new(0, 0, 10, 10));
        t.push(ROOT, panel(0, 0, 10, 10, Color::WHITE)).unwrap();
        t.clear_damage();

        let mut d = Damage::new();
        d.add(Rect::new(0, 0, 3, 3));
        let mut s = surf(10, 10);
        compose(
            &mut s,
            &t,
            &d,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
        );

        assert!(is_set(&s, 2, 2));
        assert!(!is_set(&s, 3, 0), "outside the damage must stay untouched");
    }

    #[test]
    fn a_widget_straddling_two_damage_rects_is_drawn_in_both_and_only_both() {
        // Drawing it once against the union would repaint the gap between
        // them, and the gap may hold a widget that was never dirty.
        let mut t = Tree::new(Rect::new(0, 0, 20, 4));
        t.push(ROOT, panel(0, 0, 20, 4, Color::WHITE)).unwrap();
        t.clear_damage();

        let mut d = Damage::new();
        d.add(Rect::new(0, 0, 2, 2));
        d.add(Rect::new(10, 0, 2, 2));
        let mut s = surf(20, 4);
        compose(
            &mut s,
            &t,
            &d,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
        );

        assert!(is_set(&s, 0, 0) && is_set(&s, 10, 0));
        assert!(!is_set(&s, 5, 0), "the gap between them must stay clear");
    }

    #[test]
    fn empty_damage_paints_nothing() {
        let mut t = Tree::new(Rect::new(0, 0, 10, 10));
        t.push(ROOT, panel(0, 0, 10, 10, Color::WHITE)).unwrap();
        t.clear_damage();
        let mut s = surf(10, 10);
        compose(
            &mut s,
            &t,
            &Damage::new(),
            res(&ImageTable::new(), &AnimTable::new(), &font()),
        );
        assert!(!is_set(&s, 0, 0));
    }

    #[test]
    fn a_child_may_escape_its_parents_rectangle() {
        // Nothing clips a child to its parent; only the surface and the damage
        // rectangle clip. A widget deliberately hanging outside its container
        // is a normal thing to want.
        let mut t = Tree::new(Rect::new(0, 0, 20, 20));
        let outer = t.push(ROOT, panel(0, 0, 2, 2, Color::TRANSPARENT)).unwrap();
        t.push(outer, panel(0, 0, 10, 10, Color::WHITE)).unwrap();
        let mut s = surf(20, 20);
        compose_all(
            &mut s,
            &t,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
        );
        assert!(is_set(&s, 5, 5), "the child should not be clipped to 2x2");
    }

    #[test]
    fn a_tree_deeper_than_the_damage_set_still_terminates() {
        let mut t = Tree::new(Rect::new(0, 0, 40, 40));
        let mut p = ROOT;
        for _ in 0..30 {
            p = t.push(p, panel(1, 1, 30, 30, Color::TRANSPARENT)).unwrap();
        }
        let mut s = surf(40, 40);
        compose_all(
            &mut s,
            &t,
            res(&ImageTable::new(), &AnimTable::new(), &font()),
        );
    }
}
