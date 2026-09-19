// SPDX-License-Identifier: GPL-3.0-only
//! Windowless GPIO interaction and draft-state regressions.

use super::*;
use crate::pins::test_support::Fixture;

fn app(tag: &str) -> (App, Fixture) {
    let files = Fixture::new(&format!("ui-{tag}"));
    let dir = files.parent().unwrap();
    for (file, node) in [("a.scene", 1), ("b.scene", 2)] {
        std::fs::write(dir.join(file), format!(
            r#"{{"width":64,"height":64,"node":{node},"root":{{"type":"panel","rect":[0,0,64,64]}}}}"#
        )).unwrap();
    }
    let path = dir.join("test.rig");
    std::fs::write(
        &path,
        r#"{
        "modes":["normal"],
        "displays":[
            {"name":"Cluster","node":1,"gpio":8,"pins":[
                {"pin":1,"label":"left stalk","indicator":"TURN_LEFT"},
                {"pin":7,"indicator":"BRAKE"}
            ],"scenes":{"normal":"a.scene"}},
            {"name":"Gauge","node":2,"gpio":2,"pins":[
                {"pin":2,"indicator":"ABS"}
            ],"scenes":{"normal":"b.scene"}}
        ]
    }"#,
    )
    .unwrap();
    let mut app = App::new(None, &[]);
    app.open_rig(path.clone());
    assert!(app.rig.is_some(), "{}", app.status);
    let mut dialog = PinsDialog::new(0, 8);
    dialog.select_pin(1, &app.rig.as_ref().unwrap().rig.displays[0].pins);
    app.pins_dialog = Some(dialog);
    (app, files)
}

fn frame(
    app: &mut App,
    ctx: &egui::Context,
    width: f32,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 1000.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| app.pins_window(ctx),
    )
}

fn click(app: &mut App, ctx: &egui::Context, label: &str) {
    let _ = frame(app, ctx, 1000.0, vec![]);
    let output = frame(app, ctx, 1000.0, vec![]);
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap_or_else(|| {
            let pending = app
                .pins_dialog
                .as_ref()
                .and_then(|d| d.pending.as_ref())
                .map(|p| match p {
                    Pending::Save(Act::Count(_)) => "count",
                    Pending::Save(Act::Write(_)) => "write",
                    Pending::Save(Act::Clear(_)) => "clear",
                    Pending::Leave(_) => "leave",
                });
            panic!("missing {label}; pending {pending:?}");
        });
    for pressed in [true, false] {
        let _ = frame(
            app,
            ctx,
            1000.0,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn switching_displays_refreshes_count_and_clears_the_old_pin() {
    let (app, path) = app("switch");
    let rig = &app.rig.as_ref().unwrap().rig;
    let mut dialog = PinsDialog::new(0, 8);
    dialog.select_pin(7, &rig.displays[0].pins);
    dialog.select_display(1, &rig.displays[1].pins);
    assert_eq!(dialog.count, 2);
    assert_eq!(dialog.display, 1);
    assert!(dialog.editing.is_none());
    assert!(!dialog.pin_dirty());
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn leaving_an_unsaved_pin_or_count_never_saves_or_silently_discards() {
    let (mut app, path) = app("guard");
    let pins = app.rig.as_ref().unwrap().rig.displays[0].pins.clone();
    let dialog = app.pins_dialog.as_mut().unwrap();
    dialog.editing.as_mut().unwrap().label = "unfinished".into();
    assert!(dialog.request(Nav::Pin(2), &pins).is_none());
    assert!(matches!(dialog.pending, Some(Pending::Leave(Nav::Pin(2)))));
    assert_eq!(dialog.editing.as_ref().unwrap().label, "unfinished");
    dialog.select_pin(1, &pins);
    dialog.count = 2;
    assert!(dialog.request(Nav::Display(1), &pins).is_none());
    assert!(dialog.request(Nav::Close, &pins).is_none());
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("left stalk")
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn editing_enter_and_layout_changes_never_write_the_rig() {
    let (mut app, path) = app("typing");
    let original = std::fs::read_to_string(&path).unwrap();
    let ctx = egui::Context::default();
    click(&mut app, &ctx, "left stalk");
    let _ = frame(
        &mut app,
        &ctx,
        1000.0,
        vec![
            egui::Event::Text(" edited".into()),
            egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    for width in [360.0, 1000.0, 420.0] {
        let _ = frame(&mut app, &ctx, width, vec![]);
    }
    assert!(
        app.pins_dialog
            .as_ref()
            .unwrap()
            .editing
            .as_ref()
            .unwrap()
            .label
            .contains("edited")
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    assert!(app.pins_dialog.as_ref().unwrap().pending.is_none());
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_mapping_is_written_only_after_review_and_success_matches_the_saved_fields() {
    let (mut app, path) = app("save");
    let original = std::fs::read_to_string(&path).unwrap();
    let ctx = egui::Context::default();
    let draft = app.pins_dialog.as_mut().unwrap().editing.as_mut().unwrap();
    draft.dir = copilot::gpio::Dir::Out;
    draft.active = copilot::gpio::Level::Low;
    draft.pull = copilot::gpio::Pull::Up;
    click(&mut app, &ctx, "Review & save mapping…");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    click(&mut app, &ctx, "Save pin mapping");
    let dialog = app.pins_dialog.as_ref().unwrap();
    assert!(!dialog.pin_dirty());
    assert!(dialog.success.starts_with("Saved to disk:"));
    let saved = &app.rig.as_ref().unwrap().rig.displays[0].pins;
    assert_eq!(
        dialog.editing.as_ref().unwrap(),
        &Draft::of(saved.get(1).unwrap())
    );
    assert_ne!(std::fs::read_to_string(&path).unwrap(), original);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn close_and_remove_require_explicit_draft_or_mapping_decisions() {
    let (mut app, path) = app("close-remove");
    let original = std::fs::read_to_string(&path).unwrap();
    let ctx = egui::Context::default();
    click(&mut app, &ctx, "Remove mapping…");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    click(&mut app, &ctx, "Keep editing");
    app.pins_dialog
        .as_mut()
        .unwrap()
        .editing
        .as_mut()
        .unwrap()
        .label = "unsaved".into();
    click(&mut app, &ctx, "Close GPIO");
    assert!(app.pins_dialog.is_some());
    assert!(matches!(
        app.pins_dialog.as_ref().unwrap().pending,
        Some(Pending::Leave(Nav::Close))
    ));
    click(&mut app, &ctx, "Discard draft and continue");
    assert!(app.pins_dialog.is_none());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn the_display_picker_neither_copies_nor_saves_the_previous_count() {
    let (mut app, path) = app("display-picker");
    let original = std::fs::read_to_string(&path).unwrap();
    let ctx = egui::Context::default();
    click(&mut app, &ctx, "Cluster");
    click(&mut app, &ctx, "Gauge");
    let dialog = app.pins_dialog.as_ref().unwrap();
    assert_eq!((dialog.display, dialog.count), (1, 2));
    assert!(dialog.editing.is_none());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn reducing_the_count_requires_confirmation_before_unmapping_pins() {
    let (mut app, path) = app("count-review");
    let original = std::fs::read_to_string(&path).unwrap();
    let ctx = egui::Context::default();
    app.pins_dialog.as_mut().unwrap().count = 2;
    click(&mut app, &ctx, "Review pin count…");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    click(&mut app, &ctx, "Save pin count");
    let pins = &app.rig.as_ref().unwrap().rig.displays[0].pins;
    assert_eq!(pins.count(), 2);
    assert!(pins.get(1).is_some());
    assert!(pins.get(7).is_none());
    assert!(
        app.pins_dialog
            .as_ref()
            .unwrap()
            .success
            .starts_with("Saved to disk:")
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn failed_save_keeps_the_draft_and_never_claims_success() {
    let (mut app, path) = app("failed-save");
    let original = std::fs::read_to_string(&path).unwrap();
    app.rig.as_mut().unwrap().path = path.parent().unwrap().join("missing").join("test.rig");
    let dialog = app.pins_dialog.as_mut().unwrap();
    dialog.editing.as_mut().unwrap().label = "unsaved".into();
    let ctx = egui::Context::default();
    click(&mut app, &ctx, "Review & save mapping…");
    click(&mut app, &ctx, "Save pin mapping");
    let dialog = app.pins_dialog.as_ref().unwrap();
    assert!(dialog.pin_dirty());
    assert!(dialog.success.is_empty());
    assert!(!dialog.error.is_empty());
    assert_eq!(dialog.editing.as_ref().unwrap().label, "unsaved");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_previous_rigs_pending_save_cannot_write_to_the_new_rig() {
    let (mut first, path) = app("old-rig");
    let (second, other) = app("new-rig");
    let original = std::fs::read_to_string(&other).unwrap();
    let ctx = egui::Context::default();
    let _ = frame(&mut first, &ctx, 1000.0, vec![]);
    let dialog = first.pins_dialog.as_mut().unwrap();
    dialog.editing.as_mut().unwrap().label = "old draft".into();
    dialog.pending = Some(Pending::Save(Act::Write(Box::new(
        dialog.editing.clone().unwrap(),
    ))));
    first.rig = second.rig;
    let _ = frame(&mut first, &ctx, 1000.0, vec![]);
    let dialog = first.pins_dialog.as_ref().unwrap();
    assert!(dialog.editing.is_none());
    assert!(dialog.pending.is_none());
    assert_eq!(std::fs::read_to_string(&other).unwrap(), original);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    std::fs::remove_dir_all(other.parent().unwrap()).unwrap();
}

#[path = "threshold_tests.rs"]
mod threshold_tests;

#[test]
fn pending_actions_do_not_embed_the_full_pin_draft() {
    assert!(std::mem::size_of::<Act>() <= 64);
    assert!(std::mem::size_of::<Pending>() <= 64);
}
