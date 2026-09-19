// SPDX-License-Identifier: GPL-3.0-only
//! Human-readable controller readings, with wire-level details kept secondary.

use super::ui::WARN;
use super::watch::{self, Reading, Watch};

pub(super) fn show(ui: &mut egui::Ui, watch: &Watch, all: &mut bool) {
    if let Some((status, age)) = watch.latest() {
        ui.horizontal(|ui| {
            lamp(ui, "CAN bus", status.can_up());
            lamp(ui, "Display", status.panel_up());
            lamp(ui, "Host", status.host_up());
        });
        if status.loopback() {
            ui.colored_label(WARN, "Loopback mode: data is not reaching the displays.");
        }
        if watch.silent() {
            ui.colored_label(
                WARN,
                format!(
                    "No controller status for {} s. Readings may be out of date.",
                    age.as_secs()
                ),
            );
        }
        ui.collapsing("Controller diagnostics", |ui| {
            egui::Grid::new("controller_diagnostics")
                .num_columns(2)
                .show(ui, |ui| {
                    for (label, value) in [
                        ("Uptime", watch::uptime(status.uptime_ms)),
                        ("CAN frames / second", status.can_tx_rate.to_string()),
                        ("CAN frames sent", status.can_tx.to_string()),
                        ("CAN frames received", status.can_rx.to_string()),
                        ("Requested gauges", status.subscribers.to_string()),
                        ("Frames from host", status.host_frames.to_string()),
                        ("Checksum errors", status.crc_errors.to_string()),
                    ] {
                        ui.label(label);
                        ui.label(value);
                        ui.end_row();
                    }
                });
        });
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(format!("{} gauges with data", watch.live_gauges()));
        ui.checkbox(all, "Show gauges without data");
    });
    if !watch.has_readings() {
        ui.weak("No gauge readings received yet.");
    } else {
        egui::ScrollArea::vertical()
            .id_salt("live_gauges")
            .max_height(260.0)
            .show(ui, |ui| {
                egui::Grid::new("gauge_readings")
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        for label in ["Gauge", "Value", "Raw value"] {
                            ui.strong(label);
                        }
                        ui.end_row();
                        for gauge in cogware_can::ALL_GAUGES {
                            let reading = watch.reading(gauge.id);
                            if !*all && !matches!(reading, Reading::Value(_)) {
                                continue;
                            }
                            ui.label(gauge.name);
                            match reading {
                                Reading::Value(raw) => {
                                    let value = raw as f32 / gauge.unit.scale as f32;
                                    ui.label(format!("{value:.2} {}", gauge.unit.symbol));
                                    ui.weak(raw.to_string());
                                }
                                Reading::NoData => {
                                    ui.weak("No sensor data");
                                    ui.weak("CAFEBABE")
                                        .on_hover_text("The controller's no-data sentinel.");
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
    }
    if !watch.health().is_empty() {
        ui.collapsing("Display performance", |ui| {
            egui::Grid::new("display_health")
                .num_columns(5)
                .striped(true)
                .show(ui, |ui| {
                    for label in ["Display", "FPS", "Draw time", "Pixels", "Areas"] {
                        ui.strong(label);
                    }
                    ui.end_row();
                    for (node, health, at) in watch.health() {
                        ui.label(format!("0x{node:02X}"))
                            .on_hover_text(format!("Last report {} s ago", at.elapsed().as_secs()));
                        ui.label(health.fps.to_string());
                        ui.label(format!("{} us", health.compose_us));
                        ui.label(health.painted.to_string());
                        ui.label(health.rects.to_string());
                        ui.end_row();
                    }
                });
            if let Some((_, _, oldest)) = watch.health().iter().min_by_key(|(_, _, at)| *at)
                && oldest.elapsed().as_secs() > 5
            {
                ui.colored_label(
                    WARN,
                    format!(
                        "Oldest display report: {} s ago.",
                        oldest.elapsed().as_secs()
                    ),
                );
            }
        });
    }
}

fn lamp(ui: &mut egui::Ui, name: &str, on: bool) {
    let color = if on {
        egui::Color32::from_rgb(63, 185, 80)
    } else {
        egui::Color32::from_rgb(150, 150, 150)
    };
    ui.colored_label(
        color,
        format!("{name}: {}", if on { "online" } else { "offline" }),
    );
}
