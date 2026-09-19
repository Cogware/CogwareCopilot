// SPDX-License-Identifier: GPL-3.0-only
//! Offer rig names alongside protocol addresses instead of making users memorise them.

#[derive(Default)]
pub(super) struct Choices {
    pub displays: Vec<(String, u8)>,
    pub modes: Vec<String>,
}

pub(super) fn valid(node: u8, slot: u8, rig: bool) -> bool {
    (1..=0x0f).contains(&node) && (rig || slot <= 7)
}

pub(super) fn show(ui: &mut egui::Ui, node: &mut u8, slot: &mut u8, rig: bool, choices: &Choices) {
    if !choices.displays.is_empty() {
        let selected = choices
            .displays
            .iter()
            .find(|(_, address)| *address == *node)
            .map(|(name, address)| format!("{name} (0x{address:02X})"))
            .unwrap_or_else(|| format!("Display 0x{node:02X}"));
        ui.horizontal(|ui| {
            ui.label("Target display");
            egui::ComboBox::from_id_salt("upload_display")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for (name, address) in &choices.displays {
                        ui.selectable_value(node, *address, format!("{name} (0x{address:02X})"));
                    }
                });
        });
    } else {
        ui.horizontal(|ui| {
            ui.label("Display address");
            ui.add(
                egui::DragValue::new(node)
                    .range(0..=254)
                    .hexadecimal(2, false, true),
            )
            .on_hover_text(
                "Use the target display's CAN address, from 01 to 0F. The controller is 00.",
            );
        });
        ui.small("Check this address against the display you intend to update.");
    }
    if !rig {
        if choices.modes.is_empty() {
            ui.horizontal(|ui| {
                ui.label("Mode slot");
                ui.add(egui::DragValue::new(slot).range(0..=7))
                    .on_hover_text("0 is the first mode, 1 the second, and so on.");
            });
        } else {
            let selected = choices
                .modes
                .get(usize::from(*slot))
                .cloned()
                .unwrap_or_else(|| format!("Slot {slot}"));
            ui.horizontal(|ui| {
                ui.label("Target mode");
                egui::ComboBox::from_id_salt("upload_mode")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for (index, name) in choices.modes.iter().enumerate() {
                            if let Ok(index) = u8::try_from(index) {
                                ui.selectable_value(slot, index, format!("{name} (slot {index})"));
                            }
                        }
                    });
            });
        }
        ui.small("This replaces the scene in the selected display and mode.");
    } else {
        ui.small("This sends the rig definition only. Scene files and assets are not included.");
    }
    if !valid(*node, *slot, rig) {
        ui.colored_label(
            super::ui::WARN,
            "Uploads require a display address from 01 to 0F and a scene mode slot from 0 to 7.",
        );
    }
}

impl crate::App {
    pub(crate) fn open_upload(&mut self) {
        let (node, slot, displays, modes) = match &self.rig {
            Some(rig) => (
                Some(rig.display().node),
                u8::try_from(rig.mode).ok(),
                rig.rig
                    .displays
                    .iter()
                    .map(|display| (display.name.clone(), display.node))
                    .collect(),
                rig.rig.modes.clone(),
            ),
            None => (self.preview.node, None, Vec::new(), Vec::new()),
        };
        self.uploader
            .configure_destination(node, slot, displays, modes);
        self.uploader.open(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gateway_or_out_of_range_mode_is_not_an_upload_destination() {
        assert!(!valid(0, 0, false));
        assert!(!valid(16, 0, false));
        assert!(!valid(1, 8, false));
        assert!(valid(15, 7, false));
        assert!(valid(1, 8, true));
    }
}
