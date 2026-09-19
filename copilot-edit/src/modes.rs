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

mod files;

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
        return Err("use at most 32 UTF-8 bytes for a mode name".into());
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
    let from = from.trim().replace(' ', "-");
    // A mode name in a directory or inside a display name is not a suffix.
    let (directory, file) = existing
        .rsplit_once('/')
        .map_or(("", existing), |(d, f)| (d, f));
    let (stem, ext) = file.rsplit_once('.').unwrap_or((file, "scene"));
    let renamed = if !from.is_empty() && stem == from {
        to
    } else if !from.is_empty() {
        match stem.strip_suffix(&format!("-{from}")) {
            Some(prefix) => format!("{prefix}-{to}"),
            None => format!("{stem}-{to}"),
        }
    } else {
        format!("{stem}-{to}")
    };
    if directory.is_empty() {
        format!("{renamed}.{ext}")
    } else {
        format!("{directory}/{renamed}.{ext}")
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
        if copy_from.is_some_and(|m| m >= session.rig.modes.len()) {
            return Err("the source mode no longer exists; choose another starting layout".into());
        }
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
            if writes.iter().any(|(path, _)| *path == dir.join(&file)) {
                return Err(format!(
                    "several displays would create {file}; give their source scenes distinct filenames first"
                ));
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
                &crate::pins::quoted(file),
            )
            .ok_or_else(|| format!("could not write {}'s scene into the rig", i + 1))?;
        }
        text = copilot::scene::append(&text, &[Step::Key("modes")], &format!("\"{name}\""))
            .ok_or("could not add the mode to the rig")?;

        copilot::rig::parse(&text).map_err(|e| format!("that would break the rig: {e:?}"))?;
        files::with_new_scenes(&writes, || self.commit_rig(text))?;
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
                .and_then(|t| copilot::scene::set(&t, &at, &name, &crate::pins::quoted(file)))
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
mod tests;
