// SPDX-License-Identifier: GPL-3.0-only
//! The Bands rows of the properties pane: the colours up a gauge's scale.
//!
//! Apart from the rest of the pane because bands are a *list*, and the pane
//! above is a column of one row per field. A gauge has as many bands as its
//! instrument has -- an oil pressure gauge is dangerous low and dangerous
//! again high, with caution either side of normal -- so there is no fixed set
//! of rows to lay out, and the whole list is rewritten whenever any of it
//! changes, the way [`crate::touch_ui`] rewrites a whole `touch` object.
//!
//! Rows rather than a dialog like the curve editor's: a band is two values and
//! a gauge has a handful, which fits the pane, and a colour picked next to the
//! `fill` it has to look right against is easier to judge than one picked in a
//! window over the top of the scene.

use copilot::Color;
use copilot::widget::{Kind, Zone};

use crate::canvas::{Gesture, committed, gesture};
use crate::inspect::controls;

/// What the rows asked for, and the gesture it belongs to.
///
/// The gesture travels with the edit so that dragging a band up the scale is
/// one undo step rather than one per frame, the way every other slider in the
/// pane behaves.
pub(crate) type ZoneAsk = (ZoneEdit, Option<Gesture>);

/// What the rows asked for.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum ZoneEdit {
    /// Write this `zones` array onto the widget.
    Set(String),
    /// Take the widget's bands off.
    Unset,
}

/// The bands a kind carries, if it is one that has them.
pub(crate) fn zones_of(kind: &Kind) -> Option<&[Zone]> {
    match kind {
        Kind::Bar { zones, .. } | Kind::Arc { zones, .. } | Kind::SegBar { zones, .. } => {
            Some(zones)
        }
        _ => None,
    }
}

/// The gauge's own colour, which a new band starts at.
pub(crate) fn fill_of(kind: &Kind) -> Color {
    match kind {
        Kind::Bar { fill, .. } | Kind::Arc { fill, .. } | Kind::SegBar { fill, .. } => *fill,
        _ => Color::WHITE,
    }
}

/// Whether a kind carries a marker at all, since a band's marker colour is
/// only worth offering where there is one to colour.
pub(crate) fn has_cursor(kind: &Kind) -> bool {
    match kind {
        Kind::Arc { cursor, .. } | Kind::SegBar { cursor, .. } => cursor.is_some(),
        _ => false,
    }
}

/// A lighter version of a band's colour, which is what a marker inside it
/// nearly always wants: the same hue, bright enough to pick out.
fn brighter(c: Color) -> Color {
    let up = |v: u8| v.saturating_add((255 - v) / 2);
    Color::rgba(up(c.r), up(c.g), up(c.b), c.a)
}

/// A band's colour as the file writes it.
fn hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
}

/// The whole list as the `zones` field's text.
///
/// One band per line, because a gauge's bands read as a scale and a single
/// long line of them does not.
fn to_json(zones: &[Zone]) -> String {
    let mut out = String::from("[\n");
    for z in zones {
        out.push_str(&format!(
            "        {{ \"at\": {}, \"color\": \"{}\"",
            z.at,
            hex(z.color)
        ));
        if let Some(c) = z.cursor {
            out.push_str(&format!(", \"cursor\": \"{}\"", hex(c)));
        }
        out.push_str(" },\n");
    }
    out.push_str("      ]");
    out
}

/// The Bands rows, returning what they asked for.
///
/// `fill` is the gauge's own colour, which a new band starts at: a band that
/// arrived in some unrelated colour would have to be fixed before it could be
/// judged, and the interesting edit is nearly always where it starts.
pub(crate) fn zone_rows(
    ui: &mut egui::Ui,
    zones: &[Zone],
    fill: Color,
    marked: bool,
) -> Option<ZoneAsk> {
    let mut out = None;
    let id = ui.id().with("band-draft");
    let mut state = ui
        .data(|data| data.get_temp::<BandDraft>(id))
        .unwrap_or_else(|| BandDraft::of(zones));
    state.sync(zones);
    let edited = &mut state.edited;
    let mut changed: Option<Option<Gesture>> = None;
    let mut remove = None;

    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(
            "Start positions are scale fractions: 0 = minimum, 1 = maximum. \
             Each band lasts until the next starts. Below the first band, the normal fill is used. \
             Values above 1 keep a band outside the scale.",
        )
        .small()
        .weak(),
    );

    if edited.is_empty() {
        ui.weak("No colour bands. The gauge uses its normal fill across the scale.");
    }
    for (i, z) in edited.iter_mut().enumerate() {
        ui.push_id(i, |ui| {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.strong(format!("Band {}", i + 1));
                    if ui.small_button("Remove band").clicked() {
                        remove = Some(i);
                    }
                });
                egui::Grid::new("band-fields")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Starts at");
                        let r = controls::number(ui, &mut z.at, 0.01, "", None);
                        if committed(&r) {
                            changed = Some(gesture(&r));
                        }
                        ui.end_row();
                        ui.label("Fill colour");
                        if let Some(g) = ui
                            .push_id("fill", |ui| controls::colour(ui, &mut z.color))
                            .inner
                        {
                            changed = Some(g);
                        }
                        ui.end_row();
                        if marked || z.cursor.is_some() {
                            ui.label("Marker colour");
                            let mut on = z.cursor.is_some();
                            if ui.checkbox(&mut on, "Use a separate colour").changed() {
                                z.cursor = on.then_some(brighter(z.color));
                                changed = Some(None);
                            }
                            ui.end_row();
                            if let Some(color) = z.cursor.as_mut() {
                                ui.label("");
                                if let Some(g) =
                                    ui.push_id("marker", |ui| controls::colour(ui, color)).inner
                                {
                                    changed = Some(g);
                                }
                                ui.end_row();
                            }
                        }
                    });
                if !marked && z.cursor.is_some() {
                    ui.weak(
                        "The separate colour takes effect when this gauge has a reading marker.",
                    );
                }
            });
        });
    }
    if edited
        .iter()
        .enumerate()
        .any(|(i, z)| edited[..i].iter().any(|earlier| earlier.at == z.at))
    {
        ui.weak(
            "Two bands start at the same position; the last one at that position takes precedence.",
        );
    }

    ui.horizontal(|ui| {
        if ui
            .button("Add colour band")
            .on_hover_text("Starts at the top of the scale, in the gauge's own colour")
            .clicked()
        {
            edited.push(Zone {
                at: 1.0,
                color: fill,
                cursor: None,
            });
            changed = Some(None);
        }
        if !zones.is_empty() && ui.button("Remove all bands")
            .on_hover_text("Remove every colour band and return to the gauge's normal fill. Undo restores them.").clicked() {
            out = Some((ZoneEdit::Unset, None));
        }
    });

    if let Some(i) = remove {
        edited.remove(i);
        changed = Some(None);
    }
    if let (None, Some(g)) = (&out, changed) {
        // Held in the order they are written so the file reads up the scale,
        // which is also the order the builder wants them in.
        let mut sorted = edited.clone();
        sorted.sort_by(|a, b| a.at.total_cmp(&b.at));
        state.source = to_json(&sorted);
        out = Some((
            if edited.is_empty() {
                ZoneEdit::Unset
            } else {
                ZoneEdit::Set(state.source.clone())
            },
            g,
        ));
    }
    if matches!(out, Some((ZoneEdit::Unset, _))) {
        state = BandDraft::of(&[]);
    }
    ui.data_mut(|data| data.insert_temp(id, state));
    out
}

#[derive(Clone)]
struct BandDraft {
    source: String,
    edited: Vec<Zone>,
}

impl BandDraft {
    fn of(zones: &[Zone]) -> Self {
        Self {
            source: to_json(zones),
            edited: zones.to_vec(),
        }
    }

    fn sync(&mut self, zones: &[Zone]) {
        if to_json(zones) != self.source {
            *self = Self::of(zones);
        }
    }
}

#[cfg(test)]
mod interaction_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_band_is_written_as_the_file_spells_one() {
        let z = [Zone {
            at: 0.75,
            color: Color::rgba(255, 0, 0, 255),
            cursor: None,
        }];
        let text = to_json(&z);
        assert!(text.contains(r#""at": 0.75"#), "{text}");
        assert!(text.contains(r##""color": "#ff0000ff""##), "{text}");
    }

    #[test]
    fn the_list_round_trips_through_the_scene_builder() {
        // What the rows write has to be what the builder reads, or the pane
        // and the file disagree the moment anything is dragged.
        let z = [
            Zone {
                at: 0.0,
                color: Color::rgb(248, 81, 73),
                cursor: Some(Color::rgb(255, 170, 165)),
            },
            Zone {
                at: 0.6,
                color: Color::rgb(63, 185, 80),
                cursor: None,
            },
        ];
        let src = format!(
            r#"{{"width":10,"height":10,"root":{{"type":"bar","rect":[0,0,10,10],
               "zones":{}}}}}"#,
            to_json(&z)
        );
        let doc = copilot::scene::parse(&src).expect("the rows wrote valid json");
        let tree = copilot::scene::build(&doc).expect("and a scene that builds");
        let root = tree.get(copilot::widget::ROOT).unwrap().children[0];
        let back = zones_of(&tree.get(root).unwrap().kind).expect("a banded gauge");
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].at, 0.0);
        assert_eq!(back[1].color, Color::rgb(63, 185, 80));
    }

    #[test]
    fn only_the_gauges_have_bands() {
        assert!(
            zones_of(&Kind::Panel {
                background: Color::WHITE
            })
            .is_none()
        );
    }
}
