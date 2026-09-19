// SPDX-License-Identifier: GPL-3.0-only
//! One playback path for previews, recorded data, and explicit telemetry transmission.

use super::Driver;
use crate::datalog::Log;
use crate::simulation::{self, Provenance, Scenario};
use copilot::cogware_can::{ALL_GAUGES, gauge_by_id};

impl Driver {
    /// Begin preview-only playback, optionally conditioned by recorded inputs.
    pub(crate) fn playback(log: Option<Log>) -> Self {
        Self {
            enabled: true,
            driving: true,
            log,
            ..Self::default()
        }
    }

    pub(super) fn tick_playback(&mut self, delta_us: u64) {
        self.log_at_s += delta_us as f32 / 1_000_000.0;
        let mut step_us = delta_us;
        if let Some(log) = &self.log {
            let duration = log.seconds();
            if duration > 0.0 && self.log_at_s >= duration {
                self.log_at_s %= duration;
                self.engine.reset(self.cold_start);
                step_us = (f64::from(self.log_at_s) * 1_000_000.0) as u64;
            }
        }
        let measured = self.measured_readings();
        self.engine.advance(step_us.max(1), &measured);
        self.apply_generated_values();
    }

    fn measured_readings(&self) -> Vec<(u16, i32)> {
        self.log
            .as_ref()
            .map_or_else(Vec::new, |log| log.at(self.log_at_s))
    }

    /// All supported channels, with exact recorded raw values winning over generated ones.
    pub(crate) fn generated_readings(&self) -> Vec<(u16, i32)> {
        let measured = self.measured_readings();
        ALL_GAUGES
            .iter()
            .filter_map(|gauge| {
                if matches!(
                    simulation::provenance(gauge.id),
                    Some(Provenance::Excluded) | None
                ) {
                    return None;
                }
                let raw = measured
                    .iter()
                    .find(|(id, _)| *id == gauge.id)
                    .map(|(_, raw)| *raw)
                    .or_else(|| self.engine.reading(gauge.id))?;
                Some((gauge.id, raw))
            })
            .collect()
    }

    fn apply_generated_values(&mut self) {
        for (id, raw) in self.generated_readings() {
            if let Some(gauge) = gauge_by_id(id)
                && let Some(channel) = self.gauges.get_mut(gauge.name)
            {
                channel.value = raw as f32 / gauge.unit.scale as f32;
            }
        }
    }

    /// Populate every bound widget/GPIO channel without changing the document or sending data.
    pub(crate) fn fill_preview_values(&mut self) {
        self.enabled = true;
        self.driving = false;
        self.sweep = false;
        self.engine.advance(1, &self.measured_readings());
        self.apply_generated_values();
    }

    /// Reset time, thermal state, fuel use and other model history together.
    pub(crate) fn restart_drive(&mut self) {
        self.log_at_s = 0.0;
        self.engine.reset(self.cold_start);
        self.engine.advance(1, &self.measured_readings());
        self.apply_generated_values();
    }

    /// Seek recorded samples without carrying unrelated simulated transients across the jump.
    pub(crate) fn seek_drive(&mut self, position: f32) {
        let end = self
            .log
            .as_ref()
            .map_or(0.0, |log| log.seconds().next_down().max(0.0));
        self.log_at_s = position.clamp(0.0, end);
        self.engine.reset(self.cold_start);
        self.engine.advance(1, &self.measured_readings());
        self.apply_generated_values();
    }

    pub(crate) fn use_simulator(&mut self, scenario: Scenario) {
        self.log = None;
        self.engine.scenario = scenario;
        self.restart_drive();
    }

    pub(crate) fn use_log(&mut self, log: Log) {
        self.log = Some(log);
        self.restart_drive();
    }

    /// Explain which source supplies this channel rather than claiming defaults were measured.
    pub(crate) fn source_label(&self, id: u16) -> &'static str {
        if matches!(
            simulation::provenance(id),
            Some(Provenance::Excluded) | None
        ) {
            "Not simulated"
        } else if gauge_by_id(id).is_some_and(|gauge| {
            self.log
                .as_ref()
                .is_some_and(|log| log.filled.contains(&gauge.name))
        }) {
            "Recorded"
        } else {
            match simulation::provenance(id) {
                Some(Provenance::Modelled) => "Simulated",
                Some(Provenance::Nominal) => "Nominal demo value",
                _ => "Not simulated",
            }
        }
    }
}

#[cfg(test)]
mod tests;
