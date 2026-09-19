// SPDX-License-Identifier: GPL-3.0-only
//! Searchable data sources, keeping gauge banks distinct from individual indicators.

use super::{Pick, symbol};
use copilot::cogware_can::indicators::ALL_INDICATORS;
use copilot::cogware_can::{ALL_GAUGES, GaugeData};

fn words(raw: &str) -> String {
    raw.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            if matches!(part, "ABS" | "GPS" | "ECU" | "CAN" | "RPM") {
                return part.to_owned();
            }
            let mut letters = part.chars();
            let Some(first) = letters.next() else {
                return String::new();
            };
            format!(
                "{}{}",
                first.to_uppercase(),
                letters.as_str().to_lowercase()
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn gauge_title(g: &GaugeData) -> String {
    let name = match g.name {
        "RPM" => "Engine speed",
        "CLNT" => "Coolant temperature",
        "MAP" => "Manifold absolute pressure",
        "BAT_VOL" => "Battery voltage",
        "GEAR" => "Selected gear",
        "TPS" => "Throttle position",
        "IAT" => "Intake air temperature",
        "AFR" => "Air–fuel ratio",
        "INDICATORS1" => "Indicator bank 1 (raw bits)",
        "INDICATORS2" => "Indicator bank 2 (raw bits)",
        _ => return words(g.name),
    };
    name.to_owned()
}

fn title(pick: Pick) -> String {
    match pick {
        Pick::None => "No data binding".into(),
        Pick::Gauge(g) => format!("{} · {}", gauge_title(g), symbol(g.unit)),
        Pick::Lamp(ind) => format!("{} (indicator)", words(ind.name())),
    }
}

fn matches(query: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    query
        .split_whitespace()
        .all(|term| text.contains(&term.to_lowercase()))
}

/// Choose a gauge or individual indicator without changing their stored identifiers.
pub(super) fn show(ui: &mut egui::Ui, now: Pick) -> Pick {
    let mut picked = now;
    let id = ui.id().with("source-search");
    let mut search = ui
        .data(|data| data.get_temp::<String>(id))
        .unwrap_or_default();
    egui::ComboBox::from_id_salt("data-source")
        .selected_text(title(now))
        .width(210.0)
        .height(420.0)
        .show_ui(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut search)
                        .hint_text("Search name, code, unit or source")
                        .desired_width(250.0),
                );
                if ui.small_button("Clear").clicked() {
                    search.clear();
                }
            });
            ui.selectable_value(&mut picked, Pick::None, "No data binding");
            ui.separator();
            ui.strong("Gauge readings");
            let mut count = 0;
            for g in ALL_GAUGES {
                let label = gauge_title(g);
                if !matches(
                    &search,
                    &format!("{label} {} {} {:?}", g.name, symbol(g.unit), g.source),
                ) {
                    continue;
                }
                count += 1;
                ui.selectable_value(
                    &mut picked,
                    Pick::Gauge(g),
                    format!("{label} · {} [{}]", symbol(g.unit), g.name),
                )
                .on_hover_text(format!(
                    "Source: {:?}\nCAN identifier: 0x{:03X}\nScene gauge: {}",
                    g.source, g.id, g.name
                ));
            }
            ui.separator();
            ui.strong("Individual indicators");
            ui.weak("Choose one lamp, not its whole bank: 0 = off, 1 = on.");
            for ind in ALL_INDICATORS {
                let label = words(ind.name());
                if !matches(&search, &format!("{label} {}", ind.name())) {
                    continue;
                }
                count += 1;
                ui.selectable_value(
                    &mut picked,
                    Pick::Lamp(*ind),
                    format!("{label} [{}]", ind.name()),
                );
            }
            if count == 0 {
                ui.weak("No matching data sources. Try fewer words or Clear.");
            }
        });
    ui.data_mut(|data| data.insert_temp(id, search));
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_friendly_names_raw_codes_and_units() {
        let coolant = &copilot::cogware_can::CLNT;
        let text = format!(
            "{} {} {}",
            gauge_title(coolant),
            coolant.name,
            symbol(coolant.unit)
        );
        assert!(matches("coolant", &text));
        assert!(matches("clnt", &text));
        assert!(matches("temperature °C", &text));
        assert!(!matches("voltage", &text));
        assert!(matches("", &text));
    }

    #[test]
    fn indicators_remain_distinct_from_raw_banks() {
        let bank = title(Pick::Gauge(&copilot::cogware_can::INDICATORS1));
        let lamp = title(Pick::Lamp(
            copilot::cogware_can::indicators::Indicator::OilPressure,
        ));
        assert!(bank.contains("raw bits"));
        assert!(lamp.contains("Oil Pressure"));
        assert!(lamp.contains("indicator"));
    }
}
