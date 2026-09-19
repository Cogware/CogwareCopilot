// SPDX-License-Identifier: GPL-3.0-only
//! Canvas context and navigation, separate from gestures which edit the scene.

use crate::{App, Pane};

impl App {
    pub(super) fn canvas_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if let Some(rig) = &self.rig {
                let display = rig.display();
                ui.strong(format!("Editing {} · node 0x{:02X}", display.name, display.node));
            } else {
                ui.strong("Preview");
            }
            if self.preview.tree.is_some() {
                let (w, h) = self.preview.scene_size();
                ui.weak(format!("{w} × {h} px"));
            }
            ui.menu_button("Canvas help", |ui| {
                ui.set_max_width(290.0);
                ui.label("Click a widget to select it; drag it to move. Drag an edge or corner to resize.");
                ui.label("Shift-click or Ctrl-click adds or removes a selection. Drag empty space to select a group.");
                ui.separator();
                ui.label("Scroll to zoom at the pointer. Middle-drag anywhere in the preview to pan. Fit view resets both.");
                ui.label("Hold Ctrl while dragging to temporarily ignore snapping. Right-click for widget commands.");
                if self.rig.is_some() {
                    ui.label("Click another display to edit it.");
                }
            });
            if !self.preview.missing.is_empty() {
                ui.colored_label(egui::Color32::YELLOW, format!("{} missing assets", self.preview.missing.len()))
                    .on_hover_text(self.preview.missing.join("\n"));
            }
        });
        // A fixed-height readout keeps changing coordinates and long names
        // from wrapping the canvas into a different position during a drag.
        let mut selection = self
            .selection_label()
            .unwrap_or_else(|| "No widget selected".into());
        if let Some(rect) = self
            .selected
            .and_then(|id| self.preview.tree.as_ref()?.get(id).map(|n| n.rect))
        {
            if !self.extra.is_empty() {
                selection.push_str(" · primary:");
            }
            selection.push_str(&format!(
                " · x {} · y {} · {} × {} px",
                rect.left(),
                rect.top(),
                rect.size.w,
                rect.size.h
            ));
        }
        ui.add(egui::Label::new(&selection).truncate())
            .on_hover_text(format!("{selection}\nPosition is relative to the parent. Drag to move; drag an edge or corner to resize."));
        if let Some(error) = self.preview.error.clone() {
            ui.horizontal_wrapped(|ui| {
                let warning = if self.preview.tree.is_some() {
                    "Preview out of date — showing the last valid scene. Canvas editing is paused."
                } else {
                    "Preview unavailable — fix the scene source to continue."
                };
                ui.colored_label(egui::Color32::YELLOW, warning)
                    .on_hover_text(error);
                if ui.button("Show source").clicked() {
                    self.pane = Pane::Source;
                }
            });
        } else if self
            .doc_root()
            .and_then(|id| self.preview.tree.as_ref()?.get(id))
            .is_some_and(|node| {
                matches!(node.kind, copilot::widget::Kind::Panel { .. }) && node.children.is_empty()
            })
        {
            ui.weak("Empty scene — choose Add widget to begin, then drag it into place.");
        }
        ui.separator();
    }

    pub(super) fn canvas_empty(&self, ui: &egui::Ui, area: egui::Rect) {
        let text = "No preview yet\nOpen a scene or correct its source to see it here.";
        let galley = ui.painter().layout(
            text.into(),
            egui::FontId::proportional(15.0),
            ui.visuals().weak_text_color(),
            area.width().max(1.0),
        );
        ui.painter().galley(
            area.center() - galley.size() * 0.5,
            galley,
            ui.visuals().weak_text_color(),
        );
    }

    pub(super) fn canvas_navigation(
        &mut self,
        ui: &egui::Ui,
        area: egui::Rect,
        extent: (u32, u32),
        scale: f32,
    ) -> bool {
        if self.curve.is_some() || !ui.is_enabled() {
            return false;
        }
        let ctx = ui.ctx();
        let over_canvas = ctx
            .pointer_latest_pos()
            .filter(|p| area.contains(*p) && ctx.layer_id_at(*p) == Some(ui.layer_id()));
        if let Some(pos) = over_canvas {
            let wheel = ctx.input(|i| i.smooth_scroll_delta.y);
            if wheel.abs() > 0.01 {
                self.camera.zoom_at(
                    (pos.x, pos.y),
                    (wheel * 0.004).exp(),
                    extent,
                    (area.min.x, area.min.y),
                    (area.width(), area.height()),
                );
            }
        }
        // Capture only a middle press begun on this canvas, so a pan can
        // continue beyond its edge without taking a neighbouring panel's drag.
        let id = ui.id().with("canvas middle pan");
        let (pressed, down, delta) = ctx.input(|i| {
            (
                i.pointer.button_pressed(egui::PointerButton::Middle),
                i.pointer.button_down(egui::PointerButton::Middle),
                i.pointer.delta(),
            )
        });
        let mut panning = ctx.data(|d| d.get_temp::<bool>(id).unwrap_or(false));
        if pressed {
            panning = over_canvas.is_some();
        }
        panning &= down;
        ctx.data_mut(|d| d.insert_temp(id, panning));
        if panning {
            if !pressed {
                self.camera.pan_by((delta.x, delta.y), scale);
            }
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        panning
    }
}
