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
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
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
        let mine = display_size(&self.preview);
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
                    display_size(&d.preview)
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
        let interactive = self.curve.is_none() && !self.document_blocked();
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
            let size = display_size(&doc.preview);
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
                ui.put(placed, egui::Image::new((texture, egui::vec2(sw, sh))));
                if doc.preview.shape == Shape::Round {
                    mask_round(ui, placed);
                }
            } else {
                ui.painter()
                    .rect_filled(placed, 0.0, ui.visuals().faint_bg_color);
            }
            let display = &rig.rig.displays[i];
            let action = if !interactive {
                "Choose Done before switching displays."
            } else if doc.preview.error.is_some() {
                "Click to view the scene source."
            } else {
                "Click to edit this display."
            };
            let error = doc.preview.error.as_deref().unwrap_or("");
            let resp = ui
                .interact(
                    placed,
                    ui.id().with(("rig display", i)),
                    if interactive {
                        egui::Sense::click()
                    } else {
                        egui::Sense::hover()
                    },
                )
                .on_hover_text(format!(
                    "{} · node 0x{:02X}\n{action}\n{error}",
                    display.name, display.node
                ));
            if interactive && resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if interactive && resp.clicked() {
                clicked = Some(i);
            }
            let label = if doc.preview.error.is_some() {
                format!(
                    "{} · {}",
                    display.name,
                    if doc.preview.tree.is_some() {
                        "preview out of date"
                    } else {
                        "preview unavailable"
                    }
                )
            } else {
                format!("{} · node 0x{:02X}", display.name, display.node)
            };
            let visible = placed.intersect(ui.clip_rect());
            if visible.width() > 30.0 && visible.height() > 20.0 {
                let colour = if doc.preview.error.is_some() {
                    egui::Color32::YELLOW
                } else {
                    ACTIVE
                };
                let galley = ui.painter().layout(
                    label,
                    egui::FontId::proportional(12.0),
                    colour,
                    (visible.width() - 12.0).max(1.0),
                );
                let origin = visible.min + egui::vec2(6.0, 6.0);
                let paint = ui.painter_at(visible);
                paint.rect_filled(
                    egui::Rect::from_min_size(origin, galley.size()).expand(3.0),
                    3.0,
                    egui::Color32::from_black_alpha(210),
                );
                paint.galley(origin, galley, colour);
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

fn display_size(preview: &crate::preview::Preview) -> (u32, u32) {
    if preview.tree.is_some() {
        preview.scene_size()
    } else {
        // A failed scene still needs a visible, clickable place in the rig,
        // rather than the renderer's one-pixel uninitialised surface.
        (320, 240)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rig_app(broken: bool) -> App {
        let scene = r#"{"width":80,"height":80,"root":{"type":"line","rect":[0,0,80,80],"name":"curve","points":[[0,0],[1,1]]}}"#;
        let mut a = crate::tests::app(scene, "curve");
        let mut parked = crate::preview::Preview::new();
        if broken {
            parked.error = Some("Scene file could not be read".into());
        } else {
            parked.load(scene, std::path::Path::new("."));
        }
        let doc = |preview| crate::rig::Doc {
            path: None,
            text: scene.into(),
            dirty: false,
            preview,
            undo: Vec::new(),
            redo: Vec::new(),
            texture: None,
        };
        a.rig = Some(crate::rig::RigSession {
            rig: copilot::rig::parse(
                r#"{"modes":["normal"],"displays":[
              {"name":"one","node":1,"scenes":{"normal":"one.scene"}},
              {"name":"two","node":2,"scenes":{"normal":"two.scene"}}]}"#,
            )
            .unwrap(),
            text: String::new(),
            path: "test.rig".into(),
            mode: 0,
            active: 0,
            docs: vec![doc(crate::preview::Preview::new()), doc(parked)],
        });
        a
    }

    fn others_frame(a: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> Option<usize> {
        let mut clicked = None;
        let row = row(&a.display_sizes());
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 500.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    clicked = a.draw_others(
                        ui,
                        ctx,
                        &View {
                            origin: (20.0, 100.0),
                            scale: 1.0,
                        },
                        &row,
                        0,
                        0,
                    );
                });
            },
        );
        clicked
    }

    #[test]
    fn a_display_without_a_texture_can_be_clicked_to_repair_it() {
        let mut a = rig_app(true);
        let ctx = egui::Context::default();
        assert_eq!(a.display_sizes(), vec![(80, 80), (320, 240)]);
        let _ = others_frame(&mut a, &ctx, vec![]);
        let pos = egui::pos2(200.0, 180.0);
        let mut clicked = None;
        for pressed in [true, false] {
            clicked = others_frame(
                &mut a,
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            )
            .or(clicked);
        }
        assert_eq!(clicked, Some(1));
    }

    #[test]
    fn other_displays_cannot_steal_a_curve_edit() {
        let mut a = rig_app(false);
        a.start_curve(a.selected.unwrap(), crate::curve::Shape::Points, "points");
        let ctx = egui::Context::default();
        let _ = others_frame(&mut a, &ctx, vec![]);
        let pos = egui::pos2(200.0, 140.0);
        for pressed in [true, false] {
            assert_eq!(
                others_frame(
                    &mut a,
                    &ctx,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE
                        },
                    ]
                ),
                None
            );
        }
        assert_eq!(a.active_display(), Some(0));
    }

    #[test]
    fn a_failed_display_has_a_reachable_placeholder() {
        let preview = crate::preview::Preview::new();
        assert_eq!(display_size(&preview), (320, 240));
    }

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
