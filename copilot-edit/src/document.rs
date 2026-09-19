// SPDX-License-Identifier: GPL-3.0-only
//! Protect unsaved documents consistently before replacing their contents.

use std::path::PathBuf;

use crate::{App, Driver, Pane, newdoc::NewDoc};

pub(crate) enum Action {
    New(NewDoc),
    Open(PathBuf),
    Example(usize),
    Reformat,
}

impl Action {
    fn description(&self) -> &'static str {
        match self {
            Self::New(_) => "create a new scene",
            Self::Open(_) => "open another document",
            Self::Example(_) => "open an example",
            Self::Reformat => "format the scene source",
        }
    }
}

impl App {
    /// Export a rig scene without moving the rig's active document to a different file.
    pub(crate) fn export_scene_copy(&mut self, path: &std::path::Path) {
        if self.path.as_deref() == Some(path) {
            self.write_scene(path);
            return;
        }
        let original = self.path.clone();
        let dirty = self.dirty;
        let saved = self.write_scene(path);
        self.path = original;
        self.dirty = dirty;
        if saved {
            self.status = format!(
                "Exported a scene copy to {}. The rig still uses the original file; assets were not copied.",
                path.display(),
            );
        }
    }

    /// Editing shortcuts must not change a document behind a pending decision.
    pub(crate) fn document_blocked(&self) -> bool {
        self.pending_action.is_some()
            || self.asking_close
            || self.switching.is_some()
            || self.new_doc.is_some()
            || self.resize_to.is_some()
    }

    pub(crate) fn request_document(&mut self, action: Action) {
        if matches!(action, Action::Reformat) || self.unsaved() > 0 {
            self.pending_action = Some(action);
        } else {
            self.perform_document(action);
        }
    }

    pub(crate) fn choose_document(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Scenes and rigs", &["scene", "json", "rig"])
            .add_filter("Scene", &["scene", "json"])
            .add_filter("Rig", &["rig"])
            .pick_file()
        {
            self.request_document(Action::Open(path));
        }
    }

    fn perform_document(&mut self, action: Action) {
        match action {
            Action::New(spec) => self.start(&spec),
            Action::Open(path) => self.open_document(path),
            Action::Example(index) => {
                if let Some((name, _, example)) = crate::EXAMPLES.get(index) {
                    self.open_example(name, example);
                } else {
                    self.status = "That example is no longer available.".into();
                }
            }
            Action::Reformat => self.tidy(),
        }
    }

    fn open_document(&mut self, path: PathBuf) {
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("rig"))
        {
            self.open_rig(path);
            return;
        }
        // Read before replacing anything, so an unreadable file cannot orphan the current work.
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) => {
                self.status = format!(
                    "Could not open {}: {error}. Your current document is unchanged.",
                    path.display()
                );
                return;
            }
        };
        self.close_rig();
        self.drop_selection();
        self.preview = crate::preview::Preview::new();
        self.texture = None;
        self.text = text;
        self.path = Some(path);
        self.dirty = false;
        self.undo.clear();
        self.redo.clear();
        self.driver = Driver::default();
        self.camera.fit();
        self.reload();
        if self.preview.error.is_some() {
            self.pane = Pane::Source;
            self.status = "Opened with scene errors. Edit the source to repair them; the file has not been changed.".into();
        } else if let Some(path) = &self.path {
            self.status = format!("Opened {}", path.display());
        }
    }

    fn finish_document_prompt(&mut self, save: bool) {
        if save {
            self.save_all();
            if self.unsaved() > 0 {
                return;
            }
        }
        if let Some(action) = self.pending_action.take() {
            self.perform_document(action);
        }
    }

    pub(crate) fn document_prompt(&mut self, ctx: &egui::Context) {
        let Some(action) = &self.pending_action else {
            return;
        };
        let format = matches!(action, Action::Reformat);
        let description = action.description();
        let mut open = true;
        let mut save = false;
        let mut discard = false;
        let mut cancel =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        egui::Window::new(if format {
            "Format scene source?"
        } else {
            "Save changes before continuing?"
        })
        .id(egui::Id::new("document_decision"))
        .order(egui::Order::Foreground)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(430.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            if format {
                ui.label("Formatting removes comments and rewrites the source layout.");
                ui.small(
                    "Normal property edits preserve comments. You can undo formatting with Ctrl+Z.",
                );
            } else {
                let count = self.unsaved();
                ui.label(format!(
                    "Save changes to {count} document(s) before you {description}?"
                ));
                ui.small("This includes unsaved scenes on other displays in the rig.");
                if !self.status.is_empty() {
                    ui.weak(&self.status);
                }
            }
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if !format {
                    save = ui.button("Save and continue").clicked();
                }
                discard = ui
                    .button(if format {
                        "Remove comments and format"
                    } else {
                        "Discard changes"
                    })
                    .on_hover_text(if format {
                        "Rewrite the source; Ctrl+Z restores it."
                    } else {
                        "Continue without saving the current edits."
                    })
                    .clicked();
                cancel |= ui.button("Cancel").clicked();
            });
        });
        if cancel || !open {
            self.pending_action = None;
        } else if save || discard {
            self.finish_document_prompt(save);
        }
    }

    pub(crate) fn document_title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled scene".into());
        format!(
            "{}{} - Cogware scene editor",
            if self.unsaved() > 0 { "* " } else { "" },
            name
        )
    }
}

#[cfg(test)]
mod tests;
