// SPDX-License-Identifier: GPL-3.0-only
//! Numeric rule parsing and evaluation without sharing live numeric gauge rows.

use super::*;
use alloc::format;
use cogware_can::{GaugeData, GaugeSpec, Source, Unit, Wire};

const COMPARISONS: [Comparison; 4] = [
    Comparison::Above,
    Comparison::AtLeast,
    Comparison::Below,
    Comparison::AtMost,
];

fn read_pin(fields: &str) -> Result<Pins, PinError> {
    pins(
        &crate::scene::parse(&format!(r#"{{"gpio":8,"pins":[{{"pin":6,{fields}}}]}}"#))
            .expect("valid JSON"),
    )
}

#[test]
fn comparison_names_are_exact_and_round_trip() {
    for (comparison, name) in COMPARISONS
        .into_iter()
        .zip(["above", "at_least", "below", "at_most"])
    {
        assert_eq!(comparison.name(), name);
        assert_eq!(Comparison::parse(name), Some(comparison));
    }
    for name in ["", "Above", ">", ">=", "at-least", "above "] {
        assert_eq!(Comparison::parse(name), None);
    }
}

#[test]
fn all_comparisons_respect_the_boundary() {
    for (comparison, expected) in COMPARISONS.into_iter().zip([
        [false, false, true],
        [false, true, true],
        [true, false, false],
        [true, true, false],
    ]) {
        for (value, readings) in [
            (5000.0, [4999.0, 5000.0, 5001.0]),
            (-2.5, [-3.0, -2.5, -2.0]),
        ] {
            let threshold = Threshold {
                gauge: &cogware_can::RPM,
                comparison,
                value,
            };
            assert_eq!(readings.map(|reading| threshold.matches(reading)), expected);
        }
    }
}

#[test]
fn nonfinite_readings_and_thresholds_never_command_outputs() {
    for comparison in COMPARISONS {
        for nonfinite in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let threshold = Threshold {
                gauge: &cogware_can::RPM,
                comparison,
                value: 5000.0,
            };
            assert!(!threshold.matches(nonfinite));
            let threshold = Threshold {
                value: nonfinite,
                ..threshold
            };
            assert!(!threshold.matches(5000.0));
            assert!(!threshold.matches(nonfinite));
        }
    }
}

#[test]
fn all_comparisons_parse_as_numeric_outputs() {
    for comparison in COMPARISONS {
        let p = read_pin(&format!(
            r#""dir":"out","gauge":"RPM","{}":5000"#,
            comparison.name()
        ))
        .unwrap();
        let pin = p.get(6).unwrap();
        assert_eq!(pin.dir, Dir::Out);
        assert_eq!(
            pin.what(),
            &What::Threshold(Threshold {
                gauge: &cogware_can::RPM,
                comparison,
                value: 5000.0,
            })
        );
        assert_eq!(pin.indicator(), None);
        assert_eq!(pin.action(), None);
        assert_eq!(pin.mask(), 1 << 5);
        assert_eq!(p.reported().count(), 0);
        assert_eq!(p.lamp_states().count(), 0);
    }
    let p = read_pin(r#""dir":"out","gauge":"IAT","below":-2.5"#).unwrap();
    let What::Threshold(threshold) = p.get(6).unwrap().what() else {
        panic!("numeric output");
    };
    assert_eq!(threshold.gauge, &cogware_can::IAT);
    assert_eq!(threshold.value, -2.5);
}

#[test]
fn numeric_sources_must_exist_and_cannot_be_indicator_banks() {
    for name in ["RMP", "rpm", ""] {
        assert_eq!(
            read_pin(&format!(r#""dir":"out","gauge":"{name}","above":5000"#)),
            Err(PinError::UnknownGauge(name.into()))
        );
    }
    for name in ["INDICATORS1", "INDICATORS2"] {
        assert_eq!(
            read_pin(&format!(r#""dir":"out","gauge":"{name}","above":0"#)),
            Err(PinError::NotNumericGauge(name.into()))
        );
    }
}

#[test]
fn a_numeric_source_is_output_only_and_exclusive() {
    for fields in [
        r#""gauge":"RPM","above":5000"#,
        r#""dir":"in","gauge":"RPM","above":5000"#,
    ] {
        assert_eq!(read_pin(fields), Err(PinError::InputGauge(6)));
    }
    for conflict in [
        r#""indicator":"SHIFT_LIGHT""#,
        r#""action":"emit","name":"horn""#,
        r#""indicator":"SHIFT_LIGHT","action":"emit","name":"horn""#,
        r#""indicator":null"#,
        r#""action":null"#,
    ] {
        assert_eq!(
            read_pin(&format!(
                r#""dir":"out","gauge":"RPM","above":5000,{conflict}"#
            )),
            Err(PinError::NotWired(6))
        );
    }
}

#[test]
fn exactly_one_comparison_field_is_required_including_duplicate_keys() {
    for fields in [
        "",
        r#","abvoe":5000"#,
        r#","above":5000,"below":7000"#,
        r#","above":5000,"at_least":5000,"below":7000,"at_most":7000"#,
        r#","above":5000,"above":6000"#,
        r#","above":5000,"below":null"#,
    ] {
        assert_eq!(
            read_pin(&format!(r#""dir":"out","gauge":"RPM"{fields}"#)),
            Err(PinError::BadComparison(6))
        );
    }
}

#[test]
fn comparison_fields_cannot_silently_become_lamps_or_buttons() {
    for comparison in COMPARISONS {
        for wiring in [
            r#""dir":"out","indicator":"SHIFT_LIGHT""#,
            r#""indicator":"TURN_LEFT""#,
            r#""action":"emit","name":"horn""#,
        ] {
            for value in ["5000", "null"] {
                assert_eq!(
                    read_pin(&format!(r#"{wiring},"{}":{value}"#, comparison.name())),
                    Err(PinError::MissingGauge(6))
                );
            }
        }
    }
}

#[test]
fn gauge_and_threshold_fields_have_strict_types() {
    for source in ["null", "true", "42", "[]", "{}"] {
        assert_eq!(
            read_pin(&format!(r#""dir":"out","gauge":{source},"above":5000"#)),
            Err(PinError::BadField { field: "gauge" })
        );
    }
    assert_eq!(
        read_pin(r#""dir":"out","gauge":"RPM","gauge":"IAT","above":5000"#),
        Err(PinError::BadField { field: "gauge" })
    );
    for comparison in COMPARISONS {
        for value in ["null", "true", "\"5000\"", "[]", "{}", "1e40", "-1e40"] {
            assert_eq!(
                read_pin(&format!(
                    r#""dir":"out","gauge":"RPM","{}":{value}"#,
                    comparison.name()
                )),
                Err(PinError::BadField {
                    field: comparison.name()
                })
            );
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let val = Value::Object(alloc::vec![
                ("pin".into(), Value::Int(6)),
                ("dir".into(), Value::Str("out".into())),
                ("gauge".into(), Value::Str("RPM".into())),
                (comparison.name().into(), Value::Float(value)),
            ]);
            assert_eq!(
                pin(&val, 8),
                Err(PinError::BadField {
                    field: comparison.name()
                })
            );
        }
    }
}

#[test]
fn subscriptions_include_unique_numeric_sources_without_scene_bindings() {
    let rig = crate::rig::parse(
        r#"{"modes":["normal"],"displays":[{
            "name":"cluster","node":"0x01","gpio":8,
            "pins":[
                {"pin":1,"dir":"out","gauge":"RPM","above":5000},
                {"pin":2,"dir":"out","gauge":"RPM","at_most":5000},
                {"pin":3,"dir":"out","gauge":"CLNT","below":0},
                {"pin":4,"dir":"out","indicator":"SHIFT_LIGHT"},
                {"pin":5,"indicator":"TURN_LEFT"},
                {"pin":6,"action":"emit","name":"horn"}
            ],"scenes":{"normal":"unbound.scene"}}]}"#,
    )
    .unwrap();
    assert_eq!(
        rig.displays[0].pins.wanted(),
        alloc::vec![
            cogware_can::INDICATORS1.id as u8,
            cogware_can::INDICATORS2.id as u8,
            cogware_can::RPM.id as u8,
            cogware_can::CLNT.id as u8,
        ]
    );
    let p = read_pin(r#""dir":"out","gauge":"RPM","above":5000"#).unwrap();
    assert_eq!(p.wanted(), alloc::vec![cogware_can::RPM.id as u8]);
    assert!(
        read_pin(r#""indicator":"TURN_LEFT""#)
            .unwrap()
            .wanted()
            .is_empty()
    );
    assert!(Pins::none().wanted().is_empty());
}

#[test]
fn outputs_use_canonical_units_fail_low_and_preserve_lamps_inputs_and_sweeps() {
    // Private numeric rows do not race scene-binding tests using the global table.
    static RPM_SPEC: GaugeSpec = GaugeSpec::new("RPM", 0x2d, Wire::U16, Unit::RPM, Source::Common);
    static TEMP_SPEC: GaugeSpec =
        GaugeSpec::new("IAT", 0x25, Wire::I16, Unit::CELSIUS, Source::Common);
    static VOLT_SPEC: GaugeSpec =
        GaugeSpec::new("BAT_VOL", 0x28, Wire::U16, Unit::VOLT, Source::Common);
    static RPM: GaugeData = GaugeData::new(&RPM_SPEC);
    static TEMP: GaugeData = GaugeData::new(&TEMP_SPEC);
    static VOLT: GaugeData = GaugeData::new(&VOLT_SPEC);

    struct Readings([&'static GaugeData; 3]);
    impl Drop for Readings {
        fn drop(&mut self) {
            for gauge in self.0 {
                gauge.clear();
            }
        }
    }
    let readings = Readings([&RPM, &TEMP, &VOLT]);
    for gauge in readings.0 {
        gauge.clear();
    }
    let banks = crate::banks::Banks::take();
    let mut p = pins(
        &crate::scene::parse(
            r#"{"gpio":8,"boot_sweep":[1,5],"pins":[
                {"pin":1,"dir":"out","gauge":"RPM","above":5000,"active":"low"},
                {"pin":2,"dir":"out","gauge":"RPM","at_most":5000},
                {"pin":3,"dir":"out","gauge":"BAT_VOL","at_least":12.6},
                {"pin":4,"dir":"out","gauge":"IAT","below":-2.5},
                {"pin":5,"dir":"out","indicator":"SHIFT_LIGHT","active":"low"},
                {"pin":6,"indicator":"TURN_LEFT","active":"low"},
                {"pin":7,"action":"emit","name":"horn"},
                {"pin":8,"dir":"out","gauge":"IAT","at_most":-2.5}
            ]}"#,
        )
        .unwrap(),
    )
    .unwrap();
    for pin in &mut p.pins {
        if let What::Threshold(threshold) = &mut pin.what {
            threshold.gauge = match threshold.gauge.name {
                "RPM" => &RPM,
                "IAT" => &TEMP,
                "BAT_VOL" => &VOLT,
                _ => unreachable!("only three numeric sources"),
            };
        }
    }

    assert_eq!(p.outputs(), 0, "unseen readings are not numeric zero");
    RPM.set(5000);
    assert_eq!(p.outputs(), 0b0000_0010, "above is strict at 5000 RPM");
    RPM.set(5001);
    assert_eq!(
        p.outputs(),
        0b0000_0001,
        "active-low does not invert commands"
    );
    VOLT.set(12_599);
    assert_eq!(p.outputs(), 0b0000_0001, "fixed-point counts are not volts");
    VOLT.set(12_600);
    assert_eq!(p.outputs(), 0b0000_0101);
    TEMP.set(-25);
    assert_eq!(
        p.outputs(),
        0b1000_0101,
        "minus 25 counts is minus 2.5 degrees"
    );
    TEMP.set(-26);
    assert_eq!(p.outputs(), 0b1000_1101);
    banks.set(&[Indicator::ShiftLight, Indicator::TurnLeft]);
    assert_eq!(
        p.outputs(),
        0b1001_1101,
        "an input lamp must never be driven"
    );

    p.feed(0, 0);
    p.feed(0b1001_1111, 1);
    assert!(p.feed(0b1001_1111, 1 + DEBOUNCE_US).is_empty());
    assert!(p.asserted(6), "active-low still applies to input lamps");
    p.feed(0b1101_1111, 2 + DEBOUNCE_US);
    assert!(p.feed(0b1101_1111, 2 + 2 * DEBOUNCE_US).contains(7));
    assert_eq!(
        p.get(7).and_then(Pin::action),
        Some(&Action::Emit {
            name: "horn".into()
        })
    );
    assert_eq!(
        p.reported().collect::<Vec<_>>(),
        alloc::vec![Indicator::TurnLeft]
    );
    assert_eq!(
        p.lamp_states().collect::<Vec<_>>(),
        alloc::vec![(Indicator::TurnLeft, true)]
    );

    assert_eq!(p.sweep(0, 0), Some(1));
    assert_eq!(p.sweep(SWEEP_STEP_US, 0), Some(1 << 4));
    assert_eq!(p.sweep(2 * SWEEP_STEP_US, 0), None);
    assert_eq!(p.outputs(), 0b1001_1101, "sweeping does not alter rules");
    RPM.clear();
    assert_eq!(p.outputs(), 0b1001_1100);
    TEMP.set(cogware_can::NO_DATA);
    VOLT.clear();
    assert_eq!(p.outputs(), 1 << 4);
    banks.set(&[]);
    assert_eq!(p.outputs(), 0);
}
