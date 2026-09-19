// SPDX-License-Identifier: GPL-3.0-only
//! Mode management and unsaved-scene protection when switching modes.
//! Rig edits are reviewed separately because scene undo cannot reverse them.

use crate::App;

#[cfg(test)]
mod tests;

/// The mode list, local drafts and a reviewed, not-yet-written operation.
#[derive(Default)]
pub(crate) struct ModesDialog {
    rig_path: Option<std::path::PathBuf>,
    name: String,
    suggested: bool,
    copy_from: Option<usize>,
    renaming: Option<(usize, String)>,
    pending: Option<Act>,
    error: String,
    success: String,
}

impl ModesDialog {
    /// Start by offering a copy of the mode currently on screen.
    pub(crate) fn new(showing: usize) -> Self {
        Self {
            copy_from: Some(showing),
            ..Self::default()
        }
    }

    fn duplicate(&mut self, index: usize, modes: &[String]) {
        self.copy_from = Some(index);
        self.name = suggested_name(&format!("{} copy", modes[index]), modes);
        self.renaming = None;
        self.success.clear();
    }
}

/// A valid, available draft name, without assuming a particular rig vocabulary.
fn suggested_name(base: &str, modes: &[String]) -> String {
    let base: String = base
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let base = if base.trim().is_empty() {
        "New mode"
    } else {
        base.trim()
    };
    for number in 1.. {
        let suffix = if number == 1 {
            String::new()
        } else {
            format!(" {number}")
        };
        let mut stem = base.to_string();
        while stem.len() + suffix.len() > 32 {
            stem.pop();
        }
        let candidate = format!("{}{suffix}", stem.trim_end());
        if crate::modes::check_name(&candidate, modes).is_ok() {
            return candidate;
        }
    }
    unreachable!()
}

impl App {
    /// Show modes, including explicit review before an immediate file change.
    pub(crate) fn modes_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.modes.take() else {
            return;
        };
        let Some(session) = self.rig.as_ref() else {
            return;
        };
        // A dialog must never apply an old rig's pending indices to a newly opened rig.
        if d.rig_path
            .as_ref()
            .is_some_and(|path| *path != session.path)
        {
            d = ModesDialog::new(session.mode);
        }
        d.rig_path = Some(session.path.clone());
        let modes = session.rig.modes.clone();
        let showing = session.mode;
        let displays = session.rig.displays.len();
        let folder = session.dir().display().to_string();
        if !d.suggested {
            d.name = suggested_name("New mode", &modes);
            d.suggested = true;
        }
        let unsaved = self.unsaved();
        let blocked = self.switching.is_some() || self.asking_close;
        let mut open = true;
        let mut close = false;
        let mut act = None;
        egui::Window::new("Manage modes")
            .id(egui::Id::new("modes"))
            .collapsible(false)
            .resizable(true)
            .default_width(560.0)
            .default_height((ctx.screen_rect().height() - 100.0).clamp(300.0, 680.0))
            .min_width(280.0)
            .max_width((ctx.screen_rect().width() - 24.0).max(280.0))
            .max_height((ctx.screen_rect().height() - 48.0).max(180.0))
            .vscroll(true)
            .open(&mut open)
            .show(ctx, |ui| {
                close = ui.button("Close modes").clicked();
                if !d.error.is_empty() { ui.colored_label(ui.visuals().error_fg_color, &d.error); }
                if !d.success.is_empty() { ui.label(&d.success); }
                ui.separator();
                ui.label("Each mode has a scene for every display. Select a mode to edit its layouts.");
                ui.weak("Rig changes are written immediately after confirmation. Ctrl+Z only undoes scene edits.");
                if blocked {
                    ui.label("Finish the unsaved-changes question before changing this rig.");
                }
                if unsaved > 0 {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("{unsaved} display(s) have unsaved scene edits. Save them before adding or removing modes."));
                        if ui.add_enabled(!blocked, egui::Button::new("Save all scenes")).clicked() {
                            self.save_all();
                            d.error = if self.unsaved() > 0 {
                                format!("Some scenes were not saved. {}", self.status)
                            } else { String::new() };
                        }
                    });
                }
                ui.separator();
                if d.pending.is_none() {
                ui.add_enabled_ui(!blocked, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("mode-list")
                        .max_height((ctx.screen_rect().height() * 0.38).max(100.0))
                        .show(ui, |ui| {
                            for (i, name) in modes.iter().enumerate() {
                                ui.push_id(("mode", i), |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        let label = if i == showing {
                                            format!("{name} — editing")
                                        } else { name.clone() };
                                        if ui.selectable_label(i == showing, label)
                                            .on_hover_text("Open this mode's scenes on every display. Unsaved edits are protected.")
                                            .clicked() {
                                            self.set_mode(i);
                                        }
                                        ui.weak(format!("Bus value {i}"));
                                    });
                                    ui.horizontal_wrapped(|ui| {
                                        if ui.button("Rename…").clicked() {
                                            d.renaming = Some((i, name.clone()));
                                        }
                                        if ui.button("Duplicate…")
                                            .on_hover_text("Prepare a named copy below; no files change yet.")
                                            .clicked() {
                                            d.duplicate(i, &modes);
                                        }
                                        let reason = if modes.len() == 1 {
                                            "A rig must keep at least one mode."
                                        } else if unsaved > 0 {
                                            "Save all scene edits before removing a mode."
                                        } else {
                                            "Review removal. Scene files remain on disk."
                                        };
                                        if ui.add_enabled(modes.len() > 1 && unsaved == 0, egui::Button::new("Remove…"))
                                            .on_hover_text(reason).on_disabled_hover_text(reason).clicked() {
                                            d.pending = Some(Act::Remove(i));
                                        }
                                    });
                                    if let Some((index, text)) = &mut d.renaming
                                        && *index == i {
                                            ui.add(egui::TextEdit::singleline(text).id_salt("rename").desired_width(220.0));
                                            let others: Vec<_> = modes.iter().enumerate()
                                                .filter(|(j, _)| *j != i).map(|(_, m)| m.clone()).collect();
                                            let error = crate::modes::check_name(text, &others).err();
                                            let changed = text.trim() != name;
                                            let mut cancel = false;
                                            ui.horizontal_wrapped(|ui| {
                                                if ui.add_enabled(error.is_none() && changed, egui::Button::new("Review rename…"))
                                                    .on_disabled_hover_text(error.as_deref().unwrap_or("Enter a different name."))
                                                    .clicked() {
                                                    d.pending = Some(Act::Rename(i, text.trim().into()));
                                                }
                                                cancel = ui.button("Cancel rename").clicked();
                                            });
                                            if let Some(error) = error { ui.weak(error); }
                                            if cancel { d.renaming = None; }
                                    }
                                    ui.separator();
                                });
                            }
                        });
                    ui.heading("Add a mode");
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut d.name).id_salt("new-mode-name").desired_width(260.0));
                    ui.weak("Letters, digits, spaces, hyphens and underscores; up to 32 UTF-8 bytes.");
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Starting layouts");
                        egui::ComboBox::from_id_salt("mode-copy")
                            .selected_text(d.copy_from.and_then(|i| modes.get(i)).map_or("Empty panels", String::as_str))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut d.copy_from, None, "Empty panels");
                                for (i, name) in modes.iter().enumerate() {
                                    ui.selectable_value(&mut d.copy_from, Some(i), format!("Copy {name}"));
                                }
                            });
                    });
                    ui.weak("Creates one scene per display, then opens the new mode. Empty panels keep each display's size and shape.");
                    let error = crate::modes::check_name(&d.name, &modes).err();
                    let reason = error.as_deref().unwrap_or(if unsaved > 0 {
                        "Save all scene edits first."
                    } else { "Review the files that will be created." });
                    if ui.add_enabled(error.is_none() && unsaved == 0, egui::Button::new("Review new mode…"))
                        .on_hover_text(reason).on_disabled_hover_text(reason).clicked() {
                        d.pending = Some(Act::Add(d.name.trim().into(), d.copy_from));
                    }
                    if let Some(error) = error { ui.weak(error); }
                });
                }
                if let Some(pending) = d.pending.clone() {
                    ui.separator();
                    ui.group(|ui| {
                        ui.strong("Confirm immediate rig change");
                        match &pending {
                            Act::Add(name, from) => {
                                ui.label(format!("Create “{name}” with {displays} new scene file(s), {}.",
                                    from.and_then(|i| modes.get(i)).map_or("using empty panels".into(), |n| format!("copying “{n}”"))));
                            }
                            Act::Rename(i, name) => {
                                ui.label(format!("Rename “{}” to “{name}”. Scene filenames stay unchanged.", modes[*i]));
                                ui.label("Actions referring to the old mode name are not renamed. Update them separately.");
                            }
                            Act::Remove(i) => {
                                ui.label(format!("Remove “{}” from every display? Scene files stay in the folder, but this cannot be undone with Ctrl+Z.", modes[*i]));
                                ui.label("Later modes shift down one bus value. Check mode actions and controller configuration.");
                            }
                        }
                        ui.weak(format!("Folder: {folder}"));
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(!blocked, egui::Button::new(pending.label())).clicked() {
                                act = Some(pending.clone());
                                d.pending = None;
                            }
                            if ui.button("Cancel change").clicked() { d.pending = None; }
                        });
                    });
                }
            });
        if let Some(action) = act {
            let outcome = match action {
                Act::Add(name, from) => self.add_mode(&name, from).map(|()| d.suggested = false),
                Act::Rename(i, name) => self.rename_mode(i, &name).map(|()| d.renaming = None),
                Act::Remove(i) => self.remove_mode(i).map(|()| {
                    d.renaming = None;
                    d.copy_from = self.rig.as_ref().map(|r| r.mode);
                    self.switching = None;
                }),
            };
            match outcome {
                Ok(()) => {
                    d.error.clear();
                    d.success = format!("Saved to disk: {}", self.status);
                }
                Err(e) => {
                    d.error = e;
                    d.success.clear();
                }
            }
        }
        self.modes = (open && !close && self.rig.is_some()).then_some(d);
    }

    /// Ask before switching away from unsaved scenes; failed saves keep the question open.
    pub(crate) fn switch_prompt(&mut self, ctx: &egui::Context) {
        let Some(m) = self.switching else { return };
        let Some(name) = self.rig.as_ref().and_then(|r| r.rig.modes.get(m).cloned()) else {
            self.switching = None;
            return;
        };
        let n = self.unsaved();
        if n == 0 {
            self.switching = None;
            self.swap_to_mode(m);
            return;
        }
        let mut save = false;
        let mut discard = false;
        let mut open = true;
        egui::Window::new("Switch mode — unsaved scenes")
            .id(egui::Id::new("switch-mode-confirmation"))
            .collapsible(false)
            .resizable(true)
            .default_width(440.0)
            .max_width((ctx.screen_rect().width() - 24.0).max(280.0))
            .vscroll(true)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("{n} display(s) have unsaved scene changes."));
                ui.label(format!(
                    "Switching to “{name}” reloads every display from disk."
                ));
                ui.weak("Save keeps these edits. Discard loses them and their undo history.");
                if self.status.starts_with("not switched:") {
                    ui.colored_label(ui.visuals().error_fg_color, &self.status);
                }
                ui.horizontal_wrapped(|ui| {
                    save = ui.button("Save all and switch").clicked();
                    discard = ui.button("Discard edits and switch").clicked();
                    if ui.button("Keep editing").clicked() {
                        self.switching = None;
                    }
                });
            });
        if !open {
            self.switching = None;
        }
        if discard {
            self.switching = None;
            self.swap_to_mode(m);
        } else if save {
            self.save_and_switch(m);
        }
    }

    /// Save every display before switching, leaving failed saves visible and recoverable.
    pub(crate) fn save_and_switch(&mut self, m: usize) {
        self.save_all();
        if self.unsaved() == 0 {
            self.switching = None;
            self.swap_to_mode(m);
        } else {
            self.switching = Some(m);
            self.status = format!(
                "not switched: {} display(s) still have unsaved edits. {}",
                self.unsaved(),
                self.status
            );
        }
    }
}

#[derive(Clone)]
enum Act {
    Add(String, Option<usize>),
    Rename(usize, String),
    Remove(usize),
}

impl Act {
    fn label(&self) -> &'static str {
        match self {
            Self::Add(..) => "Create mode and scenes",
            Self::Rename(..) => "Save mode name",
            Self::Remove(..) => "Remove mode from rig",
        }
    }
}
