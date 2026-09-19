// SPDX-License-Identifier: GPL-3.0-only
//! Behaviour, dimensional consistency and full-catalogue regression tests.

use super::*;

pub(super) fn physical(engine: &Engine, gauge: &GaugeData) -> f64 {
    f64::from(engine.reading(gauge.id).expect(gauge.name)) / f64::from(gauge.unit.scale)
}

pub(super) fn run(engine: &mut Engine, seconds: usize, samples: &[(u16, i32)]) {
    for _ in 0..seconds * 10 {
        engine.advance(100_000, samples);
    }
}

pub(super) fn assert_catalogue(engine: &Engine) {
    let mut excluded = 0;
    for gauge in g::ALL_GAUGES {
        let source = provenance(gauge.id).expect(gauge.name);
        let channel = channels::channel(Gauge::from_id(gauge.id).unwrap());
        if [g::INDICATORS1.id, g::INDICATORS2.id, g::MASTERALIVE.id].contains(&gauge.id) {
            assert_eq!(source, Provenance::Excluded);
            assert_eq!(engine.reading(gauge.id), None);
            excluded += 1;
            continue;
        }
        assert_ne!(source, Provenance::Excluded, "{}", gauge.name);
        assert!(
            channel.value(&engine.state).unwrap().is_finite(),
            "{}",
            gauge.name
        );
        let raw = engine.reading(gauge.id).expect(gauge.name);
        assert_ne!(raw, g::NO_DATA, "{}", gauge.name);
        let valid = match gauge.wire {
            Wire::U8 => u8::try_from(raw).is_ok(),
            Wire::I8 => i8::try_from(raw).is_ok(),
            Wire::U16 => u16::try_from(raw).is_ok(),
            Wire::I16 => i16::try_from(raw).is_ok(),
            Wire::I32 => true,
        };
        assert!(valid, "{} = {raw}", gauge.name);
    }
    assert_eq!(excluded, 3);
}

#[test]
fn every_catalogue_channel_is_explicit_finite_and_wire_safe() {
    for cold in [false, true] {
        for scenario in [
            Scenario::Road,
            Scenario::Idle,
            Scenario::Pull,
            Scenario::Overrun,
        ] {
            let mut engine = Engine::new(cold);
            engine.scenario = scenario;
            assert_catalogue(&engine);
            for _ in 0..120 {
                engine.advance(500_000, &[]);
                assert_catalogue(&engine);
            }
        }
    }
}

#[test]
fn actual_afr_has_bounded_sustained_sensor_variation_not_target_aliasing() {
    let mut engine = Engine {
        scenario: Scenario::Idle,
        ..Engine::default()
    };
    run(&mut engine, 5, &[]);
    let (mut low, mut high, mut differing) = (f64::MAX, f64::MIN, 0);
    for _ in 0..300 {
        engine.advance(50_000, &[]);
        let actual = physical(&engine, &g::AFR_PRI);
        let target = physical(&engine, &g::AFR_TARGET);
        low = low.min(actual);
        high = high.max(actual);
        differing += usize::from(engine.reading(g::AFR_PRI.id) != engine.reading(g::AFR_TARGET.id));
        assert!((actual - target).abs() < 0.7);
        assert!(physical(&engine, &g::EGO_CORRECT).abs() < 5.0);
    }
    assert!(differing > 290);
    assert!(high - low > 0.07 && high - low < 0.6, "{low}..{high}");
    assert_ne!(engine.reading(g::AFR_PRI.id), engine.reading(g::AFR_SEC.id));
}

#[test]
fn recorded_target_step_propagates_through_transport_and_sensor_lag() {
    let mut engine = Engine::default();
    let mut input = [
        (g::RPM.id, 2800),
        (g::MAP.id, 650),
        (g::TPS.id, 300),
        (g::AFR_TARGET.id, 1470),
    ];
    run(&mut engine, 4, &input);
    let previous = physical(&engine, &g::AFR_PRI);
    input[3].1 = 1250;
    engine.advance(100_000, &input);
    let transient = physical(&engine, &g::AFR_PRI);
    assert_eq!(engine.reading(g::AFR_TARGET.id), Some(1250));
    assert!(
        transient < previous && transient > 13.5,
        "{previous} -> {transient}"
    );
    run(&mut engine, 3, &input);
    let settled = physical(&engine, &g::AFR_PRI);
    assert!(settled > 12.4 && settled < 13.0, "{settled}");
}

#[test]
fn idle_pull_shifts_and_overrun_form_coherent_operating_points() {
    let mut engine = Engine {
        scenario: Scenario::Idle,
        ..Engine::default()
    };
    run(&mut engine, 3, &[]);
    assert_eq!(physical(&engine, &g::VSS), 0.0);
    assert!(physical(&engine, &g::RPM) > 800.0 && physical(&engine, &g::RPM) < 1200.0);
    assert!(physical(&engine, &g::MAP) < 40.0);
    assert_eq!(physical(&engine, &g::GEAR), 0.0);
    engine.scenario = Scenario::Pull;
    let (mut max_rpm, mut shifted, mut rpm_fell) = (0.0_f64, false, false);
    let (mut previous_rpm, mut previous_gear, mut previous_speed) = (0.0, 0.0, 0.0);
    for _ in 0..250 {
        engine.advance(100_000, &[]);
        let rpm = physical(&engine, &g::RPM);
        let gear = physical(&engine, &g::GEAR);
        let speed = physical(&engine, &g::VSS);
        max_rpm = max_rpm.max(rpm);
        shifted |= gear > previous_gear && previous_gear > 0.0;
        rpm_fell |= shifted && rpm < previous_rpm - 150.0 && speed >= previous_speed;
        (previous_rpm, previous_gear, previous_speed) = (rpm, gear, speed);
    }
    assert!(max_rpm > 5000.0, "{max_rpm}");
    assert!(shifted && rpm_fell);
    assert!(physical(&engine, &g::VSS) > 90.0);
    assert!(physical(&engine, &g::MAP) > physical(&engine, &g::BARO));
    assert!(physical(&engine, &g::AFR_TARGET) < 13.8);
    engine.scenario = Scenario::Overrun;
    run(&mut engine, 4, &[]);
    assert_eq!(physical(&engine, &g::FUEL_CUT), 100.0);
    for gauge in [
        &g::PULSE_WIDTH1,
        &g::PULSE_WIDTH2,
        &g::PULSE_WIDTH3,
        &g::PULSE_WIDTH4,
        &g::FUEL_FLOW,
        &g::FUEL_RATE,
        &g::INJ_DUTY,
    ] {
        assert_eq!(physical(&engine, gauge), 0.0, "{}", gauge.name);
    }
    assert!(physical(&engine, &g::AFR_PRI) > 21.0);
    assert!(physical(&engine, &g::AFR_TARGET) < 16.0);
    assert!(physical(&engine, &g::ACCEL_FWD) < 0.0);
}

#[test]
fn coolant_warms_before_oil_and_warm_start_is_ready() {
    let mut cold = Engine::new(true);
    let warm = Engine::default();
    cold.scenario = Scenario::Idle;
    assert!(physical(&warm, &g::CLNT) > 85.0);
    assert!(physical(&warm, &g::OIL_TEMP) > 90.0);
    assert!(physical(&cold, &g::OIL_PRES) > physical(&warm, &g::OIL_PRES) + 120.0);
    run(&mut cold, 90, &[]);
    assert!(physical(&cold, &g::CLNT) > 55.0);
    assert!(physical(&cold, &g::CLNT) > physical(&cold, &g::OIL_TEMP) + 15.0);
    assert!(physical(&cold, &g::OIL_TEMP) > 20.0);
    run(&mut cold, 900, &[]);
    assert!(physical(&cold, &g::OIL_TEMP) > 88.0);
    assert!(physical(&cold, &g::WUE_CORRECT) < 101.0);
}

#[test]
fn oil_pressure_tracks_pump_speed_and_viscosity() {
    let mut engine = Engine::default();
    run(&mut engine, 1, &[(g::RPM.id, 1000), (g::OIL_TEMP.id, 950)]);
    let idle = physical(&engine, &g::OIL_PRES);
    run(&mut engine, 1, &[(g::RPM.id, 4000), (g::OIL_TEMP.id, 950)]);
    let fast = physical(&engine, &g::OIL_PRES);
    run(&mut engine, 1, &[(g::RPM.id, 4000), (g::OIL_TEMP.id, 200)]);
    let cold = physical(&engine, &g::OIL_PRES);
    assert!(fast > idle + 140.0);
    assert!(cold > fast + 150.0 && cold <= 620.0);
}

#[test]
fn manifold_and_pneumatic_pressures_use_consistent_references() {
    let mut engine = Engine {
        scenario: Scenario::Pull,
        ..Engine::default()
    };
    run(&mut engine, 15, &[]);
    let map = physical(&engine, &g::MAP);
    let baro = physical(&engine, &g::BARO);
    let target = physical(&engine, &g::BOOST_TARGET);
    assert!(map > baro + 20.0 && target >= map - 2.0);
    assert!((physical(&engine, &g::WASTEGATE_PRES) - (map - baro)).abs() <= 0.11);
    assert!((physical(&engine, &g::FUEL_PRES) - (300.0 + map - baro)).abs() <= 0.11);
    assert!(physical(&engine, &g::BOOST_PWM) > 30.0);
}

#[test]
fn air_charge_fuel_flow_pulse_duty_and_density_have_correct_dimensions() {
    let mut engine = Engine::default();
    let input = [
        (g::RPM.id, 3000),
        (g::MAP.id, 1400),
        (g::IAT.id, 300),
        (g::TPS.id, 700),
    ];
    run(&mut engine, 5, &input);
    let air = physical(&engine, &g::CYL_AIRMASS);
    let ideal = 140000.0 * 0.0005 / (287.05 * 303.15) * 1000.0 * physical(&engine, &g::VE) / 100.0;
    assert!((air - ideal).abs() < 0.001);
    let airflow = physical(&engine, &g::AIRFLOW);
    assert!((airflow - air * 3000.0 / 120.0 * 4.0 * 3.6).abs() < 0.25);
    let fuel = physical(&engine, &g::FUEL_FLOW);
    assert!((airflow / fuel - physical(&engine, &g::AFR_PRI)).abs() < 0.3);
    assert!((physical(&engine, &g::FUEL_RATE) * 0.745 - fuel).abs() < 0.015);
    let pulse = physical(&engine, &g::PULSE_WIDTH1);
    let dead_time = 0.75 * 14.0 / physical(&engine, &g::BAT_VOL);
    let mass_flow =
        (pulse - dead_time) / 1000.0 * (500.0 * 0.745 / 60.0) * (3000.0 / 120.0) * 4.0 * 3.6;
    assert!((mass_flow - fuel).abs() < 0.02);
    assert!((physical(&engine, &g::INJ_DUTY) - pulse * 3000.0 / 1200.0).abs() < 0.06);
    assert!(pulse > 2.0 && pulse < 15.0);
    assert!(physical(&engine, &g::EGO_CORRECT).abs() < 5.0);
    assert!(physical(&engine, &g::FLEX_CORRECT) == 100.0);
}

#[test]
#[should_panic(expected = "non-finite simulated reading for RPM")]
fn invalid_internal_math_is_not_silently_encoded_as_a_healthy_zero() {
    encode(&g::RPM, f64::NAN);
}
