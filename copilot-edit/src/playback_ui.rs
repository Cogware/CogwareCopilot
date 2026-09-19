// SPDX-License-Identifier: GPL-3.0-only
//! Simulation source selection and transparent model-versus-recorded reading inspection.

use crate::{App, simulation::Scenario};

impl App {
    pub(crate) fn playback_options(&mut self, ui: &mut egui::Ui) {
        ui.strong("Playback source");
        let generic = self.driver.log.is_none();
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(generic, "Generic engine").clicked() && !generic {
                self.driver.use_simulator(self.driver.engine.scenario);
            }
            if ui
                .selectable_label(
                    self.driver
                        .log
                        .as_ref()
                        .is_some_and(crate::datalog::Log::is_built_in),
                    "Sample drive (US06)",
                )
                .clicked()
            {
                self.driver.use_log(crate::datalog::Log::us06());
            }
        });
        if ui.button("Open driving log...").clicked() {
            ui.close_menu();
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Driving logs", &["msl", "csv", "tsv", "txt", "log"])
                .pick_file()
            {
                match crate::datalog::Log::read(&path) {
                    Ok(log) => {
                        self.driver.use_log(log);
                        self.driver.enabled = true;
                        self.driver.driving = true;
                        self.driver.sweep = false;
                        self.status = "Driving log loaded. Recorded channels take priority; the engine model fills the rest.".into();
                    }
                    Err(error) => self.status = format!("Could not open driving log: {error}"),
                }
            }
        }
        if let Some(log) = &self.driver.log {
            let duration = log.seconds();
            ui.strong(&log.name);
            ui.small(format!("Recorded: {}", log.filled.join(", ")));
            if !log.skipped.is_empty() {
                ui.small(format!("Unmapped columns: {}", log.skipped.join(", ")));
            }
            ui.small("Recorded values win. Missing values are simulated using the recorded engine inputs.");
            let mut position = self.driver.log_at_s;
            if ui
                .add(
                    egui::Slider::new(&mut position, 0.0..=duration.max(0.01))
                        .text("Position")
                        .suffix(" s"),
                )
                .changed()
            {
                self.driver.seek_drive(position);
            }
            ui.small("Seeking restarts generated transients; simulated channels are not recorded measurements.");
        } else {
            let mut scenario = self.driver.engine.scenario;
            egui::ComboBox::from_id_salt("engine-scenario")
                .selected_text(scenario.label())
                .show_ui(ui, |ui| {
                    for choice in [
                        Scenario::Road,
                        Scenario::Idle,
                        Scenario::Pull,
                        Scenario::Overrun,
                    ] {
                        ui.selectable_value(&mut scenario, choice, choice.label());
                    }
                });
            if scenario != self.driver.engine.scenario {
                self.driver.use_simulator(scenario);
            }
        }
        ui.checkbox(&mut self.driver.cold_start, "Cold engine on restart")
            .on_hover_text(
                "Restart at ambient temperature. Otherwise begin with a warmed-up engine.",
            );
        ui.horizontal_wrapped(|ui| {
            if ui.button("Restart drive").clicked() { self.driver.restart_drive(); }
            if ui.button("Fill preview values").on_hover_text(
                "Take a generated snapshot for all bound gauges and GPIO sources. Stops playback and sends nothing."
            ).clicked() { self.driver.fill_preview_values(); }
        });
        ui.small("Generic petrol-engine demonstration, not a tuning or diagnostic prediction. Actual sensors lag and vary around their targets.");
        ui.collapsing("All generated readings", |ui| self.simulation_readings(ui));
        ui.separator();
        ui.add(
            egui::Slider::new(&mut self.driver.period_s, 0.5..=60.0)
                .text("Sweep each way")
                .suffix(" s"),
        );
    }

    fn simulation_readings(&self, ui: &mut egui::Ui) {
        let readings = self.driver.generated_readings();
        ui.label(format!(
            "{} populated channels; protocol heartbeats and indicator banks are excluded.",
            readings.len()
        ));
        let id = ui.id().with("simulated-gauge-search");
        let mut search = ui
            .data(|data| data.get_temp::<String>(id))
            .unwrap_or_default();
        ui.add(egui::TextEdit::singleline(&mut search).hint_text("Filter gauge name"));
        ui.data_mut(|data| data.insert_temp(id, search.clone()));
        let query = search.trim().to_ascii_uppercase();
        egui::ScrollArea::vertical()
            .id_salt("generated-readings")
            .max_height(260.0)
            .show(ui, |ui| {
                egui::Grid::new("generated-reading-table")
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        for (id, raw) in readings {
                            let Some(gauge) = copilot::cogware_can::gauge_by_id(id) else {
                                continue;
                            };
                            if !gauge.name.contains(&query) {
                                continue;
                            }
                            ui.label(gauge.name);
                            ui.monospace(format!(
                                "{:.2} {}",
                                raw as f64 / f64::from(gauge.unit.scale),
                                gauge.unit.symbol
                            ));
                            ui.weak(self.driver.source_label(id));
                            ui.end_row();
                        }
                    });
            });
    }
}
