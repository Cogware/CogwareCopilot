// SPDX-License-Identifier: MIT OR Apache-2.0
//! The preview, and everything a pointer can do to it.
//!
//! The whole direct-manipulation story: hovering, selecting, dragging,
//! resizing, snapping, the guides that explain a snap, the rubber band, the
//! right-click menu, and the zoom and pan that make a full-size cluster
//! inspectable. Apart from the rest of the chrome because this is the only
//! panel where a click means something about the scene rather than about the
//! editor.

use crate::drive::Driver;
use crate::handle::{Handle, hit_handle, resize};
use crate::menu;
use crate::snap::{Guides, snap_delta, snap_to_grid};
use crate::{App, SNAP_PX, rect_text, select};

/// The outline around the widget under the pointer, and the rubber band.
const HOVER: egui::Color32 = egui::Color32::from_rgba_premultiplied(80, 200, 255, 200);

/// The outline around the selection, and the guides.
///
/// Magenta for the same reason the missing-asset placeholder is: no scene
/// ever chooses it, so it cannot be mistaken for content.
const SELECT: egui::Color32 = egui::Color32::from_rgb(255, 0, 255);

const PRIMARY: egui::PointerButton = egui::PointerButton::Primary;

impl App {
    /// The preview, and everything a pointer can do to it.
    pub(crate) fn canvas(&mut self, ctx: &egui::Context, now_us: u64, delta_us: u64) {
        let mut cmd = None;
        let mut switch = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            // The view is settled before the frame is drawn: the handles are
            // sized in scene pixels but aimed at in screen points, so the
            // scale has to be known before anything is rasterised.
            let area = ui.available_rect_before_wrap();
            let scene = self.preview.scene_size();
            let avail_min = (area.min.x, area.min.y);
            let avail_size = (area.width(), area.height());
            // The camera frames the whole row of displays; the active one's
            // view is that, shifted to where it sits in the row. With no rig
            // the row is the scene and the shift is nothing.
            let row = crate::layout::row(&self.display_sizes());
            let extent = row.size;
            let view_all = self.camera.view(extent, avail_min, avail_size);
            let at = self.active_display().unwrap_or(0);
            let view = view_all.shifted(row.offsets.get(at).copied().unwrap_or((0, 0)));

            // The letterbox around the image. Registered before the image so
            // the image wins where they overlap. A click on the bare panel
            // means "nothing", which is the only way to select nothing with
            // the mouse, and a drag from it is a rubber band.
            let backdrop = ui.interact(
                area,
                ui.id().with("backdrop"),
                egui::Sense::click_and_drag(),
            );

            // The dummy values go onto the tree just before it is drawn and
            // come off straight after, so the tree the inspector reads holds
            // what the file says. Left on, the inspector showed a swept
            // gauge at wherever the sweep had got to, and the file's own
            // value was nowhere on screen.
            let driven = match self.preview.tree.as_mut() {
                Some(t) => self.driver.apply(t, &self.preview.bindings),
                None => Vec::new(),
            };
            if let Some((w, h, rgba)) =
                self.preview
                    .frame(now_us, delta_us, self.selected, view.grab())
            {
                // Shown smaller than life, the frame is shrunk here with
                // every pixel counted, and the GPU smooths the fraction
                // left. Shown at life size or larger it goes up as it is,
                // nearest-sampled, so a zoomed pixel stays a crisp square.
                let (img, filter) = if view.scale < 1.0 {
                    let (sw, sh) = view.shown(scene);
                    let (tw, th) = ((sw.round() as usize).max(1), (sh.round() as usize).max(1));
                    let small = crate::shrink::shrink(rgba, w, h, tw, th);
                    (
                        egui::ColorImage::from_rgba_unmultiplied([tw, th], &small),
                        egui::TextureOptions::LINEAR,
                    )
                } else {
                    (
                        egui::ColorImage::from_rgba_unmultiplied([w, h], rgba),
                        egui::TextureOptions::NEAREST,
                    )
                };
                match &mut self.texture {
                    Some(t) => t.set(img, filter),
                    None => self.texture = Some(ctx.load_texture("preview", img, filter)),
                }
            }
            if let Some(t) = self.preview.tree.as_mut() {
                Driver::restore(t, driven);
            }
            // The displays not being edited, drawn in their places. A click
            // on one makes it the one being edited, after this frame: the
            // swap replaces the tree everything below is about to read.
            switch = self.draw_others(ui, ctx, &view_all, &row, now_us, delta_us);
            let Some(texture) = self.texture.as_ref().map(|t| t.id()) else {
                return;
            };

            let (sw, sh) = view.shown(scene);
            let placed = egui::Rect::from_min_size(
                egui::pos2(view.origin.0, view.origin.1),
                egui::vec2(sw, sh),
            );
            // `put` rather than a centred layout: it returns a response whose
            // rect is the image, where a justified layout reports the space
            // the widget was allotted and leaves every click offset by the
            // letterboxing.
            let resp = ui.put(
                placed,
                egui::Image::new((texture, egui::vec2(sw, sh)))
                    .sense(egui::Sense::click_and_drag()),
            );
            if self.preview.shape == copilot::scene::Shape::Round {
                crate::layout::mask_round(ui, placed);
            }
            self.outline_active(ui, placed);
            // Shaping a curve takes the canvas: its handles sit on the very
            // pixels a selection drag would use, and one gesture cannot mean
            // both. Everything below -- picking, dragging, the marquee, the
            // context menu -- is held until Done.
            if self.curve.is_some() {
                self.curve_overlay(ui, &view);
                return;
            }
            let to_scene = |p: egui::Pos2| view.to_scene((p.x, p.y));
            // Shift or Ctrl adds to the selection rather than replacing it;
            // both, because each is what some other editor taught someone.
            let shift = ctx.input(|i| i.modifiers.shift || i.modifiers.command);

            // The wheel zooms about the pointer, and the middle button drags.
            // Both are what every other canvas does, which is the only reason
            // worth having for a gesture nobody is told about.
            if resp.hovered()
                && let Some(pos) = ctx.pointer_latest_pos()
            {
                let wheel = ctx.input(|i| i.smooth_scroll_delta.y);
                if wheel.abs() > 0.01 {
                    // Exponential, so a notch is the same proportional step
                    // whether the scene pixel is tiny or enormous.
                    let factor = (wheel * 0.004).exp();
                    self.camera
                        .zoom_at((pos.x, pos.y), factor, extent, avail_min, avail_size);
                }
            }
            if resp.dragged_by(egui::PointerButton::Middle) {
                let d = resp.drag_delta();
                self.camera.pan_by((d.x, d.y), view.scale);
            }

            // What is under the pointer, for the outline and the cursor. Only
            // while nothing is being dragged: mid-drag the outline would
            // flicker between the widget being moved and whatever it passes
            // over.
            if resp.hovered()
                && self.drag.is_none()
                && self.marquee.is_none()
                && let Some(pos) = ctx.pointer_latest_pos()
            {
                let p = to_scene(pos);
                self.hover = self.pick(p);
                // A resize cursor over a handle says what a press will do
                // before it is made.
                if let Some(h) = self.handle_at(p, view.grab()) {
                    ctx.set_cursor_icon(cursor_for(h));
                }
            }

            // A press begins a drag. The handles are tested where the button
            // went *down*, not where the pointer is once egui has decided
            // this is a drag -- which is six points on. A handle's whole
            // target is six points wide, so testing it there turned most
            // corner grabs into body drags.
            if (resp.drag_started_by(PRIMARY) || backdrop.drag_started_by(PRIMARY))
                && let Some(press) = ctx.input(|i| i.pointer.press_origin())
            {
                self.press(to_scene(press), view.grab(), shift);
            }
            let pos = ctx.input(|i| i.pointer.interact_pos());
            if (resp.dragged_by(PRIMARY) || backdrop.dragged_by(PRIMARY))
                && let Some(pos) = pos
            {
                let now = to_scene(pos);
                if self.drag.is_some() {
                    self.drag_to(now, view.scale, ctx.input(|i| i.modifiers.command));
                } else if let Some((from, _)) = self.marquee {
                    self.marquee = Some((from, now));
                }
            }
            if resp.drag_stopped_by(PRIMARY) || backdrop.drag_stopped_by(PRIMARY) {
                if let Some((a, b)) = self.marquee.take() {
                    self.marquee_select(a, b, shift);
                }
                self.drag = None;
                self.group.clear();
                self.guides = Guides::default();
                // The next edit is a new undo step.
                self.gesture = None;
            }

            self.draw_grid(ui, placed, &view, scene);
            self.draw_overlays(ui, placed, &view);

            // A click selects what is under it, and a click on nothing
            // clears the selection. With Shift it adds to it or takes from
            // it instead. Right-click does the same and then opens the menu,
            // so the menu is always about what was clicked.
            if resp.clicked()
                && let Some(pos) = resp.interact_pointer_pos()
            {
                let hit = self.pick(to_scene(pos));
                match (shift, hit) {
                    (true, Some(id)) => self.toggle(id),
                    (true, None) => {}
                    (false, hit) => self.select(hit),
                }
            }
            if resp.secondary_clicked()
                && let Some(pos) = resp.interact_pointer_pos()
            {
                let hit = self.pick(to_scene(pos));
                if !hit.is_some_and(|id| self.is_selected(id)) {
                    self.select(hit);
                }
            }
            if (backdrop.clicked() || backdrop.secondary_clicked()) && !shift {
                self.select(None);
            }
            let what = self.selection_label();
            resp.context_menu(|ui| cmd = menu::context_menu(ui, what.as_deref()));
            backdrop.context_menu(|ui| cmd = cmd.or(menu::context_menu(ui, None)));
        });
        if let Some(c) = cmd {
            self.run(c);
        }
        if let Some(i) = switch {
            self.activate(i);
        }
    }

    /// The handle of the primary selection under `p`, if any.
    fn handle_at(&self, p: copilot::Point, grab: i32) -> Option<Handle> {
        let id = self.selected?;
        let screen = self.preview.tree.as_ref()?.absolute_rect(id)?;
        hit_handle(screen, p, grab)
    }

    /// What a press at `p` takes hold of: a handle of the primary, a widget
    /// to move, or nothing, in which case a rubber band starts.
    ///
    /// A press on something not selected selects it and drags it in one
    /// motion, as every other editor does. Having to click first was a second
    /// gesture whose only effect was to make the first feel broken. A press
    /// on something already selected drags the whole selection with it.
    fn press(&mut self, p: copilot::Point, grab: i32, shift: bool) {
        let rect_of = |app: &App, id: NodeId| app.preview.tree.as_ref()?.get(id).map(|n| n.rect);
        if let Some(h) = self.handle_at(p, grab).filter(|h| *h != Handle::Body)
            && let Some(id) = self.selected
            && let Some(start) = rect_of(self, id)
        {
            self.group = vec![(id, start)];
            self.drag = Some((p, start, h));
            return;
        }
        let Some(id) = self.pick(p) else {
            self.marquee = Some((p, p));
            return;
        };
        if shift {
            self.toggle(id);
        } else if !self.is_selected(id) {
            self.select(Some(id));
        }
        if !self.is_selected(id) {
            return;
        }
        self.make_primary(id);
        self.group = self
            .members()
            .into_iter()
            .filter_map(|m| Some((m, rect_of(self, m)?)))
            .collect();
        if let Some(start) = rect_of(self, id) {
            self.drag = Some((p, start, Handle::Body));
        }
    }

    /// Move the drag to `now`, snapping the primary and carrying the rest.
    fn drag_to(&mut self, now: copilot::Point, scale: f32, free: bool) {
        let Some((from, start, grabbed)) = self.drag else {
            return;
        };
        let Some(id) = self.selected else { return };
        let (mut dx, mut dy) = (now.x.saturating_sub(from.x), now.y.saturating_sub(from.y));
        // Ctrl held down turns snapping off for this drag alone. The checkbox
        // is for a session, and the moment a snap is wrong is not the moment
        // to go and find it.
        let snapping = self.snapping && !free;
        // A tolerance in screen points, not scene pixels: how close is "close
        // enough" is a question about the hand holding the mouse, and the
        // answer must not change when the preview is scaled.
        let tol = if snapping {
            ((SNAP_PX / scale).round() as i32).max(1)
        } else {
            0
        };
        let targets = self.snap_targets(id);
        let guides;
        (dx, dy, guides) = snap_delta(start, grabbed, dx, dy, &targets, tol);
        // Second, and only where a neighbour has not already claimed the axis.
        if snapping {
            (dx, dy) = snap_to_grid(start, grabbed, dx, dy, self.grid, guides);
        }
        self.guides = guides;
        // One undo step for the whole drag, however many frames and widgets
        // it touches. Written only where it differs from what the tree holds,
        // so a drag that returns to where it began still lands there.
        let g = Some(Gesture {
            id: egui::Id::new("canvas drag"),
            ends: false,
        });
        for (member, began) in self.group.clone() {
            let out = resize(began, grabbed, dx, dy);
            let held = self
                .preview
                .tree
                .as_ref()
                .and_then(|t| t.get(member))
                .map(|n| n.rect);
            if held != Some(out) {
                self.set_field_of(member, g, "rect", &rect_text(out));
            }
        }
    }

    /// What the right-click menu is about.
    fn selection_label(&self) -> Option<String> {
        match self.members().len() {
            0 => None,
            1 => self.selected.map(|id| self.describe(id)),
            n => Some(format!("{n} selected")),
        }
    }

    /// The grid, drawn only while it is switched on and only where the lines
    /// are far enough apart to read. A grid rendered at two points a line is
    /// a grey wash over the scene, which hides the thing it is meant to help
    /// place.
    fn draw_grid(
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
    fn draw_overlays(&self, ui: &egui::Ui, placed: egui::Rect, view: &crate::view::View) {
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

use copilot::widget::NodeId;

/// The cursor that says what dragging `h` will do.
fn cursor_for(h: Handle) -> egui::CursorIcon {
    match h {
        Handle::Body => egui::CursorIcon::Grab,
        Handle::TopLeft | Handle::BottomRight => egui::CursorIcon::ResizeNwSe,
        Handle::TopRight | Handle::BottomLeft => egui::CursorIcon::ResizeNeSw,
        Handle::Left | Handle::Right => egui::CursorIcon::ResizeHorizontal,
        Handle::Top | Handle::Bottom => egui::CursorIcon::ResizeVertical,
    }
}

/// A continuing interaction that edits are grouped under for undo.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Gesture {
    /// What is being interacted with.
    pub id: egui::Id,
    /// Whether this edit is the gesture's last.
    pub ends: bool,
}

/// The gesture a widget's response is part of, if any.
///
/// A drag or a focused text field is a gesture in progress; a drag just
/// released or a field just left is its end; a click is neither, and each
/// one is its own undo step.
pub(crate) fn gesture(r: &egui::Response) -> Option<Gesture> {
    let going = r.dragged() || r.has_focus();
    let ending = r.drag_stopped() || r.lost_focus();
    (going || ending).then_some(Gesture {
        id: r.id,
        ends: !going,
    })
}

/// Whether a response represents a person changing something.
///
/// `Response::changed` alone is not that. A widget bound to a value something
/// else is moving -- an animated bar, a swept gauge -- reports a change every
/// frame as it re-reads and rounds it, and the inspector would dutifully write
/// the animation's current position back into the scene file. Selecting an
/// animated widget was enough to mark the document modified, and a save after
/// that would bake the frame it happened to be on into the source.
///
/// So a commit needs evidence of a hand: a drag in progress or just finished,
/// a click, or the keyboard focus a typed-in value has.
pub(crate) fn committed(r: &egui::Response) -> bool {
    r.changed()
        && (r.dragged() || r.drag_stopped() || r.clicked() || r.has_focus() || r.lost_focus())
}
