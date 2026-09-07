// SPDX-License-Identifier: MIT OR Apache-2.0
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

//! A visual editor for copilot scene files.
//!
//! # Why the text is the document
//!
//! The editor holds the scene *as text* and re-parses it on every change,
//! rather than holding a widget tree and serialising on save. That is slower
//! and it is the only arrangement that keeps one source of truth: a file
//! hand-edited outside the editor, or by another person, is exactly as valid
//! as one the editor produced, and there is no state to get out of step with
//! what is on disk.
//!
//! It also means the preview is never stale in a way the file is not. What you
//! are looking at is what `copilot::scene::parse` makes of the text in the
//! buffer, drawn by the same renderer a Pi would use.

mod bind_ui;
mod canvas;
mod close;
mod curve;
mod drive;
mod handle;
mod inspect;
mod layout;
mod menu;
mod modes;
mod modes_ui;
mod newdoc;
mod ops;
mod outliner;
mod place;
mod preview;
mod rescale;
mod rig;
mod select;
mod shrink;
mod snap;
mod starter;
mod ui;
mod view;

#[cfg(test)]
mod tests;

/// How close, in screen points, a dragged edge has to come to a neighbour's
/// line before it is pulled onto it.
const SNAP_PX: f32 = 7.0;

/// How far a Shift-arrow moves the selection.
///
/// Ten rather than a power of two: scene coordinates are read and typed by
/// people, and a round decimal is what makes a nudged widget land on a number
/// worth having in the file.
const COARSE_NUDGE: i32 = 10;

/// Which pane the side panel is showing.
///
/// Tabbed rather than stacked: the properties, the source and the dummy
/// values are all reference material a person consults one at a time, and
/// panels side by side left the preview -- the thing being edited -- with the
/// narrowest column on screen.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pane {
    /// The selected widget's properties. The default: it is what a click on
    /// the preview is usually a prelude to.
    Props,
    /// The scene file as text.
    Source,
    /// The sliders that drive named widgets in the preview.
    Values,
}

use std::path::PathBuf;
use std::time::Instant;

use canvas::Gesture;
use copilot::widget::{Node, NodeId, ROOT, Tree};
use drive::Driver;
use handle::Handle;
use preview::Preview;
use snap::Guides;
pub(crate) use starter::{EXAMPLES, STARTER, alloc_array, kind_name, starter_widget};

fn main() -> eframe::Result<()> {
    // `copilot-edit [FILE|RIG] [--select NAME,NAME...] [--pane properties|scene|values]
    //               [--new] [--modes] [--curve]`.
    // The flags exist so a scene can be opened straight onto the widget in
    // question, and so the editor can be photographed in a state that
    // otherwise needs a mouse.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let after = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
    };
    let path = args
        .iter()
        .enumerate()
        .find(|(i, a)| {
            !a.starts_with("--")
                && !args
                    .get(i.wrapping_sub(1))
                    .is_some_and(|f| f.starts_with("--"))
        })
        .map(|(_, a)| PathBuf::from(a));
    let select: Vec<String> = after("--select")
        .map(|names| names.split(',').map(str::to_owned).collect())
        .unwrap_or_default();
    let fresh = args.iter().any(|a| a == "--new");
    let modes = args.iter().any(|a| a == "--modes");
    let curve = args.iter().any(|a| a == "--curve");
    let pane = match after("--pane").map(String::as_str) {
        Some("scene") => Pane::Source,
        Some("values") => Pane::Values,
        _ => Pane::Props,
    };
    eframe::run_native(
        "copilot editor",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
            ..Default::default()
        },
        Box::new(move |_cc| {
            let mut app = App::new(path, &select);
            app.pane = pane;
            if fresh {
                app.new_doc = Some(newdoc::NewDoc::default());
            }
            if modes && let Some(showing) = app.rig.as_ref().map(|r| r.mode) {
                app.modes = Some(modes_ui::ModesDialog::new(showing));
            }
            if curve
                && let Some(id) = app.selected
                && let Some((shape, field)) = app
                    .preview
                    .tree
                    .as_ref()
                    .and_then(|t| t.get(id))
                    .and_then(|n| curve::Shape::of(&n.kind))
            {
                app.start_curve(id, shape, field);
            }
            Ok(Box::new(app))
        }),
    )
}

struct App {
    path: Option<PathBuf>,
    text: String,
    dirty: bool,
    preview: Preview,
    driver: Driver,
    texture: Option<egui::TextureHandle>,
    started: Instant,
    last_us: u64,
    status: String,
    /// Where a drag started, the primary's rect when it began, and what was
    /// grabbed.
    drag: Option<(copilot::Point, copilot::Rect, Handle)>,
    /// Every widget the current drag is moving, with where each began.
    group: Vec<(NodeId, copilot::Rect)>,
    /// The corners of a rubber-band selection in progress, in scene pixels.
    marquee: Option<(copilot::Point, copilot::Point)>,
    /// Whether a drag is pulled onto its neighbours' lines.
    snapping: bool,
    /// How the preview is framed.
    camera: view::Camera,
    /// Grid spacing in scene pixels. 0 turns it off.
    grid: i32,
    /// The scene size the resolution dialog is proposing, while it is open.
    resize_to: Option<(u32, u32)>,
    /// The scene the new-document dialog is proposing, while it is open.
    new_doc: Option<newdoc::NewDoc>,
    /// The rig this document is one display of, with the others parked, or
    /// `None` for a scene opened on its own.
    rig: Option<rig::RigSession>,
    /// The Modes dialog, while it is open.
    modes: Option<modes_ui::ModesDialog>,
    /// The curve being shaped on the preview, while one is.
    curve: Option<curve::CurveEdit>,
    /// Whether the unsaved-changes prompt is up.
    asking_close: bool,
    /// Whether a close has been answered and may go through.
    closing: bool,
    /// Which of the side panel's panes is showing.
    pane: Pane,
    /// The lines the current drag snapped to, for drawing guides.
    guides: Guides,
    /// Previous document states, most recent last.
    ///
    /// Whole snapshots rather than a diff. A scene file is a few kilobytes and
    /// an edit session is a few hundred edits, so the memory is irrelevant and
    /// the correctness is free -- an undo stack built from inverse operations
    /// has to get every inverse right, and getting one wrong corrupts the
    /// document silently.
    undo: Vec<String>,
    /// States undone, for redo. Cleared by any new edit.
    redo: Vec<String>,
    /// The gesture the last undo snapshot was taken for, while it continues.
    ///
    /// A drag writes the document every frame, and every one of those writes
    /// was its own undo step: undoing a move took a hundred presses of Ctrl+Z,
    /// each putting the widget back one pixel. Edits that continue the same
    /// gesture -- the same drag, the same slider, the same word being typed --
    /// share the one snapshot taken when it began.
    gesture: Option<egui::Id>,
    /// The primary selection: what the properties pane shows, what the resize
    /// handles belong to, and what a snap is measured from.
    ///
    /// A NodeId, which is an index into the tree the current text produced. It
    /// is dropped whenever the tree changes shape rather than remapped: a
    /// stale index would silently select the wrong widget, which is worse than
    /// selecting nothing.
    selected: Option<NodeId>,
    /// The rest of the selection. They move, nudge, delete and duplicate with
    /// the primary, and take the same property edits when they are the same
    /// kind.
    extra: Vec<NodeId>,
    /// The widget under the pointer, on the preview or in the outliner.
    /// Outlined on the preview so a click's target is known before the click.
    /// Recomputed every frame and never remapped, like `selected`.
    hover: Option<NodeId>,
    /// Whether the outliner should scroll to the selection this frame. Set
    /// by a selection made on the preview, whose row may be anywhere in a
    /// long tree, and consumed by the outliner once it has been shown.
    reveal: bool,
}

impl App {
    fn new(path: Option<PathBuf>, select: &[String]) -> Self {
        let mut app = Self {
            path,
            text: String::new(),
            dirty: false,
            preview: Preview::new(),
            driver: Driver::default(),
            texture: None,
            started: Instant::now(),
            last_us: 0,
            status: String::new(),
            drag: None,
            group: Vec::new(),
            marquee: None,
            snapping: true,
            camera: view::Camera::default(),
            grid: 0,
            resize_to: None,
            new_doc: None,
            rig: None,
            modes: None,
            curve: None,
            asking_close: false,
            closing: false,
            pane: Pane::Props,
            guides: Guides::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            gesture: None,
            selected: None,
            extra: Vec::new(),
            hover: None,
            reveal: false,
        };
        // A rig on the command line opens as a rig: the file says what it is
        // by its extension, the one thing knowable before it is read.
        if let Some(p) = app
            .path
            .take_if(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("rig")))
        {
            app.open_rig(p);
        } else if app.path.is_some() {
            app.open_current();
        } else {
            app.text = STARTER.to_string();
            app.reload();
        }
        let ids: Vec<NodeId> = select
            .iter()
            .filter_map(|name| app.preview.tree.as_ref()?.find(name))
            .collect();
        if !ids.is_empty() {
            app.select_many(ids);
        }
        app
    }

    fn base(&self) -> PathBuf {
        self.path
            .as_ref()
            .and_then(|p| p.parent().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn open_current(&mut self) {
        let Some(p) = self.path.clone() else { return };
        match std::fs::read_to_string(&p) {
            Ok(t) => {
                self.text = t;
                self.dirty = false;
                self.reload();
                self.status = format!("opened {}", p.display());
            }
            Err(e) => self.status = format!("cannot read {}: {e}", p.display()),
        }
    }

    fn reload(&mut self) {
        let base = self.base();
        self.preview.load(&self.text, &base);
        if let Some(t) = &self.preview.tree {
            // The other displays' gauges too, so a slider for a gauge only
            // they show still exists, and one they share stays one slider.
            let others: Vec<(&Tree, &[copilot::scene::Binding])> = self
                .rig
                .iter()
                .flat_map(|r| r.docs.iter())
                .filter_map(|d| Some((d.preview.tree.as_ref()?, d.preview.bindings.as_slice())))
                .collect();
            self.driver.sync(t, &self.preview.bindings, &others);
            // A NodeId that no longer exists would select some other widget,
            // so a shrinking tree drops the selection rather than moving it.
            if self.selected.is_some_and(|id| t.get(id).is_none()) {
                self.selected = None;
            }
            self.extra.retain(|&id| t.get(id).is_some());
        }
    }

    /// Record `before` so the next edit can be undone back to it.
    ///
    /// Consecutive identical snapshots are dropped, and a new edit invalidates
    /// anything that was undone: the future it led to no longer exists.
    fn push_undo(&mut self, before: String) {
        if self.undo.last() == Some(&before) {
            return;
        }
        self.undo.push(before);
        self.redo.clear();
        // Bounded so a long session cannot grow without limit.
        if self.undo.len() > 256 {
            self.undo.remove(0);
        }
    }

    /// Record the current text so the next edit can be undone.
    ///
    /// Called before a change, not after, so the stack holds the state to
    /// return to. For a one-off command; a continuing gesture goes through
    /// [`Self::begin_edit`].
    fn checkpoint(&mut self) {
        let now = self.text.clone();
        self.push_undo(now);
        self.gesture = None;
    }

    /// Take an undo snapshot for an edit, unless the edit continues the
    /// gesture the last snapshot was taken for.
    pub(crate) fn begin_edit(&mut self, g: Option<Gesture>) {
        let now = self.text.clone();
        self.begin_edit_from(g, now);
    }

    /// [`Self::begin_edit`], with the text as it was before the edit.
    ///
    /// For the source pane, where the text has already changed by the time
    /// the editor hears about it.
    pub(crate) fn begin_edit_from(&mut self, g: Option<Gesture>, before: String) {
        let same = g.is_some_and(|g| Some(g.id) == self.gesture);
        if !same {
            self.push_undo(before);
        }
        self.gesture = g.filter(|g| !g.ends).map(|g| g.id);
    }

    pub(crate) fn undo(&mut self) {
        let Some(prev) = self.undo.pop() else {
            self.status = "nothing to undo".into();
            return;
        };
        let before = self.selection_paths();
        self.redo.push(core::mem::replace(&mut self.text, prev));
        self.after_history_step(&before);
        self.status = "undone".into();
    }

    pub(crate) fn redo(&mut self) {
        let Some(next) = self.redo.pop() else {
            self.status = "nothing to redo".into();
            return;
        };
        let before = self.selection_paths();
        self.undo.push(core::mem::replace(&mut self.text, next));
        self.after_history_step(&before);
        self.status = "redone".into();
    }

    /// Rebuild after an undo or redo, keeping whatever selection still means
    /// the same widgets.
    ///
    /// Undoing a move used to drop the selection, so the next Ctrl+Z or arrow
    /// key did nothing and the widget had to be found again. A widget is kept
    /// when it is still at the same place in the document; anything the step
    /// renumbered is let go, since a stale id would name a different widget.
    fn after_history_step(&mut self, before: &[(NodeId, Vec<usize>)]) {
        self.gesture = None;
        self.dirty = true;
        self.reload();
        let Some(t) = self.preview.tree.as_ref() else {
            return;
        };
        let still = |id: NodeId| {
            before
                .iter()
                .any(|(b, p)| *b == id && t.document_path(id).as_ref() == Some(p))
        };
        self.extra.retain(|&id| still(id));
        if self.selected.is_some_and(|id| !still(id)) {
            self.selected = None;
        }
    }

    /// Where each selected widget sits in the document.
    fn selection_paths(&self) -> Vec<(NodeId, Vec<usize>)> {
        let Some(t) = self.preview.tree.as_ref() else {
            return Vec::new();
        };
        self.members()
            .into_iter()
            .filter_map(|id| Some((id, t.document_path(id)?)))
            .collect()
    }

    /// The rects a drag on `id` should snap against, in `id`'s parent's space.
    ///
    /// The parent's own box comes first, at the origin, because it is the
    /// frame everything in the panel is placed against and its edges and
    /// centre are what "centred" usually means. Then every other visible
    /// widget in the scene, not only the siblings: a readout in one panel is
    /// lined up with the bar in the next panel far more often than with its
    /// own frame, and a snap that could see only siblings left exactly those
    /// alignments to be done by eye. Whatever is moving with the drag -- the
    /// rest of the selection, and everything inside any of it -- is left out,
    /// since it would snap the drag to itself.
    ///
    /// Everything is converted into the parent's space, because that is the
    /// space the rect is written in, and mixing the two is how a snap lands
    /// somewhere nobody asked for.
    fn snap_targets(&self, id: NodeId) -> Vec<copilot::Rect> {
        let Some(tree) = self.preview.tree.as_ref() else {
            return Vec::new();
        };
        let Some(parent_id) = tree.get(id).and_then(|n| n.parent) else {
            return Vec::new();
        };
        let (Some(parent), Some(origin)) = (tree.get(parent_id), tree.absolute_rect(parent_id))
        else {
            return Vec::new();
        };
        let moving = self.members();
        let mut out = vec![copilot::Rect::new(
            0,
            0,
            parent.rect.size.w,
            parent.rect.size.h,
        )];
        for i in 0..tree.len() {
            let Some(other) = u32::try_from(i).ok().map(NodeId) else {
                continue;
            };
            if other == parent_id
                || other == ROOT
                || moving.contains(&other)
                || moving.iter().any(|&m| is_inside(tree, other, m))
            {
                continue;
            }
            if !tree.get(other).is_some_and(|n| n.visible) {
                continue;
            }
            let Some(abs) = tree.absolute_rect(other) else {
                continue;
            };
            out.push(copilot::Rect::new(
                abs.left().saturating_sub(origin.left()),
                abs.top().saturating_sub(origin.top()),
                abs.size.w,
                abs.size.h,
            ));
        }
        out
    }

    /// The document's root widget: the tree root's only child, the one the
    /// file spells `root`.
    fn doc_root(&self) -> Option<NodeId> {
        self.preview
            .tree
            .as_ref()?
            .get(ROOT)?
            .children
            .first()
            .copied()
    }

    /// The widget a click at `p` means, or `None` for the background.
    ///
    /// The document root covers the whole scene, so a bare hit test answered
    /// it for every click that landed on nothing else, and there was no way
    /// to end up with nothing selected. A click on the background now clears
    /// the selection. The root is still one click away in the outliner, and
    /// adding to it needs no selection at all.
    fn pick(&self, p: copilot::Point) -> Option<NodeId> {
        let tree = self.preview.tree.as_ref()?;
        copilot::widget::hit_test(tree, p).filter(|&id| Some(id) != self.doc_root())
    }

    /// A widget's name and kind, the way the outliner shows it.
    fn describe(&self, id: NodeId) -> String {
        match self.preview.tree.as_ref().and_then(|t| t.get(id)) {
            Some(node) => describe_node(node),
            None => format!("node {}", id.0),
        }
    }
}

/// A widget's name and kind, the way the outliner shows it.
fn describe_node(node: &Node) -> String {
    match &node.name {
        Some(n) => format!("{n}  ({})", kind_name(&node.kind)),
        None => kind_name(&node.kind).to_string(),
    }
}

/// Whether `id` sits somewhere below `ancestor` in the tree.
fn is_inside(tree: &Tree, id: NodeId, ancestor: NodeId) -> bool {
    let mut cursor = tree.get(id).and_then(|n| n.parent);
    for _ in 0..tree.len() {
        // Bounded by the node count, so a corrupt tree cannot hang the editor.
        match cursor {
            Some(p) if p == ancestor => return true,
            Some(p) => cursor = tree.get(p).and_then(|n| n.parent),
            None => return false,
        }
    }
    false
}

/// A rect as the scene format writes it.
pub(crate) fn rect_text(r: copilot::Rect) -> String {
    format!("[{}, {}, {}, {}]", r.left(), r.top(), r.size.w, r.size.h)
}
