// SPDX-License-Identifier: MIT OR Apache-2.0
//! A rig open in the editor: every display on the bus, side by side, with
//! one of them being edited.
//!
//! # Why the other displays are parked rather than the editor made plural
//!
//! The editor's whole document state -- text, path, history, preview -- sits
//! on [`App`], and a hundred-odd places read it as *the* document. Making all
//! of them index into a list would have turned every command into
//! bookkeeping, for a feature whose rule is simple: one display is edited at
//! a time and the rest are pictures until clicked. So the rest are parked here
//! as [`Doc`]s, and switching displays swaps one in and the current one out.
//! The commands never learn there was more than one.
//!
//! # Modes
//!
//! A mode switch is the same as opening three files at once, and is treated
//! that way: it is refused while anything is unsaved, and the undo history
//! goes with the documents it belonged to.

use std::path::{Path, PathBuf};

use copilot::rig::Rig;

use crate::App;
use crate::preview::Preview;

/// One display's document while another is the one being edited.
///
/// The same fields `App` keeps for the active document, and nothing else:
/// selection, hover and gestures are about the display being edited and do
/// not travel.
pub(crate) struct Doc {
    /// Where it was read from, and where Save puts it.
    pub path: Option<PathBuf>,
    /// The scene file, as text.
    pub text: String,
    /// Whether the text differs from the file.
    pub dirty: bool,
    /// The scene as last built from the text.
    pub preview: Preview,
    /// Texts to go back to.
    pub undo: Vec<String>,
    /// Texts undone, to go forward to again.
    pub redo: Vec<String>,
    /// The GPU copy of the last frame drawn, kept across frames so a display
    /// that did not change is not re-uploaded.
    pub texture: Option<egui::TextureHandle>,
}

impl Doc {
    /// An empty slot: the shape the active display's slot has while its
    /// state is on `App`.
    fn parked() -> Self {
        Self {
            path: None,
            text: String::new(),
            dirty: false,
            preview: Preview::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            texture: None,
        }
    }

    /// Read `path` into a document for the display at `node`.
    ///
    /// A file that cannot be read, or that builds for a different node than
    /// the rig placed it on, still gets a document -- with the reason in its
    /// preview's error -- so the rest of the rig can be looked at and the
    /// problem is shown where it is rather than stopping everything.
    fn open(path: PathBuf, node: u8) -> Self {
        let mut doc = Self::parked();
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let base = path
                    .parent()
                    .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
                doc.preview.load(&text, &base);
                doc.text = text;
                if let Some(said) = doc.preview.node.filter(|&n| n != node) {
                    doc.preview.error = Some(format!(
                        "{}: the scene says node 0x{said:02X}, the rig says 0x{node:02X}",
                        path.display()
                    ));
                }
            }
            Err(e) => doc.preview.error = Some(format!("cannot read {}: {e}", path.display())),
        }
        doc.path = Some(path);
        doc
    }
}

/// A rig and the documents of its displays, in the mode being looked at.
pub(crate) struct RigSession {
    /// What the rig file says.
    pub rig: Rig,
    /// The rig file's own text.
    ///
    /// Kept so that adding or renaming a mode splices it, the way a widget
    /// edit splices a scene: a rig file is hand-written and commented, and
    /// rewriting it from the parsed `Rig` would throw both away.
    pub text: String,
    /// The rig file itself. Scene paths resolve against its directory.
    pub path: PathBuf,
    /// The mode every display is showing.
    pub mode: usize,
    /// One per display, in rig order. The active display's slot is parked
    /// and empty; its state is on `App`.
    pub docs: Vec<Doc>,
    /// Which display is being edited.
    pub active: usize,
}

impl RigSession {
    /// The directory scene paths are relative to.
    pub(crate) fn dir(&self) -> PathBuf {
        self.path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    }

    /// The file display `i` shows in the current mode.
    fn scene_path(&self, i: usize) -> Option<PathBuf> {
        self.scene_path_in(i, self.mode)
    }

    /// The file display `i` shows in mode `mode`.
    pub(crate) fn scene_path_in(&self, i: usize, mode: usize) -> Option<PathBuf> {
        let d = self.rig.displays.get(i)?;
        Some(self.dir().join(d.scenes.get(mode)?))
    }

    /// The display being edited.
    pub fn display(&self) -> &copilot::rig::Display {
        &self.rig.displays[self.active]
    }

    /// The name of the mode being looked at.
    pub fn mode_name(&self) -> &str {
        &self.rig.modes[self.mode]
    }

    /// Whether any parked document has unsaved changes.
    fn any_parked_dirty(&self) -> bool {
        self.docs.iter().any(|d| d.dirty)
    }
}

impl App {
    /// Open a rig: every display in its first mode, the first one editable.
    ///
    /// Replaces whatever was open. A rig that does not parse leaves the
    /// current document alone and says why, since a half-opened rig is worse
    /// than the scene that was there.
    pub(crate) fn open_rig(&mut self, path: PathBuf) {
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                self.status = format!("cannot read {}: {e}", path.display());
                return;
            }
        };
        let rig = match copilot::rig::parse(&text) {
            Ok(r) => r,
            Err(e) => {
                self.status = format!("{} is not a rig: {e:?}", path.display());
                return;
            }
        };
        let mut session = RigSession {
            rig,
            text,
            path,
            mode: 0,
            docs: Vec::new(),
            active: 0,
        };
        session.docs = (0..session.rig.displays.len())
            .map(|i| Self::doc_for(&session, i))
            .collect();
        // Take the first display's document as the one to edit, leaving its
        // slot parked. Everything about the previous document goes: a rig is
        // a different thing to be looking at.
        let first = std::mem::replace(&mut session.docs[0], Doc::parked());
        self.rig = None;
        self.drop_selection();
        self.take_doc(first);
        let n = session.rig.displays.len();
        let mode = session.mode_name().to_string();
        self.rig = Some(session);
        self.camera.fit();
        self.reload();
        self.status = format!("opened a rig of {n} displays in {mode} mode");
    }

    /// The document display `i` shows in the session's mode.
    pub(crate) fn doc_for(session: &RigSession, i: usize) -> Doc {
        let node = session.rig.displays[i].node;
        match session.scene_path(i) {
            Some(p) => Doc::open(p, node),
            None => Doc::parked(),
        }
    }

    /// Make display `i` the one being edited.
    ///
    /// The current document is parked in its slot and `i`'s is taken out.
    /// Selection and gestures do not travel: they name nodes of a tree that
    /// is no longer the one on screen.
    pub(crate) fn activate(&mut self, i: usize) {
        let Some(mut session) = self.rig.take() else {
            return;
        };
        if i == session.active || i >= session.docs.len() {
            self.rig = Some(session);
            return;
        }
        let current = session.active;
        let parked = self.park_doc();
        session.docs[current] = parked;
        let next = std::mem::replace(&mut session.docs[i], Doc::parked());
        session.active = i;
        self.drop_selection();
        self.take_doc(next);
        let name = session.display().name.clone();
        let node = session.display().node;
        self.rig = Some(session);
        self.reload();
        self.status = format!("editing {name} (node 0x{node:02X})");
    }

    /// Show every display in mode `m`.
    ///
    /// Refused while anything is unsaved: switching is opening a different
    /// set of files, and an unsaved change would have nowhere to go.
    pub(crate) fn set_mode(&mut self, m: usize) {
        let Some(mut session) = self.rig.take() else {
            return;
        };
        if m == session.mode || m >= session.rig.modes.len() {
            self.rig = Some(session);
            return;
        }
        if self.dirty || session.any_parked_dirty() {
            self.rig = Some(session);
            self.status = "save or discard the unsaved changes before switching mode".into();
            return;
        }
        session.mode = m;
        for i in 0..session.docs.len() {
            if i == session.active {
                let node = session.rig.displays[i].node;
                let doc = match session.scene_path(i) {
                    Some(p) => Doc::open(p, node),
                    None => Doc::parked(),
                };
                self.drop_selection();
                self.take_doc(doc);
            } else {
                session.docs[i] = Self::doc_for(&session, i);
            }
        }
        let mode = session.mode_name().to_string();
        self.rig = Some(session);
        self.reload();
        self.status = format!("{mode} mode");
    }

    /// Save every display that has changes, the one being edited included.
    pub(crate) fn save_all(&mut self) {
        self.save();
        let Some(session) = self.rig.as_mut() else {
            return;
        };
        let mut written = 0;
        for doc in &mut session.docs {
            let (Some(p), true) = (&doc.path, doc.dirty) else {
                continue;
            };
            // The same refusal as Save: a file that will not load is not one
            // to write over the copy that did.
            if doc.preview.error.is_some() {
                self.status = format!("not saved: {} does not build", p.display());
                continue;
            }
            match std::fs::write(p, &doc.text) {
                Ok(()) => {
                    doc.dirty = false;
                    written += 1;
                }
                Err(e) => self.status = format!("cannot write {}: {e}", p.display()),
            }
        }
        if written > 0 {
            self.status = format!("saved {} display(s)", written + 1);
        }
    }

    /// Forget the rig, keeping the document being edited as a plain scene.
    ///
    /// For New, Open and the examples: each replaces the document, and a rig
    /// whose active slot no longer holds one of its scenes is not a rig.
    pub(crate) fn close_rig(&mut self) {
        self.rig = None;
    }

    /// Whether display `i` is being edited, or `None` without a rig.
    pub(crate) fn active_display(&self) -> Option<usize> {
        self.rig.as_ref().map(|r| r.active)
    }

    /// Move the active document's state off `App` into a parked slot.
    pub(crate) fn park_doc(&mut self) -> Doc {
        Doc {
            path: self.path.take(),
            text: std::mem::take(&mut self.text),
            dirty: std::mem::take(&mut self.dirty),
            preview: std::mem::take(&mut self.preview),
            undo: std::mem::take(&mut self.undo),
            redo: std::mem::take(&mut self.redo),
            texture: self.texture.take(),
        }
    }

    /// Make `doc` the active document.
    pub(crate) fn take_doc(&mut self, doc: Doc) {
        self.path = doc.path;
        self.text = doc.text;
        self.dirty = doc.dirty;
        self.preview = doc.preview;
        self.undo = doc.undo;
        self.redo = doc.redo;
        self.texture = doc.texture;
    }

    /// Clear everything that names a node of the tree being left.
    pub(crate) fn drop_selection(&mut self) {
        self.selected = None;
        self.extra.clear();
        self.hover = None;
        self.gesture = None;
        self.drag = None;
        self.group.clear();
        self.marquee = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rig of three displays in two modes, written to a directory of its
    /// own. Scene `n` in mode `m` is a panel called "d<n>-<mode>".
    fn rig_on_disk() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "copilot-rig-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let scene = |node: u8, size: u32, name: &str| {
            format!(
                r#"{{"width":{size},"height":{size},"node":{node},
                     "root":{{"type":"panel","rect":[0,0,{size},{size}],"name":"{name}"}}}}"#
            )
        };
        for (n, node, size) in [(1, 1u8, 200u32), (2, 2, 100), (3, 3, 100)] {
            for mode in ["normal", "sport"] {
                std::fs::write(
                    dir.join(format!("d{n}-{mode}.scene")),
                    scene(node, size, &format!("d{n}-{mode}")),
                )
                .expect("scene");
            }
        }
        std::fs::write(
            dir.join("test.rig"),
            r#"{"modes":["normal","sport"],"displays":[
                {"name":"one","node":1,"scenes":{"normal":"d1-normal.scene","sport":"d1-sport.scene"}},
                {"name":"two","node":2,"scenes":{"normal":"d2-normal.scene","sport":"d2-sport.scene"}},
                {"name":"three","node":3,"scenes":{"normal":"d3-normal.scene","sport":"d3-sport.scene"}}]}"#,
        )
        .expect("rig");
        dir
    }

    fn root_name(a: &App) -> String {
        let t = a.preview.tree.as_ref().expect("a tree");
        let doc_root = t.get(copilot::widget::ROOT).unwrap().children[0];
        t.get(doc_root).unwrap().name.clone().unwrap_or_default()
    }

    #[test]
    fn opening_a_rig_parks_every_display_but_the_first() {
        let dir = rig_on_disk();
        let mut a = App::new(None, &[]);
        a.open_rig(dir.join("test.rig"));
        let r = a.rig.as_ref().expect("a rig");
        assert_eq!((r.docs.len(), r.active, r.mode), (3, 0, 0));
        assert_eq!(root_name(&a), "d1-normal");
        assert_eq!(a.preview.scene_size(), (200, 200));
        assert!(r.docs[0].text.is_empty(), "the active slot is parked");
        assert_eq!(r.docs[1].preview.node, Some(2));
        assert!(!a.dirty, "{}", a.status);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn activating_another_display_swaps_documents_and_drops_the_selection() {
        let dir = rig_on_disk();
        let mut a = App::new(None, &[]);
        a.open_rig(dir.join("test.rig"));
        a.selected = a.preview.tree.as_ref().and_then(|t| t.find("d1-normal"));
        a.text.push(' ');
        a.dirty = true;

        a.activate(2);
        assert_eq!(root_name(&a), "d3-normal");
        assert_eq!(a.preview.scene_size(), (100, 100));
        assert!(a.selected.is_none(), "a selection names the old tree");
        assert!(!a.dirty, "the third display was never touched");
        let r = a.rig.as_ref().expect("a rig");
        assert_eq!(r.active, 2);
        assert!(
            r.docs[0].dirty,
            "the first display's edit is parked with it"
        );
        assert!(r.docs[0].text.ends_with(' '));
        assert!(r.docs[2].text.is_empty(), "the active slot is parked");

        // Back again, and the edit is still there.
        a.activate(0);
        assert!(a.dirty && a.text.ends_with(' '));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_mode_switch_reloads_every_display_and_is_refused_while_dirty() {
        let dir = rig_on_disk();
        let mut a = App::new(None, &[]);
        a.open_rig(dir.join("test.rig"));
        a.dirty = true;
        a.set_mode(1);
        assert_eq!(a.rig.as_ref().unwrap().mode, 0, "{}", a.status);
        assert!(a.status.contains("unsaved"), "{}", a.status);

        a.dirty = false;
        a.set_mode(1);
        let r = a.rig.as_ref().unwrap();
        assert_eq!(r.mode, 1);
        assert_eq!(root_name(&a), "d1-sport");
        assert!(
            r.docs[1].path.as_ref().unwrap().ends_with("d2-sport.scene"),
            "{:?}",
            r.docs[1].path
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_scene_on_the_wrong_node_is_reported_where_it_sits() {
        let dir = rig_on_disk();
        // Put display three's scene where display two's should be.
        std::fs::copy(dir.join("d3-normal.scene"), dir.join("d2-normal.scene")).unwrap();
        let mut a = App::new(None, &[]);
        a.open_rig(dir.join("test.rig"));
        let r = a.rig.as_ref().unwrap();
        let err = r.docs[1].preview.error.as_deref().unwrap_or("");
        assert!(err.contains("0x03") && err.contains("0x02"), "{err}");
        assert!(a.preview.error.is_none(), "the first display is fine");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_dummy_values_reach_gauges_only_another_display_shows() {
        // The point of editing a rig at once: one slider per gauge, covering
        // every display. MAP is bound by the left gauge alone, so if the
        // driver only learned the active display it would not be offered at
        // all -- and the cluster's own CLNT would be a second slider for a
        // reading the right gauge also shows.
        let dir = std::env::temp_dir().join(format!("copilot-rig-drive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        for (name, text) in [
            ("test.rig", include_str!("../../examples/z31.rig")),
            (
                "z31-normal.scene",
                include_str!("../../examples/z31-normal.scene"),
            ),
            (
                "gauge-left-normal.scene",
                include_str!("../../examples/gauge-left-normal.scene"),
            ),
            (
                "gauge-right-normal.scene",
                include_str!("../../examples/gauge-right-normal.scene"),
            ),
        ] {
            std::fs::write(dir.join(name), text).expect("example");
        }
        // Only the normal mode's files are unpacked, so the rig is trimmed to
        // the one mode rather than pointing at scenes that are not there.
        let rig = std::fs::read_to_string(dir.join("test.rig")).unwrap();
        let one_mode = rig
            .replace(
                r#""modes": ["normal", "sport", "track"]"#,
                r#""modes": ["normal"]"#,
            )
            .replace(r#""sport":  "z31-sport.scene","#, "")
            .replace(r#""track":  "z31-track.scene""#, "")
            .replace(r#""sport":  "gauge-left-sport.scene","#, "")
            .replace(r#""track":  "gauge-left-track.scene""#, "")
            .replace(r#""sport":  "gauge-right-sport.scene","#, "")
            .replace(r#""track":  "gauge-right-track.scene""#, "");
        std::fs::write(dir.join("test.rig"), one_mode).unwrap();

        let mut a = App::new(None, &[]);
        a.open_rig(dir.join("test.rig"));
        assert!(a.rig.is_some(), "{}", a.status);
        assert!(a.preview.error.is_none(), "{:?}", a.preview.error);

        let g = &a.driver.gauges;
        assert!(g.contains_key("RPM"), "the cluster's own: {:?}", g.keys());
        assert!(
            g.contains_key("MAP"),
            "MAP is only on the left gauge, and is missing: {:?}",
            g.keys()
        );
        assert!(g.contains_key("CLNT"), "{:?}", g.keys());
        // The cluster shows coolant in °F over 120..270, the round gauge in
        // °F too; one channel, in °C, spanning what both need.
        let clnt = &g["CLNT"];
        assert!(
            clnt.min < 50.0 && clnt.max > 100.0,
            "the shared range is not in the gauge's own unit: {clnt:?}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_that_is_not_a_rig_leaves_the_document_alone() {
        let dir = rig_on_disk();
        let mut a = App::new(None, &[]);
        let before = a.text.clone();
        a.open_rig(dir.join("d1-normal.scene"));
        assert!(a.rig.is_none());
        assert_eq!(a.text, before);
        assert!(a.status.contains("not a rig"), "{}", a.status);
        a.open_rig(dir.join("missing.rig"));
        assert!(a.status.contains("cannot read"), "{}", a.status);
        let _ = std::fs::remove_dir_all(dir);
    }
}
