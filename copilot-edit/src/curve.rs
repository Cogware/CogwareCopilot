// SPDX-License-Identifier: GPL-3.0-only
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

#[cfg(test)]
mod interaction_tests;
mod pointer;
mod toolbar;

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
    camera_before: crate::view::Camera,
    history_start: usize,
    redo_steps: usize,
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
            camera_before: crate::view::Camera::default(),
            history_start: 0,
            redo_steps: 0,
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
        if self.preview.error.is_some() {
            self.status = "Correct the scene source before editing its curve.".into();
            return;
        }
        self.curve = Some(CurveEdit::new(node, shape, field));
        if let Some(curve) = &mut self.curve {
            curve.camera_before = self.camera;
            curve.history_start = self.undo.len();
        }
        self.gesture = None;
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
        self.status =
            "Editing curve — changes apply immediately. Done returns to the layout.".into();
    }

    /// Keep curve handles inside the available panel, including on a rig's later displays.
    pub(crate) fn frame_curve(
        &mut self,
        extent: (u32, u32),
        offset: (i32, i32),
        available: (f32, f32),
    ) {
        let Some(rect) = self
            .curve
            .as_ref()
            .and_then(|c| self.preview.tree.as_ref()?.absolute_rect(c.node))
        else {
            return;
        };
        let rect = copilot::Rect::new(
            rect.left().saturating_add(offset.0),
            rect.top().saturating_add(offset.1),
            rect.size.w,
            rect.size.h,
        );
        self.camera.focus(rect, extent);
        let fit = ((available.0 - 30.0).max(1.0) / rect.size.w.max(1) as f32)
            .min((available.1 - 30.0).max(1.0) / rect.size.h.max(1) as f32);
        let before = self.camera.zoom.unwrap_or(1.0);
        let scale = before
            .min(fit)
            .clamp(crate::view::MIN_ZOOM, crate::view::MAX_ZOOM);
        self.camera.zoom = Some(scale);
        self.camera.pan.0 *= scale / before;
        self.camera.pan.1 *= scale / before;
    }

    /// Stop shaping, leaving the curve as the text now has it.
    pub(crate) fn finish_curve(&mut self) {
        if let Some(curve) = self.curve.take() {
            self.camera = curve.camera_before;
        }
        self.gesture = None;
        self.status = "Curve updated. Save the scene to keep your changes.".into();
    }

    /// Remove the handle last clicked.
    fn delete_curve_point(&mut self) {
        if self.preview.error.is_some() {
            return;
        }
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
            c.redo_steps = 0;
        }
        self.reserve_curve_history();
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
            // Distance to the segment, not its midpoint: a short nearby
            // leg must not steal a click near the end of a long one.
            let dist = |a: (f32, f32), b: (f32, f32)| {
                let delta = (b.0 - a.0, b.1 - a.1);
                let length_sq = delta.0 * delta.0 + delta.1 * delta.1;
                let t = if length_sq > 0.0 {
                    (((new.0 - a.0) * delta.0 + (new.1 - a.1) * delta.1) / length_sq)
                        .clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (a.0 + delta.0 * t - new.0).powi(2) + (a.1 + delta.1 * t - new.1).powi(2)
            };
            (1..pts.len())
                .min_by(|&i, &j| dist(pts[i - 1], pts[i]).total_cmp(&dist(pts[j - 1], pts[j])))
                .unwrap_or(pts.len())
        }
    }
}

#[cfg(test)]
mod tests;
