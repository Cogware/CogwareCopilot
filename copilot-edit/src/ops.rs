// SPDX-License-Identifier: GPL-3.0-only
//! The commands that change the document.
//!
//! Every one of them edits the *text* and lets the tree be rebuilt from it,
//! rather than editing the tree and serialising. That is what keeps comments,
//! key order and blank lines intact through an edit, and it is why these live
//! together: they all share the same discipline about locating a span and
//! splicing bytes into it.

use copilot::scene::Step;
use copilot::widget::NodeId;

use crate::canvas::Gesture;
use crate::{App, alloc_array, rescale, starter_widget};

impl App {
    /// Rescale the whole document to `w` by `h`.
    ///
    /// Checkpointed like any other edit, so a resolution change taken by
    /// mistake is one undo away rather than a reason to reach for the file's
    /// last saved copy.
    pub(crate) fn apply_rescale(&mut self, w: u32, h: u32) {
        let from = self.preview.scene_size();
        if from == (w, h) {
            self.status = "already that size".into();
            return;
        }
        let Some(tree) = self.preview.tree.as_ref() else {
            self.status = "nothing to rescale: the scene does not build".into();
            return;
        };
        let Some(out) = rescale::rescale(&self.text, tree, from, (w, h)) else {
            self.status = "could not rescale: a rect is not four numbers".into();
            return;
        };
        self.checkpoint();
        self.text = out;
        self.dirty = true;
        self.reload();
        self.status = format!("rescaled {}x{} to {w}x{h}", from.0, from.1);
    }

    /// Set one field of widget `id` in the *text*, adding it if the author
    /// never wrote it. `g` is the gesture the edit belongs to, for undo.
    ///
    /// Splicing bytes rather than rewriting the document is the whole point:
    /// every comment, every blank line and the author's key order survive an
    /// edit untouched. A round trip through the value tree would lose all
    /// three, and the file people hand-edit would slowly stop being theirs.
    pub(crate) fn set_field_of(
        &mut self,
        id: NodeId,
        g: Option<Gesture>,
        field: &str,
        value: &str,
    ) {
        let Some(path) = self.node_path(id) else {
            return;
        };
        self.begin_edit(g);
        let Some(out) = copilot::scene::edit::set(&self.text, &path, field, value) else {
            self.status = format!("could not write `{field}` to that widget");
            return;
        };
        self.text = out;
        self.dirty = true;
        self.reload();
    }

    /// Take `field` off widget `id`, so it goes back to whatever the default
    /// or its parent says.
    pub(crate) fn unset_field_of(&mut self, id: NodeId, field: &str) {
        let Some(path) = self.node_path(id) else {
            return;
        };
        let Some(out) = copilot::scene::unset(&self.text, &path, field) else {
            self.status = format!("`{field}` is not set on that widget");
            return;
        };
        self.checkpoint();
        self.text = out;
        self.dirty = true;
        self.reload();
    }

    /// Set a key on the scene itself rather than on a widget: `width`,
    /// `antialias`, the things the whole picture takes.
    pub(crate) fn set_scene_field(&mut self, field: &str, value: &str) {
        let Some(out) = copilot::scene::set(&self.text, &[], field, value) else {
            self.status = format!("could not write `{field}` to the scene");
            return;
        };
        self.checkpoint();
        self.text = out;
        self.dirty = true;
        self.reload();
    }

    /// Take a key off the scene itself, so it goes back to its default.
    pub(crate) fn unset_scene_field(&mut self, field: &str) {
        let Some(out) = copilot::scene::unset(&self.text, &[], field) else {
            self.status = format!("`{field}` is not set on the scene");
            return;
        };
        self.checkpoint();
        self.text = out;
        self.dirty = true;
        self.reload();
    }

    /// The document path of the selected widget's *parent* array, and the
    /// selected widget's index within it.
    pub(crate) fn selected_slot(&self) -> Option<(Vec<Step<'static>>, usize)> {
        let tree = self.preview.tree.as_ref()?;
        let indices = tree.document_path(self.selected?)?;
        let (last, parents) = indices.split_last()?;
        let mut path = doc_path(parents);
        path.push(Step::Key("children"));
        Some((path, *last))
    }

    /// The document path of `id` itself.
    pub(crate) fn node_path(&self, id: NodeId) -> Option<Vec<Step<'static>>> {
        Some(doc_path(&self.preview.tree.as_ref()?.document_path(id)?))
    }

    /// Put `text` inside `parent`'s child list, at `at`.
    ///
    /// Creates the list when the widget has never had one. A scene file omits
    /// `children` from every leaf, which is most of the file, and refusing to
    /// drop something into a panel until somebody types an empty array by hand
    /// makes the editor useless for exactly the panels it should be best at.
    pub(crate) fn put_inside(&mut self, parent: NodeId, at: usize, text: &str) -> Option<String> {
        let path = self.node_path(parent)?;
        self.put_at(&path, at, text)
    }

    /// [`Self::put_inside`], for a parent named by its document path rather
    /// than by a node: what a move needs, since removing the moved widget
    /// renumbers the tree before the destination is written.
    fn put_at(&self, path: &[Step<'_>], at: usize, text: &str) -> Option<String> {
        let mut kids = path.to_vec();
        kids.push(Step::Key("children"));
        if copilot::scene::locate(&self.text, &kids).is_some() {
            copilot::scene::insert(&self.text, &kids, at, text)
        } else {
            copilot::scene::set(&self.text, path, "children", &alloc_array(text))
        }
    }

    /// Move widget `id` to be child number `at` of `into`, keeping it
    /// selected.
    ///
    /// The one operation every rearrangement is made of: Move inwards, Move
    /// outwards and a drag in the tree all come here. The moved widget's
    /// own source text travels intact, comments and all.
    ///
    /// `at` counts the destination's children as they are *before* the
    /// move. Taking the widget out first renumbers whatever came after it
    /// in its old list, and the destination -- the list itself, or a parent
    /// sitting later in it -- is corrected for that here, once, rather than
    /// by every caller.
    pub(crate) fn move_node(&mut self, id: NodeId, into: NodeId, at: usize) -> bool {
        let Some(tree) = self.preview.tree.as_ref() else {
            return false;
        };
        let Some(src) = tree.document_path(id) else {
            self.status = "the document root cannot be moved".into();
            return false;
        };
        let Some((si, list)) = src.split_last() else {
            self.status = "the document root cannot be moved".into();
            return false;
        };
        if id == into || crate::is_inside(tree, into, id) {
            self.status = "a widget cannot be moved into itself".into();
            return false;
        }
        let Some(mut dst) = tree.document_path(into) else {
            self.status = "nothing can sit beside the document root".into();
            return false;
        };
        let mut at = at;
        if dst.starts_with(list) {
            match dst.get(list.len()) {
                // The same list: an index past the source slides down one.
                None => at = at.saturating_sub(usize::from(at > *si)),
                // A parent later in the same list: its index slides down.
                Some(&d) if d > *si => dst[list.len()] -= 1,
                _ => {}
            }
        }

        let elem = doc_path(&src);
        let Some(span) = copilot::scene::locate(&self.text, &elem) else {
            self.status = "could not find that widget in the text".into();
            return false;
        };
        let Some(moved) = self.text.get(span.start..span.end).map(str::to_owned) else {
            return false;
        };
        self.checkpoint();
        let Some(without) = copilot::scene::remove(&self.text, &elem) else {
            self.status = "could not move that widget".into();
            return false;
        };
        let kept = core::mem::replace(&mut self.text, without);
        let Some(text) = self.put_at(&doc_path(&dst), at, &moved) else {
            self.text = kept;
            self.status = "could not move that widget there".into();
            return false;
        };
        self.text = text;
        self.dirty = true;
        self.reload();
        // The ids are new, but the moved widget sits at a known place.
        dst.push(at);
        let landed = self.preview.tree.as_ref().and_then(|t| node_at(t, &dst));
        self.selected = landed;
        self.extra.clear();
        self.reveal = landed.is_some();
        true
    }

    /// Move the selection into the sibling above it, or out beside its parent.
    ///
    /// The pair a tree needs to be rearranged from the keyboard. With Raise
    /// and Lower moving a widget within its parent, these two reach every
    /// position in the document; a drag in the tree reaches them all in one
    /// go.
    pub(crate) fn reparent(&mut self, inwards: bool) {
        let Some(id) = self.selected else {
            self.status = "select a widget first".into();
            return;
        };
        let Some(tree) = self.preview.tree.as_ref() else {
            return;
        };
        let Some(parent) = tree.get(id).and_then(|n| n.parent) else {
            return;
        };
        let target = if inwards {
            let Some(prev) = tree
                .index_in_parent(id)
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| tree.get(parent)?.children.get(i).copied())
            else {
                self.status = "nothing above it to go into".into();
                return;
            };
            let count = tree.get(prev).map_or(0, |n| n.children.len());
            (prev, count)
        } else {
            // The tree's own root is a frame, not a widget the file declares,
            // and a document has exactly one root object. A child of it has
            // nowhere further out to go.
            let Some(gp) = tree
                .get(parent)
                .and_then(|n| n.parent)
                .filter(|&g| g != copilot::widget::ROOT)
            else {
                self.status = "already at the top level".into();
                return;
            };
            let after = tree.index_in_parent(parent).map_or(0, |i| i + 1);
            (gp, after)
        };
        if self.move_node(id, target.0, target.1) {
            self.status = if inwards {
                "moved inwards"
            } else {
                "moved outwards"
            }
            .into();
        }
    }

    /// Move the selection `by` places within its parent's child list.
    ///
    /// Which is to say: change what it is drawn on top of. The list is draw
    /// order, so this is the only way to put a curve behind the bars that sit
    /// on it without retyping both.
    pub(crate) fn reorder_selected(&mut self, by: isize) {
        let Some((slot, idx)) = self.selected_slot() else {
            self.status = "select a widget first".into();
            return;
        };
        let count = self
            .preview
            .tree
            .as_ref()
            .and_then(|t| {
                let node = t.get(self.selected?)?;
                Some(t.get(node.parent?)?.children.len())
            })
            .unwrap_or(0);
        if count == 0 {
            self.status = "nothing to reorder".into();
            return;
        }
        let want = (idx as isize + by).clamp(0, count as isize - 1) as usize;
        if want == idx {
            self.status = if by < 0 {
                "already at the back".into()
            } else {
                "already at the front".into()
            };
            return;
        }

        let mut elem = slot.clone();
        elem.push(Step::Index(idx));
        let Some(span) = copilot::scene::locate(&self.text, &elem) else {
            self.status = "could not find that widget in the text".into();
            return;
        };
        let Some(moved) = self.text.get(span.start..span.end).map(str::to_owned) else {
            self.status = "could not read that widget from the text".into();
            return;
        };
        self.checkpoint();
        // Removed before inserted, so the destination index is counted in the
        // list the element is no longer in. Doing it the other way round moves
        // a widget one place short of where it was asked to go, every time it
        // travels forwards.
        let Some(without) = copilot::scene::remove(&self.text, &elem) else {
            self.status = "could not move that widget".into();
            return;
        };
        match copilot::scene::insert(&without, &slot, want, &moved) {
            Some(t) => {
                self.text = t;
                self.selected = None;
                self.dirty = true;
                self.reload();
                self.status = format!("moved to {} of {count}", want + 1);
            }
            None => self.status = "could not move that widget".into(),
        }
    }

    /// Append a new widget inside the selection, or inside the document root
    /// when nothing is selected.
    ///
    /// The root is the fallback because it is the one widget a click on the
    /// preview cannot select -- a click there means "nothing" -- and a scene
    /// with nothing selected is exactly the state a new widget is wanted in.
    pub(crate) fn add_child(&mut self, kind: &str) {
        let Some(into) = self.selected.or_else(|| self.doc_root()) else {
            self.status = "nothing to add to: the scene does not build".into();
            return;
        };
        let count = self
            .preview
            .tree
            .as_ref()
            .and_then(|t| t.get(into))
            .map(|n| n.children.len())
            .unwrap_or(0);
        self.checkpoint();
        match self.put_inside(into, count, &starter_widget(kind)) {
            Some(t) => {
                self.text = t;
                self.dirty = true;
                self.reload();
                // Selected so the properties pane shows it and the next key
                // or drag acts on it: the thing just added is the thing about
                // to be edited.
                let added = self
                    .preview
                    .tree
                    .as_ref()
                    .and_then(|t| t.get(into)?.children.last().copied());
                self.select(added);
                self.status = format!("added a {kind}");
            }
            None => self.status = "could not add there".into(),
        }
    }

    pub(crate) fn save(&mut self) {
        let Some(p) = self.path.clone() else {
            self.save_as();
            return;
        };
        // Refuse to write a file that will not load. An editor that saves
        // broken text has destroyed the only good copy.
        if self.preview.error.is_some() {
            self.status = "not saved: the scene does not parse".into();
            return;
        }
        match std::fs::write(&p, &self.text) {
            Ok(()) => {
                self.dirty = false;
                self.status = format!("saved {}", p.display());
            }
            Err(e) => self.status = format!("cannot write: {e}"),
        }
    }

    pub(crate) fn save_as(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("scene", &["scene", "json"])
            .save_file()
        {
            self.path = Some(p);
            self.save();
        }
    }

    /// Reformat the buffer through the crate's own writer.
    ///
    /// Destroys comments, which is why it is a button rather than something
    /// that happens on save.
    pub(crate) fn tidy(&mut self) {
        match copilot::scene::parse(&self.text) {
            Ok(v) => {
                self.text = copilot::scene::to_string(&v);
                self.dirty = true;
                self.reload();
                self.status = "reformatted (comments were dropped)".into();
            }
            Err(e) => self.status = format!("cannot reformat: {e:?}"),
        }
    }
}

/// The document path of a chain of child indices, in the format's own terms:
/// the root object, then one `children` array and one index per level.
pub(crate) fn doc_path(indices: &[usize]) -> Vec<Step<'static>> {
    let mut path = vec![Step::Key("root")];
    for &i in indices {
        path.push(Step::Key("children"));
        path.push(Step::Index(i));
    }
    path
}

/// The node at a chain of child indices from the document root.
pub(crate) fn node_at(tree: &copilot::widget::Tree, indices: &[usize]) -> Option<NodeId> {
    let mut id = *tree.get(copilot::widget::ROOT)?.children.first()?;
    for &i in indices {
        id = *tree.get(id)?.children.get(i)?;
    }
    Some(id)
}
