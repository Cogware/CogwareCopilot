// SPDX-License-Identifier: GPL-3.0-only
//! Responsive GPIO controls, scoped to the display and logical pin being edited.

use copilot::cogware_can::indicators::ALL_INDICATORS;
use copilot::gpio::{Dir, Level, Pins, Pull};

use super::{Act, Nav, Pending, PinsDialog};
use crate::pins::{Draft, Kind};

pub(super) fn pin_list(ui: &mut egui::Ui, d: &PinsDialog, pins: &Pins, nav: &mut Option<Nav>) {
    ui.strong("Logical pins");
    egui::ScrollArea::vertical()
        .id_salt("pin-list")
        .max_height(180.0)
        .show(ui, |ui| {
            for pin in 1..=pins.count() {
                ui.push_id(("pin-row", d.display, pin), |ui| {
                    let mapping = match pins.get(pin) {
                        None => "Not mapped".into(),
                        Some(p) => {
                            let direction = if p.dir == Dir::In { "Input" } else { "Output" };
                            let label = p
                                .label
                                .as_ref()
                                .map(|s| format!("{s} · "))
                                .unwrap_or_default();
                            format!("{direction} · {label}{}", Draft::of(p).summary())
                        }
                    };
                    let picked = d.editing.as_ref().is_some_and(|e| e.pin == pin);
                    if ui
                        .selectable_label(picked, format!("Pin {pin} — {mapping}"))
                        .on_hover_text("Select to edit. Selection never saves a mapping.")
                        .clicked()
                        && !picked
                    {
                        *nav = Some(Nav::Pin(pin));
                    }
                });
            }
        });
}

pub(super) fn pin_detail(ui: &mut egui::Ui, d: &mut PinsDialog, pins: &Pins) {
    let Some(draft) = d.editing.as_mut() else {
        ui.strong("Pin settings");
        ui.label("Select a logical pin to map an indicator, value threshold or button action.");
        return;
    };
    ui.push_id(("pin-detail", d.display, draft.pin), |ui| {
        ui.strong(format!("Logical pin {}", draft.pin));
        ui.weak(if pins.get(draft.pin).is_some() { "Saved mapping exists" } else { "Not yet mapped" });
        egui::Grid::new("fields").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Wire label");
            ui.add(egui::TextEdit::singleline(&mut draft.label).id_salt("wire-label")
                .hint_text("Optional, e.g. left stalk").desired_width(180.0))
                .on_hover_text("A human-readable label only; it does not select a physical pin.");
            ui.end_row();
            ui.label("Use as");
            ui.horizontal_wrapped(|ui| {
                for (kind, label, hint) in [
                    (Kind::Indicator, "Indicator", "An individual indicator, on an input or output."),
                    (Kind::Threshold, "Value threshold", "A numeric gauge condition. Selecting this sets the direction to Output."),
                    (Kind::Button, "Button action", "An input triggers an action. Selecting this sets the direction to Input."),
                ] {
                    if ui.selectable_label(draft.kind == kind, label).on_hover_text(hint).clicked() {
                        draft.set_kind(kind);
                    }
                }
            });
            ui.end_row();
            ui.label("Direction");
            ui.horizontal_wrapped(|ui| {
                for (dir, label, hint) in [
                    (Dir::In, "Input", "Read a signal from the car. A value threshold will return to the Indicator choice."),
                    (Dir::Out, "Output", "Command an output. A button action will return to the Indicator choice."),
                ] {
                    if ui.selectable_label(draft.dir == dir, label).on_hover_text(hint).clicked() {
                        draft.set_dir(dir);
                    }
                }
            });
            ui.end_row();
            if draft.dir == Dir::In {
                ui.label("Active level");
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut draft.active, Level::High, "High")
                        .on_hover_text("A high logic level means asserted or pressed.");
                    ui.selectable_value(&mut draft.active, Level::Low, "Low")
                        .on_hover_text("A low logic level means asserted or pressed.");
                });
                ui.end_row();
                ui.label("Input pull");
                egui::ComboBox::from_id_salt("pull").selected_text(pull_label(draft.pull)).show_ui(ui, |ui| {
                    for pull in [Pull::None, Pull::Up, Pull::Down] {
                        ui.selectable_value(&mut draft.pull, pull, pull_label(pull));
                    }
                }).response.on_hover_text("The board applies this bias when nothing drives the input. Match it to the real circuit.");
                ui.end_row();
            }
            match draft.kind {
                Kind::Threshold => super::threshold::rows(ui, draft, &mut d.gauge_filter),
                Kind::Indicator => {
                    ui.label("Indicator");
                    egui::ComboBox::from_id_salt("indicator")
                        .selected_text(if draft.indicator.is_empty() { "Choose an indicator" } else { &draft.indicator })
                        .height(260.0)
                        .show_ui(ui, |ui| {
                            ui.add(egui::TextEdit::singleline(&mut d.filter).id_salt("indicator-search")
                                .hint_text("Filter indicators").desired_width(180.0));
                            let filter = d.filter.trim().to_ascii_uppercase().replace(' ', "_");
                            let mut matches = 0;
                            for indicator in ALL_INDICATORS {
                                let name = indicator.name();
                                if name.contains(&filter) {
                                    matches += 1;
                                    ui.selectable_value(&mut draft.indicator, name.into(), name);
                                }
                            }
                            if matches == 0 { ui.weak("No matching indicators. Clear the filter to see all."); }
                        });
                    ui.end_row();
                }
                Kind::Button => {
                    ui.label("Action");
                    egui::ComboBox::from_id_salt("action")
                        .selected_text(action_label(draft.action.action()))
                        .show_ui(ui, |ui| {
                            for action in crate::touch_ui::ACTIONS {
                                if ui.selectable_label(draft.action.action() == *action, action_label(action)).clicked() {
                                    draft.action.set_action(action);
                                }
                            }
                        });
                    ui.end_row();
                    crate::touch_ui::action_rows(ui, &mut draft.action);
                }
            }
        });
        if draft.dir == Dir::Out {
            if draft.kind == Kind::Threshold {
                if draft.complete() { ui.label(draft.summary()); }
                ui.weak("Thresholds use the gauge's canonical unit, not a widget's display unit. Requires threshold-capable firmware on the controller/display.");
            } else {
                ui.weak("Output follows the selected indicator.");
            }
            ui.weak("HIGH/LOW are logical commands, not measured voltages. Electrical polarity and driver circuitry belong to the board; input level and pull do not apply.");
        } else {
            ui.weak("Active level defines asserted/pressed. Pull-up biases an undriven input high; pull-down biases it low. No pull leaves the circuit to set the level.");
            if draft.kind == Kind::Indicator {
                ui.weak("Inputs report indicator changes to the master; the returned indicator state drives the scene. This is not a numeric gauge binding.");
            } else {
                ui.weak(match draft.action.action() {
                    "page" => "A GPIO button must name its pager: unlike a widget, it has no enclosing pager.",
                    "menu" => "Use a menu name from this rig and choose the navigation button to press.",
                    "mode" => "Use a mode name from this rig. The master handles the request for every display.",
                    _ => "Use an event name understood by the host. The editor does not assign it a meaning.",
                });
            }
        }
        let reason = incomplete_reason(draft);
        let dirty = d.baseline.as_ref().is_none_or(|baseline| !draft.same_mapping(baseline));
        if let Some(reason) = reason { ui.colored_label(ui.visuals().warn_fg_color, reason); }
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(reason.is_none() && dirty, egui::Button::new("Review & save mapping…"))
                .on_disabled_hover_text(reason.unwrap_or("No mapping changes to save."))
                .clicked() {
                d.pending = Some(Pending::Save(Act::Write(Box::new(draft.clone()))));
            }
            if ui.add_enabled(pins.get(draft.pin).is_some(), egui::Button::new("Remove mapping…"))
                .on_disabled_hover_text("This pin has no saved mapping.")
                .clicked() {
                d.pending = Some(Pending::Save(Act::Clear(draft.pin)));
            }
            if dirty && ui.button("Reset draft").on_hover_text("Restore this pin's saved mapping without changing the rig file.").clicked() {
                if let Some(baseline) = &d.baseline { *draft = baseline.clone(); }
                d.error.clear();
            }
        });
    });
}

fn pull_label(pull: Pull) -> &'static str {
    match pull {
        Pull::None => "No pull",
        Pull::Up => "Pull-up",
        Pull::Down => "Pull-down",
    }
}

/// A readable action name for the picker and its save review.
pub(super) fn action_label(action: &str) -> &'static str {
    match action {
        "page" => "Change a page",
        "menu" => "Press a menu button",
        "mode" => "Request a rig mode",
        _ => "Emit a host event",
    }
}

fn incomplete_reason(draft: &Draft) -> Option<&'static str> {
    draft.validate().err()
}

#[cfg(test)]
mod tests {
    use super::*;
    use copilot::scene::touch::{Action, Page};

    #[test]
    fn a_page_pin_distinguishes_missing_pager_from_incomplete_destination() {
        let mut draft = Draft::fresh(1);
        draft.set_kind(Kind::Button);
        draft.action = crate::touch_ui::Draft::of(&Action::Page {
            pager: Some("pages".into()),
            to: Page::Named(String::new()),
        });
        assert!(incomplete_reason(&draft).unwrap().contains("destination"));
        draft.action = crate::touch_ui::Draft::of(&Action::Page {
            pager: None,
            to: Page::Named("1".into()),
        });
        assert!(incomplete_reason(&draft).unwrap().contains("pager"));
        draft.action = crate::touch_ui::Draft::of(&Action::Page {
            pager: Some("pages".into()),
            to: Page::Named("1".into()),
        });
        assert!(incomplete_reason(&draft).is_none());
        assert!(draft.to_rig().unwrap().contains(r#""to": "1""#));
    }
}
