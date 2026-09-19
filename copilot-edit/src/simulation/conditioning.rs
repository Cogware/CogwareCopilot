// SPDX-License-Identifier: GPL-3.0-only
//! First recorded samples establish history, not instantaneous physical derivatives.

use super::state::{Inputs, State, rpm_per_kmh};
use cogware_can::{self as g, GaugeData};

impl State {
    pub(super) fn seed_recorded(&mut self, next: &Inputs, previous: &Inputs) {
        let first = |gauge: &GaugeData| next.raw(gauge).is_some() && previous.raw(gauge).is_none();
        let startup = self.time == 0.0;
        let fresh = [
            &g::RPM,
            &g::VSS,
            &g::GEAR,
            &g::TPS,
            &g::PEDAL,
            &g::MAP,
            &g::BARO,
            &g::CLNT,
            &g::IAT,
            &g::OIL_TEMP,
            &g::AFR_TARGET,
            &g::AFR_PRI,
            &g::AFR_SEC,
            &g::BAT_VOL,
            &g::FUEL_CUT,
            &g::FUEL_LEVEL,
            &g::FUEL_VOLUME,
        ]
        .into_iter()
        .any(first);
        if !fresh && !startup {
            return;
        }
        self.condition_fuel(next);
        macro_rules! seed {
            ($field:ident, $gauge:ident, $low:expr, $high:expr) => {
                if first(&g::$gauge) {
                    self.$field = next.bounded(&g::$gauge, self.$field, $low, $high);
                }
            };
        }
        seed!(baro, BARO, 60.0, 110.0);
        seed!(coolant, CLNT, -30.0, 130.0);
        seed!(iat, IAT, -30.0, 100.0);
        seed!(oil_temp, OIL_TEMP, -30.0, 160.0);
        seed!(rpm, RPM, 0.0, 7200.0);
        seed!(speed, VSS, 0.0, 300.0);
        seed!(tps, TPS, 0.0, 100.0);
        seed!(pedal, PEDAL, 0.0, 100.0);
        seed!(map, MAP, 15.0, 250.0);
        if first(&g::TPS) {
            self.tps_dot = 0.0;
            if next.get(&g::PEDAL).is_none() {
                self.pedal = self.tps;
            }
        }
        if first(&g::RPM) {
            self.rpm_dot = 0.0;
        }
        if first(&g::VSS) || first(&g::RPM) || first(&g::GEAR) {
            self.seed_drivetrain(next);
            if first(&g::VSS) && next.get(&g::RPM).is_none() {
                self.rpm = (self.speed * rpm_per_kmh(self.gear)).max(self.idle_rpm());
                self.rpm_dot = 0.0;
                if next.get(&g::TPS).is_none() && next.get(&g::PEDAL).is_none() {
                    self.pedal = if self.speed < 0.5 {
                        0.0
                    } else {
                        10.0 + self.speed * 0.16
                    };
                    self.tps = self.pedal.max(4.5);
                    self.tps_dot = 0.0;
                }
            }
            self.gps_speed = next.bounded(&g::GPS_SPEED, self.speed, 0.0, 300.0);
            self.acceleration = 0.0;
        }
        if first(&g::MAP) {
            self.boost = (self.map - self.baro).max(0.0);
            self.boost_target =
                next.bounded(&g::BOOST_TARGET, self.baro.max(self.map), self.baro, 250.0);
            self.map_dot = 0.0;
        } else if startup && next.get(&g::MAP).is_none() {
            self.map = self.baro * (0.28 + 0.72 * self.tps / 100.0) + self.boost;
            self.map_dot = 0.0;
        }
        let warm = (80.0 - self.coolant).clamp(0.0, 60.0) / 60.0;
        self.afr_target = next.bounded(
            &g::AFR_TARGET,
            14.7 - warm - 2.5 * ((self.load() - 0.75) / 0.85).clamp(0.0, 1.0),
            9.0,
            20.0,
        );
        self.fuel_cut = next.get(&g::FUEL_CUT).map_or(
            self.tps < 2.0 && self.rpm > 1450.0 && self.speed > 8.0,
            |cut| cut >= 99.0,
        ) || self.fuel_litres <= 0.0
            || self.rpm < 100.0;
        if startup {
            self.combustion_afr = if self.fuel_cut {
                22.4
            } else {
                self.afr_target + 0.18
            };
            self.afr = self.combustion_afr + 0.03;
            self.afr_secondary = self.afr + 0.08;
        }
        if first(&g::AFR_PRI) {
            self.afr = next.bounded(&g::AFR_PRI, self.afr, 8.0, 25.0);
            self.combustion_afr = self.afr.clamp(8.0, 24.0);
        }
        seed!(afr_secondary, AFR_SEC, 8.0, 25.0);
        self.warmup = 100.0 + warm * 35.0;
        self.afterstart = 100.0 + warm * 18.0 * (-self.time / 25.0).exp();
        self.derive(next);
    }

    fn seed_drivetrain(&mut self, next: &Inputs) {
        if next.get(&g::GEAR).is_some() {
            self.gear = next.bounded(&g::GEAR, 0.0, 0.0, 6.0).round() as u8;
        } else if self.speed < 0.5 {
            self.gear = 0;
        } else {
            let rpm = next.bounded(&g::RPM, 2700.0, 0.0, 7200.0);
            self.gear = (1..=6)
                .min_by(|a, b| {
                    (self.speed * rpm_per_kmh(*a) - rpm)
                        .abs()
                        .total_cmp(&(self.speed * rpm_per_kmh(*b) - rpm).abs())
                })
                .unwrap_or(1);
        }
        if next.get(&g::VSS).is_none()
            && next.get(&g::RPM).is_some()
            && next.get(&g::GEAR).is_some()
            && self.gear > 0
        {
            self.speed = self.rpm / rpm_per_kmh(self.gear);
        }
    }
}
