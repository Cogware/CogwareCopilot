// SPDX-License-Identifier: GPL-3.0-only
//! Point gestures use screen-sized targets and one history step per drag.

use super::{ACTIVE, Edit, GRAB, HANDLE, Shape, insert_at, to_scene};
use crate::{App, canvas::Gesture};

impl App {
    /// Draw the handles over the widget and let the pointer move them.
    ///
    /// Called from the canvas with the view the preview was drawn in, after
    /// the image is placed, so the handles land exactly on the pixels they
    /// belong to however the preview is zoomed.
    pub(crate) fn curve_overlay(&mut self, ui: &mut egui::Ui, view: &crate::view::View) {
        if self.document_blocked() || !ui.is_enabled() || self.preview.error.is_some() {
            if self
                .curve
                .as_mut()
                .is_some_and(|curve| curve.drag.take().is_some())
            {
                // A dialog can interrupt a held pointer; the next point
                // drag must not continue the interrupted undo gesture.
                self.gesture = None;
            }
            return;
        }
        let Some(c) = self.curve.as_ref() else { return };
        let (node, shape, field) = (c.node, c.shape, c.field);
        let Some(rect) = self
            .preview
            .tree
            .as_ref()
            .and_then(|t| t.absolute_rect(node))
        else {
            return;
        };
        let pts = self.curve_handles();

        // Where the widget's box lands on screen, and the two conversions
        // between a handle's fraction of it and a point in the panel.
        let (x0, y0) = view.to_screen(copilot::Point {
            x: rect.left(),
            y: rect.top(),
        });
        let (w, h) = (
            rect.size.w as f32 * view.scale,
            rect.size.h as f32 * view.scale,
        );
        let box_ = egui::Rect::from_min_size(egui::pos2(x0, y0), egui::vec2(w, h));
        let to_screen = |p: (f32, f32)| egui::pos2(x0 + p.0 * w, y0 + p.1 * h);
        let to_frac = |p: egui::Pos2| {
            (
                ((p.x - x0) / w.max(1.0)).clamp(0.0, 1.0),
                ((p.y - y0) / h.max(1.0)).clamp(0.0, 1.0),
            )
        };

        // Everything but the widget goes behind a veil: the mode is exclusive
        // and it should look it.
        let painter = ui.painter();
        let veil = egui::Color32::from_black_alpha(150);
        let full = ui.clip_rect();
        for r in [
            egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, box_.min.y)),
            egui::Rect::from_min_max(egui::pos2(full.min.x, box_.max.y), full.max),
            egui::Rect::from_min_max(
                egui::pos2(full.min.x, box_.min.y),
                egui::pos2(box_.min.x, box_.max.y),
            ),
            egui::Rect::from_min_max(
                egui::pos2(box_.max.x, box_.min.y),
                egui::pos2(full.max.x, box_.max.y),
            ),
        ] {
            painter.rect_filled(r, 0.0, veil);
        }
        painter.rect_stroke(box_, 0.0, egui::Stroke::new(1.0_f32, ACTIVE));

        // The curve itself, so a row of handles reads as the shape it is.
        if pts.len() > 1 {
            let line: Vec<egui::Pos2> = pts.iter().map(|p| to_screen(*p)).collect();
            painter.add(egui::Shape::line(
                line,
                egui::Stroke::new(1.0_f32, HANDLE.gamma_multiply(0.6)),
            ));
        }

        let resp = ui.interact(
            box_.expand(GRAB * 2.0),
            ui.id().with(("curve", node.0)),
            egui::Sense::click_and_drag(),
        );
        let pointer = resp.interact_pointer_pos().or_else(|| resp.hover_pos());
        let nearest = |p: egui::Pos2| {
            pts.iter()
                .enumerate()
                .map(|(i, q)| (i, to_screen(*q).distance(p)))
                .filter(|(_, d)| *d <= GRAB * 2.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        };
        let near = pointer.and_then(nearest);
        if resp.hovered() {
            ui.ctx().set_cursor_icon(if near.is_some() {
                match shape {
                    Shape::Points => egui::CursorIcon::Grab,
                    Shape::Envelope { vertical: true } => egui::CursorIcon::ResizeHorizontal,
                    _ => egui::CursorIcon::ResizeVertical,
                }
            } else {
                egui::CursorIcon::Crosshair
            });
        }

        for (i, p) in pts.iter().enumerate() {
            let at = to_screen(*p);
            let hot = near == Some(i) || self.curve.as_ref().is_some_and(|c| c.picked == Some(i));
            painter.circle_filled(at, GRAB, if hot { ACTIVE } else { HANDLE });
        }

        // --- the gestures ---
        let mut edit: Option<Edit> = None;
        let primary = egui::PointerButton::Primary;
        if resp.drag_started_by(primary)
            && let Some(c) = self.curve.as_mut()
        {
            // The pointer may already be past the hit target on the first
            // drag frame; the press is the handle the person took hold of.
            c.drag = ui.input(|i| i.pointer.press_origin()).and_then(nearest);
            c.picked = c.drag;
        }

        let dragging = self.curve.as_ref().and_then(|c| c.drag);
        if let (Some(i), Some(p)) = (dragging, resp.interact_pointer_pos())
            && i < pts.len()
            && (resp.dragged_by(primary) || resp.drag_stopped_by(primary))
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            let mut next = pts.clone();
            let f = to_frac(p);
            // Along the curve is the author's only where the curve is a shape.
            next[i] = if shape.free_x() {
                f
            } else if matches!(shape, Shape::Envelope { vertical: true }) {
                (f.0, pts[i].1)
            } else {
                (pts[i].0, f.1)
            };
            edit = Some((
                next,
                Some(Gesture {
                    id: resp.id,
                    ends: resp.drag_stopped_by(primary),
                }),
            ));
        } else if resp.clicked() && near.is_none() {
            // A click on nothing adds a point where it landed, in the place
            // along the curve that keeps the row in order.
            if let Some(p) = resp.interact_pointer_pos().filter(|p| box_.contains(*p)) {
                let f = to_frac(p);
                let mut next = pts.clone();
                let at = insert_at(&pts, f, shape);
                next.insert(at, f);
                if let Some(c) = self.curve.as_mut() {
                    c.picked = Some(at);
                }
                edit = Some((next, None));
            }
        } else if resp.clicked()
            && let Some(c) = self.curve.as_mut()
        {
            c.picked = near;
        }

        if ui.ctx().memory(|m| m.focused().is_none())
            && ui.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
            })
        {
            self.delete_curve_point();
            return;
        }
        if let Some((next, g)) = edit
            && next != pts
        {
            if let Some(c) = self.curve.as_mut() {
                c.redo_steps = 0;
            }
            self.reserve_curve_history();
            self.set_field_of(node, g, field, &to_scene(&next, shape));
        }
        if resp.drag_stopped_by(primary) {
            if let Some(c) = self.curve.as_mut() {
                c.drag = None;
            }
            self.gesture = None;
        }
    }
}
