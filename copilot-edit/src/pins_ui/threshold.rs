// SPDX-License-Identifier: GPL-3.0-only
//! Numeric-only gauge search and threshold controls in canonical gauge units.

use copilot::cogware_can::{ALL_GAUGES, GaugeData};
use copilot::gpio::Comparison;
use copilot::scene::bind::gauge_by_name;

use crate::pins::Draft;

fn numeric(gauge: &GaugeData) -> bool {
    !matches!(gauge.name, "INDICATORS1" | "INDICATORS2")
}

fn unit(gauge: &GaugeData) -> &str {
    if gauge.unit.symbol.is_empty() {
        "unitless"
    } else {
        gauge.unit.symbol
    }
}

fn matches(gauge: &GaugeData, query: &str) -> bool {
    let text = format!("{} {} {:?}", gauge.name, unit(gauge), gauge.source).to_lowercase();
    numeric(gauge)
        && query
            .split_whitespace()
            .all(|term| text.contains(&term.to_lowercase()))
}

pub(super) fn rows(ui: &mut egui::Ui, draft: &mut Draft, search: &mut String) {
    ui.label("Numeric gauge");
    let selected = gauge_by_name(&draft.gauge)
        .filter(|gauge| numeric(gauge))
        .map(|gauge| format!("{} · {}", gauge.name, unit(gauge)))
        .unwrap_or_else(|| "Choose a numeric gauge".into());
    egui::ComboBox::from_id_salt("threshold-gauge")
        .selected_text(selected)
        .width(190.0)
        .height(300.0)
        .show_ui(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(search)
                        .id_salt("gauge-search")
                        .hint_text("Search name, unit or source")
                        .desired_width(180.0),
                );
                if ui.small_button("Clear").clicked() {
                    search.clear();
                }
            });
            let mut count = 0;
            for gauge in ALL_GAUGES {
                if !matches(gauge, search) {
                    continue;
                }
                count += 1;
                ui.selectable_value(
                    &mut draft.gauge,
                    gauge.name.into(),
                    format!("{} · {}", gauge.name, unit(gauge)),
                )
                .on_hover_text(format!(
                    "Canonical unit: {}\nSource: {:?}",
                    unit(gauge),
                    gauge.source
                ));
            }
            if count == 0 {
                ui.weak("No matching numeric gauges. Try fewer words or Clear.");
            }
        });
    ui.end_row();
    ui.label("Comparison");
    egui::ComboBox::from_id_salt("threshold-comparison")
        .selected_text(comparison_label(draft.comparison))
        .show_ui(ui, |ui| {
            for comparison in [
                Comparison::Above,
                Comparison::AtLeast,
                Comparison::Below,
                Comparison::AtMost,
            ] {
                ui.selectable_value(
                    &mut draft.comparison,
                    comparison,
                    comparison_label(comparison),
                );
            }
        });
    ui.end_row();
    ui.label("Threshold");
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut draft.value)
            .id_salt("threshold-value")
            .hint_text("Finite number, e.g. 5000")
            .desired_width(100.0))
            .on_hover_text("Negative and fractional thresholds are allowed. Typing, Enter and focus loss never save.");
        if let Some(gauge) = gauge_by_name(&draft.gauge) {
            ui.label(unit(gauge));
        }
    });
    ui.end_row();
}

fn comparison_label(comparison: Comparison) -> &'static str {
    match comparison {
        Comparison::Above => "Above (>)",
        Comparison::AtLeast => "At least (>=)",
        Comparison::Below => "Below (<)",
        Comparison::AtMost => "At most (<=)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_search_matches_names_units_and_excludes_banks() {
        assert!(matches(&copilot::cogware_can::RPM, "rpm"));
        assert!(matches(&copilot::cogware_can::CLNT, "clnt °C"));
        assert!(!matches(&copilot::cogware_can::RPM, "voltage"));
        assert!(!matches(&copilot::cogware_can::INDICATORS1, ""));
        assert!(!matches(&copilot::cogware_can::INDICATORS2, ""));
    }
}
