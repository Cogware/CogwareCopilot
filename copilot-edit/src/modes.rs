// SPDX-License-Identifier: GPL-3.0-only
//! Adding, renaming and removing a rig's modes.
//!
//! The three names this project shipped -- normal, sport, track -- are an
//! example and nothing more. A mode is whatever a car needs one for: rock
//! crawl, tow, valet, wet. Nothing in the format or the code knows those three
//! names, and this is where a person makes their own.
//!
//! # Why these write to disk immediately
//!
//! Every other edit in the editor changes text and is undone with Ctrl+Z.
//! These cannot be: adding a mode creates one scene *file* per display, and a
//! rig that lists a file nobody wrote is a rig that will not open. So the
//! files and the rig are written together, at once, and the dialog says so.
//! Removing a mode is the exception in the other direction -- it unlists the
//! scenes and leaves them on disk, because deleting a panel somebody drew is
//! not a thing to do as a side effect of tidying a list.
//!
//! # Why the rig is spliced rather than rewritten
//!
//! A rig file is hand-written and carries comments explaining which screen is
//! which. Rewriting it from the parsed [`Rig`] would lose them, so the edits
//! go through [`copilot::scene::edit`], exactly as a widget's properties do.
//!
//! [`Rig`]: copilot::rig::Rig

use copilot::scene::Step;

use crate::App;
use crate::newdoc::NewDoc;

/// Why a mode name cannot be used.
///
/// A mode name is two things at once: a key in the rig file and a piece of
/// every scene file name it creates. What is rejected here is what would
/// break one of those.
pub(crate) fn check_name(name: &str, existing: &[String]) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a mode needs a name".into());
    }
    if name.len() > 32 {
        return Err("that name is too long to sit on a tab".into());
    }
    if existing.iter().any(|m| m == name) {
        return Err(format!("there is already a mode called {name}"));
    }
    // Letters, digits, spaces, dashes and underscores. A dot would confuse
    // the file extension, and a slash would put a scene somewhere else
    // entirely; neither is worth supporting to save someone a hyphen.
    if let Some(bad) = name
        .chars()
        .find(|c| !c.is_alphanumeric() && !matches!(c, ' ' | '-' | '_'))
    {
        return Err(format!(
            "`{bad}` cannot be in a mode name: letters, digits, spaces, - and _ only"
        ));
    }
    Ok(())
}

/// What a display's scene file should be called for `to`, given the one it
/// uses for `from`.
///
/// The convention every rig here follows is `<panel>-<mode>.scene`, so the
/// mode's name is swapped where it appears and the rest of the name is left
/// alone: `z31-normal.scene` becomes `z31-rockcrawl.scene`. A file that does
/// not follow the convention gets the mode appended instead, which is still
/// a name that says what it is.
pub(crate) fn scene_name_for(existing: &str, from: &str, to: &str) -> String {
    let to = to.trim().replace(' ', "-");
    let (stem, ext) = match existing.rsplit_once('.') {
        Some((s, e)) => (s, e),
        None => (existing, "scene"),
    };
    match stem.rfind(from) {
        Some(at) if !from.is_empty() => {
            let mut out = String::with_capacity(stem.len() + to.len());
            out.push_str(&stem[..at]);
            out.push_str(&to);
            out.push_str(&stem[at + from.len()..]);
            format!("{out}.{ext}")
        }
        _ => format!("{stem}-{to}.{ext}"),
    }
}

impl App {
    /// Add a mode, giving every display a scene for it.
    ///
    /// `copy_from` is the mode whose layouts the new one starts as, or `None`
    /// for an empty panel of each display's size. The new mode becomes the
    /// one being shown, since adding one is a prelude to laying it out.
    pub(crate) fn add_mode(&mut self, name: &str, copy_from: Option<usize>) -> Result<(), String> {
        let name = name.trim().to_string();
        let Some(session) = self.rig.as_ref() else {
            return Err("no rig is open".into());
        };
        check_name(&name, &session.rig.modes)?;
        if self.dirty || session.docs.iter().any(|d| d.dirty) {
            return Err("save or discard the unsaved changes first".into());
        }

        // Everything is worked out before anything is written, so a rig that
        // cannot be added to is not half added to.
        let dir = session.dir();
        let mut writes: Vec<(std::path::PathBuf, String)> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        for (i, display) in session.rig.displays.iter().enumerate() {
            let from = copy_from.and_then(|m| display.scenes.get(m)).cloned();
            let file = scene_name_for(
                from.as_deref().unwrap_or(&display.scenes[0]),
                copy_from.map_or("", |m| session.rig.modes[m].as_str()),
                &name,
            );
            if dir.join(&file).exists() {
                return Err(format!("{file} is already there; pick another name"));
            }
            let text = match copy_from {
                // The text as it is in the editor, which is what the person
                // is looking at and what they mean by "copy this one".
                Some(m) if m == session.mode => Ok(self.text_of(i)),
                Some(_) => std::fs::read_to_string(dir.join(from.unwrap_or_default()))
                    .map_err(|e| format!("cannot read {}'s scene: {e}", display.name)),
                None => Ok(self.blank_scene(i)),
            }?;
            writes.push((dir.join(&file), text));
            names.push(file);
        }

        // Splice the rig: the mode onto the list, then the file onto each
        // display, so a half-applied edit still names files that exist.
        let mut text = self.rig_text()?;
        for (i, file) in names.iter().enumerate() {
            text = copilot::scene::set(
                &text,
                &[Step::Key("displays"), Step::Index(i), Step::Key("scenes")],
                &name,
                &format!("\"{file}\""),
            )
            .ok_or_else(|| format!("could not write {}'s scene into the rig", i + 1))?;
        }
        text = copilot::scene::append(&text, &[Step::Key("modes")], &format!("\"{name}\""))
            .ok_or("could not add the mode to the rig")?;

        for (path, body) in writes {
            std::fs::write(&path, body)
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        }
        self.commit_rig(text)?;
        let to = self.rig.as_ref().and_then(|r| r.rig.mode_named(&name));
        if let Some(m) = to {
            self.show_mode(m);
        }
        self.status = match copy_from {
            Some(m) => format!(
                "added the {name} mode, copied from {}",
                self.rig
                    .as_ref()
                    .map_or(String::new(), |r| r.rig.modes[m].clone())
            ),
            None => format!("added the {name} mode, empty"),
        };
        Ok(())
    }

    /// Rename mode `i`.
    ///
    /// The scene *files* keep their names: renaming them would break any
    /// copy of the rig elsewhere -- on a card, in a backup -- that still
    /// points at the old ones, and the name of a file is not what the panel
    /// is called.
    pub(crate) fn rename_mode(&mut self, i: usize, name: &str) -> Result<(), String> {
        let name = name.trim().to_string();
        let Some(session) = self.rig.as_ref() else {
            return Err("no rig is open".into());
        };
        let Some(old) = session.rig.modes.get(i).cloned() else {
            return Err("no such mode".into());
        };
        if old == name {
            return Ok(());
        }
        let others: Vec<String> = session
            .rig
            .modes
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, m)| m.clone())
            .collect();
        check_name(&name, &others)?;

        let files: Vec<String> = session
            .rig
            .displays
            .iter()
            .map(|d| d.scenes[i].clone())
            .collect();
        let mut text = self.rig_text()?;
        for (d, file) in files.iter().enumerate() {
            let at = [Step::Key("displays"), Step::Index(d), Step::Key("scenes")];
            text = copilot::scene::unset(&text, &at, &old)
                .and_then(|t| copilot::scene::set(&t, &at, &name, &format!("\"{file}\"")))
                .ok_or("could not rename the mode on every display")?;
        }
        text = copilot::scene::replace(
            &text,
            &[Step::Key("modes"), Step::Index(i)],
            &format!("\"{name}\""),
        )
        .ok_or("could not rename the mode")?;
        self.commit_rig(text)?;
        self.status = format!("{old} is now {name}");
        Ok(())
    }

    /// Take mode `i` off the rig, leaving its scene files on disk.
    pub(crate) fn remove_mode(&mut self, i: usize) -> Result<(), String> {
        let Some(session) = self.rig.as_ref() else {
            return Err("no rig is open".into());
        };
        if session.rig.modes.len() <= 1 {
            return Err("a rig needs a mode; this is the last one".into());
        }
        let Some(name) = session.rig.modes.get(i).cloned() else {
            return Err("no such mode".into());
        };
        if self.dirty || session.docs.iter().any(|d| d.dirty) {
            return Err("save or discard the unsaved changes first".into());
        }

        let displays = session.rig.displays.len();
        let mut text = self.rig_text()?;
        for d in 0..displays {
            text = copilot::scene::unset(
                &text,
                &[Step::Key("displays"), Step::Index(d), Step::Key("scenes")],
                &name,
            )
            .ok_or("could not take the mode off every display")?;
        }
        text = copilot::scene::remove(&text, &[Step::Key("modes"), Step::Index(i)])
            .ok_or("could not take the mode off the rig")?;

        // Whatever is being shown, by name, so the view does not jump to a
        // different mode because the one it was on shifted down the list.
        let showing = self
            .rig
            .as_ref()
            .map(|r| r.rig.modes[r.mode].clone())
            .unwrap_or_default();
        self.commit_rig(text)?;
        let to = self
            .rig
            .as_ref()
            .and_then(|r| r.rig.mode_named(&showing))
            .unwrap_or(0);
        self.show_mode(to);
        self.status = format!("{name} is off the rig; its scene files are still in the folder");
        Ok(())
    }

    /// The rig's text, or the reason there is none.
    fn rig_text(&self) -> Result<String, String> {
        self.rig
            .as_ref()
            .map(|r| r.text.clone())
            .ok_or_else(|| "no rig is open".into())
    }

    /// Write `text` as the rig, re-read it, and reload every display.
    ///
    /// The re-parse is the check: a splice that produced something the rig
    /// parser will not take is refused here, with the file on disk left as
    /// it was.
    fn commit_rig(&mut self, text: String) -> Result<(), String> {
        let rig =
            copilot::rig::parse(&text).map_err(|e| format!("that would break the rig: {e:?}"))?;
        let Some(session) = self.rig.as_mut() else {
            return Err("no rig is open".into());
        };
        std::fs::write(&session.path, &text)
            .map_err(|e| format!("cannot write {}: {e}", session.path.display()))?;
        session.text = text;
        session.rig = rig;
        // The mode index may now name a different mode, or none.
        session.mode = session.mode.min(session.rig.modes.len() - 1);
        Ok(())
    }

    /// Show mode `m`, reloading every display, without the unsaved-changes
    /// check [`App::set_mode`] makes -- the caller has already made it.
    fn show_mode(&mut self, m: usize) {
        let Some(session) = self.rig.as_mut() else {
            return;
        };
        session.mode = m.min(session.rig.modes.len().saturating_sub(1));
        let (active, count) = (session.active, session.docs.len());
        for i in 0..count {
            let session = self.rig.as_ref().expect("a rig");
            let doc = Self::doc_for(session, i);
            if i == active {
                self.drop_selection();
                self.take_doc(doc);
            } else if let Some(s) = self.rig.as_mut() {
                s.docs[i] = doc;
            }
        }
        self.reload();
    }

    /// The text of display `i`, wherever it is being kept.
    fn text_of(&self, i: usize) -> String {
        match self.rig.as_ref() {
            Some(r) if i == r.active => self.text.clone(),
            Some(r) => r.docs[i].text.clone(),
            None => self.text.clone(),
        }
    }

    /// An empty scene the size and shape display `i` already is.
    fn blank_scene(&self, i: usize) -> String {
        let (preview, node) = match self.rig.as_ref() {
            Some(r) => (
                if i == r.active {
                    &self.preview
                } else {
                    &r.docs[i].preview
                },
                r.rig.displays[i].node,
            ),
            None => (&self.preview, 0),
        };
        let (width, height) = preview.scene_size();
        NewDoc {
            width,
            height,
            node,
            round: preview.shape == copilot::scene::Shape::Round,
            ..NewDoc::default()
        }
        .compose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rig of two displays in one mode, in a directory of its own, with a
    /// comment in it so the splicing can be shown not to eat it.
    fn rig_on_disk(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("copilot-modes-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        for (node, size, name) in [(1u8, 200u32, "cluster"), (2, 100, "gauge")] {
            std::fs::write(
                dir.join(format!("{name}-normal.scene")),
                format!(
                    r#"{{"width":{size},"height":{size},"node":{node},
                        "root":{{"type":"panel","rect":[0,0,{size},{size}],"name":"{name}"}}}}"#
                ),
            )
            .expect("scene");
        }
        std::fs::write(
            dir.join("test.rig"),
            r#"{
                // The cluster is the big one.
                "modes": ["normal"],
                "displays": [
                    {"name":"cluster","node":1,"scenes":{"normal":"cluster-normal.scene"}},
                    {"name":"gauge","node":2,"scenes":{"normal":"gauge-normal.scene"}},
                ],
            }"#,
        )
        .expect("rig");
        dir
    }

    fn open(dir: &std::path::Path) -> App {
        let mut a = App::new(None, &[]);
        a.open_rig(dir.join("test.rig"));
        assert!(a.rig.is_some(), "{}", a.status);
        a
    }

    #[test]
    fn a_mode_can_be_added_with_any_name_the_person_likes() {
        let dir = rig_on_disk("add");
        let mut a = open(&dir);
        a.add_mode("rockcrawl", Some(0))
            .unwrap_or_else(|e| panic!("{e}"));

        let r = a.rig.as_ref().expect("a rig");
        assert_eq!(r.rig.modes, ["normal", "rockcrawl"]);
        assert_eq!(r.mode, 1, "the new mode is the one being shown");
        // One scene file per display, copied from normal.
        for f in ["cluster-rockcrawl.scene", "gauge-rockcrawl.scene"] {
            assert!(dir.join(f).exists(), "{f} was not written");
        }
        assert_eq!(r.rig.scene_path(1, 1), Some("cluster-rockcrawl.scene"));
        assert_eq!(r.rig.scene_path(2, 1), Some("gauge-rockcrawl.scene"));
        // The copy is the layout it came from, and still builds.
        assert!(a.preview.error.is_none(), "{:?}", a.preview.error);
        assert_eq!(a.preview.scene_size(), (200, 200));
        assert_eq!(a.preview.node, Some(1));
        // And the comment in the rig survived being spliced.
        assert!(r.text.contains("The cluster is the big one"), "{}", r.text);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_empty_mode_is_a_bare_panel_of_each_displays_size() {
        let dir = rig_on_disk("empty");
        let mut a = open(&dir);
        a.add_mode("valet", None).unwrap_or_else(|e| panic!("{e}"));

        assert!(a.preview.error.is_none(), "{:?}", a.preview.error);
        assert_eq!(a.preview.scene_size(), (200, 200), "the cluster's size");
        assert_eq!(a.preview.node, Some(1), "and its node");
        let r = a.rig.as_ref().expect("a rig");
        let gauge = &r.docs[1];
        assert!(gauge.preview.error.is_none(), "{:?}", gauge.preview.error);
        assert_eq!(gauge.preview.scene_size(), (100, 100));
        // Empty means empty: the panel has nothing in it to lay out yet.
        let t = a.preview.tree.as_ref().expect("a tree");
        let doc_root = t.get(copilot::widget::ROOT).unwrap().children[0];
        assert!(t.get(doc_root).unwrap().children.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_bad_or_repeated_name_is_refused_before_anything_is_written() {
        let dir = rig_on_disk("bad");
        let mut a = open(&dir);
        for bad in ["", "  ", "normal", "a/b", "a.b"] {
            let before = std::fs::read_to_string(dir.join("test.rig")).unwrap();
            assert!(a.add_mode(bad, Some(0)).is_err(), "{bad} was accepted");
            assert_eq!(
                std::fs::read_to_string(dir.join("test.rig")).unwrap(),
                before,
                "{bad} changed the rig on its way to being refused"
            );
        }
        assert_eq!(a.rig.as_ref().unwrap().rig.modes, ["normal"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_mode_can_be_renamed_and_the_files_keep_their_names() {
        let dir = rig_on_disk("rename");
        let mut a = open(&dir);
        a.add_mode("sport", Some(0))
            .unwrap_or_else(|e| panic!("{e}"));
        a.rename_mode(1, "track").unwrap_or_else(|e| panic!("{e}"));

        let r = a.rig.as_ref().expect("a rig");
        assert_eq!(r.rig.modes, ["normal", "track"]);
        // The file is still the one that was written; renaming a mode is not
        // a reason to break a card that points at it.
        assert_eq!(r.rig.scene_path(1, 1), Some("cluster-sport.scene"));
        assert!(dir.join("cluster-sport.scene").exists());
        assert!(a.preview.error.is_none(), "{:?}", a.preview.error);
        // Renaming to something already taken is refused.
        assert!(a.rename_mode(1, "normal").is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn removing_a_mode_unlists_it_and_leaves_its_scenes_alone() {
        let dir = rig_on_disk("remove");
        let mut a = open(&dir);
        a.add_mode("tow", Some(0)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(a.rig.as_ref().unwrap().mode, 1);

        a.remove_mode(1).unwrap_or_else(|e| panic!("{e}"));
        let r = a.rig.as_ref().expect("a rig");
        assert_eq!(r.rig.modes, ["normal"]);
        assert_eq!(r.mode, 0, "back to a mode that still exists");
        assert!(
            dir.join("cluster-tow.scene").exists(),
            "a panel somebody drew must not vanish with the list entry"
        );
        assert!(a.preview.error.is_none(), "{:?}", a.preview.error);
        // The last mode cannot go.
        assert!(a.remove_mode(0).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_mode_edit_waits_for_unsaved_work() {
        let dir = rig_on_disk("dirty");
        let mut a = open(&dir);
        a.dirty = true;
        assert!(a.add_mode("tow", Some(0)).is_err());
        assert!(a.remove_mode(0).is_err());
        assert_eq!(a.rig.as_ref().unwrap().rig.modes, ["normal"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_mode_name_has_to_work_as_a_key_and_as_a_file_name() {
        let have = ["normal".to_string(), "sport".to_string()];
        assert!(check_name("rockcrawl", &have).is_ok());
        assert!(check_name("rock crawl", &have).is_ok());
        assert!(check_name("wet-weather", &have).is_ok());
        assert!(check_name("Tow_2", &have).is_ok());

        assert!(check_name("", &have).is_err());
        assert!(check_name("   ", &have).is_err());
        assert!(check_name("sport", &have).is_err(), "already there");
        for bad in ["a/b", "a.b", "a\"b", "a\\b", "a:b"] {
            assert!(check_name(bad, &have).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn a_new_scene_is_named_after_the_one_it_came_from() {
        assert_eq!(
            scene_name_for("z31-normal.scene", "normal", "rockcrawl"),
            "z31-rockcrawl.scene"
        );
        assert_eq!(
            scene_name_for("gauge-left-normal.scene", "normal", "tow"),
            "gauge-left-tow.scene"
        );
        // A space is a hyphen in a file name, whatever the tab says.
        assert_eq!(
            scene_name_for("z31-normal.scene", "normal", "rock crawl"),
            "z31-rock-crawl.scene"
        );
        // Not following the convention: the mode goes on the end instead.
        assert_eq!(
            scene_name_for("cluster.scene", "normal", "tow"),
            "cluster-tow.scene"
        );
        // Copying from nothing, so there is no name to swap out.
        assert_eq!(
            scene_name_for("z31-normal.scene", "", "tow"),
            "z31-normal-tow.scene"
        );
        // An extension that is not .scene is kept.
        assert_eq!(
            scene_name_for("panel-normal.json", "normal", "tow"),
            "panel-tow.json"
        );
    }
}
