// SPDX-License-Identifier: GPL-3.0-only
//! Windowless numeric GPIO interaction, review, and unsaved-draft tests.

use super::*;
use crate::pins::Kind;
use copilot::gpio::{Comparison, Dir, What};

#[test]
fn selecting_a_threshold_is_discoverable_and_only_an_explicit_reviewed_save_writes() {
    let (mut app, path) = app("threshold-review");
    let original = std::fs::read_to_string(&path).unwrap();
    let ctx = egui::Context::default();
    click(&mut app, &ctx, "Value threshold");
    let draft = app.pins_dialog.as_ref().unwrap().editing.as_ref().unwrap();
    assert_eq!((draft.kind, draft.dir), (Kind::Threshold, Dir::Out));
    assert_eq!((&*draft.gauge, &*draft.value), ("RPM", "5000"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    click(&mut app, &ctx, "Review & save mapping…");
    assert!(matches!(
        app.pins_dialog.as_ref().unwrap().pending,
        Some(Pending::Save(Act::Write(_)))
    ));
    let output = frame(&mut app, &ctx, 1000.0, vec![]);
    assert!(output.shapes.iter().any(|shape| matches!(
        &shape.shape, egui::Shape::Text(text)
        if text.galley.job.text.contains("RPM >5000 rpm → HIGH command, otherwise LOW command")
    )));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    click(&mut app, &ctx, "Keep editing");
    assert!(app.pins_dialog.as_ref().unwrap().pin_dirty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    click(&mut app, &ctx, "Review & save mapping…");
    click(&mut app, &ctx, "Save pin mapping");
    let dialog = app.pins_dialog.as_ref().unwrap();
    assert!(!dialog.pin_dirty());
    assert!(dialog.success.starts_with("Saved to disk:"));
    let condition = app.rig.as_ref().unwrap().rig.displays[0]
        .pins
        .get(1)
        .unwrap();
    assert!(matches!(condition.what(), What::Threshold(t)
        if t.gauge.name == "RPM" && t.value == 5000.0 && t.comparison == Comparison::Above));
    let mut reopened = App::new(None, &[]);
    reopened.open_rig(path.clone());
    let pins = &reopened.rig.as_ref().unwrap().rig.displays[0].pins;
    assert_eq!(
        Draft::of(pins.get(1).unwrap()),
        *dialog.editing.as_ref().unwrap()
    );
}

#[test]
fn numeric_text_entry_enter_and_focus_loss_do_not_save_or_erase_incomplete_text() {
    let (mut app, path) = app("threshold-typing");
    let original = std::fs::read_to_string(&path).unwrap();
    let ctx = egui::Context::default();
    click(&mut app, &ctx, "Value threshold");
    app.pins_dialog
        .as_mut()
        .unwrap()
        .editing
        .as_mut()
        .unwrap()
        .value = "-12".into();
    click(&mut app, &ctx, "-12");
    let _ = frame(
        &mut app,
        &ctx,
        1000.0,
        vec![
            egui::Event::Text("e".into()),
            egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    click(&mut app, &ctx, "Output");
    let draft = app.pins_dialog.as_ref().unwrap().editing.as_ref().unwrap();
    assert!(draft.value.contains('e'), "{}", draft.value);
    assert!(!draft.complete());
    let incomplete = draft.value.clone();
    click(&mut app, &ctx, "Review & save mapping…");
    assert!(app.pins_dialog.as_ref().unwrap().pending.is_none());
    click(&mut app, &ctx, "Close GPIO");
    assert!(matches!(
        app.pins_dialog.as_ref().unwrap().pending,
        Some(Pending::Leave(Nav::Close))
    ));
    click(&mut app, &ctx, "Keep editing");
    assert_eq!(
        app.pins_dialog
            .as_ref()
            .unwrap()
            .editing
            .as_ref()
            .unwrap()
            .value,
        incomplete
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn source_picker_switches_directions_without_losing_threshold_or_indicator_choices() {
    let (mut app, path) = app("threshold-source-switch");
    let original = std::fs::read_to_string(&path).unwrap();
    let ctx = egui::Context::default();
    click(&mut app, &ctx, "Value threshold");
    let draft = app.pins_dialog.as_mut().unwrap().editing.as_mut().unwrap();
    draft.gauge = "CLNT".into();
    draft.comparison = Comparison::AtLeast;
    draft.value = "-4.5".into();
    click(&mut app, &ctx, "Button action");
    assert_eq!(
        app.pins_dialog
            .as_ref()
            .unwrap()
            .editing
            .as_ref()
            .unwrap()
            .dir,
        Dir::In
    );
    click(&mut app, &ctx, "Indicator");
    assert_eq!(
        app.pins_dialog
            .as_ref()
            .unwrap()
            .editing
            .as_ref()
            .unwrap()
            .indicator,
        "TURN_LEFT"
    );
    click(&mut app, &ctx, "Value threshold");
    let draft = app.pins_dialog.as_ref().unwrap().editing.as_ref().unwrap();
    assert_eq!(
        (&*draft.gauge, &*draft.value, draft.comparison),
        ("CLNT", "-4.5", Comparison::AtLeast)
    );
    assert_eq!(draft.dir, Dir::Out);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn invalid_gauge_or_number_blocks_review_and_retains_its_explanation() {
    let (mut app, path) = app("threshold-errors");
    let original = std::fs::read_to_string(&path).unwrap();
    let ctx = egui::Context::default();
    click(&mut app, &ctx, "Value threshold");
    for (gauge, value) in [
        ("NOT_A_GAUGE", "5000"),
        ("INDICATORS1", "5000"),
        ("RPM", "NaN"),
        ("RPM", "-"),
    ] {
        let draft = app.pins_dialog.as_mut().unwrap().editing.as_mut().unwrap();
        draft.gauge = gauge.into();
        draft.value = value.into();
        assert!(draft.validate().is_err());
        click(&mut app, &ctx, "Review & save mapping…");
        assert!(app.pins_dialog.as_ref().unwrap().pending.is_none());
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn opening_and_rendering_an_authored_threshold_never_canonicalises_it_on_disk() {
    let (mut app, path) = app("threshold-authored");
    let original = std::fs::read_to_string(&path).unwrap().replace(
        r#""indicator":"TURN_LEFT""#,
        r#""dir":"out", "gauge":"RPM", /* chosen limit */ "above":5.000e3"#,
    );
    std::fs::write(&path, &original).unwrap();
    app.open_rig(path.clone());
    let pins = &app.rig.as_ref().unwrap().rig.displays[0].pins;
    let mut dialog = PinsDialog::new(0, pins.count());
    dialog.open_pin(1, pins);
    assert!(!dialog.pin_dirty());
    assert_eq!(dialog.editing.as_ref().unwrap().kind, Kind::Threshold);
    app.pins_dialog = Some(dialog);
    let ctx = egui::Context::default();
    for width in [1000.0, 420.0, 360.0, 1000.0] {
        let _ = frame(&mut app, &ctx, width, vec![]);
        assert!(!app.pins_dialog.as_ref().unwrap().pin_dirty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }
}
