// SPDX-License-Identifier: GPL-3.0-only
use super::*;
use crate::drive::Channel;
use copilot::cogware_can as can;

fn running() -> Driver {
    Driver {
        enabled: true,
        driving: true,
        ..Default::default()
    }
}

fn raw(driver: &Driver, gauge: &can::GaugeData) -> i32 {
    driver
        .generated_readings()
        .into_iter()
        .find(|(id, _)| *id == gauge.id)
        .unwrap()
        .1
}

#[test]
fn every_supported_channel_is_filled_in_canonical_units() {
    let mut driver = running();
    for gauge in can::ALL_GAUGES {
        if matches!(simulation::provenance(gauge.id), Some(Provenance::Excluded)) {
            continue;
        }
        driver.gauges.insert(
            gauge.name,
            Channel {
                value: -9999.0,
                min: 0.0,
                max: 1.0,
                sweep: true,
                authored: 0.0,
            },
        );
    }
    driver.tick(5_000_000);
    let readings = driver.readings();
    assert_eq!(readings.len(), can::ALL_GAUGES.len() - 3);
    assert!(
        readings.len() * 6 <= can::wire::MAX_PAYLOAD,
        "all readings must fit a telemetry frame"
    );
    for (id, value) in readings {
        let gauge = can::gauge_by_id(id).unwrap();
        assert_eq!(
            driver.gauges[gauge.name].value,
            value as f32 / gauge.unit.scale as f32,
            "{}",
            gauge.name
        );
    }
}

#[test]
fn measured_and_target_afr_are_independent_live_channels() {
    let mut driver = running();
    let mut different = 0;
    for _ in 0..250 {
        driver.tick(40_000);
        if raw(&driver, &can::AFR_PRI) != raw(&driver, &can::AFR_TARGET) {
            different += 1;
        }
    }
    assert!(
        different > 200,
        "measured AFR should not simply copy the target: {different}"
    );
    assert_ne!(raw(&driver, &can::AFR_PRI), raw(&driver, &can::AFR_SEC));
}

#[test]
fn logs_keep_raw_values_outside_the_old_fourteen_channel_model() {
    let mut driver = running();
    driver.use_log(Log::parse("recorded.csv",
        "Time,RPM,VSS,AFR_PRI,AFR_TARGET,EGT1,FUEL_PRES\n0,4321,95,13.12,12.7,643.2,347.6\n10,4500,100,12.91,12.5,670,351\n"
    ).unwrap());
    driver.tick(1_000_000);
    let measured = driver.log.as_ref().unwrap().at(1.0);
    assert_eq!(measured.len(), 6);
    for (id, value) in measured {
        assert_eq!(
            driver
                .generated_readings()
                .iter()
                .find(|(gauge, _)| *gauge == id)
                .unwrap()
                .1,
            value
        );
        assert_eq!(driver.source_label(id), "Recorded");
    }
    assert_eq!(raw(&driver, &can::AFR_PRI), 1312);
    assert_eq!(raw(&driver, &can::AFR_TARGET), 1270);
}

#[test]
fn a_speed_only_log_conditions_engine_and_wheel_readings() {
    let mut driver = running();
    driver.use_log(Log::parse("speed.csv", "Time,VSS\n0,100\n20,100\n").unwrap());
    for _ in 0..100 {
        driver.tick(40_000);
    }
    assert_eq!(raw(&driver, &can::VSS), 1000);
    assert!(raw(&driver, &can::RPM) > 1200);
    assert_eq!(driver.source_label(can::VSS.id), "Recorded");
    assert_eq!(driver.source_label(can::RPM.id), "Simulated");
}

#[test]
fn disabling_or_pausing_preview_freezes_the_playback_clock_and_model() {
    let mut driver = running();
    driver.tick(2_000_000);
    let snapshot = driver.generated_readings();
    let position = driver.log_at_s;
    driver.enabled = false;
    driver.tick(40_000_000);
    assert_eq!(driver.log_at_s, position);
    assert_eq!(driver.generated_readings(), snapshot);
    driver.enabled = true;
    driver.driving = false;
    driver.tick(40_000_000);
    assert_eq!(driver.log_at_s, position);
    assert_eq!(driver.generated_readings(), snapshot);
}

#[test]
fn restart_resets_history_and_preserves_explicit_options() {
    let mut driver = running();
    driver.use_simulator(Scenario::Pull);
    driver.cold_start = true;
    driver.to_car = true;
    driver.restart_drive();
    let fresh = driver.generated_readings();
    for _ in 0..100 {
        driver.tick(100_000);
    }
    assert_ne!(driver.generated_readings(), fresh);
    driver.restart_drive();
    assert_eq!(driver.log_at_s, 0.0);
    assert_eq!(driver.generated_readings(), fresh);
    assert_eq!(driver.engine.scenario, Scenario::Pull);
    assert!(driver.cold_start && driver.to_car && driver.driving);
}

#[test]
fn seeking_uses_the_selected_recorded_frame_without_old_model_history() {
    let mut driver = running();
    driver
        .use_log(Log::parse("seek.csv", "Time,RPM,VSS\n0,950,0\n2,3000,70\n10,3200,75\n").unwrap());
    driver.tick(3_000_000);
    assert_eq!(raw(&driver, &can::RPM), 3000);
    driver.seek_drive(0.0);
    assert_eq!(raw(&driver, &can::RPM), 950);
    let fresh = driver.generated_readings();
    driver.tick(5_000_000);
    driver.seek_drive(0.0);
    assert_eq!(driver.generated_readings(), fresh);
    driver.seek_drive(10.0);
    assert_eq!(
        raw(&driver, &can::RPM),
        3000,
        "end-of-log seeks must not silently wrap to the start"
    );
}

#[test]
fn filling_is_a_paused_snapshot_not_an_implicit_transmission_request() {
    let mut driver = running();
    driver.sweep = true;
    driver.lamps.insert("OIL_PRESSURE", true);
    driver.gauges.insert(
        "RPM",
        Channel {
            value: 0.0,
            min: 0.0,
            max: 1.0,
            sweep: true,
            authored: 0.0,
        },
    );
    driver.fill_preview_values();
    assert!(driver.enabled);
    assert!(!driver.driving && !driver.sweep && !driver.to_car);
    assert!(driver.gauges["RPM"].value > 500.0);
    assert_eq!(driver.lamps.get("OIL_PRESSURE"), Some(&true));
    for gauge in [&can::INDICATORS1, &can::INDICATORS2, &can::MASTERALIVE] {
        assert!(
            !driver
                .generated_readings()
                .iter()
                .any(|(id, _)| *id == gauge.id)
        );
        assert_eq!(driver.source_label(gauge.id), "Not simulated");
    }
}

#[test]
fn driving_transmits_only_the_bound_preview_sources() {
    let mut driver = running();
    driver.gauges.insert(
        "RPM",
        Channel {
            value: 0.0,
            min: 0.0,
            max: 8000.0,
            sweep: true,
            authored: 0.0,
        },
    );
    driver.tick(40_000);
    let readings = driver.readings();
    assert_eq!(readings.len(), 1);
    assert_eq!(
        readings[0],
        (can::RPM.id, driver.engine.reading(can::RPM.id).unwrap())
    );
    assert!(driver.generated_readings().len() > readings.len());
}
