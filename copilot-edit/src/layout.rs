// SPDX-License-Identifier: GPL-3.0-only
//! Where the displays of a rig sit beside each other, and how the ones not
//! being edited are drawn.
//!
//! One camera frames the whole row, so zooming and panning move every
//! display together and a change to the cluster can be checked against the
//! gauges beside it without re-fitting. The row is a coordinate space of its
//! own -- scene pixels, with each display at an offset -- and the active
//! display's [`View`] is the row's view shifted to where it sits.

use copilot::scene::Shape;

use crate::App;
use crate::drive::Driver;
use crate::view::View;

/// Scene pixels between neighbouring displays.
///
/// Wide enough that a round gauge's bezel ring does not touch the cluster,
/// narrow enough that three displays still fit a laptop screen when fitted.
pub(crate) const GAP: u32 = 80;

/// The outline around the display being edited when there is more than one.
const ACTIVE: egui::Color32 = egui::Color32::from_rgba_premultiplied(80, 200, 255, 160);

/// The ring drawn where a round panel's bezel would be.
const BEZEL: egui::Color32 = egui::Color32::from_rgb(0x3d, 0x46, 0x50);

/// A row of displays in one coordinate space.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    /// The whole row's extent, in scene pixels.
    pub size: (u32, u32),
    /// Where each display's top-left sits in the row, in rig order.
    pub offsets: Vec<(i32, i32)>,
}

/// Lay `sizes` out left to right with [`GAP`] between, each centred on the
/// row's height.
pub(crate) fn row(sizes: &[(u32, u32)]) -> Row {
    let height = sizes.iter().map(|s| s.1).max().unwrap_or(1).max(1);
    let mut offsets = Vec::with_capacity(sizes.len());
    let mut x: u32 = 0;
    for (i, &(w, h)) in sizes.iter().enumerate() {
        if i > 0 {
            x += GAP;
        }
        offsets.push((x as i32, ((height - h.min(height)) / 2) as i32));
        x += w;
    }
    Row {
        size: (x.max(1), height),
        offsets,
    }
}

impl View {
    /// This view, for something drawn at `offset` within what it frames.
    #[must_use]
    pub fn shifted(self, offset: (i32, i32)) -> View {
        View {
            origin: (
                self.origin.0 + offset.0 as f32 * self.scale,
                self.origin.1 + offset.1 as f32 * self.scale,
            ),
            scale: self.scale,
        }
    }
}

/// Paint the corners a round panel hides behind its bezel, and the bezel.
///
/// A stroke of the panel colour, wide enough to reach the corners, clipped to
/// the display's own rectangle so it cannot spill onto a neighbour. The
/// compositor still draws the whole rectangle underneath -- on the real panel
/// it is the glass that clips -- so this is the preview telling the truth
/// about what will be seen rather than about what is drawn.
pub(crate) fn mask_round(ui: &egui::Ui, rect: egui::Rect) {
    let painter = ui.painter().with_clip_rect(rect);
    let r = rect.width().min(rect.height()) / 2.0;
    // From the circle out to past the corner: the corner is r√2 away, and a
    // stroke straddles its radius, so a width of r centred at 1.5r covers
    // r..2r.
    painter.circle_stroke(
        rect.center(),
        r * 1.5,
        egui::Stroke::new(r, ui.visuals().panel_fill),
    );
    painter.circle_stroke(rect.center(), r - 1.0, egui::Stroke::new(2.0_f32, BEZEL));
}

impl App {
    /// The sizes of every display in rig order, or just the scene's own.
    pub(crate) fn display_sizes(&self) -> Vec<(u32, u32)> {
        let mine = self.preview.scene_size();
        let Some(rig) = &self.rig else {
            return vec![mine];
        };
        rig.docs
            .iter()
            .enumerate()
            .map(|(i, d)| {
                if i == rig.active {
                    mine
                } else {
                    d.preview.scene_size()
                }
            })
            .collect()
    }

    /// Draw the displays not being edited, each in its place in `row`, and
    /// return the one that was clicked, if any.
    ///
    /// The dummy values go onto each tree just before it is drawn and come
    /// off after, exactly as they do for the active display, so one RPM
    /// slider moves every tachometer in the rig.
    pub(crate) fn draw_others(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        view: &View,
        row: &Row,
        now_us: u64,
        delta_us: u64,
    ) -> Option<usize> {
        let rig = self.rig.as_mut()?;
        let driver = &self.driver;
        let mut clicked = None;
        for (i, doc) in rig.docs.iter_mut().enumerate() {
            if i == rig.active {
                continue;
            }
            let Some(&offset) = row.offsets.get(i) else {
                continue;
            };
            let size = doc.preview.scene_size();
            let here = view.shifted(offset);
            let (sw, sh) = here.shown(size);
            let placed = egui::Rect::from_min_size(
                egui::pos2(here.origin.0, here.origin.1),
                egui::vec2(sw, sh),
            );

            let driven = match doc.preview.tree.as_mut() {
                Some(t) => driver.apply(t, &doc.preview.bindings),
                None => Vec::new(),
            };
            if let Some((w, h, rgba)) = doc.preview.frame(now_us, delta_us, None, 0) {
                let img = if here.scale < 1.0 {
                    let (tw, th) = ((sw.round() as usize).max(1), (sh.round() as usize).max(1));
                    let small = crate::shrink::shrink(rgba, w, h, tw, th);
                    egui::ColorImage::from_rgba_unmultiplied([tw, th], &small)
                } else {
                    egui::ColorImage::from_rgba_unmultiplied([w, h], rgba)
                };
                let filter = if here.scale < 1.0 {
                    egui::TextureOptions::LINEAR
                } else {
                    egui::TextureOptions::NEAREST
                };
                match &mut doc.texture {
                    Some(t) => t.set(img, filter),
                    None => {
                        doc.texture = Some(ctx.load_texture(format!("display-{i}"), img, filter))
                    }
                }
            }
            if let Some(t) = doc.preview.tree.as_mut() {
                Driver::restore(t, driven);
            }

            if let Some(texture) = doc.texture.as_ref().map(|t| t.id()) {
                let resp = ui.put(
                    placed,
                    egui::Image::new((texture, egui::vec2(sw, sh))).sense(egui::Sense::click()),
                );
                if doc.preview.shape == Shape::Round {
                    mask_round(ui, placed);
                }
                let name = &rig.rig.displays[i].name;
                let node = rig.rig.displays[i].node;
                let resp =
                    resp.on_hover_text(format!("{name} · node 0x{node:02X}. Click to edit."));
                if resp.clicked() {
                    clicked = Some(i);
                }
            }
            // A display that did not load says so where it would have been.
            if let Some(e) = &doc.preview.error {
                ui.painter().text(
                    placed.center(),
                    egui::Align2::CENTER_CENTER,
                    e,
                    egui::FontId::proportional(13.0),
                    egui::Color32::RED,
                );
            }
        }
        clicked
    }

    /// Outline the display being edited, when there is more than one.
    pub(crate) fn outline_active(&self, ui: &egui::Ui, placed: egui::Rect) {
        if self.rig.as_ref().is_some_and(|r| r.docs.len() > 1) {
            ui.painter()
                .rect_stroke(placed.expand(2.0), 0.0, egui::Stroke::new(1.5_f32, ACTIVE));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_places_displays_left_to_right_with_a_gap_and_centres_them() {
        let r = row(&[(2400, 900), (480, 480), (480, 480)]);
        assert_eq!(r.size, (2400 + GAP + 480 + GAP + 480, 900));
        assert_eq!(r.offsets[0], (0, 0));
        assert_eq!(r.offsets[1], ((2400 + GAP) as i32, 210));
        assert_eq!(r.offsets[2], ((2400 + GAP + 480 + GAP) as i32, 210));
    }

    #[test]
    fn a_row_of_one_is_the_scene_itself() {
        let r = row(&[(320, 160)]);
        assert_eq!(r.size, (320, 160));
        assert_eq!(r.offsets, vec![(0, 0)]);
        assert_eq!(row(&[]).size, (1, 1), "nothing to show is still a size");
    }

    #[test]
    fn a_shifted_view_maps_the_display_origin_to_its_offset() {
        let v = View {
            origin: (10.0, 20.0),
            scale: 0.5,
        };
        let s = v.shifted((2480, 210));
        assert_eq!(
            s.to_scene((10.0 + 1240.0, 20.0 + 105.0)),
            copilot::Point { x: 0, y: 0 }
        );
        assert_eq!(s.scale, v.scale);
    }
}
