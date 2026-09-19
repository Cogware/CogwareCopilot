// SPDX-License-Identifier: GPL-3.0-only
//! GPIO preview sources survive reloads and share the exact runtime comparison.

use super::*;
use crate::{App, Driver};
use copilot::gpio::{Comparison, Dir, Level as Active, Pin, Pull, Threshold};
use copilot::widget::Tree;

#[test]
fn all_displays_gpio_sources_share_preview_controls_without_editing_scene_or_rig() {
    use crate::rig::{Doc, RigSession};
    let text = r#"{
        "modes":["normal"],"displays":[
            {"name":"Cluster","node":"0x01","gpio":1,
             "scenes":{"normal":"a.scene"},
             "pins":[{"pin":1,"dir":"out","gauge":"RPM","above":5000}]},
            {"name":"Gauge","node":"0x02","gpio":1,
             "scenes":{"normal":"b.scene"},
             "pins":[{"pin":1,"dir":"out","gauge":"CLNT","above":100}]}
        ]
    }"#;
    let mut app = App::new(None, &[]);
    let scene = app.text.clone();
    app.rig = Some(RigSession {
        rig: copilot::rig::parse(text).unwrap(),
        text: text.into(),
        path: "gpio-preview-test.rig".into(),
        mode: 0,
        active: 0,
        docs: vec![
            Doc {
                path: None,
                text: String::new(),
                dirty: false,
                preview: crate::preview::Preview::new(),
                undo: Vec::new(),
                redo: Vec::new(),
                texture: None,
            },
            Doc {
                path: None,
                text: String::new(),
                dirty: false,
                preview: crate::preview::Preview::new(),
                undo: Vec::new(),
                redo: Vec::new(),
                texture: None,
            },
        ],
    });
    app.sync_driver();
    assert!(app.driver.gauges.contains_key("RPM"));
    assert!(
        app.driver.gauges.contains_key("CLNT"),
        "parked displays contribute GPIO sources"
    );
    app.driver.gauges.get_mut("RPM").unwrap().value = 5500.0;
    app.reload();
    assert_eq!(app.driver.gauges["RPM"].value, 5500.0);
    assert_eq!(app.text, scene);
    assert_eq!(app.rig.as_ref().unwrap().text, text);
    assert!(!app.dirty);
    app.start(&crate::newdoc::NewDoc::default());
    assert!(!app.driver.gauges.contains_key("RPM"));
    assert!(!app.driver.gauges.contains_key("CLNT"));
}

#[test]
fn threshold_preview_uses_current_playback_values_without_writing_global_gauges() {
    let mut app = App::new(None, &[]);
    let pins = pins();
    app.driver.sync(
        &Tree::new(copilot::Rect::new(0, 0, 64, 64)),
        &[],
        &[],
        &[&pins],
    );
    app.driver.enabled = true;
    app.driver.driving = true;
    app.driver.tick(1_000_000);
    let current = app.driver.gauges["RPM"].value;
    let expected = if current > 5000.0 {
        crate::gpio_strip::Level::High
    } else {
        crate::gpio_strip::Level::Low
    };
    assert!(app.pin_level(pins.get(1).unwrap()) == expected);
}

fn pins() -> Pins {
    let rig = copilot::rig::parse(
        r#"{
        "modes":["normal"], "displays":[{
            "name":"Cluster", "node":"0x01", "gpio":8,
            "scenes":{"normal":"a.scene"},
            "pins":[
                {"pin":1,"dir":"out","gauge":"RPM","above":5000},
                {"pin":2,"dir":"out","gauge":"RPM","above":7500},
                {"pin":3,"dir":"out","indicator":"HIGH_BEAM"}
            ]
        }]
    }"#,
    )
    .unwrap();
    rig.displays[0].pins.clone()
}

#[test]
fn gpio_only_sources_are_available_preserved_and_removed_on_reload() {
    let pins = pins();
    let tree = Tree::new(copilot::Rect::new(0, 0, 64, 64));
    let mut driver = Driver::default();
    driver.sync(&tree, &[], &[], &[&pins]);
    assert_eq!(driver.gauges.len(), 1, "shared sources get one slider");
    assert!(driver.gauges["RPM"].max > 7500.0);
    assert!(driver.lamps.contains_key("HIGH_BEAM"));
    driver.gauges.get_mut("RPM").unwrap().value = 5500.0;
    driver.lamps.insert("HIGH_BEAM", true);
    driver.sync(&tree, &[], &[], &[&pins]);
    assert_eq!(driver.gauges["RPM"].value, 5500.0);
    assert!(driver.lamps["HIGH_BEAM"]);
    driver.sync(&tree, &[], &[], &[]);
    assert!(driver.gauges.is_empty());
    assert!(driver.lamps.is_empty());
}

#[test]
fn rpm_preview_is_high_only_above_5000_and_unknown_when_disabled() {
    let mut app = App::new(None, &[]);
    let pins = pins();
    app.driver.sync(
        &Tree::new(copilot::Rect::new(0, 0, 64, 64)),
        &[],
        &[],
        &[&pins],
    );
    let pin = pins.get(1).unwrap();
    assert!(app.pin_level(pin) == crate::gpio_strip::Level::Unsaid);
    app.driver.enabled = true;
    for (value, expected) in [
        (4999.0, crate::gpio_strip::Level::Low),
        (5000.0, crate::gpio_strip::Level::Low),
        (5001.0, crate::gpio_strip::Level::High),
    ] {
        app.driver.gauges.get_mut("RPM").unwrap().value = value;
        assert!(app.pin_level(pin) == expected);
    }
    app.driver.enabled = false;
    assert!(app.pin_level(pin) == crate::gpio_strip::Level::Unsaid);
}

#[test]
fn sweep_and_fractional_negative_thresholds_use_the_shared_comparison() {
    let mut app = App::new(None, &[]);
    let pins = pins();
    app.driver.sync(
        &Tree::new(copilot::Rect::new(0, 0, 64, 64)),
        &[],
        &[],
        &[&pins],
    );
    app.driver.enabled = true;
    app.driver.sweep = true;
    app.driver.period_s = 4.0;
    app.driver.tick(4_000_000);
    assert!(app.pin_level(pins.get(1).unwrap()) == crate::gpio_strip::Level::High);
    app.driver.tick(4_000_000);
    assert!(app.pin_level(pins.get(1).unwrap()) == crate::gpio_strip::Level::Low);
    let mut seen = BTreeMap::new();
    let pins = copilot::rig::parse(
        r#"{
        "modes":["normal"],"displays":[{
            "name":"Gauge","node":"0x02","gpio":1,
            "scenes":{"normal":"a.scene"},
            "pins":[{"pin":1,"dir":"out","gauge":"CLNT","at_most":-10.5}]
        }]
    }"#,
    )
    .unwrap()
    .displays[0]
        .pins
        .clone();
    gauges(&mut seen, &[&pins]);
    assert!(seen["CLNT"].1 < -10.5);
    assert!(seen["CLNT"].2 > -10.5);
    app.driver.sync(
        &Tree::new(copilot::Rect::new(0, 0, 64, 64)),
        &[],
        &[],
        &[&pins],
    );
    app.driver.sweep = false;
    app.driver.gauges.get_mut("CLNT").unwrap().value = -10.5;
    assert!(app.pin_level(pins.get(1).unwrap()) == crate::gpio_strip::Level::High);
}

#[test]
fn unknown_or_nonfinite_preview_values_are_not_claimed_as_outputs() {
    let mut app = App::new(None, &[]);
    app.driver.enabled = true;
    let pin = Pin {
        pin: 1,
        label: None,
        dir: Dir::Out,
        active: Active::High,
        pull: Pull::None,
        what: What::Threshold(Threshold {
            gauge: &copilot::cogware_can::RPM,
            comparison: Comparison::Above,
            value: 5000.0,
        }),
    };
    assert!(app.pin_level(&pin) == crate::gpio_strip::Level::Unsaid);
    app.driver.sync(
        &Tree::new(copilot::Rect::new(0, 0, 64, 64)),
        &[],
        &[],
        &[&pins()],
    );
    app.driver.gauges.get_mut("RPM").unwrap().value = f32::NAN;
    assert!(app.pin_level(&pin) == crate::gpio_strip::Level::Unsaid);
}
