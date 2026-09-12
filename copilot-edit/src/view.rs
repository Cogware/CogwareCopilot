// SPDX-License-Identifier: GPL-3.0-only
//! Where the preview sits on screen, and how the two coordinate systems line
//! up.
//!
//! Separate from [`crate::handle`] because the two answer different questions.
//! A handle knows which part of a widget a scene coordinate falls on; a view
//! knows what scene coordinate a screen position *is*. Mixing them is how a
//! click ends up measured in the wrong units.

use copilot::Point;

/// Half the width of a handle's clickable square, in screen points.
///
/// Screen rather than scene pixels: a 2400-pixel cluster scaled to fit a
/// window shrinks a scene-pixel target to under two physical pixels, which is
/// not a thing anyone can hit with a mouse. Aiming is done in the units the
/// hand works in.
pub const GRAB_PX: f32 = 6.0;

/// How the preview is framed: fitted to the panel, or zoomed and dragged.
///
/// Held apart from [`View`] because a view is derived afresh every frame from
/// whatever space the panel happens to have, while this is what the person
/// looking at it asked for and has to survive a window resize.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Scene pixels to screen points, or `None` to fit whatever room there is.
    pub zoom: Option<f32>,
    /// How far the image has been dragged from centred, in screen points.
    pub pan: (f32, f32),
}

/// The closest a scene pixel may shrink to, and the furthest it may grow.
///
/// The floor stops a click mapping every pixel of a huge scene to the same
/// place; the ceiling stops a stray scroll turning one pixel into the whole
/// panel with no obvious way back.
pub const MIN_ZOOM: f32 = 0.02;
/// The largest a scene pixel may be drawn, in screen points.
pub const MAX_ZOOM: f32 = 64.0;
/// How large, in screen points, a widget is drawn when a mode focuses it.
///
/// Big enough that a handle is a target and small enough that the box does
/// not run off the panel on a laptop screen.
const FOCUS_SIZE: f32 = 420.0;

impl Default for Camera {
    fn default() -> Self {
        Camera {
            zoom: None,
            pan: (0.0, 0.0),
        }
    }
}

impl Camera {
    /// The view this camera gives, in the space it has been handed.
    #[must_use]
    pub fn view(self, scene: (u32, u32), avail_min: (f32, f32), avail_size: (f32, f32)) -> View {
        let fitted = View::fit(scene, avail_min, avail_size);
        let Some(zoom) = self.zoom else {
            return fitted;
        };
        let scale = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let (sw, sh) = (scene.0.max(1) as f32, scene.1.max(1) as f32);
        View {
            origin: (
                avail_min.0 + (avail_size.0 - sw * scale) / 2.0 + self.pan.0,
                avail_min.1 + (avail_size.1 - sh * scale) / 2.0 + self.pan.1,
            ),
            scale,
        }
    }

    /// Zoom by `factor`, keeping whatever is under `cursor` under it.
    ///
    /// Anchoring on the pointer rather than the centre is what makes a wheel
    /// usable for looking at a detail: zooming about the middle pushes the
    /// thing being examined off the edge, and the pan needed to bring it back
    /// is a second gesture nobody should have to think about.
    pub fn zoom_at(
        &mut self,
        cursor: (f32, f32),
        factor: f32,
        scene: (u32, u32),
        avail_min: (f32, f32),
        avail_size: (f32, f32),
    ) {
        let before = self.view(scene, avail_min, avail_size);
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let scale = (before.scale * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        // Where in the scene the pointer is now, in fractional pixels: the
        // integer `to_scene` would quantise the anchor and let it creep across
        // a run of small steps.
        let px = (cursor.0 - before.origin.0) / before.scale;
        let py = (cursor.1 - before.origin.1) / before.scale;

        let (sw, sh) = (scene.0.max(1) as f32, scene.1.max(1) as f32);
        let base_x = avail_min.0 + (avail_size.0 - sw * scale) / 2.0;
        let base_y = avail_min.1 + (avail_size.1 - sh * scale) / 2.0;
        self.zoom = Some(scale);
        self.pan = (
            cursor.0 - px * scale - base_x,
            cursor.1 - py * scale - base_y,
        );
    }

    /// Drag the image by `delta` screen points.
    ///
    /// Panning pins the zoom at whatever it was showing. A drag that left the
    /// camera fitting would spring back to centred on the next window resize,
    /// having apparently ignored the gesture.
    pub fn pan_by(&mut self, delta: (f32, f32), current: f32) {
        if self.zoom.is_none() {
            self.zoom = Some(current);
        }
        self.pan = (self.pan.0 + delta.0, self.pan.1 + delta.1);
    }

    /// Go back to filling the panel.
    pub fn fit(&mut self) {
        *self = Camera::default();
    }

    /// Show the scene at one screen point per scene pixel, centred.
    /// Frame `rect` -- one widget's box in scene pixels -- rather than the
    /// whole scene.
    ///
    /// For a mode about to work on one widget, whose handles have to be big
    /// enough to aim at. `view` centres the *scene* and then applies `pan` in
    /// screen points, so centring a widget is the offset of its middle from
    /// the scene's, scaled, and negated.
    pub fn focus(&mut self, rect: copilot::Rect, scene: (u32, u32)) {
        let (w, h) = (rect.size.w.max(1) as f32, rect.size.h.max(1) as f32);
        // Big enough to drag a handle on without filling the panel edge to
        // edge, whatever size the widget is in the scene.
        let scale = (FOCUS_SIZE / w.max(h)).clamp(MIN_ZOOM, MAX_ZOOM);
        let (sw, sh) = (scene.0.max(1) as f32, scene.1.max(1) as f32);
        let cx = rect.left() as f32 + w / 2.0;
        let cy = rect.top() as f32 + h / 2.0;
        self.zoom = Some(scale);
        self.pan = (-(cx - sw / 2.0) * scale, -(cy - sh / 2.0) * scale);
    }

    pub fn actual_size(&mut self) {
        self.zoom = Some(1.0);
        self.pan = (0.0, 0.0);
    }
}

/// Where the preview image sits on screen, and how the two coordinate systems
/// line up.
///
/// Computed here rather than read back from the layout: a justified egui
/// layout reports the space the widget was allotted, not the letterboxed
/// rectangle the image was actually drawn in, and every click would land at
/// an offset that varies with the window's shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Top-left of the drawn image, in screen points.
    pub origin: (f32, f32),
    /// Screen points per scene pixel.
    pub scale: f32,
}

impl View {
    /// Fit a `scene`-sized image inside the box at `avail_min` of `avail_size`,
    /// centred on both axes.
    #[must_use]
    pub fn fit(scene: (u32, u32), avail_min: (f32, f32), avail_size: (f32, f32)) -> View {
        let (sw, sh) = (scene.0.max(1) as f32, scene.1.max(1) as f32);
        // A floor keeps a colossal scene from collapsing to a scale of zero,
        // which would make every click map to the same scene pixel.
        let scale = (avail_size.0 / sw).min(avail_size.1 / sh).max(0.01);
        let shown = (sw * scale, sh * scale);
        View {
            origin: (
                avail_min.0 + (avail_size.0 - shown.0) / 2.0,
                avail_min.1 + (avail_size.1 - shown.1) / 2.0,
            ),
            scale,
        }
    }

    /// The size the image occupies on screen, in points.
    #[must_use]
    pub fn shown(&self, scene: (u32, u32)) -> (f32, f32) {
        (
            scene.0.max(1) as f32 * self.scale,
            scene.1.max(1) as f32 * self.scale,
        )
    }

    /// A screen position as a scene pixel.
    #[must_use]
    pub fn to_scene(self, pos: (f32, f32)) -> Point {
        Point {
            // Floor rather than truncate: `as i32` rounds towards zero, so a
            // click just left of the image would map to scene pixel 0 instead
            // of a negative, and read as a hit on the far edge.
            x: ((pos.0 - self.origin.0) / self.scale).floor() as i32,
            y: ((pos.1 - self.origin.1) / self.scale).floor() as i32,
        }
    }

    /// A scene pixel as a screen position.
    #[must_use]
    pub fn to_screen(self, p: Point) -> (f32, f32) {
        (
            self.origin.0 + p.x as f32 * self.scale,
            self.origin.1 + p.y as f32 * self.scale,
        )
    }

    /// The handle radius in scene pixels that yields [`GRAB_PX`] on screen.
    #[must_use]
    pub fn grab(&self) -> i32 {
        // At least one, or a preview zoomed past the point where a scene pixel
        // fills the handle would leave nothing to aim at.
        ((GRAB_PX / self.scale).ceil() as i32).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // The reachability test spans both halves on purpose: a grab radius is
    // only correct in terms of what it lets you actually hit.
    use crate::handle::{Handle, hit_handle};
    use copilot::Rect;

    fn at(x: i32, y: i32) -> Point {
        Point { x, y }
    }

    // --- the view mapping ---

    /// A 200x100 scene in a 400x400 box: fits on width, letterboxed vertically.
    fn wide() -> View {
        View::fit((200, 100), (0.0, 0.0), (400.0, 400.0))
    }

    #[test]
    fn a_view_fits_on_the_tighter_axis() {
        assert!((wide().scale - 2.0).abs() < 1e-6, "{}", wide().scale);
    }

    #[test]
    fn a_view_centres_what_it_letterboxes() {
        let v = wide();
        // 100 scene tall at 2x is 200 on screen, leaving 100 above and below.
        assert!((v.origin.0 - 0.0).abs() < 1e-6, "x {}", v.origin.0);
        assert!((v.origin.1 - 100.0).abs() < 1e-6, "y {}", v.origin.1);
    }

    #[test]
    fn a_view_maps_the_corners_of_the_image_to_the_corners_of_the_scene() {
        let v = wide();
        assert_eq!(v.to_scene(v.origin), at(0, 0));
        let (w, h) = v.shown((200, 100));
        let last = v.to_scene((v.origin.0 + w - 0.5, v.origin.1 + h - 0.5));
        assert_eq!(last, at(199, 99), "the far corner is the last pixel");
    }

    #[test]
    fn a_view_round_trips_a_scene_point() {
        let v = wide();
        for p in [at(0, 0), at(1, 1), at(100, 50), at(199, 99)] {
            assert_eq!(v.to_scene(v.to_screen(p)), p, "{p:?} did not survive");
        }
    }

    #[test]
    fn a_click_above_the_image_is_a_negative_scene_row() {
        // The bug this guards: `as i32` truncates towards zero, so a click in
        // the letterbox would land on row 0 and read as a hit on the top edge.
        let v = wide();
        let p = v.to_scene((v.origin.0 + 10.0, v.origin.1 - 10.0));
        assert!(p.y < 0, "landed on row {}", p.y);
    }

    #[test]
    fn a_click_left_of_the_image_is_a_negative_scene_column() {
        let v = View::fit((100, 200), (0.0, 0.0), (400.0, 400.0));
        let p = v.to_scene((v.origin.0 - 10.0, v.origin.1 + 10.0));
        assert!(p.x < 0, "landed on column {}", p.x);
    }

    #[test]
    fn the_view_offsets_by_where_the_panel_starts() {
        // The preview does not begin at the window's origin: there is a
        // toolbar above it and an outliner beside it.
        let v = View::fit((200, 100), (300.0, 40.0), (400.0, 400.0));
        assert_eq!(v.to_scene(v.origin), at(0, 0));
        assert!(v.origin.0 >= 300.0 && v.origin.1 >= 40.0, "{:?}", v.origin);
    }

    #[test]
    fn a_grab_target_stays_the_same_size_on_screen() {
        // The whole point: a handle you can hit on an 800-pixel scene has to
        // still be hittable on a 2400-pixel one.
        for scene_w in [200u32, 800, 2400, 4096] {
            let v = View::fit((scene_w, scene_w / 2), (0.0, 0.0), (1200.0, 700.0));
            let on_screen = v.grab() as f32 * v.scale;
            assert!(
                on_screen >= GRAB_PX,
                "{scene_w} wide gave a {on_screen}pt target"
            );
        }
    }

    #[test]
    fn a_grab_radius_is_never_zero() {
        // A preview zoomed in far enough that one scene pixel exceeds the
        // handle must still leave something to aim at.
        let v = View::fit((4, 4), (0.0, 0.0), (1000.0, 1000.0));
        assert!(v.grab() >= 1);
    }

    #[test]
    fn a_view_of_an_empty_scene_does_not_divide_by_zero() {
        let v = View::fit((0, 0), (0.0, 0.0), (100.0, 100.0));
        assert!(v.scale.is_finite() && v.scale > 0.0, "{}", v.scale);
        assert!(v.grab() >= 1);
    }

    #[test]
    fn a_view_with_no_room_still_maps_finitely() {
        let v = View::fit((800, 480), (0.0, 0.0), (0.0, 0.0));
        assert!(v.scale.is_finite() && v.scale > 0.0);
        let p = v.to_scene((0.0, 0.0));
        assert!(p.x.abs() < 1_000_000 && p.y.abs() < 1_000_000, "{p:?}");
    }

    #[test]
    fn corners_are_reachable_at_the_scale_a_big_scene_gets() {
        // The reported bug, as a test: on a cluster-sized scene fitted to a
        // window, a click within a handle's worth of screen points of a corner
        // has to read as that corner and not as the body.
        let v = View::fit((2400, 900), (0.0, 0.0), (1200.0, 700.0));
        let r = Rect::new(230, 410, 1500, 300);
        let corner = v.to_screen(at(r.left(), r.top()));
        for (ox, oy) in [(0.0, 0.0), (GRAB_PX - 1.0, 0.0), (0.0, GRAB_PX - 1.0)] {
            let p = v.to_scene((corner.0 + ox, corner.1 + oy));
            assert_eq!(
                hit_handle(r, p, v.grab()),
                Some(Handle::TopLeft),
                "offset ({ox}, {oy}) missed the corner"
            );
        }
    }

    // --- the camera ---

    const SCENE: (u32, u32) = (800, 480);
    const MIN: (f32, f32) = (100.0, 40.0);
    const SIZE: (f32, f32) = (900.0, 600.0);

    #[test]
    fn a_default_camera_fits_like_the_bare_view() {
        let c = Camera::default();
        assert_eq!(c.view(SCENE, MIN, SIZE), View::fit(SCENE, MIN, SIZE));
    }

    #[test]
    fn zooming_keeps_what_is_under_the_cursor_under_it() {
        // The whole point of anchoring on the pointer. A drift of a fraction
        // of a point is rounding; a drift of pixels is the detail you were
        // looking at sliding off the panel.
        let mut c = Camera::default();
        for cursor in [(300.0, 200.0), (150.0, 60.0), (900.0, 600.0)] {
            let before = c.view(SCENE, MIN, SIZE);
            let px = (cursor.0 - before.origin.0) / before.scale;
            let py = (cursor.1 - before.origin.1) / before.scale;

            c.zoom_at(cursor, 1.3, SCENE, MIN, SIZE);

            let after = c.view(SCENE, MIN, SIZE);
            let qx = (cursor.0 - after.origin.0) / after.scale;
            let qy = (cursor.1 - after.origin.1) / after.scale;
            assert!(
                (px - qx).abs() < 0.01 && (py - qy).abs() < 0.01,
                "anchor moved from ({px:.3}, {py:.3}) to ({qx:.3}, {qy:.3})"
            );
        }
    }

    #[test]
    fn zooming_in_makes_a_scene_pixel_bigger() {
        let mut c = Camera::default();
        let before = c.view(SCENE, MIN, SIZE).scale;
        c.zoom_at((400.0, 300.0), 2.0, SCENE, MIN, SIZE);
        assert!(c.view(SCENE, MIN, SIZE).scale > before);
    }

    #[test]
    fn zoom_stops_at_its_limits() {
        let mut c = Camera::default();
        for _ in 0..200 {
            c.zoom_at((400.0, 300.0), 2.0, SCENE, MIN, SIZE);
        }
        assert!((c.view(SCENE, MIN, SIZE).scale - MAX_ZOOM).abs() < 1e-3);
        for _ in 0..400 {
            c.zoom_at((400.0, 300.0), 0.5, SCENE, MIN, SIZE);
        }
        assert!((c.view(SCENE, MIN, SIZE).scale - MIN_ZOOM).abs() < 1e-3);
    }

    #[test]
    fn a_zero_or_negative_factor_is_ignored() {
        let mut c = Camera::default();
        c.zoom_at((400.0, 300.0), 1.5, SCENE, MIN, SIZE);
        let kept = c;
        for bad in [0.0, -2.0, f32::NAN, f32::INFINITY] {
            c.zoom_at((400.0, 300.0), bad, SCENE, MIN, SIZE);
            if bad.is_finite() {
                assert_eq!(c, kept, "a factor of {bad} changed the camera");
            }
        }
        assert!(c.view(SCENE, MIN, SIZE).scale.is_finite());
    }

    #[test]
    fn panning_moves_the_image_by_exactly_what_it_was_given() {
        let mut c = Camera::default();
        let before = c.view(SCENE, MIN, SIZE);
        c.pan_by((37.0, -12.0), before.scale);
        let after = c.view(SCENE, MIN, SIZE);
        assert!((after.origin.0 - before.origin.0 - 37.0).abs() < 1e-3);
        assert!((after.origin.1 - before.origin.1 + 12.0).abs() < 1e-3);
        assert!(
            (after.scale - before.scale).abs() < 1e-6,
            "panning rescaled"
        );
    }

    #[test]
    fn panning_pins_the_zoom_it_was_showing() {
        // Otherwise the camera stays in fitting mode and springs back to
        // centred the next time the window changes shape.
        let mut c = Camera::default();
        let fitted = c.view(SCENE, MIN, SIZE).scale;
        c.pan_by((10.0, 10.0), fitted);
        assert_eq!(c.zoom, Some(fitted));
        let wider = c.view(SCENE, MIN, (SIZE.0 * 2.0, SIZE.1));
        assert!((wider.scale - fitted).abs() < 1e-6, "the zoom drifted");
    }

    #[test]
    fn fitting_undoes_a_zoom_and_a_pan() {
        let mut c = Camera::default();
        c.zoom_at((400.0, 300.0), 3.0, SCENE, MIN, SIZE);
        c.pan_by((80.0, -40.0), 3.0);
        c.fit();
        assert_eq!(c.view(SCENE, MIN, SIZE), View::fit(SCENE, MIN, SIZE));
    }

    #[test]
    fn actual_size_is_one_point_per_scene_pixel() {
        let mut c = Camera::default();
        c.pan_by((80.0, -40.0), 0.5);
        c.actual_size();
        let v = c.view(SCENE, MIN, SIZE);
        assert!((v.scale - 1.0).abs() < 1e-6);
        // Centred, so the scene's middle sits in the panel's middle.
        let mid = v.to_screen(at(400, 240));
        assert!((mid.0 - (MIN.0 + SIZE.0 / 2.0)).abs() < 1.0, "{mid:?}");
    }

    #[test]
    fn a_zoomed_view_still_round_trips_a_point() {
        let mut c = Camera::default();
        c.zoom_at((500.0, 250.0), 4.0, SCENE, MIN, SIZE);
        let v = c.view(SCENE, MIN, SIZE);
        for p in [at(0, 0), at(1, 1), at(400, 240), at(799, 479)] {
            assert_eq!(v.to_scene(v.to_screen(p)), p, "{p:?} did not survive");
        }
    }

    #[test]
    fn a_grab_target_stays_usable_when_zoomed_right_in() {
        let mut c = Camera::default();
        c.zoom_at((500.0, 250.0), 20.0, SCENE, MIN, SIZE);
        let v = c.view(SCENE, MIN, SIZE);
        // At high zoom the handle shrinks to a scene pixel; it must not reach
        // zero, or nothing on the widget is grabbable at all.
        assert!(v.grab() >= 1);
    }
}
