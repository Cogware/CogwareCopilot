// SPDX-License-Identifier: MIT OR Apache-2.0
//! Shaping a curve on the preview instead of typing it as a row of numbers.
//!
//! Two widgets carry one: a `segbar`'s `profile`, the envelope its cells are
//! cut to, and a `line` or `polygon`'s `points`. Both were text-only, and a
//! power curve typed as twenty-eight decimals is a curve nobody adjusts twice.
//!
//! # Why it takes the whole editor over
//!
//! Shaping a curve means dragging handles that sit *on top of* the widget,
//! in the same pixels a drag would otherwise use to move the widget itself.
//! Leaving the rest of the editor live would make every gesture ambiguous:
//! the same downward drag would either pull a point down or move the box. So
//! entering the mode dims everything else, disables the panels, and takes the
//! canvas for handles alone, until Done gives it back. The mode is the answer
//! to "which of these two things did that drag mean".
//!
//! # Why it writes on every drag rather than on Done
//!
//! The document is the text, and the preview is whatever the text currently
//! makes. Holding the curve aside and splicing it once at the end would mean
//! dragging a handle showed nothing until the mode closed. Each change is
//! spliced immediately -- and grouped under one gesture, so the whole drag is
//! a single Ctrl+Z.

use copilot::widget::{Kind, NodeId};

use crate::App;
use crate::canvas::Gesture;

/// The colour of a handle, and of the curve joining them.
const HANDLE: egui::Color32 = egui::Color32::from_rgb(255, 0, 255);
/// The handle under the pointer, or last touched.
const ACTIVE: egui::Color32 = egui::Color32::from_rgb(80, 200, 255);
/// How much of the screen a handle takes, in points.
const GRAB: f32 = 5.0;

/// Which curve a widget carries, and how its numbers reach the screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Shape {
    /// A row of heights, one per cell, measured across a bank from its base
    /// edge. The position along the bank is the index, not the author's to
    /// move: cell three is cell three.
    Envelope {
        /// Whether the bank's cells stack upwards rather than run rightwards.
        vertical: bool,
    },
    /// A row of heights across the box, evenly spaced: a chart's series.
    Series,
    /// Free points, each an `(x, y)` fraction of the box.
    Points,
}

impl Shape {
    /// The curve a kind carries, and the field it is written to.
    pub(crate) fn of(kind: &Kind) -> Option<(Shape, &'static str)> {
        match kind {
            Kind::SegBar { vertical, .. } => Some((
                Shape::Envelope {
                    vertical: *vertical,
                },
                "profile",
            )),
            Kind::Chart { .. } => Some((Shape::Series, "values")),
            Kind::Line { .. } | Kind::Polygon { .. } => Some((Shape::Points, "points")),
            _ => None,
        }
    }

    /// What the button offering it should say.
    pub(crate) fn verb(self) -> &'static str {
        match self {
            Shape::Envelope { .. } => "Shape the envelope…",
            Shape::Series => "Shape the series…",
            Shape::Points => "Move the points…",
        }
    }

    /// Whether a handle's position along the curve is the author's to move.
    ///
    /// An envelope's is not: its handles are cells, and a cell's place in the
    /// row is what it is. A polyline's is, because a polyline is a shape.
    fn free_x(self) -> bool {
        matches!(self, Shape::Points)
    }
}

/// A curve as the gestures below hand it back: the new handles, and the
/// gesture they belong to so a whole drag is one undo step.
type Edit = (Vec<(f32, f32)>, Option<Gesture>);

/// A curve being shaped, and the state of the gesture shaping it.
pub(crate) struct CurveEdit {
    /// The widget whose curve this is.
    pub node: NodeId,
    /// Which of its fields the numbers are written back to.
    pub field: &'static str,
    /// How the numbers reach the screen.
    pub shape: Shape,
    /// The handle being dragged, if one is.
    drag: Option<usize>,
    /// The handle a click last landed on, which Delete removes.
    picked: Option<usize>,
}

impl CurveEdit {
    /// Start shaping the curve of the widget at `node`.
    pub(crate) fn new(node: NodeId, shape: Shape, field: &'static str) -> Self {
        Self {
            node,
            field,
            shape,
            drag: None,
            picked: None,
        }
    }
}

/// The handles a kind's curve currently has, as `(x, y)` fractions of the
/// widget's box with `y` measured down from the top, the way the screen is.
///
/// One shape for every curve, so the drawing and the dragging below do not
/// each have to know which kind they are looking at.
fn handles(kind: &Kind, shape: Shape) -> Vec<(f32, f32)> {
    match (kind, shape) {
        (Kind::SegBar { profile, .. }, Shape::Envelope { vertical }) => {
            let n = profile.len();
            profile
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    // The middle of the cell it belongs to, along the bank.
                    let along = (i as f32 + 0.5) / n.max(1) as f32;
                    // Cut from the base edge: the bottom of a horizontal bank,
                    // the left of a vertical one.
                    if vertical {
                        (v.clamp(0.0, 1.0), 1.0 - along)
                    } else {
                        (along, 1.0 - v.clamp(0.0, 1.0))
                    }
                })
                .collect()
        }
        (Kind::Chart { values, .. }, Shape::Series) => {
            let n = values.len();
            values
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let x = if n > 1 {
                        i as f32 / (n - 1) as f32
                    } else {
                        0.5
                    };
                    (x, 1.0 - v.clamp(0.0, 1.0))
                })
                .collect()
        }
        (Kind::Line { points, .. } | Kind::Polygon { points, .. }, Shape::Points) => {
            points.iter().map(|(x, y)| (*x, *y)).collect()
        }
        _ => Vec::new(),
    }
}

/// The scene-file text for `pts`, written back the way the field wants it.
fn to_scene(pts: &[(f32, f32)], shape: Shape) -> String {
    let n = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        // Two places is finer than any panel resolves and still reads as a
        // number a person could have typed.
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    match shape {
        Shape::Envelope { vertical } => {
            let vals: Vec<String> = pts
                .iter()
                .map(|(x, y)| n(if vertical { *x } else { 1.0 - *y }))
                .collect();
            format!("[{}]", vals.join(", "))
        }
        Shape::Series => {
            let vals: Vec<String> = pts.iter().map(|(_, y)| n(1.0 - *y)).collect();
            format!("[{}]", vals.join(", "))
        }
        Shape::Points => {
            let vals: Vec<String> = pts
                .iter()
                .map(|(x, y)| format!("[{}, {}]", n(*x), n(*y)))
                .collect();
            format!("[{}]", vals.join(", "))
        }
    }
}

impl App {
    /// Begin shaping the selected widget's curve.
    pub(crate) fn start_curve(&mut self, node: NodeId, shape: Shape, field: &'static str) {
        self.curve = Some(CurveEdit::new(node, shape, field));
        // The handles are the point of the mode, and a handle five points
        // wide on a bank drawn a tenth of life size is not a target. Fitting
        // the widget is what "pull it forward" means in a preview that stays
        // where it is.
        if let Some(r) = self
            .preview
            .tree
            .as_ref()
            .and_then(|t| t.absolute_rect(node))
        {
            self.camera.focus(r, self.preview.scene_size());
        }
        self.status = "shaping a curve: drag a handle, click to add, Delete to remove".into();
    }

    /// Stop shaping, leaving the curve as the text now has it.
    pub(crate) fn finish_curve(&mut self) {
        self.curve = None;
        self.camera.fit();
        self.status = "done shaping".into();
    }

    /// The bar across the top while a curve is being shaped, in place of the
    /// usual one: the mode owns the editor until Done.
    pub(crate) fn curve_toolbar(&mut self, ctx: &egui::Context) {
        let Some(c) = self.curve.as_ref() else { return };
        let (what, picked) = (c.field, c.picked);
        let mut done = false;
        let mut remove = false;
        egui::TopBottomPanel::top("bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                done = ui.button("Done").clicked();
                ui.separator();
                ui.strong(format!("shaping {what}"));
                ui.separator();
                remove = ui
                    .add_enabled(picked.is_some(), egui::Button::new("Delete point"))
                    .on_hover_text("Or press Delete")
                    .clicked();
                ui.separator();
                ui.label(
                    egui::RichText::new(
                        "Drag a handle to move it. Click the curve to add a point. \
                         Everything else is held until Done.",
                    )
                    .small()
                    .weak(),
                );
            });
        });
        if remove {
            self.delete_curve_point();
        }
        if done {
            self.finish_curve();
        }
    }

    /// Remove the handle last clicked.
    fn delete_curve_point(&mut self) {
        let Some(c) = self.curve.as_ref() else { return };
        let (node, shape, field) = (c.node, c.shape, c.field);
        let Some(i) = c.picked else { return };
        let mut pts = self.curve_handles();
        // A curve needs something left to be a curve; the last point is not
        // a thing to delete into nothing.
        if pts.len() <= 1 || i >= pts.len() {
            self.status = "a curve needs at least one point".into();
            return;
        }
        pts.remove(i);
        if let Some(c) = self.curve.as_mut() {
            c.picked = None;
            c.drag = None;
        }
        self.set_field_of(node, None, field, &to_scene(&pts, shape));
    }

    /// The handles of the curve being shaped, read fresh from the tree.
    fn curve_handles(&self) -> Vec<(f32, f32)> {
        let Some(c) = self.curve.as_ref() else {
            return Vec::new();
        };
        self.preview
            .tree
            .as_ref()
            .and_then(|t| t.get(c.node))
            .map(|n| handles(&n.kind, c.shape))
            .unwrap_or_default()
    }

    /// Draw the handles over the widget and let the pointer move them.
    ///
    /// Called from the canvas with the view the preview was drawn in, after
    /// the image is placed, so the handles land exactly on the pixels they
    /// belong to however the preview is zoomed.
    pub(crate) fn curve_overlay(&mut self, ui: &mut egui::Ui, view: &crate::view::View) {
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
            box_,
            ui.id().with(("curve", node.0)),
            egui::Sense::click_and_drag(),
        );
        let pointer = resp.interact_pointer_pos().or_else(|| resp.hover_pos());
        let near = pointer.and_then(|p| {
            pts.iter()
                .enumerate()
                .map(|(i, q)| (i, to_screen(*q).distance(p)))
                .filter(|(_, d)| *d <= GRAB * 2.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        });

        for (i, p) in pts.iter().enumerate() {
            let at = to_screen(*p);
            let hot = near == Some(i) || self.curve.as_ref().is_some_and(|c| c.picked == Some(i));
            painter.circle_filled(at, GRAB, if hot { ACTIVE } else { HANDLE });
        }

        // --- the gestures ---
        let mut edit: Option<Edit> = None;
        if resp.drag_started()
            && let Some(c) = self.curve.as_mut()
        {
            c.drag = near;
            c.picked = near;
        }
        if resp.drag_stopped()
            && let Some(c) = self.curve.as_mut()
        {
            c.drag = None;
        }

        let dragging = self.curve.as_ref().and_then(|c| c.drag);
        if let (Some(i), Some(p)) = (dragging, resp.interact_pointer_pos())
            && i < pts.len()
        {
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
                    ends: false,
                }),
            ));
        } else if resp.clicked() && near.is_none() {
            // A click on nothing adds a point where it landed, in the place
            // along the curve that keeps the row in order.
            if let Some(p) = resp.interact_pointer_pos() {
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

        if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
            self.delete_curve_point();
            return;
        }
        if let Some((next, g)) = edit {
            self.set_field_of(node, g, field, &to_scene(&next, shape));
        }
    }
}

/// Where a new point belongs in the row.
///
/// An envelope and a series are read in order along the axis, so a point
/// dropped in the middle belongs in the middle; a polyline is a path, and a
/// point belongs on the leg the click was nearest to.
fn insert_at(pts: &[(f32, f32)], new: (f32, f32), shape: Shape) -> usize {
    if pts.is_empty() {
        return 0;
    }
    match shape {
        Shape::Envelope { vertical } => {
            // The handles run along the bank; for a vertical one that is up
            // the screen, which is backwards in y.
            let key = |p: &(f32, f32)| if vertical { 1.0 - p.1 } else { p.0 };
            let k = key(&new);
            pts.iter().position(|p| key(p) > k).unwrap_or(pts.len())
        }
        Shape::Series => pts.iter().position(|p| p.0 > new.0).unwrap_or(pts.len()),
        Shape::Points => {
            // The leg whose two ends the click sits nearest to, so a point
            // added to a shape lands on the edge it was aimed at.
            let dist = |a: (f32, f32), b: (f32, f32)| {
                let m = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                (m.0 - new.0).powi(2) + (m.1 - new.1).powi(2)
            };
            (1..pts.len())
                .min_by(|&i, &j| dist(pts[i - 1], pts[i]).total_cmp(&dist(pts[j - 1], pts[j])))
                .unwrap_or(pts.len())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_envelope_round_trips_through_the_screen_and_back() {
        // A horizontal bank's profile is cut from the bottom, so a value of
        // 1.0 is at the top of the box and 0.0 at the bottom.
        let kind = Kind::SegBar {
            value: 0.0,
            segments: 4,
            gap: 1,
            fill: copilot::Color::WHITE,
            track: copilot::Color::BLACK,
            warn: 2.0,
            warn_fill: copilot::Color::WHITE,
            danger: 2.0,
            danger_fill: copilot::Color::WHITE,
            vertical: false,
            profile: vec![0.25, 1.0],
            height: 1.0,
            divisions: 0,
            div_gap: 1,
        };
        let shape = Shape::Envelope { vertical: false };
        let pts = handles(&kind, shape);
        assert_eq!(pts.len(), 2);
        assert!((pts[0].1 - 0.75).abs() < 1e-6, "0.25 up is 0.75 down");
        assert!(pts[0].0 < pts[1].0, "the handles run along the bank");
        assert_eq!(to_scene(&pts, shape), "[0.25, 1]");
    }

    #[test]
    fn a_vertical_bank_measures_across_the_other_way() {
        let shape = Shape::Envelope { vertical: true };
        // Value along x, position up the screen: the first cell is at the
        // bottom, which is the largest y.
        let pts = vec![(0.5, 0.75), (1.0, 0.25)];
        assert_eq!(to_scene(&pts, shape), "[0.5, 1]");
    }

    #[test]
    fn points_are_written_as_pairs_and_a_series_as_heights() {
        assert_eq!(
            to_scene(&[(0.0, 1.0), (0.5, 0.0)], Shape::Points),
            "[[0, 1], [0.5, 0]]"
        );
        assert_eq!(
            to_scene(&[(0.0, 0.75), (1.0, 0.0)], Shape::Series),
            "[0.25, 1]"
        );
    }

    #[test]
    fn a_new_point_lands_in_order_along_the_curve() {
        let row = vec![(0.0, 0.5), (0.5, 0.5), (1.0, 0.5)];
        let e = Shape::Envelope { vertical: false };
        assert_eq!(insert_at(&row, (0.25, 0.1), e), 1);
        assert_eq!(insert_at(&row, (0.75, 0.1), e), 2);
        assert_eq!(insert_at(&row, (2.0, 0.1), e), 3, "past the end goes last");
        assert_eq!(insert_at(&[], (0.5, 0.5), e), 0);
    }

    #[test]
    fn a_new_point_on_a_shape_lands_on_the_leg_it_was_aimed_at() {
        // A triangle: a click near the middle of the second leg belongs
        // between its two ends, not at the end of the row.
        let tri = vec![(0.0, 1.0), (0.5, 0.0), (1.0, 1.0)];
        assert_eq!(insert_at(&tri, (0.75, 0.5), Shape::Points), 2);
        assert_eq!(insert_at(&tri, (0.25, 0.5), Shape::Points), 1);
    }

    #[test]
    fn every_curve_bearing_kind_offers_one_and_others_do_not() {
        let line = Kind::Line {
            points: vec![],
            width: 1,
            color: copilot::Color::WHITE,
            closed: false,
        };
        assert_eq!(Shape::of(&line).map(|(_, f)| f), Some("points"));
        assert_eq!(
            Shape::of(&Kind::Panel {
                background: copilot::Color::WHITE
            }),
            None
        );
    }
}
