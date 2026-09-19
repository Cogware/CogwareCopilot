// SPDX-License-Identifier: GPL-3.0-only
//! Physical state and sanitised inputs; neither reads nor writes global readings.

use cogware_can::{self as g, GaugeData};

#[derive(Clone, Debug)]
pub(super) struct Inputs {
    samples: [Option<i32>; 256],
    pub(super) speed_acceleration: f64,
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            samples: [None; 256],
            speed_acceleration: 0.0,
        }
    }
}

impl Inputs {
    pub(super) fn from_samples(samples: &[(u16, i32)]) -> Self {
        let mut result = Self::default();
        for &(id, raw) in samples {
            if raw != g::NO_DATA
                && let Some(slot) = result.samples.get_mut(usize::from(id))
            {
                *slot = Some(raw);
            }
        }
        result
    }

    pub(super) fn raw(&self, gauge: &GaugeData) -> Option<i32> {
        self.samples.get(usize::from(gauge.id)).copied().flatten()
    }

    pub(super) fn get(&self, gauge: &GaugeData) -> Option<f64> {
        self.raw(gauge)
            .map(|raw| f64::from(raw) / f64::from(gauge.unit.scale))
    }

    pub(super) fn bounded(&self, gauge: &GaugeData, fallback: f64, lo: f64, hi: f64) -> f64 {
        self.get(gauge).unwrap_or(fallback).clamp(lo, hi)
    }
}

#[derive(Debug)]
pub(super) struct State {
    pub(super) time: f64,
    pub(super) speed: f64,
    pub(super) gps_speed: f64,
    pub(super) rpm: f64,
    pub(super) gear: u8,
    pub(super) shift_left: f64,
    pub(super) pedal: f64,
    pub(super) tps: f64,
    pub(super) tps_dot: f64,
    pub(super) rpm_dot: f64,
    pub(super) map_dot: f64,
    pub(super) acceleration: f64,
    pub(super) baro: f64,
    pub(super) map: f64,
    pub(super) boost: f64,
    pub(super) boost_target: f64,
    pub(super) boost_pwm: f64,
    pub(super) coolant: f64,
    pub(super) oil_temp: f64,
    pub(super) iat: f64,
    pub(super) fuel_temp: f64,
    pub(super) trans_temp: f64,
    pub(super) diff_temp: f64,
    pub(super) ecu_temp: f64,
    pub(super) oil_pressure: f64,
    pub(super) battery: f64,
    pub(super) afr_target: f64,
    pub(super) combustion_afr: f64,
    pub(super) afr: f64,
    pub(super) afr_secondary: f64,
    pub(super) trim: f64,
    pub(super) enrichment: f64,
    pub(super) warmup: f64,
    pub(super) afterstart: f64,
    pub(super) ve: f64,
    pub(super) air_mass: f64,
    pub(super) airflow: f64,
    pub(super) fuel_flow: f64,
    pub(super) fuel_rate: f64,
    pub(super) fuel_used: f64,
    pub(super) fuel_litres: f64,
    pub(super) pulse: f64,
    pub(super) injector_duty: f64,
    pub(super) dwell: f64,
    pub(super) spark: f64,
    pub(super) egt: f64,
    pub(super) vvt: f64,
    pub(super) vvt_target: f64,
    pub(super) vvt_exhaust: f64,
    pub(super) vvt_exhaust_target: f64,
    pub(super) slip: f64,
    pub(super) trip: f64,
    pub(super) revolutions: f64,
    pub(super) fuel_cut: bool,
}

impl State {
    pub(super) fn new(cold: bool) -> Self {
        Self {
            time: 0.0,
            speed: 0.0,
            gps_speed: 0.0,
            rpm: if cold { 1100.0 } else { 850.0 },
            gear: 0,
            shift_left: 0.0,
            pedal: 0.0,
            tps: 5.0,
            tps_dot: 0.0,
            rpm_dot: 0.0,
            map_dot: 0.0,
            acceleration: 0.0,
            baro: 100.0,
            map: 34.0,
            boost: 0.0,
            boost_target: 100.0,
            boost_pwm: 0.0,
            coolant: if cold { 20.0 } else { 89.0 },
            oil_temp: if cold { 20.0 } else { 94.0 },
            iat: if cold { 20.0 } else { 29.0 },
            fuel_temp: if cold { 20.0 } else { 27.0 },
            trans_temp: if cold { 20.0 } else { 68.0 },
            diff_temp: if cold { 20.0 } else { 55.0 },
            ecu_temp: if cold { 20.0 } else { 42.0 },
            oil_pressure: 150.0,
            battery: 14.1,
            afr_target: if cold { 13.7 } else { 14.7 },
            combustion_afr: if cold { 13.85 } else { 14.85 },
            afr: if cold { 13.95 } else { 14.95 },
            afr_secondary: if cold { 14.02 } else { 15.02 },
            trim: 0.0,
            enrichment: 100.0,
            warmup: if cold { 135.0 } else { 100.0 },
            afterstart: if cold { 118.0 } else { 100.0 },
            ve: 75.0,
            air_mass: 0.0,
            airflow: 0.0,
            fuel_flow: 0.0,
            fuel_rate: 0.0,
            fuel_used: 0.0,
            fuel_litres: 42.0,
            pulse: 0.0,
            injector_duty: 0.0,
            dwell: 2.8,
            spark: 12.0,
            egt: if cold { 20.0 } else { 390.0 },
            vvt: 0.0,
            vvt_target: 0.0,
            vvt_exhaust: 0.0,
            vvt_exhaust_target: 0.0,
            slip: 0.0,
            trip: 0.0,
            revolutions: 0.0,
            fuel_cut: false,
        }
    }

    pub(super) fn condition_fuel(&mut self, input: &Inputs) {
        self.fuel_litres = if input.get(&g::FUEL_VOLUME).is_some() {
            input.bounded(&g::FUEL_VOLUME, self.fuel_litres, 0.0, 55.0)
        } else if input.get(&g::FUEL_LEVEL).is_some() {
            input.bounded(&g::FUEL_LEVEL, 0.0, 0.0, 100.0) * 0.55
        } else {
            self.fuel_litres
        };
    }

    pub(super) fn idle_rpm(&self) -> f64 {
        850.0 + (80.0 - self.coolant).clamp(0.0, 60.0) * 4.0
    }

    pub(super) fn load(&self) -> f64 {
        self.map / self.baro
    }

    pub(super) fn idle_duty(&self) -> f64 {
        if self.tps < 12.0 && self.rpm < 1500.0 {
            25.0 + (80.0 - self.coolant).max(0.0) * 0.35
        } else {
            0.0
        }
    }

    pub(super) fn knock(&self) -> f64 {
        // Background sensor energy is not a detected knock event.
        (3.0 + self.rpm * 0.002 + self.load() * 6.0).min(60.0)
    }
}

pub(super) fn lag(value: f64, target: f64, dt: f64, tau: f64) -> f64 {
    value + (target - value) * -(-dt / tau).exp_m1()
}

pub(super) fn rpm_per_kmh(gear: u8) -> f64 {
    let ratio = [0.0, 3.50, 2.15, 1.48, 1.12, 0.90, 0.75][usize::from(gear.min(6))];
    ratio * 3.70 * 1000.0 / (60.0 * 1.98)
}
