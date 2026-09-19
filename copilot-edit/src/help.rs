// SPDX-License-Identifier: GPL-3.0-only
//! A built-in reference makes essential workflows discoverable without external docs.

use crate::App;

impl App {
    pub(crate) fn help_window(&mut self, ctx: &egui::Context) {
        if !self.help_open {
            return;
        }
        let mut open = true;
        egui::Window::new("Editor guide")
            .open(&mut open)
            .default_width(560.0)
            .default_height(520.0)
            .vscroll(true)
            .show(ctx, |ui| {
                ui.heading("Create a scene");
                ui.label("Use File > New scene to choose the display size, or File > Examples to explore a working dashboard.");
                ui.label("Add widget creates an item inside the selected widget. Clear the selection first to add directly to the scene.");
                ui.separator();
                ui.heading("Select and edit");
                ui.label("Click a widget on the canvas or in the widget tree. Drag to move it; use the edge handles to resize.");
                ui.label("Shift-click or Ctrl-click adds to the selection. Drag a box on empty canvas to select several widgets.");
                ui.label("Widget properties edits appearance and layout. Scene source edits the same document as text; comments survive normal property edits.");
                ui.label("Hold Ctrl during a drag to bypass snapping. Fit scene restores the whole view; 100% shows scene pixels at their natural size.");
                ui.separator();
                ui.heading("Preview safely");
                ui.label("Preview values and drive playback do not change the document. Send simulated readings is a separate, explicit choice that sends values to a connected controller.");
                ui.label("Upload to display sends the document, not the preview values. The connection and upload windows show their own progress.");
                ui.label("Closing Connections leaves an established connection running. Use Disconnect to end it.");
                ui.separator();
                ui.heading("Engine simulation");
                ui.label("Enable Use preview values and Play drive. Playback options selects Generic engine, a sample drive or your own driving log.");
                ui.label("Choose road driving, idle, acceleration or overrun. Cold engine on restart begins at ambient temperature; Restart drive resets the engine and playback together.");
                ui.label("Fill preview values takes a static snapshot for every bound gauge and GPIO source. All generated readings lists all simulated, recorded and nominal demo channels.");
                ui.label("AFR varies and lags behind its target. Other engine readings are related rather than independent waves. This is a dashboard demonstration, not an engine calibration tool.");
                ui.label("Recorded log values win and condition the missing simulated channels. Pausing preview freezes the drive; seeking restarts generated transients. Indicator switches remain manual.");
                ui.separator();
                ui.heading("Rigs and modes");
                ui.label("A rig opens several displays side by side. Click a display to edit it. Mode tabs switch every display together.");
                ui.label("Save scene writes the active scene; Save all scenes includes changed scenes on other displays. Rig mode and GPIO changes are saved separately by their dialogs.");
                ui.label("Export scene copy keeps the rig using its original file. Upload defaults to the current scene and offers display and mode names; choose Rig definition separately when needed.");
                ui.label("GPIO lights show preview state; hover for details or click to configure a pin. Use as > Value threshold selects Output and can command a pin from a gauge, such as RPM above 5000. Enable Use preview values to try it. Real hardware needs firmware supporting these mappings.");
                ui.separator();
                ui.heading("Keyboard shortcuts");
                egui::Grid::new("editor_shortcuts").striped(true).num_columns(2).show(ui, |ui| {
                    for (keys, action) in [
                        ("Ctrl+N", "New scene"),
                        ("Ctrl+O", "Open scene or rig"),
                        ("Ctrl+S", "Save active scene"),
                        ("Ctrl+Shift+S", "Save scene as / export a rig scene copy"),
                        ("Ctrl+Z", "Undo one edit or drag"),
                        ("Ctrl+Y / Ctrl+Shift+Z", "Redo"),
                        ("Ctrl+D", "Duplicate selected widgets"),
                        ("Delete", "Delete selected widgets"),
                        ("Arrow keys", "Move selection by 1 pixel"),
                        ("Shift+Arrow", "Move selection by 10 pixels"),
                        ("Ctrl+A", "Select siblings / top-level widgets"),
                        ("Escape", "Clear selection or cancel a document prompt"),
                        ("F1", "Open this guide"),
                    ] {
                        ui.monospace(keys);
                        ui.label(action);
                        ui.end_row();
                    }
                });
                ui.small("Text fields keep their own typing, cursor and selection keys. Document actions pause while a confirmation dialog is open.");
                ui.separator();
                ui.heading("Recover from a mistake");
                ui.label("Undo restores widget edits, drags and source formatting. New, Open and Examples ask before discarding unsaved scenes.");
                ui.label("If the source has errors, the canvas may show the last valid preview. Repair Scene source before changing widgets or uploading.");
                ui.label("Use View > Interface size if text or controls are too small. It does not change scene resolution.");
                if ui.button("Close guide").clicked() { self.help_open = false; }
            });
        self.help_open &= open;
    }
}
