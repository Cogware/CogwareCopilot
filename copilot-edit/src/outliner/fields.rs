// SPDX-License-Identifier: GPL-3.0-only
//! Grouped widget properties with scene keys kept separate from their display names.

use crate::canvas::{Gesture, committed, gesture};
use crate::inspect::{self, Value, controls, presentation};
use copilot::widget::Kind;

/// Draw the type-specific fields and return at most one source edit.
pub(super) fn properties(
    ui: &mut egui::Ui,
    kind: &Kind,
) -> Option<(&'static str, String, Option<Gesture>)> {
    let fields = inspect::fields(kind);
    let mut edit = None;
    for group in [
        "Content and behaviour",
        "Shape and layout",
        "Appearance",
        "Chart thresholds",
    ] {
        if !fields
            .iter()
            .any(|(key, _)| presentation::group(key) == group)
        {
            continue;
        }
        ui.strong(group);
        if group == "Chart thresholds" {
            ui.weak("Scale fractions: 0 = minimum, 1 = maximum; above 1 switches a threshold off.");
        }
        egui::Grid::new(("type-properties", group))
            .num_columns(2)
            .spacing([8.0, 5.0])
            .show(ui, |ui| {
                for (key, value) in fields
                    .iter()
                    .filter(|(key, _)| presentation::group(key) == group)
                {
                    let (label, hint, suffix) = presentation::field(kind, key);
                    ui.label(label)
                        .on_hover_text(format!("{hint}\nScene field: {key}"));
                    let mut next = value.clone();
                    let changed = ui
                        .push_id(*key, |ui| match &mut next {
                            Value::Color(c) => controls::colour(ui, c),
                            Value::Text(t) => {
                                let r = if *key == "text" {
                                    ui.add(
                                        egui::TextEdit::multiline(t)
                                            .desired_rows(2)
                                            .desired_width(180.0),
                                    )
                                } else {
                                    ui.add(egui::TextEdit::singleline(t).desired_width(180.0))
                                };
                                commit(r)
                            }
                            Value::Frac(f) => {
                                ui.vertical(|ui| {
                                    let r = controls::number(ui, f, 0.01, "", Some((0.0, 1.0)));
                                    ui.weak("0 = none · 1 = full");
                                    commit(r)
                                })
                                .inner
                            }
                            Value::Ratio(f) => commit(controls::number(ui, f, 0.01, suffix, None)),
                            Value::Count(n) => {
                                let bounds = (*key == "scale").then_some((1.0, 255.0));
                                commit(controls::number(ui, n, 1.0, suffix, bounds))
                            }
                            Value::Whole(n) => commit(controls::number(ui, n, 1.0, suffix, None)),
                            Value::Flag(b) => commit(ui.checkbox(b, "Enabled")),
                            Value::Choice(word, all) => {
                                let mut picked = *word;
                                let response = egui::ComboBox::from_id_salt(*key)
                                    .selected_text(presentation::choice(word))
                                    .show_ui(ui, |ui| {
                                        let mut changed = false;
                                        for option in all.iter() {
                                            changed |= ui
                                                .selectable_value(
                                                    &mut picked,
                                                    option,
                                                    presentation::choice(option),
                                                )
                                                .changed();
                                        }
                                        changed
                                    });
                                *word = picked;
                                response.inner.unwrap_or(false).then_some(None)
                            }
                        })
                        .inner;
                    ui.end_row();
                    if let Some(g) = changed
                        && edit.is_none()
                    {
                        edit = Some((*key, next.to_scene(), g));
                    }
                }
            });
        ui.add_space(6.0);
    }
    if matches!(kind, Kind::Chart { .. }) {
        ui.weak("Authored sample values remain editable in Scene text.");
    }
    edit
}

fn commit(response: egui::Response) -> Option<Option<Gesture>> {
    committed(&response).then(|| gesture(&response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merely_showing_properties_never_commits_live_or_out_of_range_values() {
        let ctx = egui::Context::default();
        let kinds = [
            Kind::Anim {
                anim: 0,
                frame: 12,
                playing: true,
                speed: -2.0,
                elapsed_us: 0,
            },
            Kind::Bar {
                value: 1.2,
                fill: copilot::Color::rgba(200, 100, 50, 64),
                track: copilot::Color::BLACK,
                vertical: false,
                zones: Vec::new(),
            },
        ];
        for kind in kinds {
            for _ in 0..3 {
                let _ = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        assert!(properties(ui, &kind).is_none());
                    });
                });
            }
        }
    }
}
