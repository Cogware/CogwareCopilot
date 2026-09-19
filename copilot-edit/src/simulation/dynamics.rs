// SPDX-License-Identifier: GPL-3.0-only
//! Coupled drivetrain, gas path, fuelling and slow thermal evolution.

use super::Scenario;
use super::state::{Inputs, State, lag, rpm_per_kmh};
use cogware_can as g;

impl State {
    pub(super) fn tick(&mut self, dt: f64, scenario: Scenario, input: &Inputs) {
        let (old_speed, old_rpm, old_tps, old_map) = (self.speed, self.rpm, self.tps, self.map);
        self.time += dt;
        self.condition_fuel(input);
        self.baro = input.bounded(&g::BARO, 100.0, 60.0, 110.0);
        self.coolant = input.bounded(&g::CLNT, self.coolant, -30.0, 130.0);
        self.oil_temp = input.bounded(&g::OIL_TEMP, self.oil_temp, -30.0, 160.0);
        self.iat = input.bounded(&g::IAT, self.iat, -30.0, 100.0);
        let phase = self.time % 48.0;
        let (command, brake) = match scenario {
            Scenario::Idle => (0.0, 5.0),
            Scenario::Pull => (94.0, 0.0),
            Scenario::Overrun => (0.0, 0.0),
            Scenario::Road => match phase {
                p if p < 3.0 => (0.0, 3.0),
                p if p < 18.0 => (88.0, 0.0),
                p if p < 25.0 => (24.0, 0.0),
                p if p < 35.0 => (0.0, 1.4),
                p if p < 43.0 => (36.0, 0.0),
                _ => (0.0, 3.8),
            },
        };
        if input.get(&g::VSS).is_some() {
            self.speed = input.bounded(&g::VSS, self.speed, 0.0, 300.0);
            self.acceleration = lag(self.acceleration, input.speed_acceleration, dt, 0.30);
        }
        let demand = if input.get(&g::VSS).is_some() {
            if self.speed < 0.5 {
                0.0
            } else {
                (10.0 + self.speed * 0.16 + self.acceleration * 16.0).clamp(0.0, 100.0)
            }
        } else {
            command
        };
        let pedal_target = input.bounded(
            &g::PEDAL,
            input.bounded(&g::TPS, demand, 0.0, 100.0),
            0.0,
            100.0,
        );
        self.pedal = if input.get(&g::PEDAL).is_some() {
            pedal_target
        } else {
            lag(self.pedal, pedal_target, dt, 0.18)
        };
        let idle_blade = if self.speed < 3.0 {
            4.5 + (80.0 - self.coolant).max(0.0) * 0.025
        } else {
            0.0
        };
        let throttle_target = if self.shift_left > 0.0 {
            self.pedal * 0.40
        } else {
            self.pedal.max(idle_blade)
        };
        self.tps = input.bounded(
            &g::TPS,
            lag(self.tps, throttle_target, dt, 0.12),
            0.0,
            100.0,
        );
        self.shift_left = (self.shift_left - dt).max(0.0);
        self.select_gear(input);
        if input.get(&g::VSS).is_none() {
            let coupling = rpm_per_kmh(self.gear) / rpm_per_kmh(1);
            let torque = (0.55 + self.load() * 0.45) * self.tps / 100.0;
            let drive = if self.fuel_cut || self.fuel_litres <= 0.0 {
                0.0
            } else {
                7.2 * torque * coupling
            };
            let engine_brake = if self.tps < 6.0 && self.gear > 0 {
                0.35 + coupling * 0.5
            } else {
                0.0
            };
            let road_brake = if input.get(&g::TPS).is_some() || input.get(&g::PEDAL).is_some() {
                0.0
            } else {
                brake
            };
            let acceleration =
                drive - 0.12 - self.speed * self.speed * 0.000035 - engine_brake - road_brake;
            self.speed =
                if input.get(&g::RPM).is_some() && input.get(&g::GEAR).is_some() && self.gear > 0 {
                    input.bounded(&g::RPM, self.rpm, 0.0, 7200.0) / rpm_per_kmh(self.gear)
                } else {
                    (self.speed + acceleration * 3.6 * dt).clamp(0.0, 285.0)
                };
            self.acceleration = lag(
                self.acceleration,
                ((self.speed - old_speed) / (dt * 3.6)).clamp(-8.0, 8.0),
                dt,
                0.20,
            );
        }
        let engine_running = self.fuel_litres > 0.0;
        let idle = if engine_running { self.idle_rpm() } else { 0.0 };
        let launch = if self.gear == 1 && self.speed < 15.0 {
            self.tps * 17.0
        } else {
            0.0
        };
        let free_rev = if self.gear == 0 {
            (self.tps - idle_blade).max(0.0) * 42.0
        } else {
            0.0
        };
        let rpm_target = (self.speed * rpm_per_kmh(self.gear)).max(idle + launch + free_rev);
        self.rpm = input.bounded(&g::RPM, lag(self.rpm, rpm_target, dt, 0.14), 0.0, 7200.0);
        self.tps_dot = lag(
            self.tps_dot,
            ((self.tps - old_tps) / dt).clamp(-3000.0, 3000.0),
            dt,
            0.08,
        );
        self.rpm_dot = lag(
            self.rpm_dot,
            ((self.rpm - old_rpm) / dt).clamp(-30000.0, 30000.0),
            dt,
            0.08,
        );
        let spool = ((self.rpm - 1400.0) / 2600.0).clamp(0.0, 1.0);
        // A small turbo tapers boost near redline rather than sustaining peak torque.
        let taper = ((6800.0 - self.rpm) / 1500.0).clamp(0.65, 1.0);
        let boost_request = 85.0 * (self.tps / 100.0).powf(1.7) * spool * taper;
        self.boost_target = input.bounded(
            &g::BOOST_TARGET,
            self.baro + boost_request,
            self.baro,
            230.0,
        );
        self.boost = lag(self.boost, (self.boost_target - self.baro) * spool, dt, 0.8);
        let map_target = self.baro * (0.28 + 0.72 * self.tps / 100.0) + self.boost;
        self.map = input.bounded(&g::MAP, lag(self.map, map_target, dt, 0.10), 15.0, 250.0);
        if input.get(&g::MAP).is_some() {
            self.boost = (self.map - self.baro).max(0.0);
            if input.get(&g::BOOST_TARGET).is_none() {
                self.boost_target = self.baro.max(self.map);
            }
        }
        self.boost_pwm = if self.boost_target > self.baro + 1.0 {
            (22.0
                + (self.boost_target - self.baro) * 0.55
                + (self.boost_target - self.map).max(0.0) * 0.10)
                .clamp(0.0, 90.0)
        } else {
            0.0
        };
        self.map_dot = lag(
            self.map_dot,
            ((self.map - old_map) / dt).clamp(-3000.0, 3000.0),
            dt,
            0.10,
        );
        let overrun = self.tps < 2.0 && self.rpm > 1450.0 && self.speed > 8.0;
        self.fuel_cut = input.get(&g::FUEL_CUT).map_or(overrun, |cut| cut >= 99.0)
            || self.fuel_litres <= 0.0
            || self.rpm < 100.0;
        let warm = (80.0 - self.coolant).clamp(0.0, 60.0) / 60.0;
        self.afr_target = input.bounded(
            &g::AFR_TARGET,
            14.7 - warm - 2.5 * ((self.load() - 0.75) / 0.85).clamp(0.0, 1.0),
            9.0,
            20.0,
        );
        self.enrichment = 100.0 + self.tps_dot.clamp(0.0, 150.0) * 0.12;
        let mixture = if self.fuel_cut {
            22.4
        } else {
            self.afr_target
                + 0.18
                + 0.12 * (self.time * 3.4).sin()
                + 0.045 * (self.time * 9.7).sin()
                - (self.enrichment - 100.0) * 0.055
        };
        self.combustion_afr = input.bounded(
            &g::AFR_PRI,
            lag(self.combustion_afr, mixture, dt, 0.18),
            8.0,
            24.0,
        );
        let exhaust_delay = 0.16 + 130.0 / self.rpm.max(300.0);
        self.afr = input.bounded(
            &g::AFR_PRI,
            lag(self.afr, self.combustion_afr, dt, exhaust_delay),
            8.0,
            25.0,
        );
        self.afr_secondary = input.bounded(
            &g::AFR_SEC,
            lag(
                self.afr_secondary,
                self.afr + 0.08 + 0.025 * (self.time * 2.1).sin(),
                dt,
                0.28,
            ),
            8.0,
            25.0,
        );
        self.trim = if self.fuel_cut || self.load() > 1.05 {
            0.0
        } else {
            ((self.afr - self.afr_target) * 1.8 + (self.time * 0.7).sin() * 0.6).clamp(-5.0, 5.0)
        };
        self.warmup = 100.0 + warm * 35.0;
        self.afterstart = 100.0 + warm * 18.0 * (-self.time / 25.0).exp();
        self.thermal(dt, input);
        self.vvt_target = input.bounded(
            &g::VVT_TARGET_ANGLE,
            (self.tps * 0.30 * (1.0 - ((self.rpm - 4000.0) / 4000.0).max(0.0))).clamp(0.0, 32.0),
            0.0,
            45.0,
        );
        self.vvt = input.bounded(
            &g::VVT_ANGLE,
            lag(self.vvt, self.vvt_target, dt, 0.32),
            -10.0,
            50.0,
        );
        self.vvt_exhaust_target =
            input.bounded(&g::VVT_EXH_TARGET, -self.vvt_target * 0.40, -30.0, 10.0);
        self.vvt_exhaust = input.bounded(
            &g::VVT_EXH_ANGLE,
            lag(self.vvt_exhaust, self.vvt_exhaust_target, dt, 0.42),
            -35.0,
            15.0,
        );
        self.gps_speed = lag(self.gps_speed, self.speed, dt, 0.65);
        self.slip = if self.speed > 3.0 {
            self.acceleration.max(0.0) * self.tps / 100.0 * 0.55
        } else {
            0.0
        };
        self.derive(input);
        self.fuel_used += self.fuel_flow / 3.6 * dt;
        self.fuel_litres = (self.fuel_litres - self.fuel_rate / 3600.0 * dt).max(0.0);
        self.trip += (old_speed + self.speed) * 0.5 * dt / 3600.0;
        // 131072 revolutions contains an integer number of both 16-bit wraps.
        self.revolutions = (self.revolutions + (old_rpm + self.rpm) * 0.5 / 60.0 * dt) % 131072.0;
    }

    fn select_gear(&mut self, input: &Inputs) {
        if input.get(&g::GEAR).is_some() {
            self.gear = input.bounded(&g::GEAR, 0.0, 0.0, 6.0).round() as u8;
        } else if input.get(&g::RPM).is_some() && self.speed > 5.0 {
            let rpm = input.bounded(&g::RPM, self.rpm, 0.0, 7200.0);
            self.gear = (1..=6)
                .min_by(|a, b| {
                    (self.speed * rpm_per_kmh(*a) - rpm)
                        .abs()
                        .total_cmp(&(self.speed * rpm_per_kmh(*b) - rpm).abs())
                })
                .unwrap_or(1);
        } else if self.speed < 1.0 && self.pedal < 10.0 {
            self.gear = 0;
        } else if self.gear == 0 {
            self.gear = 1;
        } else if self.shift_left == 0.0 {
            let coupled_rpm = self.speed * rpm_per_kmh(self.gear);
            let shift_rpm = if self.tps < 45.0 { 3000.0 } else { 5600.0 };
            if coupled_rpm > shift_rpm && self.gear < 6 {
                self.gear += 1;
                self.shift_left = 0.28;
            } else if coupled_rpm < 1500.0 && self.gear > 1 {
                self.gear -= 1;
                self.shift_left = 0.20;
            }
        }
    }

    fn thermal(&mut self, dt: f64, input: &Inputs) {
        let load = self.load().clamp(0.0, 2.0);
        let running = self.rpm >= 100.0 && self.fuel_litres > 0.0;
        self.coolant = input.bounded(
            &g::CLNT,
            lag(
                self.coolant,
                if running { 88.0 + load * 5.0 } else { 20.0 },
                dt,
                95.0,
            ),
            -30.0,
            130.0,
        );
        self.oil_temp = input.bounded(
            &g::OIL_TEMP,
            lag(
                self.oil_temp,
                self.coolant + if running { 7.0 + load * 5.0 } else { 0.0 },
                dt,
                240.0,
            ),
            -30.0,
            160.0,
        );
        self.iat = input.bounded(
            &g::IAT,
            lag(
                self.iat,
                20.0 + (self.coolant - 20.0) * 0.12 + self.boost * 0.065,
                dt,
                8.0,
            ),
            -30.0,
            100.0,
        );
        self.fuel_temp = input.bounded(
            &g::FLEX_FUEL_TEMP,
            lag(
                self.fuel_temp,
                20.0 + (self.coolant - 20.0) * 0.16,
                dt,
                300.0,
            ),
            -30.0,
            85.0,
        );
        self.trans_temp = input.bounded(
            &g::TRANS_TEMP,
            lag(
                self.trans_temp,
                20.0 + (self.oil_temp - 20.0) * 0.70 + self.speed * 0.08,
                dt,
                350.0,
            ),
            -30.0,
            140.0,
        );
        self.diff_temp = input.bounded(
            &g::DIFF_TEMP,
            lag(
                self.diff_temp,
                20.0 + self.speed * 0.25 + load * 12.0,
                dt,
                450.0,
            ),
            -30.0,
            140.0,
        );
        self.ecu_temp = input.bounded(
            &g::ECU_TEMP,
            lag(
                self.ecu_temp,
                25.0 + (self.coolant - 20.0) * 0.24,
                dt,
                200.0,
            ),
            -30.0,
            100.0,
        );
        let exhaust = if self.rpm < 100.0 {
            20.0
        } else if self.fuel_cut {
            230.0
        } else {
            230.0 + self.rpm * 0.03 + load * 330.0
        };
        self.egt = input.bounded(&g::EGT1, lag(self.egt, exhaust, dt, 3.0), 20.0, 1050.0);
    }

    pub(super) fn derive(&mut self, input: &Inputs) {
        self.battery = input.bounded(
            &g::BAT_VOL,
            if self.rpm < 500.0 {
                12.6
            } else {
                14.1 - (self.rpm < 1100.0) as u8 as f64 * 0.18
            },
            9.0,
            16.0,
        );
        self.oil_pressure = input.bounded(
            &g::OIL_PRES,
            if self.rpm < 100.0 {
                0.0
            } else {
                (95.0 + self.rpm * 0.052 + (95.0 - self.oil_temp).max(0.0) * 2.4).min(620.0)
            },
            0.0,
            750.0,
        );
        self.ve = input.bounded(
            &g::VE,
            (74.0 + self.tps * 0.16 + self.vvt * 0.14
                - ((self.rpm - 5000.0) / 1000.0).max(0.0) * 3.0)
                .clamp(65.0, 99.0),
            20.0,
            120.0,
        );
        // Ideal-gas charge in one 500 cc cylinder; four strokes take two revolutions.
        self.air_mass =
            self.map * 1000.0 * 0.0005 / (287.05 * (self.iat + 273.15)) * 1000.0 * self.ve / 100.0;
        self.airflow = self.air_mass * self.rpm / 120.0 * 4.0 * 3.6;
        self.fuel_flow = if self.fuel_cut {
            0.0
        } else {
            self.airflow / self.combustion_afr
        };
        // Petrol density 0.745 kg/L, four 500 cc/min injectors, voltage-linked dead time.
        self.fuel_rate = self.fuel_flow / 0.745;
        let dead_time = 0.75 * 14.0 / self.battery;
        self.pulse = if self.fuel_cut {
            0.0
        } else {
            self.air_mass / self.combustion_afr / (500.0 * 0.745 / 60.0) * 1000.0 + dead_time
        };
        self.injector_duty = self.pulse * self.rpm / 1200.0;
        self.dwell = 2.8 * 14.0 / self.battery;
        self.spark = input.bounded(
            &g::CUR_SPARK_ADVANCE,
            if self.fuel_cut {
                8.0
            } else {
                (12.0 + self.rpm * 0.004
                    - (self.load() - 0.45).max(0.0) * 17.0
                    - (self.iat - 45.0).max(0.0) * 0.12)
                    .clamp(2.0, 38.0)
            },
            -15.0,
            50.0,
        );
    }
}
