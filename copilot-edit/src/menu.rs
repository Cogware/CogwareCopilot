// SPDX-License-Identifier: GPL-3.0-only
//! Shared commands keep the main menus and context menus consistent.

use crate::{App, Pane, document::Action, place};

/// Connection settings can be inspected without interrupting an active session.
pub(crate) const CONNECTIONS_HINT: &str =
    "Open connection settings and live readings without disconnecting an active session.";

/// An operation shared by the menu bar, outliner and canvas.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Command {
    /// Restore the previous edit.
    Undo,
    /// Reapply the last undone edit.
    Redo,
    /// Show the selected widget's properties.
    Properties,
    /// Copy the selected widgets.
    Duplicate,
    /// Remove the selected widgets.
    Delete,
    /// Draw the selection one step nearer the front.
    Raise,
    /// Draw the selection one step nearer the back.
    Lower,
    /// Nest the selection under its previous sibling.
    Inwards,
    /// Move the selection out of its parent.
    Outwards,
    /// Select the containing widget.
    SelectParent,
    /// Clear selection.
    Deselect,
    /// Select siblings or top-level widgets.
    SelectAll,
    /// Add the named scene-format widget.
    Add(&'static str),
    /// Position or size the selection inside its parent.
    Arrange(place::Placement),
}

/// Widget types that can be created without external assets or a rig menu.
pub(crate) const KINDS: &[&str] = &[
    "panel",
    "pager",
    "roundrect",
    "frame",
    "gradient",
    "grid",
    "label",
    "sevenseg",
    "bar",
    "segbar",
    "led",
    "arc",
    "needle",
    "scale",
    "ruler",
    "line",
    "chart",
    "polygon",
];

impl App {
    pub(crate) fn run(&mut self, command: Command) {
        if self.document_blocked() {
            self.status = "Finish or cancel the open document dialog before editing.".into();
            return;
        }
        if self.preview.error.is_some()
            && !matches!(
                command,
                Command::Undo | Command::Redo | Command::Properties | Command::Deselect
            )
        {
            self.status = "Fix the errors in Scene source before editing widgets.".into();
            return;
        }
        match command {
            Command::Undo => self.undo(),
            Command::Redo => self.redo(),
            Command::Properties => self.pane = Pane::Props,
            Command::Duplicate => self.duplicate_selected(),
            Command::Delete => self.delete_selected(),
            Command::Raise => self.reorder_selected(1),
            Command::Lower => self.reorder_selected(-1),
            Command::Inwards => self.reparent(true),
            Command::Outwards => self.reparent(false),
            Command::SelectParent => self.select_parent(),
            Command::Deselect => self.select(None),
            Command::SelectAll => self.select_all(),
            Command::Add(kind) => self.add_child(kind),
            Command::Arrange(how) => self.arrange(how),
        }
    }

    pub(crate) fn file_menu(&mut self, ui: &mut egui::Ui) {
        if item(
            ui,
            true,
            "New scene...",
            "Ctrl+N",
            "Choose a display size and start a scene.",
        ) {
            self.new_doc = Some(crate::newdoc::NewDoc::default());
        }
        if item(
            ui,
            true,
            "Open scene or rig...",
            "Ctrl+O",
            "Open a scene or all displays in a rig.",
        ) {
            self.choose_document();
        }
        let mut chosen = None;
        ui.menu_button("Examples", |ui| {
            for (index, (label, hint, _)) in crate::EXAMPLES.iter().enumerate() {
                if item(ui, true, label, "", hint) {
                    chosen = Some(index);
                }
            }
        });
        if let Some(index) = chosen {
            self.request_document(Action::Example(index));
        }
        ui.separator();
        let valid = self.preview.error.is_none();
        if item(
            ui,
            valid,
            "Save scene",
            "Ctrl+S",
            "Save the active scene. Fix scene errors before saving.",
        ) {
            self.save();
        }
        if item(
            ui,
            valid,
            if self.rig.is_some() {
                "Export scene copy..."
            } else {
                "Save scene as..."
            },
            "Ctrl+Shift+S",
            "Save to another file. In a rig, export a copy without changing which file the rig uses.",
        ) {
            self.save_as();
        }
        if item(
            ui,
            self.rig.is_some(),
            "Save all scenes",
            "",
            "Save changed scenes on every display in this rig.",
        ) {
            self.save_all();
        }
        ui.separator();
        if item(ui, true, "Connections...", "", CONNECTIONS_HINT) {
            self.monitor.open();
        }
        if item(
            ui,
            true,
            "Upload to display...",
            "",
            "Choose a connection and send the scene or rig definition to a display.",
        ) {
            self.open_upload();
        }
        ui.separator();
        if item(
            ui,
            valid,
            "Resize scene...",
            "",
            "Scale widget positions, sizes and text to a new resolution. Undo is available.",
        ) {
            self.resize_to = Some(self.preview.scene_size());
        }
        if item(
            ui,
            valid,
            "Format scene source...",
            "",
            "Confirm before removing comments and reformatting. Undo restores the original.",
        ) {
            self.request_document(Action::Reformat);
        }
    }
}

pub(crate) fn item(
    ui: &mut egui::Ui,
    enabled: bool,
    label: &str,
    shortcut: &str,
    hint: &str,
) -> bool {
    let mut button = egui::Button::new(label);
    if !shortcut.is_empty() {
        button = button.shortcut_text(shortcut);
    }
    let mut response = ui.add_enabled(enabled, button);
    if !hint.is_empty() {
        response = response.on_hover_text(hint).on_disabled_hover_text(hint);
    }
    if response.clicked() {
        ui.close_menu();
        true
    } else {
        false
    }
}

pub(crate) fn edit_menu(
    ui: &mut egui::Ui,
    has_selection: bool,
    can_undo: bool,
    can_redo: bool,
) -> Option<Command> {
    let mut command = None;
    if item(
        ui,
        can_undo,
        "Undo",
        "Ctrl+Z",
        "Restore the previous edit. Each drag is one step.",
    ) {
        command = Some(Command::Undo);
    }
    if item(
        ui,
        can_redo,
        "Redo",
        "Ctrl+Y",
        "Reapply the last edit you undid.",
    ) {
        command = Some(Command::Redo);
    }
    ui.separator();
    command.or(selection_items(ui, has_selection))
}

fn selection_items(ui: &mut egui::Ui, has: bool) -> Option<Command> {
    let mut command = None;
    for (label, shortcut, hint, action) in [
        (
            "Widget properties",
            "",
            "Select a widget to inspect and edit it.",
            Command::Properties,
        ),
        (
            "Duplicate",
            "Ctrl+D",
            "Make a copy of the selected widgets.",
            Command::Duplicate,
        ),
        (
            "Delete",
            "Del",
            "Remove selected widgets. Undo restores them.",
            Command::Delete,
        ),
        (
            "Bring forward",
            "",
            "Move one step towards the front, over the next sibling.",
            Command::Raise,
        ),
        (
            "Send backward",
            "",
            "Move one step towards the back, under the previous sibling.",
            Command::Lower,
        ),
        (
            "Nest in previous widget",
            "",
            "Make the selection a child of the widget above it in the tree.",
            Command::Inwards,
        ),
        (
            "Move out of parent",
            "",
            "Make the selection a sibling of its current parent.",
            Command::Outwards,
        ),
        (
            "Select parent",
            "",
            "Select the widget containing the selection.",
            Command::SelectParent,
        ),
        (
            "Clear selection",
            "Esc",
            "Leave all widgets unselected.",
            Command::Deselect,
        ),
    ] {
        if item(ui, has, label, shortcut, hint) {
            command = Some(action);
        }
    }
    ui.separator();
    if item(
        ui,
        true,
        "Select all siblings",
        "Ctrl+A",
        "Select widgets sharing this parent, or top-level widgets if none is selected.",
    ) {
        command = Some(Command::SelectAll);
    }
    command
}

pub(crate) fn add_menu(ui: &mut egui::Ui) -> Option<Command> {
    ui.weak("Added inside the selected widget, or the scene's root.");
    let mut command = None;
    for (title, kinds) in [
        ("Layout and backgrounds", &KINDS[..6]),
        ("Text and readouts", &KINDS[6..8]),
        ("Gauges and indicators", &KINDS[8..15]),
        ("Lines, charts and shapes", &KINDS[15..]),
    ] {
        ui.separator();
        ui.strong(title);
        for kind in kinds {
            if item(
                ui,
                true,
                crate::starter::widget_title(kind),
                "",
                crate::starter::widget_hint(kind),
            ) {
                command = Some(Command::Add(kind));
            }
        }
    }
    command
}

pub(crate) fn arrange_menu(ui: &mut egui::Ui) -> Option<Command> {
    ui.weak("Position and size within the parent widget.");
    let mut command = None;
    for (label, placement) in [
        ("Centre horizontally", place::Placement::CenterX),
        ("Centre vertically", place::Placement::CenterY),
        ("Align left", place::Placement::Left),
        ("Align right", place::Placement::Right),
        ("Align top", place::Placement::Top),
        ("Align bottom", place::Placement::Bottom),
        ("Match parent width", place::Placement::FillX),
        ("Match parent height", place::Placement::FillY),
    ] {
        if item(
            ui,
            true,
            label,
            "",
            "Apply to the selected widgets relative to their parent.",
        ) {
            command = Some(Command::Arrange(placement));
        }
    }
    command
}

pub(crate) fn context_menu(ui: &mut egui::Ui, selected: Option<&str>) -> Option<Command> {
    ui.strong(selected.unwrap_or("Scene canvas"));
    ui.separator();
    let mut command = selection_items(ui, selected.is_some());
    ui.separator();
    ui.menu_button("Add widget", |ui| command = command.or(add_menu(ui)));
    ui.add_enabled_ui(selected.is_some(), |ui| {
        ui.menu_button("Arrange", |ui| command = command.or(arrange_menu(ui)))
            .response
            .on_disabled_hover_text("Select a widget first.");
    });
    if command.is_some() {
        ui.close_menu();
    }
    command
}
