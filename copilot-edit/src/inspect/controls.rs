// SPDX-License-Identifier: GPL-3.0-only
//! Numeric validation and RGBA editing shared by inspector sections.

use crate::canvas::{Gesture, committed, gesture};
use copilot::Color;

fn parsed_number<T: egui::emath::Numeric>(text: &str, bounds: (f64, f64)) -> Option<f64> {
    let n = text.trim().parse::<f64>().ok()?;
    (n.is_finite() && n >= bounds.0 && n <= bounds.1 && (!T::INTEGRAL || n.fract() == 0.0))
        .then_some(n)
}

/// A drag-or-type number that explains rejected input instead of silently ignoring it.
pub(crate) fn number<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    value: &mut T,
    speed: f64,
    suffix: &str,
    bounds: Option<(f64, f64)>,
) -> egui::Response {
    let bounds = bounds.unwrap_or((T::MIN.to_f64(), T::MAX.to_f64()));
    ui.vertical(|ui| {
        let id = ui.next_auto_id().with("invalid-number");
        let ctx = ui.ctx().clone();
        let response = ui
            .add(
                egui::DragValue::new(value)
                    .speed(speed)
                    .suffix(suffix)
                    .range(bounds.0..=bounds.1)
                    .clamp_existing_to_range(false)
                    .custom_parser(move |text| {
                        let parsed = parsed_number::<T>(text, bounds);
                        ctx.data_mut(|data| {
                            if parsed.is_some() {
                                data.remove::<String>(id);
                            } else {
                                data.insert_temp(id, text.to_owned());
                            }
                        });
                        parsed
                    }),
            )
            .on_hover_text(
                "Drag to adjust; click to type. Hold Shift while dragging for finer changes.",
            );
        if committed(&response) {
            ui.data_mut(|data| data.remove::<String>(id));
        }
        if let Some(invalid) = ui.data(|data| data.get_temp::<String>(id)) {
            let expected = if T::INTEGRAL {
                "whole number"
            } else {
                "finite number"
            };
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("“{invalid}” is not a valid {expected}. Value unchanged."),
            )
            .on_hover_text(format!("Allowed range: {} to {}", bounds.0, bounds.1));
        }
        response
    })
    .inner
}

#[derive(Clone)]
struct HexDraft {
    source: Color,
    text: String,
}

fn hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
}

fn parse_hex(text: &str) -> Option<Color> {
    let text = text.trim().strip_prefix('#')?;
    if !matches!(text.len(), 6 | 8) || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let component = |start| u8::from_str_radix(&text[start..start + 2], 16).ok();
    Some(Color::rgba(
        component(0)?,
        component(2)?,
        component(4)?,
        if text.len() == 8 { component(6)? } else { 255 },
    ))
}

/// A picker interaction is one undo step for the lifetime of its popup.
pub(crate) fn colour_gesture(ui: &egui::Ui, r: &egui::Response) -> Option<Gesture> {
    Some(Gesture {
        id: r.id,
        ends: !ui.memory(|m| m.any_popup_open()),
    })
}

/// Edit straight RGBA components, with a retained, validated hexadecimal draft.
pub(crate) fn colour(ui: &mut egui::Ui, color: &mut Color) -> Option<Option<Gesture>> {
    let id = ui.id().with("hex-draft");
    let mut draft = ui
        .data(|data| data.get_temp::<HexDraft>(id))
        .filter(|draft| draft.source == *color)
        .unwrap_or_else(|| HexDraft {
            source: *color,
            text: hex(*color),
        });
    let mut change = None;
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            let mut rgba = [color.r, color.g, color.b, color.a];
            let r = ui
                .color_edit_button_srgba_unmultiplied(&mut rgba)
                .on_hover_text(
                    "Choose a colour and opacity. Alpha 00 is transparent; ff is opaque.",
                );
            if r.changed() {
                *color = Color::rgba(rgba[0], rgba[1], rgba[2], rgba[3]);
                draft = HexDraft {
                    source: *color,
                    text: hex(*color),
                };
                change = Some(colour_gesture(ui, &r));
            }
            let r = ui
                .add(
                    egui::TextEdit::singleline(&mut draft.text)
                        .desired_width(100.0)
                        .hint_text("#rrggbbaa"),
                )
                .on_hover_text("#RRGGBB or #RRGGBBAA. Press Enter or leave the field to apply.");
            if (r.lost_focus() || (r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))))
                && let Some(next) = parse_hex(&draft.text)
                && next != *color
            {
                *color = next;
                draft.source = next;
                change = Some(gesture(&r));
            }
        });
        if parse_hex(&draft.text).is_none() {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "Use #RRGGBB or #RRGGBBAA. Colour unchanged.",
            );
        }
    });
    ui.data_mut(|data| data.insert_temp(id, draft));
    change
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_numbers_are_rejected_without_losing_valid_extremes() {
        for text in ["", "-", "NaN", "inf", "1e100", "-1", "2.5"] {
            assert!(
                parsed_number::<u32>(text, (0.0, u32::MAX as f64)).is_none(),
                "{text}"
            );
        }
        assert_eq!(
            parsed_number::<u32>("4294967295", (0.0, u32::MAX as f64)),
            Some(u32::MAX as f64)
        );
        assert_eq!(parsed_number::<f32>("-2.5", (-100.0, 100.0)), Some(-2.5));
    }

    #[test]
    fn hex_colours_keep_straight_rgb_and_alpha() {
        let c = Color::rgba(200, 100, 50, 64);
        assert_eq!(parse_hex(&hex(c)), Some(c));
        assert_eq!(parse_hex("#AaBbCc"), Some(Color::rgb(170, 187, 204)));
        for invalid in ["ffffff", "#ff", "#ff00xx", "#αααααα"] {
            assert_eq!(parse_hex(invalid), None);
        }
    }

    #[test]
    fn merely_drawing_a_translucent_colour_does_not_change_it() {
        let ctx = egui::Context::default();
        let expected = Color::rgba(200, 100, 50, 64);
        let mut c = expected;
        for _ in 0..3 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    assert!(colour(ui, &mut c).is_none());
                });
            });
            assert_eq!(c, expected);
        }
    }

    #[test]
    fn an_invalid_colour_draft_survives_another_frame() {
        let ctx = egui::Context::default();
        let mut c = Color::WHITE;
        for frame in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let id = ui.id().with("hex-draft");
                    if frame == 0 {
                        ui.data_mut(|data| {
                            data.insert_temp(
                                id,
                                HexDraft {
                                    source: c,
                                    text: "#12".into(),
                                },
                            )
                        });
                    }
                    assert!(colour(ui, &mut c).is_none());
                    assert_eq!(
                        ui.data(|data| data.get_temp::<HexDraft>(id)).unwrap().text,
                        "#12"
                    );
                });
            });
        }
        assert_eq!(c, Color::WHITE);
    }

    #[test]
    fn invalid_numeric_typing_is_visible_and_does_not_commit() {
        let ctx = egui::Context::default();
        let mut value = 7u32;
        let mut response_id = egui::Id::NULL;
        let draw = |ctx: &egui::Context, value: &mut u32, response_id: &mut egui::Id| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let r = number(ui, value, 1.0, "", None);
                *response_id = r.id;
                assert!(!committed(&r));
            });
        };
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            draw(ctx, &mut value, &mut response_id)
        });
        ctx.memory_mut(|memory| memory.request_focus(response_id));
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            draw(ctx, &mut value, &mut response_id)
        });
        let input = egui::RawInput {
            events: vec![egui::Event::Text("not-a-number".into())],
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| draw(ctx, &mut value, &mut response_id));
        assert_eq!(value, 7);
        assert!(
            ctx.data(|data| data.get_temp::<String>(response_id.with("invalid-number")))
                .is_some()
        );
        let visible = output.shapes.iter().any(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) => text.galley.text().contains("Value unchanged"),
            _ => false,
        });
        assert!(visible, "the invalid number needs an inline explanation");
    }
}
