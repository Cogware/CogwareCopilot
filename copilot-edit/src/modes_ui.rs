// SPDX-License-Identifier: GPL-3.0-only
//! The Modes dialog: the rig's mode list, and what can be done to it, and
//! the prompt a mode switch puts when it would cost unsaved work.
//!
//! Apart from [`crate::modes`], which holds the operations themselves, for the
//! reason the widget property table is apart from the pane that draws it: what
//! a mode edit *is* can be tested without a window, and is, while this half is
//! the buttons that ask for one.

use crate::App;

/// The Modes dialog while it is open.
#[derive(Default)]
pub(crate) struct ModesDialog {
    /// The name being typed for a new mode.
    name: String,
    /// The mode the new one copies its layouts from, or `None` for empty.
    copy_from: Option<usize>,
    /// The mode being renamed, and the name typed so far.
    renaming: Option<(usize, String)>,
    /// What went wrong with the last thing tried.
    error: String,
}

impl ModesDialog {
    /// A fresh dialog, copying from whatever is being shown.
    pub(crate) fn new(showing: usize) -> Self {
        Self {
            copy_from: Some(showing),
            ..Self::default()
        }
    }
}

impl App {
    /// The Modes dialog: the whole list, and what can be done to it.
    pub(crate) fn modes_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.modes.take() else {
            return;
        };
        let Some(modes) = self.rig.as_ref().map(|r| r.rig.modes.clone()) else {
            return;
        };
        let showing = self.rig.as_ref().map_or(0, |r| r.mode);
        let mut open = true;
        // One action per frame, for the reason a property edit is one per
        // frame: each rewrites the rig and re-reads every display, and the
        // second would be working from a list the first had changed.
        let mut act: Option<Act> = None;

        egui::Window::new("Modes")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "The modes this rig switches between. The names are yours: a \
                         mode is whatever the car needs one for. Adding one gives every \
                         display a scene for it.",
                    )
                    .small()
                    .weak(),
                );
                ui.separator();
                egui::Grid::new("mode-list")
                    .num_columns(3)
                    .spacing([8.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for (i, m) in modes.iter().enumerate() {
                            match &mut d.renaming {
                                Some((at, text)) if *at == i => {
                                    let r = ui.text_edit_singleline(text);
                                    let go = ui.button("Save").clicked()
                                        || (r.lost_focus()
                                            && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                                    if go {
                                        act = Some(Act::Rename(i, text.clone()));
                                    }
                                    if ui.button("Cancel").clicked() {
                                        d.renaming = None;
                                    }
                                }
                                _ => {
                                    let label = if i == showing {
                                        egui::RichText::new(m).strong()
                                    } else {
                                        egui::RichText::new(m)
                                    };
                                    ui.label(label).on_hover_text(format!(
                                        "the value on the bus for this mode is {i}"
                                    ));
                                    if ui.button("Rename").clicked() {
                                        d.renaming = Some((i, m.clone()));
                                    }
                                    // The last mode cannot go: a rig with no
                                    // mode has nothing to show.
                                    if ui
                                        .add_enabled(modes.len() > 1, egui::Button::new("Remove"))
                                        .on_hover_text(
                                            "Takes it off the rig. The scene files stay \
                                             in the folder.",
                                        )
                                        .clicked()
                                    {
                                        act = Some(Act::Remove(i));
                                    }
                                }
                            }
                            ui.end_row();
                        }
                    });

                ui.separator();
                ui.horizontal(|ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut d.name)
                            .hint_text("new mode")
                            .desired_width(140.0),
                    );
                    ui.label("from");
                    let from = d
                        .copy_from
                        .and_then(|i| modes.get(i))
                        .map_or("empty", String::as_str);
                    egui::ComboBox::from_id_salt("mode-copy")
                        .selected_text(from)
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            for (i, m) in modes.iter().enumerate() {
                                ui.selectable_value(&mut d.copy_from, Some(i), m);
                            }
                            ui.selectable_value(&mut d.copy_from, None, "empty")
                                .on_hover_text("A bare panel the size of each display");
                        });
                    let go = ui.button("Add").clicked()
                        || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    if go && !d.name.trim().is_empty() {
                        act = Some(Act::Add(d.name.clone(), d.copy_from));
                    }
                });
                ui.label(
                    egui::RichText::new(
                        "Written to the folder as soon as you ask, so this is not \
                         undoable with Ctrl+Z.",
                    )
                    .small()
                    .weak(),
                );
                if !d.error.is_empty() {
                    ui.colored_label(egui::Color32::from_rgb(255, 120, 120), &d.error);
                }
            });

        let outcome = match act {
            Some(Act::Add(name, from)) => Some(self.add_mode(&name, from).map(|()| {
                d.name.clear();
            })),
            Some(Act::Rename(i, name)) => Some(self.rename_mode(i, &name).map(|()| {
                d.renaming = None;
            })),
            Some(Act::Remove(i)) => Some(self.remove_mode(i).map(|()| {
                // Every index below is into a list that just got shorter,
                // and a stale one names a different mode than it did.
                d.renaming = None;
                d.copy_from = self.rig.as_ref().map(|r| r.mode);
            })),
            None => None,
        };
        match outcome {
            Some(Ok(())) => d.error.clear(),
            Some(Err(e)) => d.error = e,
            None => {}
        }
        // Closed by the X, or by the rig being closed out from under it.
        self.modes = (open && self.rig.is_some()).then_some(d);
    }
}

impl App {
    /// The question a mode switch asks when it would cost unsaved work.
    ///
    /// The same three answers the close prompt offers, for the same reason:
    /// a switch re-reads every display from disk, so the edit in front of
    /// the person is about to go. Saying so and offering the way through
    /// beats the old silent refusal, which was taken for a dead button --
    /// and it is one click either way, which the status line never was.
    pub(crate) fn switch_prompt(&mut self, ctx: &egui::Context) {
        let Some(m) = self.switching else {
            return;
        };
        // The rig can be closed, or the mode taken off it, while the prompt
        // is up; a question about a mode that is gone is not one to put.
        let Some(name) = self.rig.as_ref().and_then(|r| r.rig.modes.get(m).cloned()) else {
            self.switching = None;
            return;
        };
        // Saved by some other route -- Ctrl+S, Save all -- since the click.
        // There is nothing left to ask about, so the switch just happens.
        let n = self.unsaved();
        if n == 0 {
            self.switching = None;
            self.swap_to_mode(m);
            return;
        }

        let mut save = false;
        let mut discard = false;
        let mut open = true;
        egui::Window::new("Unsaved changes")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!(
                    "{n} display{} of this rig {} unsaved changes.",
                    if n == 1 { "" } else { "s" },
                    if n == 1 { "has" } else { "have" }
                ));
                ui.label(
                    egui::RichText::new(format!(
                        "Switching to {name} re-reads every display's scene from the folder."
                    ))
                    .small()
                    .weak(),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    save = ui.button("Save and switch").clicked();
                    discard = ui
                        .button("Switch without saving")
                        .on_hover_text("The changes are lost")
                        .clicked();
                    if ui.button("Cancel").clicked() {
                        self.switching = None;
                    }
                });
            });
        // The X on the prompt means the same as Cancel: stay, unsaved.
        if !open {
            self.switching = None;
        }

        if discard {
            self.switching = None;
            self.swap_to_mode(m);
        } else if save {
            self.switching = None;
            self.save_and_switch(m);
        }
    }

    /// Write every display out and then switch, or stay put and say why.
    ///
    /// Still unsaved after a save means the save did not happen -- a scene
    /// that will not parse is refused rather than written over the copy that
    /// did. Switching over the top of that would throw the work away in the
    /// one case the prompt exists to prevent, so it stays where it is.
    pub(crate) fn save_and_switch(&mut self, m: usize) {
        self.switching = None;
        self.save_all();
        if self.unsaved() == 0 {
            self.swap_to_mode(m);
        } else {
            self.status = format!("not switched: {}", self.status);
        }
    }
}

/// What the dialog asked for this frame.
enum Act {
    /// Add a mode by this name, copying that one's layouts.
    Add(String, Option<usize>),
    /// Rename the mode at this index.
    Rename(usize, String),
    /// Take the mode at this index off the rig.
    Remove(usize),
}
