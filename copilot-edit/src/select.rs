// SPDX-License-Identifier: GPL-3.0-only
//! More than one widget at a time.
//!
//! A selection is one primary widget and any number of others. The primary
//! is what the properties pane shows, what the resize handles belong to and
//! what a snap is measured from; the others come along for a move, a nudge, a
//! delete. One plus a list rather than a list alone, because almost every
//! command wants exactly one widget, and a list that is usually one long
//! would make every one of them ask which.

use copilot::widget::{NodeId, ROOT, Tree};
use copilot::{Point, Rect};

use crate::canvas::Gesture;
use crate::ops::doc_path;
use crate::{App, place, rect_text};

impl App {
    /// Everything selected, the primary first.
    pub(crate) fn members(&self) -> Vec<NodeId> {
        self.selected
            .into_iter()
            .chain(self.extra.iter().copied())
            .collect()
    }

    /// Whether `id` is any part of the selection.
    pub(crate) fn is_selected(&self, id: NodeId) -> bool {
        self.selected == Some(id) || self.extra.contains(&id)
    }

    /// Make `id` the selection, or clear it, and say so in the status bar.
    pub(crate) fn select(&mut self, id: Option<NodeId>) {
        self.selected = id;
        self.extra.clear();
        self.reveal = id.is_some();
        self.status = match id {
            Some(id) => format!("selected {}", self.describe(id)),
            None => "nothing selected".into(),
        };
    }

    /// Select all of `ids`, the first as the primary.
    pub(crate) fn select_many(&mut self, mut ids: Vec<NodeId>) {
        ids.dedup();
        self.selected = ids.first().copied();
        self.extra = ids.get(1..).map(<[NodeId]>::to_vec).unwrap_or_default();
        self.reveal = self.selected.is_some();
        self.status = match ids.len() {
            0 => "nothing selected".into(),
            1 => format!("selected {}", self.describe(ids[0])),
            n => format!("{n} selected"),
        };
    }

    /// Add `id` to the selection, or take it out: a Shift-click.
    pub(crate) fn toggle(&mut self, id: NodeId) {
        toggle_in(&mut self.selected, &mut self.extra, id);
        self.reveal = true;
        self.status = match self.members().len() {
            0 => "nothing selected".into(),
            1 => format!("selected {}", self.describe(id)),
            n => format!("{n} selected"),
        };
    }

    /// Make `id`, already selected, the primary.
    ///
    /// The widget a drag takes hold of is the one whose edges the snap should
    /// follow and whose properties are wanted next; that need not be the one
    /// clicked first.
    pub(crate) fn make_primary(&mut self, id: NodeId) {
        if self.selected == Some(id) || !self.extra.contains(&id) {
            return;
        }
        self.extra.retain(|&e| e != id);
        if let Some(old) = self.selected.replace(id) {
            self.extra.insert(0, old);
        }
    }

    /// Select every sibling of the primary, or every top-level widget.
    pub(crate) fn select_all(&mut self) {
        let Some(tree) = self.preview.tree.as_ref() else {
            return;
        };
        let parent = self
            .selected
            .and_then(|id| tree.get(id)?.parent)
            .filter(|&p| p != ROOT)
            .or_else(|| self.doc_root());
        let ids = parent
            .and_then(|p| tree.get(p))
            .map(|n| n.children.clone())
            .unwrap_or_default();
        self.select_many(ids);
    }

    /// Select what a rubber band from `a` to `b` encloses, adding to the
    /// selection when `add` is held.
    pub(crate) fn marquee_select(&mut self, a: Point, b: Point, add: bool) {
        let area = span(a, b);
        let Some(tree) = self.preview.tree.as_ref() else {
            return;
        };
        let found = enclosed(tree, area);
        if add {
            let mut ids = self.members();
            let known = ids.clone();
            ids.extend(found.into_iter().filter(|id| !known.contains(id)));
            self.select_many(ids);
        } else {
            self.select_many(found);
        }
    }

    /// Write `field` on every widget in `ids` as one undo step.
    ///
    /// `g` is the gesture the edit belongs to, if it continues one; the
    /// writes within a single call always share a snapshot, whatever `g` is.
    pub(crate) fn edit_each(
        &mut self,
        ids: &[NodeId],
        g: Option<Gesture>,
        field: &str,
        value: &str,
    ) {
        let id = g.map_or_else(|| egui::Id::new("edit each"), |g| g.id);
        for &n in ids {
            self.set_field_of(n, Some(Gesture { id, ends: false }), field, value);
        }
        if g.is_none_or(|g| g.ends) {
            self.gesture = None;
        }
    }

    /// Give each of `edits` its rect, as one undo step.
    fn edit_rects(&mut self, edits: Vec<(NodeId, Rect)>) {
        if edits.is_empty() {
            self.status = "select a widget first".into();
            return;
        }
        let id = egui::Id::new("edit rects");
        for (n, r) in edits {
            self.set_field_of(n, Some(Gesture { id, ends: false }), "rect", &rect_text(r));
        }
        self.gesture = None;
    }

    /// Shift the selection by whole pixels.
    ///
    /// The keyboard is the only way to place something one pixel over: at any
    /// preview scale below 1:1 a mouse cannot address a single scene pixel,
    /// and at any scale snapping will pull the drag to a line instead.
    pub(crate) fn nudge(&mut self, dx: i32, dy: i32) {
        let Some(tree) = self.preview.tree.as_ref() else {
            return;
        };
        let edits = self
            .members()
            .into_iter()
            .filter_map(|id| {
                let r = tree.get(id)?.rect;
                Some((
                    id,
                    Rect::new(
                        r.left().saturating_add(dx),
                        r.top().saturating_add(dy),
                        r.size.w,
                        r.size.h,
                    ),
                ))
            })
            .collect();
        self.edit_rects(edits);
    }

    /// Put each selected widget somewhere exact inside its own parent.
    ///
    /// The parent's *size* is what matters: a child's rect is already
    /// parent-relative, so the parent's own position must not enter the sum.
    pub(crate) fn arrange(&mut self, how: place::Placement) {
        let Some(tree) = self.preview.tree.as_ref() else {
            return;
        };
        let edits: Vec<(NodeId, Rect)> = self
            .members()
            .into_iter()
            .filter_map(|id| {
                let node = tree.get(id)?;
                let parent = tree.get(node.parent?)?;
                Some((id, place::place(node.rect, parent.rect.size, how)))
            })
            .collect();
        if edits.is_empty() && self.selected.is_some() {
            self.status = "the root has nothing to be placed inside".into();
            return;
        }
        self.edit_rects(edits);
    }

    /// The selection as document paths, outermost first, with anything that
    /// sits inside another selected widget left out.
    ///
    /// A widget inside a selected panel is deleted or copied with the panel;
    /// doing it a second time on its own would remove it twice or copy it
    /// twice. The document root, whose path is empty, is left out too: it
    /// has no siblings to be copied beside and cannot be deleted.
    fn top_paths(&self) -> Vec<Vec<usize>> {
        let Some(tree) = self.preview.tree.as_ref() else {
            return Vec::new();
        };
        let mut paths: Vec<Vec<usize>> = self
            .members()
            .into_iter()
            .filter_map(|id| tree.document_path(id))
            .filter(|p| !p.is_empty())
            .collect();
        paths.sort();
        paths.dedup();
        let nested = |p: &Vec<usize>| paths.iter().any(|q| q.len() < p.len() && p.starts_with(q));
        paths.iter().filter(|p| !nested(p)).cloned().collect()
    }

    /// Delete the selection from the text.
    pub(crate) fn delete_selected(&mut self) {
        let mut paths = self.top_paths();
        if paths.is_empty() {
            self.status = "select a widget first (the document root cannot be deleted)".into();
            return;
        }
        // Last first: removing an element renumbers everything after it in
        // the same list, and a path worked out earlier would be off by one.
        paths.reverse();
        self.checkpoint();
        let mut text = self.text.clone();
        let mut count = 0;
        for p in &paths {
            if let Some(t) = copilot::scene::remove(&text, &doc_path(p)) {
                text = t;
                count += 1;
            }
        }
        if count == 0 {
            self.status = "could not delete that widget".into();
            return;
        }
        self.text = text;
        // The tree is about to be a different shape, and the ids are indices
        // into the old one.
        self.select(None);
        self.dirty = true;
        self.reload();
        self.status = format!("deleted {count}");
    }

    /// Copy each selected widget and drop the copy in beside it.
    ///
    /// The copy is the widget's own source text, so one with children brings
    /// its whole subtree and, more to the point, brings its comments.
    /// Rebuilding it from the parsed tree would hand back something that
    /// renders identically and reads like a stranger wrote it.
    pub(crate) fn duplicate_selected(&mut self) {
        let mut paths = self.top_paths();
        if paths.is_empty() {
            self.status = "select a widget first (the document root has no siblings)".into();
            return;
        }
        paths.reverse();
        self.checkpoint();
        let mut text = self.text.clone();
        let mut count = 0;
        for p in &paths {
            let Some((last, parents)) = p.split_last() else {
                continue;
            };
            let elem = doc_path(p);
            let Some(span) = copilot::scene::locate(&text, &elem) else {
                continue;
            };
            let Some(copy) = text.get(span.start..span.end).map(str::to_owned) else {
                continue;
            };
            let mut slot = doc_path(parents);
            slot.push(copilot::scene::Step::Key("children"));
            // Straight after the original rather than at the end of the
            // array: the array is draw order, and a copy that jumped to the
            // front would come back on top of whatever it was sitting behind.
            if let Some(t) = copilot::scene::insert(&text, &slot, last + 1, &copy) {
                text = t;
                count += 1;
            }
        }
        if count == 0 {
            self.status = "could not duplicate that widget".into();
            return;
        }
        self.text = text;
        self.select(None);
        self.dirty = true;
        self.reload();
        self.status = format!("duplicated {count}");
    }
}

/// Add `id` to a selection, or take it out.
///
/// Removing the primary promotes the first of the others, so that a
/// selection never has extras but no primary.
pub(crate) fn toggle_in(primary: &mut Option<NodeId>, extra: &mut Vec<NodeId>, id: NodeId) {
    if *primary == Some(id) {
        *primary = if extra.is_empty() {
            None
        } else {
            Some(extra.remove(0))
        };
    } else if let Some(i) = extra.iter().position(|&e| e == id) {
        extra.remove(i);
    } else if primary.is_none() {
        *primary = Some(id);
    } else {
        extra.push(id);
    }
}

/// The rectangle with `a` and `b` at opposite corners.
pub(crate) fn span(a: Point, b: Point) -> Rect {
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    Rect::new(x0, y0, x1.abs_diff(x0), y1.abs_diff(y0))
}

/// The widgets a rubber band over `area` picks: every visible leaf it
/// touches, and every panel it wholly encloses, outermost only.
///
/// Touching is enough for a leaf, because dragging a band right across a
/// row of gauges to take all of them is the whole point of a band. It is
/// not enough for a panel: a band that only has to touch one grabs the panel
/// behind everything, and moving what was meant then moves the whole scene.
/// A panel not enclosed is looked inside instead. Outermost only, because a
/// panel that is enclosed brings its children with it, and selecting them
/// too would move them twice.
pub(crate) fn enclosed(tree: &Tree, area: Rect) -> Vec<NodeId> {
    fn walk(tree: &Tree, id: NodeId, area: Rect, out: &mut Vec<NodeId>, depth: u32) {
        if depth > 64 {
            return;
        }
        let Some(node) = tree.get(id) else { return };
        if !node.visible {
            return;
        }
        let Some(abs) = tree.absolute_rect(id) else {
            return;
        };
        let whole = abs.left() >= area.left()
            && abs.top() >= area.top()
            && abs.right() <= area.right()
            && abs.bottom() <= area.bottom();
        if whole || (node.children.is_empty() && abs.intersects(area)) {
            out.push(id);
            return;
        }
        for &child in &node.children {
            walk(tree, child, area, out, depth + 1);
        }
    }
    let mut out = Vec::new();
    let Some(root) = tree.get(ROOT) else {
        return out;
    };
    // The document root is skipped: it is the whole scene, and a band that
    // encloses it has selected everything already.
    for &doc in &root.children {
        for &child in tree
            .get(doc)
            .map(|n| n.children.as_slice())
            .unwrap_or_default()
        {
            walk(tree, child, area, &mut out, 0);
        }
    }
    out
}

impl App {
    /// Select the parent of the primary, which no click on the preview can
    /// reach: a click lands on the topmost widget, and a panel is underneath
    /// everything it holds.
    pub(crate) fn select_parent(&mut self) {
        let parent = self
            .selected
            .and_then(|id| self.preview.tree.as_ref()?.get(id)?.parent)
            .filter(|&p| p != ROOT);
        match parent {
            Some(p) => self.select(Some(p)),
            None => self.status = "nothing above it to select".into(),
        }
    }
}
