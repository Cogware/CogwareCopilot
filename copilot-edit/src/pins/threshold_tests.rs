// SPDX-License-Identifier: GPL-3.0-only
//! Numeric GPIO draft, persistence, and source-switching regressions.

use super::test_support::TestApp;
use super::*;
use copilot::gpio::{Comparison, Dir, Level, Pin, Pull, What};

fn threshold(comparison: Comparison, value: &str) -> Draft {
    let mut draft = Draft::fresh(6);
    draft.set_kind(Kind::Threshold);
    draft.comparison = comparison;
    draft.value = value.into();
    draft
}

#[test]
fn every_comparison_round_trips_negative_fractional_and_inclusive_values() {
    let mut app = TestApp::open(super::tests::rig_on_disk("threshold-roundtrip"));
    for (comparison, value) in [
        (Comparison::Above, "5000"),
        (Comparison::AtLeast, "-12.75"),
        (Comparison::Below, "0.125"),
        (Comparison::AtMost, "-0.5"),
    ] {
        let draft = threshold(comparison, value);
        app.set_pin(0, &draft).unwrap();
        let session = app.rig.as_ref().unwrap();
        let text = std::fs::read_to_string(&session.path).unwrap();
        let rig = copilot::rig::parse(&text).unwrap();
        let pin = rig.displays[0].pins.get(6).unwrap();
        assert_eq!(Draft::of(pin), draft);
        let What::Threshold(condition) = pin.what() else {
            panic!("not a threshold")
        };
        assert_eq!(condition.comparison, comparison);
        assert_eq!(condition.value, value.parse::<f32>().unwrap());
        assert_eq!(condition.gauge.name, "RPM");
        let mut reopened = App::new(None, &[]);
        reopened.open_rig(session.path.clone());
        let pin = reopened.rig.as_ref().unwrap().rig.displays[0]
            .pins
            .get(6)
            .unwrap();
        assert_eq!(Draft::of(pin), draft);
    }
}

#[test]
fn invalid_values_gauges_and_directions_never_change_disk_or_the_live_rig() {
    let mut app = TestApp::open(super::tests::rig_on_disk("threshold-invalid"));
    let before = app.rig.as_ref().unwrap().text.clone();
    let mut draft = threshold(Comparison::Above, "5000");
    for value in [
        "", "-", ".", "1e", "NaN", "inf", "-inf", "1e100", "5000 rpm",
    ] {
        draft.value = value.into();
        assert!(!draft.complete(), "{value}");
        assert!(draft.to_rig().is_err(), "{value}");
        assert!(app.set_pin(0, &draft).is_err(), "{value}");
    }
    draft.value = "5000".into();
    for gauge in ["", "warp_core", "rpm", "INDICATORS1", "INDICATORS2"] {
        draft.gauge = gauge.into();
        assert!(draft.validate().is_err(), "{gauge}");
        assert!(app.set_pin(0, &draft).is_err(), "{gauge}");
    }
    draft.gauge = "RPM".into();
    draft.dir = Dir::In;
    assert!(app.set_pin(0, &draft).unwrap_err().contains("output"));
    let session = app.rig.as_ref().unwrap();
    assert_eq!(session.text, before);
    assert!(session.rig.displays[0].pins.get(6).is_none());
    assert_eq!(std::fs::read_to_string(&session.path).unwrap(), before);
}

#[test]
fn source_switching_preserves_inactive_choices_but_only_writes_relevant_fields() {
    let mut draft = threshold(Comparison::AtMost, "-2.5");
    draft.gauge = "CLNT".into();
    draft.indicator = "SHIFT_LIGHT".into();
    draft.active = Level::Low;
    draft.pull = Pull::Up;
    draft.action.set_action("emit");
    draft.action.set_emit("horn");
    let original = draft.clone();
    let output = draft.to_rig().unwrap();
    for irrelevant in ["active", "pull", "indicator", "action"] {
        assert!(!output.contains(irrelevant), "{output}");
    }
    assert_eq!(output.matches("at_most").count(), 1);
    draft.set_kind(Kind::Button);
    assert_eq!(draft.dir, Dir::In);
    let output = draft.to_rig().unwrap();
    assert!(output.contains("horn"));
    assert!(output.contains("active"));
    assert!(!output.contains("gauge"));
    assert!(!output.contains("at_most"));
    draft.set_kind(Kind::Indicator);
    assert!(draft.to_rig().unwrap().contains("SHIFT_LIGHT"));
    draft.set_kind(Kind::Threshold);
    assert_eq!(draft, original);
    draft.value = "-".into();
    draft.set_kind(Kind::Button);
    draft.set_kind(Kind::Threshold);
    assert_eq!(draft.value, "-");
    assert!(!draft.complete());
}

#[test]
fn direction_changes_cannot_leave_an_incompatible_source_selected() {
    let mut draft = threshold(Comparison::Above, "5000");
    draft.set_dir(Dir::In);
    assert_eq!(draft.kind, Kind::Indicator);
    draft.set_kind(Kind::Button);
    draft.set_dir(Dir::Out);
    assert_eq!(draft.kind, Kind::Indicator);
    draft.set_kind(Kind::Threshold);
    assert_eq!(draft.value, "5000");
}

#[test]
fn invalid_text_changes_are_dirty_without_treating_inactive_fields_as_edits() {
    let mut draft = threshold(Comparison::Above, "5000");
    let baseline = draft.clone();
    draft.indicator = "ABS".into();
    draft.active = Level::Low;
    assert!(draft.same_mapping(&baseline));
    draft.value = "-".into();
    assert!(!draft.same_mapping(&baseline));
    let incomplete = draft.clone();
    draft.value = "1e".into();
    assert!(!draft.same_mapping(&incomplete));
}

#[test]
fn summaries_use_canonical_units_and_logical_commands() {
    let draft = threshold(Comparison::Above, "5000");
    assert_eq!(
        draft.summary(),
        "RPM >5000 rpm → HIGH command, otherwise LOW command"
    );
    let draft = threshold(Comparison::AtMost, "-0.25");
    assert!(draft.summary().contains("RPM <=-0.25 rpm"));
}

#[test]
fn replacing_an_indicator_keeps_comments_and_does_not_reload_the_scene() {
    let files = super::tests::rig_on_disk("threshold-comments");
    let original = std::fs::read_to_string(&files).unwrap();
    let text = original.replace(
        r#""gpio": 8,"#,
        r#""gpio": 8, "pins": [
        { "pin": 6,
          // The relay wire; keep this note when switching sources.
          "indicator": "SHIFT_LIGHT", "dir": "out" }
        ],"#,
    );
    std::fs::write(&files, text).unwrap();
    let mut app = TestApp::open(files);
    let unsaved = format!("// Unsaved scene work\n{}", app.text);
    app.text = unsaved.clone();
    app.dirty = true;
    assert!(!app.driver.gauges.contains_key("RPM"));
    app.set_pin(0, &threshold(Comparison::Above, "5000"))
        .unwrap();
    assert_eq!(app.text, unsaved);
    assert!(app.dirty);
    assert!(app.driver.gauges.contains_key("RPM"));
    let session = app.rig.as_ref().unwrap();
    assert!(session.text.contains("The relay wire; keep this note"));
    assert!(session.text.contains("The cluster, behind the wheel."));
    assert!(!session.text.contains("SHIFT_LIGHT"));
    assert!(matches!(
        session.rig.displays[0].pins.get(6).map(Pin::what),
        Some(What::Threshold(_))
    ));
}

#[test]
fn switching_all_source_types_preserves_comments_between_keys_values_and_commas() {
    let files = super::tests::rig_on_disk("threshold-comment-tokens");
    let original = std::fs::read_to_string(&files).unwrap();
    let text = original.replace(
        r#""gpio": 8,"#,
        r#""gpio": 8, "pins": [
        { "pin": 6,
          /* relay */ "indicator" /* source */ : /* signal */ "SHIFT_LIGHT" /* board */,
          "dir": "out" }
        ],"#,
    );
    std::fs::write(&files, text).unwrap();
    let mut app = TestApp::open(files);
    let mut draft = threshold(Comparison::Above, "5000");
    draft.indicator = "ABS".into();
    draft.action.set_action("emit");
    draft.action.set_emit("horn");
    for kind in [
        Kind::Threshold,
        Kind::Indicator,
        Kind::Button,
        Kind::Threshold,
    ] {
        draft.set_kind(kind);
        app.set_pin(0, &draft).unwrap();
        let session = app.rig.as_ref().unwrap();
        for note in ["/* relay */", "/* source */", "/* signal */", "/* board */"] {
            assert!(session.text.contains(note), "{note}\n{}", session.text);
        }
        let reopened = Draft::of(session.rig.displays[0].pins.get(6).unwrap());
        assert!(reopened.same_mapping(&draft));
    }
    app.clear_pin(0, 6).unwrap();
    assert!(!app.driver.gauges.contains_key("RPM"));
}

#[test]
fn persisted_mapping_edits_refresh_gpio_only_preview_sources_without_a_scene_reload() {
    let mut app = TestApp::open(super::tests::rig_on_disk("threshold-driver-sync"));
    app.text = format!("// Unsaved scene work\n{}", app.text);
    app.dirty = true;
    let scene = app.text.clone();
    let epoch = app.document_epoch;
    assert!(app.driver.gauges.is_empty());
    assert!(app.driver.lamps.is_empty());

    let mut draft = threshold(Comparison::Above, "5000");
    app.set_pin(0, &draft).unwrap();
    assert!(app.driver.gauges.contains_key("RPM"));
    let initial_max = app.driver.gauges["RPM"].max;
    assert!(initial_max > 5000.0);
    app.driver.gauges.get_mut("RPM").unwrap().value = 5500.0;
    assert_persisted_mapping(&app, &draft);

    draft.value = "9000".into();
    draft.comparison = Comparison::AtLeast;
    app.set_pin(0, &draft).unwrap();
    assert!(app.driver.gauges["RPM"].max > initial_max);
    assert_eq!(app.driver.gauges["RPM"].value, 5500.0);
    assert_persisted_mapping(&app, &draft);

    draft.gauge = "CLNT".into();
    draft.value = "100.5".into();
    app.set_pin(0, &draft).unwrap();
    assert!(!app.driver.gauges.contains_key("RPM"));
    assert!(app.driver.gauges.contains_key("CLNT"));
    assert!(app.driver.gauges["CLNT"].max > 100.5);
    assert_persisted_mapping(&app, &draft);

    draft.set_kind(Kind::Indicator);
    draft.indicator = "SHIFT_LIGHT".into();
    app.set_pin(0, &draft).unwrap();
    assert!(app.driver.gauges.is_empty());
    assert!(app.driver.lamps.contains_key("SHIFT_LIGHT"));
    assert_persisted_mapping(&app, &draft);

    draft.set_kind(Kind::Threshold);
    draft.gauge = "RPM".into();
    app.set_pin(0, &draft).unwrap();
    assert!(app.driver.gauges.contains_key("RPM"));
    assert!(!app.driver.lamps.contains_key("SHIFT_LIGHT"));
    app.clear_pin(0, draft.pin).unwrap();
    assert!(app.driver.gauges.is_empty());
    assert!(app.driver.lamps.is_empty());
    let text = std::fs::read_to_string(&app.rig.as_ref().unwrap().path).unwrap();
    assert!(
        copilot::rig::parse(&text).unwrap().displays[0]
            .pins
            .get(draft.pin)
            .is_none()
    );
    assert_eq!(app.text, scene);
    assert!(app.dirty);
    assert_eq!(app.document_epoch, epoch);
}

fn assert_persisted_mapping(app: &App, draft: &Draft) {
    let text = std::fs::read_to_string(&app.rig.as_ref().unwrap().path).unwrap();
    let rig = copilot::rig::parse(&text).unwrap();
    let saved = Draft::of(rig.displays[0].pins.get(draft.pin).unwrap());
    assert!(saved.same_mapping(draft));
}
