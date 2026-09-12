// SPDX-License-Identifier: GPL-3.0-only
//! The commands, and the menus that offer them.
//!
//! One list of commands, each with a name, so that the menu bar, a right-click
//! on the preview and a right-click on a row of the outliner all offer the
//! same things and cannot drift apart. A menu only *chooses* a command;
//! running it is a separate step. That is what lets a menu be drawn while the
//! tree it is drawn from is still borrowed, since the tree is replaced by the
//! command and not by the menu.

use crate::{App, Pane, place};

/// One thing a menu can ask for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Command {
    /// Put the document back one edit.
    Undo,
    /// Redo the edit last undone.
    Redo,
    /// Show the properties pane.
    Properties,
    /// Copy the selection in beside itself.
    Duplicate,
    /// Remove the selection.
    Delete,
    /// Draw the selection one place later, so it sits on top of more.
    Raise,
    /// Draw the selection one place earlier.
    Lower,
    /// Make the selection a child of the widget above it.
    Inwards,
    /// Make the selection a sibling of its parent.
    Outwards,
    /// Select the selection's parent.
    SelectParent,
    /// Clear the selection.
    Deselect,
    /// Select every sibling of the selection, or every top-level widget.
    SelectAll,
    /// Add a widget of this kind inside the selection.
    Add(&'static str),
    /// Put the selection somewhere exact inside its parent.
    Arrange(place::Placement),
}

/// The widget kinds the Add menu offers, in the order it offers them.
///
/// Grouped by what they are for rather than alphabetically: the boxes, then
/// the text, then the gauges, then the drawn shapes. Image and animation are
/// absent because each needs a file the menu cannot ask for.
pub(crate) const KINDS: &[&str] = &[
    "panel",
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
    /// Carry out `cmd` against the current selection.
    pub(crate) fn run(&mut self, cmd: Command) {
        match cmd {
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

    /// The command bar across the top.
    pub(crate) fn toolbar(&mut self, ctx: &egui::Context) {
        let mut cmd = None;
        let mut mode = None;
        let mut open_modes = None;
        let has = self.selected.is_some();
        let (can_undo, can_redo) = (!self.undo.is_empty(), !self.redo.is_empty());
        egui::TopBottomPanel::top("bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Menus rather than a row of buttons: the row grew past the
                // width of a laptop screen, and a command that has fallen off
                // the end of a toolbar may as well not exist.
                ui.menu_button("File", |ui| self.file_menu(ui));
                ui.menu_button("Edit", |ui| {
                    cmd = cmd.or(edit_menu(ui, has, can_undo, can_redo));
                });
                ui.menu_button("Add", |ui| cmd = cmd.or(add_menu(ui)));
                ui.menu_button("Arrange", |ui| cmd = cmd.or(arrange_menu(ui)));
                ui.separator();
                ui.checkbox(&mut self.snapping, "Snap").on_hover_text(
                    "Pull a dragged edge onto other widgets' edges and centres. \
                     Hold Ctrl while dragging to place freely. Shift-click, \
                     Ctrl-click or drag a band over the background to select \
                     more than one.",
                );
                ui.add_enabled_ui(self.snapping, |ui| {
                    ui.label("grid");
                    ui.add(
                        egui::DragValue::new(&mut self.grid)
                            .speed(1.0)
                            .range(0..=256),
                    )
                    .on_hover_text("Spacing in scene pixels; 0 turns it off");
                });
                ui.separator();
                let mut aa = self.preview.antialias;
                if ui
                    .checkbox(&mut aa, "Antialias")
                    .on_hover_text(
                        "Blend the edges of needles, rings, curves, bars and cells \
                         across the whole scene. One widget can differ: see the \
                         antialias row in Properties.",
                    )
                    .changed()
                {
                    self.set_scene_field("antialias", &aa.to_string());
                }
                if ui
                    .button("Fit")
                    .on_hover_text("Show the whole scene")
                    .clicked()
                {
                    self.camera.fit();
                }
                if ui
                    .button("1:1")
                    .on_hover_text("One screen point per scene pixel")
                    .clicked()
                {
                    self.camera.actual_size();
                }
                let (sw, sh) = self.preview.scene_size();
                ui.weak(format!("{sw}x{sh}"));
                if let Some(rig) = &self.rig {
                    let d = rig.display();
                    ui.weak(format!("{} · 0x{:02X}", d.name, d.node))
                        .on_hover_text("The display being edited; click another to switch");
                    ui.separator();
                    // One click swaps every display's scene: the modes are a
                    // property of the rig, not of the display in front.
                    for (i, m) in rig.rig.modes.iter().enumerate() {
                        if ui.selectable_label(i == rig.mode, m).clicked() {
                            mode = Some(i);
                        }
                    }
                    let showing = rig.mode;
                    if ui
                        .button("+")
                        .on_hover_text("Add, rename or remove a mode")
                        .clicked()
                    {
                        open_modes = Some(showing);
                    }
                }
                ui.separator();
                ui.checkbox(&mut self.driver.enabled, "Dummy values");
                ui.add_enabled_ui(self.driver.enabled, |ui| {
                    ui.checkbox(&mut self.driver.sweep, "Sweep");
                });
                ui.separator();
                if self.dirty {
                    ui.colored_label(egui::Color32::YELLOW, "modified");
                }
                if let Some(e) = &self.preview.error {
                    ui.colored_label(egui::Color32::RED, e);
                }
            });
        });
        if let Some(c) = cmd {
            self.run(c);
        }
        if let Some(m) = mode {
            self.set_mode(m);
        }
        if let Some(showing) = open_modes {
            self.modes = Some(crate::modes_ui::ModesDialog::new(showing));
        }
    }

    /// The File menu: the things that touch the disk, and the whole-document
    /// operations that are one step from it.
    fn file_menu(&mut self, ui: &mut egui::Ui) {
        if item(
            ui,
            true,
            "New…",
            "Ctrl+N",
            "Start a scene: size, colour and settings",
        ) {
            self.new_doc = Some(crate::newdoc::NewDoc::default());
        }
        if item(ui, true, "Open…", "", "")
            && let Some(p) = rfd::FileDialog::new()
                .add_filter("scene", &["scene", "json"])
                .pick_file()
        {
            self.close_rig();
            self.path = Some(p);
            self.open_current();
        }
        if item(
            ui,
            true,
            "Open rig…",
            "",
            "Every display on the bus, side by side, with a tab per mode",
        ) && let Some(p) = rfd::FileDialog::new()
            .add_filter("rig", &["rig"])
            .pick_file()
        {
            self.open_rig(p);
        }
        // A submenu rather than four more rows: the examples are a shelf to
        // browse, and the disk commands under them are what the File menu is
        // for. Each one is a whole document, so it sits with New and Open
        // rather than anywhere else.
        let mut example = None;
        ui.menu_button("Examples", |ui| {
            for (label, hint, ex) in crate::EXAMPLES {
                if item(ui, true, label, "", hint) {
                    example = Some((*label, ex));
                }
            }
        });
        if let Some((label, ex)) = example {
            self.open_example(label, ex);
        }
        ui.separator();
        if item(ui, true, "Save", "Ctrl+S", "") {
            self.save();
        }
        if item(ui, true, "Save as…", "", "") {
            self.save_as();
        }
        if item(
            ui,
            self.rig.is_some(),
            "Save all",
            "",
            "Every display of the rig that has changes",
        ) {
            self.save_all();
        }
        ui.separator();
        if item(
            ui,
            true,
            "Reformat",
            "",
            "Rewrite the text through the crate's own writer. Drops comments.",
        ) {
            self.tidy();
        }
        if item(ui, true, "Scene resolution…", "", "") {
            self.resize_to = Some(self.preview.scene_size());
        }
    }
}

/// One menu entry, which closes the menu when picked.
///
/// The shortcut is shown beside the label rather than documented elsewhere,
/// so a person who has found the command with the mouse is told how not to
/// need the mouse next time.
fn item(ui: &mut egui::Ui, enabled: bool, label: &str, shortcut: &str, hint: &str) -> bool {
    let mut button = egui::Button::new(label);
    if !shortcut.is_empty() {
        button = button.shortcut_text(shortcut);
    }
    let mut r = ui.add_enabled(enabled, button);
    if !hint.is_empty() {
        r = r.on_hover_text(hint);
    }
    if r.clicked() {
        ui.close_menu();
        return true;
    }
    false
}

/// The Edit menu.
pub(crate) fn edit_menu(
    ui: &mut egui::Ui,
    has_selection: bool,
    can_undo: bool,
    can_redo: bool,
) -> Option<Command> {
    let mut out = None;
    if item(ui, can_undo, "Undo", "Ctrl+Z", "") {
        out = Some(Command::Undo);
    }
    if item(ui, can_redo, "Redo", "Ctrl+Y", "") {
        out = Some(Command::Redo);
    }
    ui.separator();
    out.or(selection_items(ui, has_selection))
}

/// The commands that act on the selection, shared by the Edit menu and both
/// right-click menus.
fn selection_items(ui: &mut egui::Ui, has: bool) -> Option<Command> {
    let mut out = None;
    let mut group = |ui: &mut egui::Ui, items: &[(&str, &str, &str, Command)]| {
        for &(label, shortcut, hint, cmd) in items {
            if item(ui, has, label, shortcut, hint) {
                out = Some(cmd);
            }
        }
    };
    group(
        ui,
        &[
            ("Properties", "", "", Command::Properties),
            ("Duplicate", "Ctrl+D", "", Command::Duplicate),
            ("Delete", "Del", "", Command::Delete),
        ],
    );
    ui.separator();
    group(
        ui,
        &[
            (
                "Raise",
                "",
                "Draw it on top of the widget after it",
                Command::Raise,
            ),
            (
                "Lower",
                "",
                "Draw it underneath the widget before it",
                Command::Lower,
            ),
            (
                "Move inwards",
                "",
                "Make it a child of the widget above it",
                Command::Inwards,
            ),
            (
                "Move outwards",
                "",
                "Make it a sibling of its parent",
                Command::Outwards,
            ),
        ],
    );
    ui.separator();
    group(
        ui,
        &[
            ("Select parent", "", "", Command::SelectParent),
            ("Deselect", "Esc", "", Command::Deselect),
        ],
    );
    if item(
        ui,
        true,
        "Select all",
        "Ctrl+A",
        "Every sibling of the selection, or every top-level widget",
    ) {
        out = Some(Command::SelectAll);
    }
    out
}

/// The Add menu.
pub(crate) fn add_menu(ui: &mut egui::Ui) -> Option<Command> {
    ui.label(
        egui::RichText::new("Inside the selection, or the root")
            .small()
            .weak(),
    );
    let mut out = None;
    for kind in KINDS {
        if item(ui, true, kind, "", "") {
            out = Some(Command::Add(kind));
        }
    }
    out
}

/// The Arrange menu.
pub(crate) fn arrange_menu(ui: &mut egui::Ui) -> Option<Command> {
    ui.label(egui::RichText::new("Relative to the parent").small().weak());
    let mut out = None;
    let mut group = |ui: &mut egui::Ui, items: &[(&str, place::Placement)]| {
        for &(label, how) in items {
            if item(ui, true, label, "", "") {
                out = Some(Command::Arrange(how));
            }
        }
    };
    group(
        ui,
        &[
            ("Centre", place::Placement::CenterX),
            ("Middle", place::Placement::CenterY),
        ],
    );
    ui.separator();
    group(
        ui,
        &[
            ("Left edge", place::Placement::Left),
            ("Right edge", place::Placement::Right),
            ("Top edge", place::Placement::Top),
            ("Bottom edge", place::Placement::Bottom),
        ],
    );
    ui.separator();
    group(
        ui,
        &[
            ("Fill width", place::Placement::FillX),
            ("Fill height", place::Placement::FillY),
        ],
    );
    out
}

/// The right-click menu, on the preview or on a row of the outliner.
///
/// The selection's commands, with Add and Arrange folded in as submenus, so
/// that nothing a person does to one widget needs the menu bar. `selected`
/// is what the menu is about, named at the top so a right-click that landed
/// on the wrong thing is caught before the command is.
pub(crate) fn context_menu(ui: &mut egui::Ui, selected: Option<&str>) -> Option<Command> {
    match selected {
        Some(what) => {
            ui.label(egui::RichText::new(what).strong());
        }
        None => {
            ui.weak("nothing selected");
        }
    }
    ui.separator();
    let mut out = selection_items(ui, selected.is_some());
    ui.separator();
    ui.menu_button("Add", |ui| out = out.or(add_menu(ui)));
    ui.menu_button("Arrange", |ui| out = out.or(arrange_menu(ui)));
    // A pick inside a submenu closes the submenu; the menu it hangs from is
    // closed here so the whole thing goes away in one click.
    if out.is_some() {
        ui.close_menu();
    }
    out
}
