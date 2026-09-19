// SPDX-License-Identifier: GPL-3.0-only
//! Canvas selection guides are editor overlays, never saved scene content.

use super::{HOVER, SELECT};
use crate::{App, select};
use copilot::widget::NodeId;

impl App {
    /// The grid, drawn only while it is switched on and only where the lines
    /// are far enough apart to read. A grid rendered at two points a line is
    /// a grey wash over the scene, which hides the thing it is meant to help
    /// place.
    pub(super) fn draw_grid(
        &self,
        ui: &egui::Ui,
        placed: egui::Rect,
        view: &crate::view::View,
        scene: (u32, u32),
    ) {
        if self.grid <= 1 {
            return;
        }
        let spacing = self.grid as f32 * view.scale;
        if spacing < 6.0 {
            return;
        }
        let paint = ui.painter_at(placed);
        let faint = egui::Stroke::new(
            1.0_f32,
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18),
        );
        let mut gx = 0;
        while gx <= scene.0 as i32 {
            let (x, _) = view.to_screen(copilot::Point { x: gx, y: 0 });
            paint.line_segment(
                [egui::pos2(x, placed.min.y), egui::pos2(x, placed.max.y)],
                faint,
            );
            gx += self.grid;
        }
        let mut gy = 0;
        while gy <= scene.1 as i32 {
            let (_, y) = view.to_screen(copilot::Point { x: 0, y: gy });
            paint.line_segment(
                [egui::pos2(placed.min.x, y), egui::pos2(placed.max.x, y)],
                faint,
            );
            gy += self.grid;
        }
    }

    /// The hover outline, the selection outlines, the rubber band and the
    /// snap guides.
    ///
    /// Drawn as an egui overlay rather than into the scene: they are facts
    /// about the edit in progress, not about the picture, and a scene saved
    /// mid-drag must not contain them. The selection is outlined here as
    /// well as in the frame because the frame's one-pixel line is drawn in
    /// scene pixels, and at any scale below 1:1 most of it is thrown away.
    pub(super) fn draw_overlays(
        &self,
        ui: &egui::Ui,
        placed: egui::Rect,
        view: &crate::view::View,
    ) {
        let Some(tree) = self.preview.tree.as_ref() else {
            return;
        };
        let paint = ui.painter_at(placed);
        let screen = |r: copilot::Rect| {
            let (x0, y0) = view.to_screen(copilot::Point {
                x: r.left(),
                y: r.top(),
            });
            let (x1, y1) = view.to_screen(copilot::Point {
                x: r.right(),
                y: r.bottom(),
            });
            egui::Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1))
        };
        let outline = |id: NodeId, color: egui::Color32, width: f32| {
            if let Some(abs) = tree.absolute_rect(id) {
                paint.rect_stroke(screen(abs), 0.0, egui::Stroke::new(width, color));
            }
        };
        for id in &self.extra {
            outline(*id, SELECT, 1.0);
        }
        if let Some(id) = self.selected {
            outline(id, SELECT, 1.5);
        }
        if let Some(id) = self.hover
            && !self.is_selected(id)
        {
            outline(id, HOVER, 1.0);
        }
        if let Some((a, b)) = self.marquee {
            // What the band has reached so far is outlined as it grows, so
            // the hand knows what letting go will select before it does.
            for id in select::enclosed(tree, select::span(a, b)) {
                outline(id, HOVER, 1.0);
            }
            let band = screen(select::span(a, b));
            paint.rect_filled(band, 0.0, HOVER.gamma_multiply(0.15));
            paint.rect_stroke(band, 0.0, egui::Stroke::new(1.0_f32, HOVER));
        }

        // Absolute coordinates, since the snap worked in the parent's space.
        if (self.guides.x.is_some() || self.guides.y.is_some())
            && let Some(id) = self.selected
            && let Some(node) = tree.get(id)
            && let Some(origin) = node.parent.and_then(|p| tree.absolute_rect(p))
        {
            let stroke = egui::Stroke::new(1.0_f32, SELECT);
            if let Some(gx) = self.guides.x {
                let (x, _) = view.to_screen(copilot::Point {
                    x: origin.left().saturating_add(gx),
                    y: 0,
                });
                paint.line_segment(
                    [egui::pos2(x, placed.min.y), egui::pos2(x, placed.max.y)],
                    stroke,
                );
            }
            if let Some(gy) = self.guides.y {
                let (_, y) = view.to_screen(copilot::Point {
                    x: 0,
                    y: origin.top().saturating_add(gy),
                });
                paint.line_segment(
                    [egui::pos2(placed.min.x, y), egui::pos2(placed.max.x, y)],
                    stroke,
                );
            }
        }
    }
}
