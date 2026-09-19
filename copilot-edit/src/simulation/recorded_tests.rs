// SPDX-License-Identifier: GPL-3.0-only
//! Recorded-data conditioning, time partition and auxiliary-channel tests.

use super::tests::{assert_catalogue, physical, run};
use super::*;

#[test]
fn derivative_channels_and_cam_actuators_follow_steps_then_settle() {
    let mut engine = Engine::default();
    run(
        &mut engine,
        2,
        &[(g::RPM.id, 1200), (g::MAP.id, 400), (g::TPS.id, 50)],
    );
    let step = [(g::RPM.id, 3500), (g::MAP.id, 1500), (g::TPS.id, 850)];
    engine.advance(10_000, &step);
    assert!(physical(&engine, &g::TPS_DOT) > 100.0);
    assert!(physical(&engine, &g::RPM_DOT) > 1000.0);
    assert!(physical(&engine, &g::MAP_DOT) > 100.0);
    assert!(physical(&engine, &g::ACCEL_ENRICH) > 110.0);
    assert!(physical(&engine, &g::VVT_ANGLE) < physical(&engine, &g::VVT_TARGET_ANGLE) - 5.0);
    run(&mut engine, 3, &step);
    assert!(physical(&engine, &g::TPS_DOT).abs() < 0.1);
    assert!(physical(&engine, &g::RPM_DOT).abs() < 1.0);
    assert!(
        (physical(&engine, &g::VVT_ANGLE) - physical(&engine, &g::VVT_TARGET_ANGLE)).abs() < 0.2
    );
    assert!(physical(&engine, &g::VVT_EXH_ANGLE) < 0.0);
    assert_ne!(engine.reading(g::EGT1.id), engine.reading(g::EGT2.id));
    assert_eq!(engine.reading(g::KNOCK_COUNT.id), Some(0));
}

#[test]
fn voltage_affects_dwell_and_dead_time_without_synthetic_faults() {
    let mut engine = Engine::default();
    let mut input = [(g::RPM.id, 2000), (g::MAP.id, 650), (g::BAT_VOL.id, 14000)];
    run(&mut engine, 4, &input);
    let dwell = physical(&engine, &g::DWELL);
    let pulse = physical(&engine, &g::PULSE_WIDTH1);
    input[2].1 = 11000;
    run(&mut engine, 1, &input);
    assert!(physical(&engine, &g::DWELL) > dwell * 1.2);
    assert!(physical(&engine, &g::PULSE_WIDTH1) > pulse + 0.1);
    assert!(physical(&engine, &g::BAT_CORRECT) > 120.0);
    for gauge in [
        &g::NEXT_ERROR,
        &g::ERROR_COUNT,
        &g::LOST_SYNC,
        &g::MISS_COUNT,
        &g::ENG_PROTECT_STA,
    ] {
        assert_eq!(physical(&engine, gauge), 0.0);
        assert_eq!(provenance(gauge.id), Some(Provenance::Nominal));
    }
}

#[test]
fn frame_partition_does_not_change_fuel_distance_counters_or_noise() {
    let mut fine = Engine::default();
    let mut coarse = Engine::default();
    let mut irregular = Engine::default();
    for _ in 0..6000 {
        fine.advance(10_000, &[]);
    }
    for _ in 0..60 {
        coarse.advance(1_000_000, &[]);
    }
    for _ in 0..1000 {
        irregular.advance(7_111, &[]);
        irregular.advance(19_333, &[]);
        irregular.advance(33_556, &[]);
    }
    for gauge in g::ALL_GAUGES {
        assert_eq!(
            fine.reading(gauge.id),
            coarse.reading(gauge.id),
            "{}",
            gauge.name
        );
        assert_eq!(
            fine.reading(gauge.id),
            irregular.reading(gauge.id),
            "{}",
            gauge.name
        );
    }
    assert!(physical(&fine, &g::FUEL_USED) > 10.0);
    assert!(physical(&fine, &g::TRIP_DIST) > 0.5);
    assert!(physical(&fine, &g::FUEL_VOLUME) < 42.0);
}

#[test]
fn recorded_speed_controls_drivetrain_and_retains_exact_samples() {
    let mut engine = Engine::default();
    for step in 0..200 {
        engine.advance(100_000, &[(g::VSS.id, step * 5)]);
    }
    run(&mut engine, 5, &[(g::VSS.id, 1000)]);
    assert_eq!(engine.reading(g::VSS.id), Some(1000));
    assert!(physical(&engine, &g::GEAR) >= 3.0);
    assert!(physical(&engine, &g::RPM) > 1800.0 && physical(&engine, &g::RPM) < 5700.0);
    assert!((physical(&engine, &g::GPS_SPEED) - 100.0).abs() < 0.2);
    assert!((physical(&engine, &g::WHEEL_SPEED_DRIVEN) - 100.0).abs() < 0.2);
    let input = [
        (g::VSS.id, 833),
        (g::RPM.id, 4200),
        (g::TPS.id, 180),
        (g::PEDAL.id, 900),
        (g::AFR_TARGET.id, 1317),
    ];
    run(&mut engine, 3, &input);
    for (id, raw) in input {
        assert_eq!(engine.reading(id), Some(raw));
    }
    assert!(
        physical(&engine, &g::MAP) < 75.0,
        "TPS must control load before PEDAL"
    );
    assert!(physical(&engine, &g::AFR_PRI) > 13.0 && physical(&engine, &g::AFR_PRI) < 13.8);
    assert!(physical(&engine, &g::OIL_PRES) > 290.0);
}

#[test]
fn constant_recorded_speed_integrates_real_distance_and_fuel() {
    let mut engine = Engine::default();
    let samples = [(g::VSS.id, 900), (g::RPM.id, 2500), (g::TPS.id, 220)];
    run(&mut engine, 120, &samples);
    assert!((physical(&engine, &g::TRIP_DIST) - 3.0).abs() < 0.06);
    assert!((physical(&engine, &g::ODOMETER) - 12403.0).abs() < 0.06);
    let used = physical(&engine, &g::FUEL_USED);
    assert!(used > 40.0);
    assert!((physical(&engine, &g::FUEL_VOLUME) - (42.0 - used / 745.0)).abs() < 0.06);
    assert!(
        (physical(&engine, &g::FUEL_LEVEL) - physical(&engine, &g::FUEL_VOLUME) / 55.0 * 100.0)
            .abs()
            < 0.15
    );
}

#[test]
fn zero_delta_reset_and_extreme_gaps_are_bounded_and_deterministic() {
    let mut engine = Engine::new(true);
    engine.scenario = Scenario::Pull;
    run(&mut engine, 4, &[]);
    let before: Vec<_> = g::ALL_GAUGES.iter().map(|g| engine.reading(g.id)).collect();
    engine.advance(0, &[(g::RPM.id, 100)]);
    let after: Vec<_> = g::ALL_GAUGES.iter().map(|g| engine.reading(g.id)).collect();
    assert_eq!(before, after);
    engine.reset(false);
    assert_eq!(engine.scenario, Scenario::Pull);
    assert_eq!(engine.reading(g::FUEL_USED.id), Some(0));
    assert_eq!(engine.reading(g::TRIP_DIST.id), Some(0));
    assert_eq!(engine.reading(g::TRIGGER_COUNT.id), Some(0));
    assert_eq!(engine.state.time, 0.0);
    for dt in [1, 9_999, 200_000, 57_000_000, 7_200_000_000, u64::MAX] {
        engine.advance(dt, &[]);
        assert_catalogue(&engine);
    }
}

#[test]
fn recorded_nonphysical_inputs_are_sanitised_only_inside_engine() {
    let mut engine = Engine::default();
    let samples = [
        (g::BARO.id, 0),
        (g::IAT.id, -2732),
        (g::RPM.id, 65000),
        (g::TPS.id, 50000),
        (g::AFR_TARGET.id, 1),
    ];
    run(&mut engine, 2, &samples);
    for (id, raw) in samples {
        assert_eq!(engine.reading(id), Some(raw));
    }
    assert_catalogue(&engine);
    assert!(engine.state.baro >= 60.0);
    assert!(engine.state.iat >= -30.0);
    assert!(engine.state.rpm <= 7200.0);
    assert!(engine.state.afr_target >= 9.0);
    engine.advance(
        100_000,
        &[
            (g::RPM.id, g::NO_DATA),
            (g::INDICATORS1.id, 123),
            (g::MASTERALIVE.id, 1),
        ],
    );
    assert_ne!(engine.reading(g::RPM.id), Some(g::NO_DATA));
    assert_eq!(engine.reading(g::INDICATORS1.id), None);
    assert_eq!(engine.reading(g::MASTERALIVE.id), None);
    assert_eq!(engine.reading(0xffff), None);
    assert_eq!(provenance(0xffff), None);
}

#[test]
fn trigger_and_home_counters_wrap_at_their_wire_widths() {
    let mut engine = Engine::default();
    engine.advance(10_000, &[(g::RPM.id, 6000)]);
    let start = engine.state.revolutions;
    run(&mut engine, 12, &[(g::RPM.id, 6000)]);
    let expected = ((start + 1200.0) * 58.0).floor() as u64 % 65536;
    assert_eq!(engine.reading(g::TRIGGER_COUNT.id), Some(expected as i32));
    assert_eq!(
        engine.reading(g::HOME_COUNT.id),
        Some(((start + 1200.0) / 2.0).floor() as i32)
    );
    assert!(physical(&engine, &g::TRIG_SINCE_HOME) < 116.0);
}

#[test]
fn standalone_simulator_never_writes_global_gauges() {
    let before: Vec<_> = g::ALL_GAUGES.iter().map(|g| g.get()).collect();
    let mut engine = Engine::default();
    run(&mut engine, 50, &[]);
    for (gauge, previous) in g::ALL_GAUGES.iter().zip(before) {
        assert_eq!(gauge.get(), previous, "{}", gauge.name);
    }
    assert!(!Scenario::Road.label().is_empty());
}

#[test]
fn speed_log_acceleration_is_not_a_per_frame_impulse() {
    let mut fine = Engine::default();
    let mut coarse = Engine::default();
    fine.advance(10_000, &[(g::VSS.id, 0)]);
    coarse.advance(10_000, &[(g::VSS.id, 0)]);
    for step in 1..=1000 {
        fine.advance(20_000, &[(g::VSS.id, step)]);
        if step % 5 == 0 {
            coarse.advance(100_000, &[(g::VSS.id, step)]);
        }
    }
    for engine in [&fine, &coarse] {
        assert!((physical(engine, &g::ACCEL_FWD) - 0.14).abs() < 0.015);
        assert!(physical(engine, &g::TPS) > 44.0);
    }
    assert!((physical(&fine, &g::TPS) - physical(&coarse, &g::TPS)).abs() < 1.0);
    assert!((physical(&fine, &g::MAP) - physical(&coarse, &g::MAP)).abs() < 2.0);
}

#[test]
fn recorded_gear_and_rpm_condition_speed_and_recorded_afr_conditions_fuel() {
    let mut engine = Engine::default();
    let samples = [
        (g::RPM.id, 3000),
        (g::GEAR.id, 4),
        (g::TPS.id, 300),
        (g::MAP.id, 800),
        (g::AFR_PRI.id, 1250),
    ];
    run(&mut engine, 4, &samples);
    assert!((physical(&engine, &g::VSS) - 3000.0 / state::rpm_per_kmh(4)).abs() < 0.1);
    assert!(
        (physical(&engine, &g::AIRFLOW) / physical(&engine, &g::FUEL_FLOW) - 12.5).abs() < 0.02
    );
    assert_eq!(engine.reading(g::AFR_PRI.id), Some(1250));
}

#[test]
fn sustained_pull_does_not_saturate_airflow_or_injectors() {
    let mut engine = Engine {
        scenario: Scenario::Pull,
        ..Engine::default()
    };
    for _ in 0..1800 {
        engine.advance(100_000, &[]);
        let air = physical(&engine, &g::CYL_AIRMASS);
        let rpm = physical(&engine, &g::RPM);
        let flow = physical(&engine, &g::AIRFLOW);
        assert!(
            (flow - air * rpm / 120.0 * 4.0 * 3.6).abs() < 0.5,
            "{rpm} rpm, {air} g, {flow} kg/h"
        );
        assert!(physical(&engine, &g::INJ_DUTY) < 90.0);
    }
}

#[test]
fn explicit_recorded_full_fuel_cut_controls_missing_fuelling_channels() {
    let mut engine = Engine::default();
    let mut samples = [
        (g::RPM.id, 3500),
        (g::MAP.id, 1500),
        (g::TPS.id, 900),
        (g::FUEL_CUT.id, 1000),
    ];
    run(&mut engine, 3, &samples);
    assert_eq!(physical(&engine, &g::FUEL_FLOW), 0.0);
    assert_eq!(physical(&engine, &g::PULSE_WIDTH1), 0.0);
    assert!(physical(&engine, &g::AFR_PRI) > 21.0);
    samples[3].1 = 0;
    run(&mut engine, 3, &samples);
    assert!(physical(&engine, &g::FUEL_FLOW) > 20.0);
    assert!(physical(&engine, &g::AFR_PRI) < 14.0);
}

#[test]
fn one_microsecond_recorded_seed_initialises_values_without_derivative_spikes() {
    let mut engine = Engine::default();
    engine.reset(true);
    let samples = [
        (g::RPM.id, 4321),
        (g::VSS.id, 833),
        (g::TPS.id, 650),
        (g::MAP.id, 1380),
        (g::BARO.id, 1000),
        (g::CLNT.id, 870),
        (g::IAT.id, 320),
        (g::OIL_TEMP.id, 950),
        (g::AFR_TARGET.id, 1280),
    ];
    engine.advance(1, &samples);
    for &(id, raw) in &samples {
        assert_eq!(engine.reading(id), Some(raw));
    }
    assert_eq!(engine.state.time, 0.0);
    assert_eq!(physical(&engine, &g::FUEL_USED), 0.0);
    assert_eq!(physical(&engine, &g::TRIP_DIST), 0.0);
    assert!((physical(&engine, &g::AIRFLOW) - 344.74).abs() < 0.1);
    assert!(physical(&engine, &g::FUEL_FLOW) > 25.0);
    assert!(physical(&engine, &g::AFR_PRI) > 12.8 && physical(&engine, &g::AFR_PRI) < 13.2);
    assert_eq!(physical(&engine, &g::WUE_CORRECT), 100.0);
    for gauge in [&g::RPM_DOT, &g::TPS_DOT, &g::MAP_DOT, &g::ACCEL_FWD] {
        assert_eq!(physical(&engine, gauge), 0.0, "{}", gauge.name);
    }
    engine.advance(9_999, &samples);
    for gauge in [&g::RPM_DOT, &g::TPS_DOT, &g::MAP_DOT, &g::ACCEL_FWD] {
        assert_eq!(physical(&engine, gauge), 0.0, "{}", gauge.name);
    }
}

#[test]
fn newly_available_recorded_channels_seed_once_then_report_real_derivatives() {
    let mut engine = Engine::default();
    run(&mut engine, 5, &[]);
    let samples = [(g::RPM.id, 4321), (g::TPS.id, 650), (g::MAP.id, 1380)];
    engine.advance(1, &samples);
    engine.advance(9_999, &samples);
    for gauge in [&g::RPM_DOT, &g::TPS_DOT, &g::MAP_DOT] {
        assert_eq!(physical(&engine, gauge), 0.0, "{}", gauge.name);
    }
    engine.advance(
        10_000,
        &[(g::RPM.id, 4421), (g::TPS.id, 700), (g::MAP.id, 1430)],
    );
    assert!(physical(&engine, &g::RPM_DOT) > 500.0);
    assert!(physical(&engine, &g::TPS_DOT) > 20.0);
    assert!(physical(&engine, &g::MAP_DOT) > 20.0);
}

#[test]
fn sub_tick_seed_and_first_full_frame_establish_identical_history() {
    let mut seeded = Engine::default();
    let mut direct = Engine::default();
    let samples = [
        (g::RPM.id, 4321),
        (g::VSS.id, 833),
        (g::TPS.id, 650),
        (g::MAP.id, 1380),
        (g::AFR_TARGET.id, 1280),
    ];
    seeded.advance(1, &samples);
    seeded.advance(99_999, &samples);
    direct.advance(100_000, &samples);
    for gauge in g::ALL_GAUGES {
        assert_eq!(
            seeded.reading(gauge.id),
            direct.reading(gauge.id),
            "{}",
            gauge.name
        );
    }
}

#[test]
fn an_empty_recorded_tank_cuts_fuel_in_the_first_preview_snapshot() {
    for gauge in [&g::FUEL_LEVEL, &g::FUEL_VOLUME] {
        let mut engine = Engine::default();
        engine.advance(1, &[(gauge.id, 0)]);
        assert_eq!(physical(&engine, &g::FUEL_CUT), 100.0);
        for output in [&g::PULSE_WIDTH1, &g::INJ_DUTY, &g::FUEL_FLOW, &g::FUEL_RATE] {
            assert_eq!(physical(&engine, output), 0.0, "{}", output.name);
        }
        assert_eq!(physical(&engine, &g::FUEL_LEVEL), 0.0);
        assert_eq!(physical(&engine, &g::FUEL_VOLUME), 0.0);
    }
}

#[test]
fn a_recorded_stopped_engine_does_not_charge_or_heat_itself() {
    let stopped = [(g::RPM.id, 0), (g::TPS.id, 0), (g::VSS.id, 0)];
    let mut cold = Engine::new(true);
    cold.advance(1, &stopped);
    assert!((physical(&cold, &g::BAT_VOL) - 12.6).abs() < 0.01);
    run(&mut cold, 120, &stopped);
    assert_eq!(physical(&cold, &g::CLNT), 20.0);
    assert_eq!(physical(&cold, &g::OIL_TEMP), 20.0);
    assert_eq!(physical(&cold, &g::EGT1), 20.0);
    assert_eq!(physical(&cold, &g::FUEL_FLOW), 0.0);
    assert_eq!(physical(&cold, &g::OIL_PRES), 0.0);
    let mut warm = Engine::default();
    run(&mut warm, 300, &stopped);
    assert!(physical(&warm, &g::CLNT) < 30.0);
    assert!(physical(&warm, &g::OIL_TEMP) < 65.0);
}
