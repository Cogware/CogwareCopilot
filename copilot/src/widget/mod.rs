// SPDX-License-Identifier: MIT OR Apache-2.0
//! The retained widget tree.
//!
//! A scene is built once and then persists. Between frames the application
//! mutates widgets through [`Node`] accessors, each of which records that the
//! widget changed; the compositor then repaints only what those changes
//! touched. Nothing here walks the tree looking for differences — a widget
//! knows when it is dirty because the setter that made it dirty said so.
//!
//! # Why the tree is a flat arena and not `Box<dyn Widget>`
//!
//! Trait objects would be the idiomatic shape and are the wrong one here.
//! Every node would be a separate allocation, a repaint would chase pointers
//! all over the heap, and on a target where the heap is a fixed pool that
//! fragments badly. A `Vec<Node>` with `u32` indices keeps the whole tree in
//! one contiguous block, makes a node id a plain number that a scene file can
//! reference, and turns "visit every child" into a linear scan.
//!
//! The cost is that a node's kind is an enum rather than an open trait, so a
//! downstream crate cannot add a widget type. That is a real limitation and it
//! is deliberate: an embedded UI has a closed set of primitives, and the scene
//! format has to name them anyway.

use alloc::string::String;
use alloc::vec::Vec;

use crate::{Color, Rect};

mod hit;
mod kind;
pub use hit::hit_test;
pub use kind::{Align, BindTarget, Kind, VAlign};

/// Index of a node within a [`Tree`].
///
/// A plain number rather than a reference so that a scene file can name a node
/// and a widget can hold its children without borrowing the tree.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct NodeId(pub u32);

/// The root of every tree, created by [`Tree::new`].
pub const ROOT: NodeId = NodeId(0);

/// One widget.
#[derive(Clone, Debug)]
pub struct Node {
    /// Position and size, in the coordinate space of the parent.
    pub rect: Rect,
    /// What this widget draws.
    pub kind: Kind,
    /// Whether this widget and its children are drawn at all.
    pub visible: bool,
    /// Whether edges are blended by coverage, or `None` to draw the way the
    /// parent does. The document root takes the scene's own setting.
    ///
    /// Inherited rather than flat because it is a look, and a look is set
    /// once for a panel and overridden for the one widget that wants to be
    /// crisp -- a bitmap readout in a smooth cluster, say.
    pub antialias: Option<bool>,
    /// Optional name from the scene file, for the application to look up by.
    pub name: Option<String>,
    /// Children, drawn in order, so later siblings paint over earlier ones.
    pub children: Vec<NodeId>,
    /// Parent, or `None` for the root.
    pub parent: Option<NodeId>,
}

/// A whole widget tree, stored as one contiguous arena.
#[derive(Clone, Debug)]
pub struct Tree {
    nodes: Vec<Node>,
    dirty: crate::render::Damage,
}

impl Tree {
    /// A new tree whose root fills `bounds`.
    #[must_use]
    pub fn new(bounds: Rect) -> Self {
        let root = Node {
            rect: bounds,
            kind: Kind::Panel {
                background: Color::TRANSPARENT,
            },
            visible: true,
            antialias: None,
            name: None,
            children: Vec::new(),
            parent: None,
        };
        let mut dirty = crate::render::Damage::new();
        // The first frame has to paint everything; there is no previous frame
        // for anything to be unchanged relative to.
        dirty.add(bounds);
        Self {
            nodes: alloc::vec![root],
            dirty,
        }
    }

    /// Number of nodes, including the root.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the tree holds only its root.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    /// Borrow a node.
    #[must_use]
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.0 as usize)
    }

    /// Add `node` as the last child of `parent`.
    ///
    /// Returns `None` if `parent` does not exist. The new node's rectangle is
    /// marked dirty, because a widget that has just appeared has never been
    /// painted.
    pub fn push(&mut self, parent: NodeId, mut node: Node) -> Option<NodeId> {
        self.nodes.get(parent.0 as usize)?;
        let id = NodeId(u32::try_from(self.nodes.len()).ok()?);
        node.parent = Some(parent);
        // The tree owns parentage, so any children the caller put on the node
        // are ids it invented. Keeping them would leave the tree holding
        // references to nodes it never adopted; children arrive by being
        // pushed, not by being declared.
        node.children.clear();
        let absolute = self.absolute_of(parent, node.rect);
        self.dirty.add(absolute);
        self.nodes.push(node);
        self.nodes[parent.0 as usize].children.push(id);
        Some(id)
    }

    /// Move a node, marking both the old and new positions dirty.
    ///
    /// Both, because the pixels it vacated still show it. Marking only the
    /// destination is the single most common dirty-rectangle bug and it leaves
    /// a trail of the widget behind as it moves.
    pub fn set_rect(&mut self, id: NodeId, rect: Rect) -> Option<()> {
        let old = self.nodes.get(id.0 as usize)?.rect;
        if old == rect {
            return Some(());
        }
        let parent = self.nodes[id.0 as usize].parent;
        self.dirty.add(self.absolute_of_opt(parent, old));
        self.dirty.add(self.absolute_of_opt(parent, rect));
        self.nodes[id.0 as usize].rect = rect;
        Some(())
    }

    /// Replace what a node draws, marking it dirty.
    pub fn set_kind(&mut self, id: NodeId, kind: Kind) -> Option<()> {
        let node = self.nodes.get(id.0 as usize)?;
        let absolute = self.absolute_of_opt(node.parent, node.rect);
        self.nodes[id.0 as usize].kind = kind;
        self.dirty.add(absolute);
        Some(())
    }

    /// Show or hide a node and its subtree.
    pub fn set_visible(&mut self, id: NodeId, visible: bool) -> Option<()> {
        let node = self.nodes.get(id.0 as usize)?;
        if node.visible == visible {
            return Some(());
        }
        let absolute = self.absolute_of_opt(node.parent, node.rect);
        self.nodes[id.0 as usize].visible = visible;
        // Dirty either way: appearing needs painting, and disappearing needs
        // whatever was underneath painting back.
        self.dirty.add(absolute);
        Some(())
    }

    /// Set the fraction a gauge widget shows, marking it dirty if it changed.
    ///
    /// `None` if the node does not exist or has no reading. A value equal to
    /// the current one marks nothing: a display fed the same reading sixty
    /// times a second must not repaint sixty times a second.
    pub fn set_reading(&mut self, id: NodeId, value: f32) -> Option<()> {
        let node = self.nodes.get(id.0 as usize)?;
        if node.kind.reading()? == value {
            return Some(());
        }
        let kind = node.kind.with_reading(value)?;
        self.set_kind(id, kind)
    }

    /// Set how tall a segbar's lit cells stand, marking it dirty if changed.
    ///
    /// `None` if the node does not exist or has no second axis.
    pub fn set_height(&mut self, id: NodeId, value: f32) -> Option<()> {
        let node = self.nodes.get(id.0 as usize)?;
        if node.kind.height()? == value {
            return Some(());
        }
        let kind = node.kind.with_height(value)?;
        self.set_kind(id, kind)
    }

    /// Set the text a label or readout shows, marking it dirty if it changed.
    ///
    /// `None` if the node does not exist or shows no text.
    pub fn set_text(&mut self, id: NodeId, text: &str) -> Option<()> {
        let node = self.nodes.get(id.0 as usize)?;
        if node.kind.text()? == text {
            return Some(());
        }
        let kind = node.kind.with_text(text)?;
        self.set_kind(id, kind)
    }

    /// Start or stop a [`Kind::Anim`] widget.
    ///
    /// Returns `None` if the node does not exist or is not an animation, so a
    /// caller driving a scene it did not author can tell the difference
    /// between "paused it" and "there is nothing there to pause".
    pub fn set_playing(&mut self, id: NodeId, playing: bool) -> Option<()> {
        let node = self.nodes.get(id.0 as usize)?;
        let Kind::Anim {
            anim,
            frame,
            speed,
            elapsed_us,
            ..
        } = node.kind
        else {
            return None;
        };
        self.set_kind(
            id,
            Kind::Anim {
                anim,
                frame,
                playing,
                speed,
                elapsed_us,
            },
        )
    }

    /// Jump a [`Kind::Anim`] widget to a frame.
    ///
    /// The accumulated time is reset, so a seek lands on the frame rather than
    /// immediately stepping off it by whatever was left over.
    pub fn seek(&mut self, id: NodeId, frame: u32) -> Option<()> {
        let node = self.nodes.get(id.0 as usize)?;
        let Kind::Anim {
            anim,
            playing,
            speed,
            ..
        } = node.kind
        else {
            return None;
        };
        self.set_kind(
            id,
            Kind::Anim {
                anim,
                frame,
                playing,
                speed,
                elapsed_us: 0,
            },
        )
    }

    /// Set a [`Kind::Anim`] widget's playback rate.
    pub fn set_speed(&mut self, id: NodeId, speed: f32) -> Option<()> {
        let node = self.nodes.get(id.0 as usize)?;
        let Kind::Anim {
            anim,
            frame,
            playing,
            elapsed_us,
            ..
        } = node.kind
        else {
            return None;
        };
        self.set_kind(
            id,
            Kind::Anim {
                anim,
                frame,
                playing,
                speed,
                elapsed_us,
            },
        )
    }

    /// The position of `id` among its parent's children.
    #[must_use]
    pub fn index_in_parent(&self, id: NodeId) -> Option<usize> {
        let parent = self.get(id)?.parent?;
        self.get(parent)?.children.iter().position(|c| *c == id)
    }

    /// The chain of child indices from the document root down to `id`.
    ///
    /// The builder inserts nodes in document order, so this is exactly the
    /// route through the source text: the document's root node, then one
    /// `children` index per level. An editor turns it into a path and splices
    /// the bytes it finds there, which is how a property edit preserves every
    /// comment in the file.
    ///
    /// Returns `None` for [`ROOT`] itself, which corresponds to no node in the
    /// document -- the document's root is ROOT's only child.
    #[must_use]
    pub fn document_path(&self, id: NodeId) -> Option<alloc::vec::Vec<usize>> {
        if id == ROOT {
            return None;
        }
        let mut out = alloc::vec::Vec::new();
        let mut cursor = id;
        // Bounded by the node count: parents strictly precede their children,
        // so this terminates, but a corrupt tree must not hang an editor.
        for _ in 0..self.nodes.len() {
            out.push(self.index_in_parent(cursor)?);
            let parent = self.get(cursor)?.parent?;
            if parent == ROOT {
                out.reverse();
                // The first hop is ROOT -> the document's root node, which the
                // format spells "root" rather than children[0].
                out.remove(0);
                return Some(out);
            }
            cursor = parent;
        }
        None
    }

    /// Find a node by the `name` its scene file gave it.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<NodeId> {
        self.nodes
            .iter()
            .position(|n| n.name.as_deref() == Some(name))
            .and_then(|i| u32::try_from(i).ok())
            .map(NodeId)
    }

    /// What needs repainting.
    #[must_use]
    pub fn damage(&self) -> &crate::render::Damage {
        &self.dirty
    }

    /// Forget the current damage, after a frame has been presented.
    pub fn clear_damage(&mut self) {
        self.dirty.clear();
    }

    /// A node's rectangle in screen coordinates.
    ///
    /// Walks to the root adding each ancestor's origin. Recomputing this
    /// rather than caching it keeps a move from having to touch every
    /// descendant; trees here are shallow enough that the walk is cheaper than
    /// the invalidation would be.
    #[must_use]
    pub fn absolute_rect(&self, id: NodeId) -> Option<Rect> {
        let node = self.get(id)?;
        Some(self.absolute_of_opt(node.parent, node.rect))
    }

    fn absolute_of_opt(&self, parent: Option<NodeId>, rect: Rect) -> Rect {
        match parent {
            Some(p) => self.absolute_of(p, rect),
            None => rect,
        }
    }

    fn absolute_of(&self, parent: NodeId, rect: Rect) -> Rect {
        let mut out = rect;
        let mut cursor = Some(parent);
        // Bounded by the node count: `push` only ever sets a parent that
        // already exists, so the chain cannot contain a cycle, but a corrupt
        // tree must not hang the renderer either.
        for _ in 0..self.nodes.len() {
            let Some(id) = cursor else { break };
            let Some(n) = self.nodes.get(id.0 as usize) else {
                break;
            };
            out = out.translate(n.rect.left(), n.rect.top());
            cursor = n.parent;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Tree {
        let mut t = Tree::new(Rect::new(0, 0, 200, 100));
        t.clear_damage(); // ignore the initial full-screen mark
        t
    }

    fn panel(x: i32, y: i32, w: u32, h: u32) -> Node {
        Node {
            rect: Rect::new(x, y, w, h),
            kind: Kind::Panel {
                background: Color::WHITE,
            },
            visible: true,
            antialias: None,
            name: None,
            children: alloc::vec::Vec::new(),
            parent: None,
        }
    }

    #[test]
    fn a_new_tree_marks_everything_dirty() {
        // There is no previous frame for anything to be unchanged against.
        let t = Tree::new(Rect::new(0, 0, 40, 30));
        assert_eq!(t.damage().bounds(), Some(Rect::new(0, 0, 40, 30)));
    }

    #[test]
    fn a_pushed_node_is_dirty_because_it_has_never_been_painted() {
        let mut t = tree();
        t.push(ROOT, panel(10, 10, 5, 5)).unwrap();
        assert_eq!(t.damage().bounds(), Some(Rect::new(10, 10, 5, 5)));
    }

    #[test]
    fn pushing_to_a_missing_parent_fails_rather_than_panicking() {
        let mut t = tree();
        assert!(t.push(NodeId(99), panel(0, 0, 1, 1)).is_none());
    }

    #[test]
    fn child_coordinates_are_relative_to_the_parent() {
        let mut t = tree();
        let outer = t.push(ROOT, panel(10, 20, 100, 50)).unwrap();
        let inner = t.push(outer, panel(5, 5, 10, 10)).unwrap();
        assert_eq!(t.absolute_rect(inner), Some(Rect::new(15, 25, 10, 10)));
    }

    #[test]
    fn moving_a_node_dirties_where_it_was_and_where_it_went() {
        // Marking only the destination leaves a trail of the widget behind as
        // it moves; it is the most common dirty-rectangle bug there is.
        let mut t = tree();
        let n = t.push(ROOT, panel(0, 0, 10, 10)).unwrap();
        t.clear_damage();
        t.set_rect(n, Rect::new(100, 0, 10, 10)).unwrap();
        let b = t.damage().bounds().unwrap();
        assert!(
            b.contains_rect(Rect::new(0, 0, 10, 10)),
            "old position lost"
        );
        assert!(
            b.contains_rect(Rect::new(100, 0, 10, 10)),
            "new position lost"
        );
    }

    #[test]
    fn a_move_that_changes_nothing_dirties_nothing() {
        let mut t = tree();
        let n = t.push(ROOT, panel(3, 3, 4, 4)).unwrap();
        t.clear_damage();
        t.set_rect(n, Rect::new(3, 3, 4, 4)).unwrap();
        assert!(t.damage().is_empty());
    }

    #[test]
    fn hiding_dirties_so_what_was_underneath_repaints() {
        let mut t = tree();
        let n = t.push(ROOT, panel(1, 2, 3, 4)).unwrap();
        t.clear_damage();
        t.set_visible(n, false).unwrap();
        assert_eq!(t.damage().bounds(), Some(Rect::new(1, 2, 3, 4)));
    }

    #[test]
    fn a_redundant_visibility_change_dirties_nothing() {
        let mut t = tree();
        let n = t.push(ROOT, panel(1, 2, 3, 4)).unwrap();
        t.clear_damage();
        t.set_visible(n, true).unwrap();
        assert!(t.damage().is_empty());
    }

    #[test]
    fn a_moved_child_dirties_in_screen_space_not_parent_space() {
        // The damage set is consumed by the compositor, which works in screen
        // coordinates; handing it parent-relative rectangles would repaint the
        // wrong part of the display.
        let mut t = tree();
        let outer = t.push(ROOT, panel(50, 50, 100, 40)).unwrap();
        let inner = t.push(outer, panel(0, 0, 10, 10)).unwrap();
        t.clear_damage();
        t.set_rect(inner, Rect::new(20, 0, 10, 10)).unwrap();
        let b = t.damage().bounds().unwrap();
        assert!(b.contains_rect(Rect::new(50, 50, 10, 10)));
        assert!(b.contains_rect(Rect::new(70, 50, 10, 10)));
    }

    #[test]
    fn nodes_can_be_found_by_name() {
        let mut t = tree();
        let mut n = panel(0, 0, 1, 1);
        n.name = Some("speedo".into());
        let id = t.push(ROOT, n).unwrap();
        assert_eq!(t.find("speedo"), Some(id));
        assert_eq!(t.find("absent"), None);
    }

    #[test]
    fn setting_a_kind_dirties_the_node() {
        let mut t = tree();
        let n = t.push(ROOT, panel(2, 2, 6, 6)).unwrap();
        t.clear_damage();
        t.set_kind(
            n,
            Kind::Frame {
                color: Color::BLACK,
            },
        )
        .unwrap();
        assert_eq!(t.damage().bounds(), Some(Rect::new(2, 2, 6, 6)));
    }

    #[test]
    fn a_changed_reading_dirties_and_an_unchanged_one_does_not() {
        // Live data arrives every frame whether or not it moved; repainting a
        // gauge that reads the same is the cost this check saves.
        let mut t = tree();
        let mut bar = panel(2, 2, 6, 6);
        bar.kind = Kind::Bar {
            value: 0.25,
            fill: Color::WHITE,
            track: Color::BLACK,
            vertical: false,
        };
        let n = t.push(ROOT, bar).unwrap();
        t.clear_damage();
        t.set_reading(n, 0.25).unwrap();
        assert!(t.damage().is_empty(), "same reading, no repaint");
        t.set_reading(n, 0.75).unwrap();
        assert_eq!(t.damage().bounds(), Some(Rect::new(2, 2, 6, 6)));
        assert_eq!(t.get(n).unwrap().kind.reading(), Some(0.75));
        // A panel has nothing to read, and says so rather than pretending.
        let p = t.push(ROOT, panel(0, 0, 1, 1)).unwrap();
        assert!(t.set_reading(p, 0.5).is_none());
        assert!(t.set_text(p, "x").is_none());
    }

    #[test]
    fn deep_nesting_accumulates_offsets_without_hanging() {
        let mut t = tree();
        let mut parent = ROOT;
        for _ in 0..50 {
            parent = t.push(parent, panel(1, 1, 100, 100)).unwrap();
        }
        assert_eq!(t.absolute_rect(parent).unwrap().left(), 50);
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;

    fn panel() -> Node {
        Node {
            rect: Rect::new(0, 0, 1, 1),
            kind: Kind::Panel {
                background: Color::WHITE,
            },
            visible: true,
            antialias: None,
            name: None,
            children: alloc::vec::Vec::new(),
            parent: None,
        }
    }

    #[test]
    fn the_root_has_no_document_path() {
        // ROOT is the canvas, not a node in the file.
        let t = Tree::new(Rect::new(0, 0, 10, 10));
        assert_eq!(t.document_path(ROOT), None);
    }

    #[test]
    fn the_documents_root_node_is_the_empty_path() {
        // It is reached by the key "root", not by a child index.
        let mut t = Tree::new(Rect::new(0, 0, 10, 10));
        let doc_root = t.push(ROOT, panel()).unwrap();
        assert_eq!(t.document_path(doc_root), Some(alloc::vec![]));
    }

    #[test]
    fn a_child_is_its_index() {
        let mut t = Tree::new(Rect::new(0, 0, 10, 10));
        let doc_root = t.push(ROOT, panel()).unwrap();
        let a = t.push(doc_root, panel()).unwrap();
        let b = t.push(doc_root, panel()).unwrap();
        assert_eq!(t.document_path(a), Some(alloc::vec![0]));
        assert_eq!(t.document_path(b), Some(alloc::vec![1]));
    }

    #[test]
    fn nesting_accumulates_indices_outermost_first() {
        let mut t = Tree::new(Rect::new(0, 0, 10, 10));
        let doc_root = t.push(ROOT, panel()).unwrap();
        t.push(doc_root, panel()).unwrap();
        let second = t.push(doc_root, panel()).unwrap();
        let deep = t.push(second, panel()).unwrap();
        let deeper = t.push(deep, panel()).unwrap();
        assert_eq!(t.document_path(deeper), Some(alloc::vec![1, 0, 0]));
    }

    #[test]
    fn index_in_parent_matches_document_order() {
        let mut t = Tree::new(Rect::new(0, 0, 10, 10));
        let doc_root = t.push(ROOT, panel()).unwrap();
        let a = t.push(doc_root, panel()).unwrap();
        let b = t.push(doc_root, panel()).unwrap();
        assert_eq!(t.index_in_parent(a), Some(0));
        assert_eq!(t.index_in_parent(b), Some(1));
        assert_eq!(t.index_in_parent(ROOT), None);
    }
}

#[cfg(test)]
mod push_tests {
    use super::*;

    #[test]
    fn push_discards_children_the_caller_invented() {
        // Node::children is public, so a caller can hand over ids the tree
        // never issued. Keeping them would leave dangling references that
        // paint and hit-test would both follow.
        let mut t = Tree::new(Rect::new(0, 0, 10, 10));
        let mut n = Node {
            rect: Rect::new(0, 0, 1, 1),
            kind: Kind::Panel {
                background: Color::WHITE,
            },
            visible: true,
            antialias: None,
            name: None,
            children: alloc::vec![NodeId(99), NodeId(1234)],
            parent: None,
        };
        n.children.push(NodeId(7));
        let id = t.push(ROOT, n).unwrap();
        assert!(t.get(id).unwrap().children.is_empty());
    }
}
