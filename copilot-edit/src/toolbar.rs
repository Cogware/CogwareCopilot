// SPDX-License-Identifier: GPL-3.0-only
//! Keep document, view and playback controls in predictable, separate rows.

use crate::{
    App, Pane,
    menu::{self, Command},
};

impl App {
    pub(crate) fn toolbar(&mut self, ctx: &egui::Context) {
        let mut command = None;
        let blocked = self.document_blocked();
        let valid = self.preview.error.is_none();
        let has = self.selected.is_some() && valid;
        egui::TopBottomPanel::top("bar").show(ctx, |ui| {
            ui.add_enabled_ui(!blocked, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.menu_button("File", |ui| self.file_menu(ui));
                    ui.menu_button("Edit", |ui| {
                        command = command.or(menu::edit_menu(ui, has, !self.undo.is_empty(), !self.redo.is_empty()));
                    });
                    ui.add_enabled_ui(valid, |ui| {
                        ui.menu_button("Add widget", |ui| command = command.or(menu::add_menu(ui)));
                    });
                    ui.add_enabled_ui(has, |ui| {
                        ui.menu_button("Arrange", |ui| command = command.or(menu::arrange_menu(ui)))
                            .response.on_disabled_hover_text("Select a widget first.");
                    });
                    ui.menu_button("View", |ui| self.view_menu(ui));
                    ui.menu_button("Help", |ui| {
                        if menu::item(ui, true, "Editor guide and shortcuts", "F1", "How to create, edit, preview and upload a scene.") {
                            self.help_open = true;
                        }
                    });
                    ui.separator();
                    if ui.button("Open...").on_hover_text("Open a scene or rig (Ctrl+O)").clicked() {
                        self.choose_document();
                    }
                    if ui.add_enabled(valid, egui::Button::new("Save"))
                        .on_hover_text("Save the active scene (Ctrl+S)")
                        .on_disabled_hover_text("Fix scene errors before saving.").clicked()
                    {
                        self.save();
                    }
                    if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                        .on_hover_text("Undo the last edit (Ctrl+Z)").clicked()
                    {
                        command = Some(Command::Undo);
                    }
                    if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"))
                        .on_hover_text("Redo the last undone edit (Ctrl+Y)").clicked()
                    {
                        command = Some(Command::Redo);
                    }
                });
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Canvas");
                    if ui.button("Fit scene").on_hover_text("Show every display in the available space.").clicked() {
                        self.camera.fit();
                    }
                    if ui.button("100%").on_hover_text("One screen point per scene pixel.").clicked() {
                        self.camera.actual_size();
                    }
                    ui.separator();
                    ui.checkbox(&mut self.snapping, "Snap to edges")
                        .on_hover_text("Align dragged widgets to neighbouring edges and centres. Hold Ctrl to move freely.");
                    ui.add_enabled_ui(self.snapping, |ui| {
                        ui.label("Grid");
                        ui.add(egui::DragValue::new(&mut self.grid).range(0..=256).suffix(" px"))
                            .on_hover_text("Snap to a grid as well. Set 0 to turn the grid off.");
                    });
                    let mut smooth = self.preview.antialias;
                    if ui.add_enabled(valid, egui::Checkbox::new(&mut smooth, "Smooth edges"))
                        .on_hover_text("Save antialiasing for the whole scene. Individual widgets can override it.")
                        .changed()
                    {
                        self.set_scene_field("antialias", &smooth.to_string());
                    }
                    let (width, height) = self.preview.scene_size();
                    ui.weak(format!("{width} x {height} px"));
                    if self.unsaved() > 0 {
                        ui.colored_label(egui::Color32::from_rgb(255, 190, 80), "Unsaved changes");
                    }
                });
                self.rig_toolbar(ui);
            });
        });
        self.playback_toolbar(ctx);
        if let Some(command) = command {
            self.run(command);
        }
    }

    fn view_menu(&mut self, ui: &mut egui::Ui) {
        for (title, pane) in [
            ("Widget properties", Pane::Props),
            ("Scene source", Pane::Source),
            ("Preview values", Pane::Values),
        ] {
            if ui.selectable_label(self.pane == pane, title).clicked() {
                self.pane = pane;
                ui.close_menu();
            }
        }
        ui.separator();
        ui.label("Interface size");
        let mut scale = ctx_scale(ui.ctx());
        if ui
            .add(egui::Slider::new(&mut scale, 80.0..=160.0).suffix("%"))
            .changed()
        {
            ui.ctx().set_zoom_factor(scale / 100.0);
        }
        if ui.button("Reset interface size").clicked() {
            ui.ctx().set_zoom_factor(1.0);
        }
        ui.small("This changes controls and text, not your scene's resolution.");
    }

    fn rig_toolbar(&mut self, ui: &mut egui::Ui) {
        let Some(rig) = &self.rig else { return };
        let mut mode = None;
        let mut edit_modes = false;
        let mut edit_pins = false;
        let showing = rig.mode;
        let active = rig.active;
        let pin_count = rig.display().pins.count();
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.strong("Rig");
            ui.label(format!(
                "Editing {} (0x{:02X})",
                rig.display().name,
                rig.display().node
            ));
            ui.label("Mode");
            for (index, name) in rig.rig.modes.iter().enumerate() {
                if ui
                    .selectable_label(index == showing, name)
                    .on_hover_text(
                        "Switch every display to this mode. Unsaved changes are protected.",
                    )
                    .clicked()
                {
                    mode = Some(index);
                }
            }
            edit_modes = ui.button("Manage modes...").clicked();
            edit_pins = ui
                .button("GPIO pins...")
                .on_hover_text("Configure the selected display's logical pins.")
                .clicked();
        });
        if let Some(index) = mode {
            self.set_mode(index);
        }
        if edit_modes {
            self.modes = Some(crate::modes_ui::ModesDialog::new(showing));
        }
        if edit_pins {
            self.pins_dialog = Some(crate::pins_ui::PinsDialog::new(active, pin_count));
        }
    }

    fn playback_toolbar(&mut self, ctx: &egui::Context) {
        let blocked = self.document_blocked();
        egui::TopBottomPanel::top("preview_controls").show(ctx, |ui| {
            ui.add_enabled_ui(!blocked, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Preview");
                    ui.checkbox(&mut self.driver.enabled, "Use preview values")
                        .on_hover_text("Try simulated or recorded values without changing the saved scene.");
                    ui.add_enabled_ui(self.driver.enabled, |ui| {
                        if ui.checkbox(&mut self.driver.sweep, "Sweep gauges")
                            .on_hover_text("Move gauges through their ranges; stop drive playback.")
                            .changed() && self.driver.sweep
                        {
                            self.driver.driving = false;
                        }
                        if ui.checkbox(&mut self.driver.driving, "Play drive")
                            .on_hover_text("Animate the selected engine simulator or driving log; stop gauge sweeping.")
                            .changed() && self.driver.driving
                        {
                            self.driver.sweep = false;
                        }
                        ui.menu_button("Playback options", |ui| self.playback_options(ui));
                        ui.add_enabled_ui(self.driver.driving || self.driver.sweep, |ui| {
                            ui.checkbox(&mut self.driver.to_car, "Send simulated readings")
                                .on_hover_text("Explicitly send preview values over the live connection. This does not upload a scene.")
                                .on_disabled_hover_text("Start a sweep or drive playback first. A live connection is also required.");
                        });
                    });
                    if ui.button("Edit preview values").clicked() { self.pane = Pane::Values; }
                });
                if self.driver.enabled && self.driver.to_car && (self.driver.sweep || self.driver.driving) {
                    let connected = matches!(self.monitor.link_state(), crate::upload::ui::LinkState::Open { .. });
                    ui.colored_label(egui::Color32::from_rgb(255, 190, 80), if connected {
                        "Sending preview readings to the connected controller. Turn off Send simulated readings to stop."
                    } else {
                        "No live connection: preview readings are not being sent. Open Connections to connect."
                    });
                }
            });
        });
    }
}

fn ctx_scale(ctx: &egui::Context) -> f32 {
    ctx.zoom_factor() * 100.0
}
