// SPDX-License-Identifier: GPL-3.0-only
//! The curve mode's actions and point readout, kept reachable in narrow windows.

use super::{Shape, to_scene};
use crate::App;

impl App {
    /// The mode's actions, in place of the layout toolbar until Done.
    pub(crate) fn curve_toolbar(&mut self, ctx: &egui::Context) {
        let Some(curve) = self.curve.as_ref() else {
            return;
        };
        let (shape, picked) = (curve.shape, curve.picked);
        let points = self.curve_handles();
        let editable = self.preview.error.is_none();
        let can_remove = editable && points.len() > 1 && picked.is_some_and(|i| i < points.len());
        let can_undo = self.undo.len() > curve.history_start;
        let can_redo = curve.redo_steps > 0 && !self.redo.is_empty();
        let title = match shape {
            Shape::Envelope { .. } => "Edit segment envelope",
            Shape::Series => "Edit chart values",
            Shape::Points => "Edit shape points",
        };
        let (mut done, mut add, mut remove, mut undo, mut redo) =
            (false, false, false, false, false);
        egui::TopBottomPanel::top("bar").show(ctx, |ui| {
            if self.document_blocked() {
                ui.disable();
            }
            ui.horizontal_wrapped(|ui| {
                done = ui.button("Done").on_hover_text("Return to the layout. Changes are already in the document; Save writes them to disk.").clicked();
                ui.strong(title);
                add = ui.add_enabled(editable, egui::Button::new("Add point"))
                    .on_hover_text("Insert halfway between the selected point and its neighbour, or click empty space inside the curve box.")
                    .clicked();
                remove = ui.add_enabled(can_remove, egui::Button::new("Remove point").wrap_mode(egui::TextWrapMode::Extend))
                    .on_hover_text(if points.len() <= 1 {
                        "Keep at least one point."
                    } else if picked.is_none() {
                        "Click a point to select it first."
                    } else {
                        "Remove the selected point, or press Delete."
                    }).clicked();
                undo = ui.add_enabled(can_undo, egui::Button::new("Undo"))
                    .on_hover_text("Undo the last change made in this curve session.").clicked();
                redo = ui.add_enabled(can_redo, egui::Button::new("Redo")).clicked();
            });
            ui.horizontal_wrapped(|ui| {
                let readout = picked.and_then(|i| points.get(i).map(|p| (i, p)));
                if let Some((i, &(x, y))) = readout {
                    let value = match shape {
                        Shape::Points => format!("x {:.0}% · y {:.0}%", x * 100.0, y * 100.0),
                        Shape::Envelope { vertical: true } => format!("Width {:.0}%", x * 100.0),
                        _ => format!("Height {:.0}%", (1.0 - y) * 100.0),
                    };
                    ui.label(format!("Point {} of {} · {value}", i + 1, points.len()));
                } else {
                    ui.label(format!("{} points · click one to select it", points.len()));
                }
                ui.weak("Drag a point to move it. Click empty space inside the box to add.");
            });
            if !editable {
                ui.colored_label(egui::Color32::YELLOW, "Preview out of date. Choose Done and correct the scene source.");
            }
        });
        if add {
            self.add_curve_point();
        } else if remove {
            self.delete_curve_point();
        } else if undo || redo {
            if undo {
                self.undo();
            } else {
                self.redo();
            }
            if let Some(curve) = self.curve.as_mut() {
                curve.picked = None;
                curve.drag = None;
                if undo {
                    curve.redo_steps += 1;
                } else {
                    curve.redo_steps = curve.redo_steps.saturating_sub(1);
                }
            }
        }
        if done {
            self.finish_curve();
        }
    }

    pub(super) fn add_curve_point(&mut self) {
        if self.preview.error.is_some() {
            return;
        }
        let Some(curve) = self.curve.as_ref() else {
            return;
        };
        let (node, shape, field, picked) = (curve.node, curve.shape, curve.field, curve.picked);
        let mut points = self.curve_handles();
        let (at, point) = match points.len() {
            0 => (0, (0.5, 0.5)),
            1 => {
                let x = if points[0].0 < 0.5 { 0.75 } else { 0.25 };
                (1, (x, 0.5))
            }
            n => {
                let i = picked.unwrap_or(n - 2).min(n - 2);
                (
                    i + 1,
                    (
                        (points[i].0 + points[i + 1].0) / 2.0,
                        (points[i].1 + points[i + 1].1) / 2.0,
                    ),
                )
            }
        };
        points.insert(at, point);
        if let Some(curve) = self.curve.as_mut() {
            curve.picked = Some(at);
            curve.drag = None;
            curve.redo_steps = 0;
        }
        self.reserve_curve_history();
        self.set_field_of(node, None, field, &to_scene(&points, shape));
    }

    pub(super) fn reserve_curve_history(&mut self) {
        // The editor discards its oldest snapshot at the history limit;
        // the curve-session boundary must move with that retained history.
        if self.undo.len() == 256
            && self.gesture.is_none()
            && let Some(curve) = self.curve.as_mut()
        {
            curve.history_start = curve.history_start.saturating_sub(1);
        }
    }
}
