// SPDX-License-Identifier: MIT OR Apache-2.0
//! The Modes dialog: the rig's mode list, and what can be done to it.
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

/// What the dialog asked for this frame.
enum Act {
    /// Add a mode by this name, copying that one's layouts.
    Add(String, Option<usize>),
    /// Rename the mode at this index.
    Rename(usize, String),
    /// Take the mode at this index off the rig.
    Remove(usize),
}
