// SPDX-License-Identifier: GPL-3.0-only
//! Deterministic, generic petrol-engine telemetry for previews, never ECU prediction.
//! A 2-litre, four-cylinder, port-injected turbo engine drives a six-speed,
//! 1500 kg car; values are canonical physical units until the output boundary.

mod channels;
mod conditioning;
mod dynamics;
#[cfg(test)]
mod recorded_tests;
mod state;
#[cfg(test)]
mod tests;

use cogware_can::{self as g, Gauge, GaugeData, Wire};
use state::{Inputs, State};

/// Safe preview driving patterns, with no effect on connected hardware.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum Scenario {
    /// Stop, accelerate, cruise and coast around a repeating road cycle.
    #[default]
    Road,
    /// Stationary idle, including the higher cold-start idle.
    Idle,
    /// Sustained acceleration with automatic upshifts.
    Pull,
    /// Closed-throttle coast, entering at 85 km/h if initially stationary.
    Overrun,
}

impl Scenario {
    /// Friendly label for preview controls.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Road => "Road cycle",
            Self::Idle => "Idle",
            Self::Pull => "Acceleration pull",
            Self::Overrun => "Overrun / fuel cut",
        }
    }
}

/// Whether a channel is derived, an explicit demo default, or never simulated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Provenance {
    /// Derived from the common engine and drivetrain state.
    Modelled,
    /// Healthy, not-fitted or illustrative fixed demo value.
    Nominal,
    /// Live protocol or deliberate tell-tale controls, never synthesised.
    Excluded,
}

/// Identify every supported catalogue channel, including deliberate exclusions.
pub(crate) fn provenance(id: u16) -> Option<Provenance> {
    Some(channels::channel(Gauge::from_id(id)?).provenance())
}

/// Independent deterministic engine state; never writes to the global gauge table.
#[derive(Debug)]
pub(crate) struct Engine {
    /// Selected safe preview driving pattern.
    pub(crate) scenario: Scenario,
    previous_scenario: Scenario,
    state: State,
    measured: Inputs,
    remainder_us: u64,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Engine {
    /// Start warm and immediately useful, or at a 20 °C cold start.
    pub(crate) fn new(cold_start: bool) -> Self {
        let mut engine = Self {
            scenario: Scenario::Road,
            previous_scenario: Scenario::Road,
            state: State::new(cold_start),
            measured: Inputs::default(),
            remainder_us: 0,
        };
        engine.state.derive(&engine.measured);
        engine
    }

    /// Reset all readings, time and counters while retaining the selected pattern.
    pub(crate) fn reset(&mut self, cold_start: bool) {
        let scenario = self.scenario;
        *self = Self::new(cold_start);
        self.scenario = scenario;
    }

    /// Advance elapsed microseconds with fresh optional canonical recorded samples.
    ///
    /// Recorded TPS takes precedence over pedal for engine load; recorded RPM and
    /// VSS both win over the drivetrain, with gear inferred only when absent.
    /// First recorded snapshots initialise history immediately, even below one tick.
    /// Fixed 10 ms ticks make normal frame partitions identical; gaps over 40 s
    /// use at most 4000 coarser ticks, preserving elapsed time without a catch-up loop.
    pub(crate) fn advance(&mut self, delta_us: u64, measured: &[(u16, i32)]) {
        if delta_us == 0 {
            return;
        }
        let mut next = Inputs::from_samples(measured);
        if let (Some(previous), Some(current)) = (self.measured.get(&g::VSS), next.get(&g::VSS)) {
            // Different log sample rates must not change inferred acceleration.
            next.speed_acceleration = ((current.clamp(0.0, 300.0) - previous.clamp(0.0, 300.0))
                / (delta_us as f64 * 0.000001 * 3.6))
                .clamp(-8.0, 8.0);
        }
        if self.previous_scenario != self.scenario {
            if self.scenario == Scenario::Overrun
                && self.state.speed < 25.0
                && next.get(&g::VSS).is_none()
            {
                self.state.speed = 85.0;
                self.state.gear = 3;
                self.state.rpm = 85.0 * state::rpm_per_kmh(3);
                self.state.pedal = 0.0;
                self.state.tps = 0.0;
            }
            self.previous_scenario = self.scenario;
        }
        self.state.seed_recorded(&next, &self.measured);
        self.measured = next;
        let total = u128::from(delta_us) + u128::from(self.remainder_us);
        let ticks = total / 10_000;
        self.remainder_us = (total % 10_000) as u64;
        let steps = ticks.min(4000) as usize;
        if steps == 0 {
            return;
        }
        let dt = ticks as f64 * 0.01 / steps as f64;
        for _ in 0..steps {
            self.state.tick(dt, self.scenario, &self.measured);
        }
    }

    /// Canonical fixed-point reading, or no reading for unknown/protocol channels.
    ///
    /// Supported recorded samples are returned verbatim, even if nonphysical;
    /// internals separately sanitise them. The absence sentinel is never a sample.
    pub(crate) fn reading(&self, id: u16) -> Option<i32> {
        let gauge = Gauge::from_id(id)?;
        let channel = channels::channel(gauge);
        if channel.provenance() == Provenance::Excluded {
            return None;
        }
        if let Some(value) = self.measured.raw(gauge.data()) {
            return Some(value);
        }
        let value = channel.value(&self.state)?;
        Some(encode(gauge.data(), value))
    }
}

fn encode(gauge: &GaugeData, physical: f64) -> i32 {
    let (low, high) = match gauge.wire {
        Wire::U8 => (0, u8::MAX as i32),
        Wire::I8 => (i8::MIN as i32, i8::MAX as i32),
        Wire::U16 => (0, u16::MAX as i32),
        Wire::I16 => (i16::MIN as i32, i16::MAX as i32),
        Wire::I32 => (i32::MIN + 1, i32::MAX),
    };
    let scaled = (physical * f64::from(gauge.unit.scale)).round();
    assert!(
        scaled.is_finite(),
        "non-finite simulated reading for {}",
        gauge.name
    );
    let raw = scaled.clamp(f64::from(low), f64::from(high)) as i32;
    if raw == g::NO_DATA { raw + 1 } else { raw }
}
