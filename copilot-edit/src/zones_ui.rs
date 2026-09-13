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
            "        {{ \"at\": {:.2}, \"color\": \"{}\" }},\n",
            z.at,
            hex(z.color)
        ));
    }
    out.push_str("      ]");
    out
}

/// The Bands rows, returning what they asked for.
///
/// `fill` is the gauge's own colour, which a new band starts at: a band that
/// arrived in some unrelated colour would have to be fixed before it could be
/// judged, and the interesting edit is nearly always where it starts.
pub(crate) fn zone_rows(ui: &mut egui::Ui, zones: &[Zone], fill: Color) -> Option<ZoneAsk> {
    let mut out = None;
    let mut edited: Vec<Zone> = zones.to_vec();
    let mut changed: Option<Option<Gesture>> = None;
    let mut remove = None;

    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(
            "Bands. Each one starts where it says and runs until the next \
             begins, so a gauge can be dangerous low and dangerous again high.",
        )
        .small()
        .weak(),
    );

    egui::Grid::new("props-zones")
        .num_columns(3)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            for (i, z) in edited.iter_mut().enumerate() {
                ui.label("from");
                // Dragged rather than typed: a band's position is judged
                // against the gauge behind it, and a drag redraws it as it
                // moves.
                let r = ui.add(egui::Slider::new(&mut z.at, 0.0..=1.0).fixed_decimals(2));
                if committed(&r) {
                    changed = Some(gesture(&r));
                }
                ui.horizontal(|ui| {
                    let mut rgba = [z.color.r, z.color.g, z.color.b, z.color.a];
                    let r = ui.color_edit_button_srgba_unmultiplied(&mut rgba);
                    if r.changed() {
                        z.color = Color::rgba(rgba[0], rgba[1], rgba[2], rgba[3]);
                        changed = Some(gesture(&r));
                    }
                    if ui.button("×").on_hover_text("Remove this band").clicked() {
                        remove = Some(i);
                    }
                });
                ui.end_row();
            }
        });

    ui.horizontal(|ui| {
        if ui
            .button("Add a band")
            .on_hover_text("Starts at the top of the scale, in the gauge's own colour")
            .clicked()
        {
            edited.push(Zone {
                at: 1.0,
                color: fill,
            });
            changed = Some(None);
        }
        if !zones.is_empty() && ui.button("No bands").clicked() {
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
        edited.sort_by(|a, b| a.at.total_cmp(&b.at));
        out = Some((
            if edited.is_empty() {
                ZoneEdit::Unset
            } else {
                ZoneEdit::Set(to_json(&edited))
            },
            g,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_band_is_written_as_the_file_spells_one() {
        let z = [Zone {
            at: 0.75,
            color: Color::rgba(255, 0, 0, 255),
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
            },
            Zone {
                at: 0.6,
                color: Color::rgb(63, 185, 80),
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
