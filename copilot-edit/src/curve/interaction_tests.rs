// SPDX-License-Identifier: GPL-3.0-only
//! Curve hit targets, history boundaries and toolbar actions without a window.

use super::*;

#[test]
fn document_dialogs_block_curve_buttons_dragging_and_delete() {
    let mut a = editor();
    let before = a.text.clone();
    a.asking_close = true;
    a.curve.as_mut().unwrap().picked = Some(1);
    let ctx = egui::Context::default();
    click_label(&mut a, &ctx, "Add point");
    click_label(&mut a, &ctx, "Remove point");
    click_label(&mut a, &ctx, "Done");
    drag(
        &mut a,
        &ctx,
        egui::pos2(200.0, 200.0),
        egui::pos2(240.0, 220.0),
        egui::PointerButton::Primary,
    );
    let _ = frame(
        &mut a,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Delete,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(a.text, before);
    assert!(a.curve.is_some());
    assert!(a.undo.is_empty());
    a.asking_close = false;
    click_label(&mut a, &ctx, "Done");
    assert!(a.curve.is_none());
}

#[test]
fn a_dialog_ends_an_interrupted_point_drag_without_an_extra_edit() {
    let mut a = editor();
    let ctx = egui::Context::default();
    let _ = frame(&mut a, &ctx, vec![]);
    let _ = frame(
        &mut a,
        &ctx,
        button(egui::pos2(200.0, 200.0), egui::PointerButton::Primary, true),
    );
    let _ = frame(
        &mut a,
        &ctx,
        vec![egui::Event::PointerMoved(egui::pos2(240.0, 220.0))],
    );
    assert!(a.curve.as_ref().unwrap().drag.is_some());
    assert!(a.gesture.is_some());
    let edited = a.text.clone();
    a.asking_close = true;
    let _ = frame(
        &mut a,
        &ctx,
        vec![egui::Event::PointerMoved(egui::pos2(260.0, 230.0))],
    );
    assert_eq!(a.text, edited);
    assert!(a.curve.as_ref().unwrap().drag.is_none());
    assert!(a.gesture.is_none());
}

const SCENE: &str = r#"{"width":400,"height":400,"root":{"type":"panel","rect":[0,0,400,400],"name":"root","children":[
{"type":"line","rect":[100,100,200,200],"name":"curve","points":[[0,1],[0.5,0.5],[1,0]]}]}}"#;

fn editor() -> App {
    let mut a = crate::tests::app(SCENE, "curve");
    a.start_curve(a.selected.unwrap(), Shape::Points, "points");
    a
}

fn frame(a: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            a.curve_toolbar(ctx);
            egui::CentralPanel::default().show(ctx, |ui| {
                a.curve_overlay(
                    ui,
                    &crate::view::View {
                        origin: (0.0, 0.0),
                        scale: 1.0,
                    },
                )
            });
        },
    )
}

fn button(pos: egui::Pos2, which: egui::PointerButton, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: which,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

fn drag(
    a: &mut App,
    ctx: &egui::Context,
    from: egui::Pos2,
    to: egui::Pos2,
    which: egui::PointerButton,
) {
    let _ = frame(a, ctx, button(from, which, true));
    let _ = frame(a, ctx, vec![egui::Event::PointerMoved(to)]);
    let _ = frame(a, ctx, button(to, which, false));
}

fn click_label(a: &mut App, ctx: &egui::Context, label: &str) {
    let out = frame(a, ctx, vec![]);
    let pos = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == label => {
                Some(t.pos + t.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {label}"));
    for pressed in [true, false] {
        let _ = frame(a, ctx, button(pos, egui::PointerButton::Primary, pressed));
    }
}

#[test]
fn a_fast_drag_uses_the_handle_where_the_press_began() {
    let mut a = editor();
    let ctx = egui::Context::default();
    let _ = frame(&mut a, &ctx, vec![]);
    drag(
        &mut a,
        &ctx,
        egui::pos2(200.0, 200.0),
        egui::pos2(240.0, 220.0),
        egui::PointerButton::Primary,
    );
    assert_eq!(a.curve_handles()[1], (0.7, 0.6));
    assert_eq!(a.undo.len(), 1);
    assert!(a.gesture.is_none());
}

#[test]
fn edge_handles_can_be_grabbed_from_outside_the_widget_box() {
    let mut a = editor();
    let ctx = egui::Context::default();
    let _ = frame(&mut a, &ctx, vec![]);
    drag(
        &mut a,
        &ctx,
        egui::pos2(95.0, 300.0),
        egui::pos2(130.0, 270.0),
        egui::PointerButton::Primary,
    );
    assert_eq!(a.curve_handles()[0], (0.15, 0.85));
}

#[test]
fn separate_point_drags_remain_separate_undo_steps() {
    let mut a = editor();
    let initial = a.text.clone();
    let ctx = egui::Context::default();
    let _ = frame(&mut a, &ctx, vec![]);
    drag(
        &mut a,
        &ctx,
        egui::pos2(200.0, 200.0),
        egui::pos2(240.0, 220.0),
        egui::PointerButton::Primary,
    );
    let first = a.text.clone();
    drag(
        &mut a,
        &ctx,
        egui::pos2(240.0, 220.0),
        egui::pos2(260.0, 230.0),
        egui::PointerButton::Primary,
    );
    assert_eq!(a.undo.len(), 2);
    a.undo();
    assert_eq!(a.text, first);
    a.undo();
    assert_eq!(a.text, initial);
}

#[test]
fn middle_drag_does_not_move_curve_points() {
    let mut a = editor();
    let before = a.text.clone();
    let ctx = egui::Context::default();
    let _ = frame(&mut a, &ctx, vec![]);
    drag(
        &mut a,
        &ctx,
        egui::pos2(200.0, 200.0),
        egui::pos2(240.0, 220.0),
        egui::PointerButton::Middle,
    );
    assert_eq!(a.text, before);
    assert!(a.undo.is_empty());
}

#[test]
fn add_remove_undo_redo_and_done_are_working_toolbar_actions() {
    let mut a = editor();
    let ctx = egui::Context::default();
    click_label(&mut a, &ctx, "Add point");
    assert_eq!(a.curve_handles().len(), 4);
    assert!(a.curve.as_ref().unwrap().picked.is_some());
    click_label(&mut a, &ctx, "Undo");
    assert_eq!(a.curve_handles().len(), 3);
    click_label(&mut a, &ctx, "Redo");
    assert_eq!(a.curve_handles().len(), 4);
    click_label(&mut a, &ctx, "Add point");
    click_label(&mut a, &ctx, "Remove point");
    assert_eq!(a.curve_handles().len(), 4);
    click_label(&mut a, &ctx, "Done");
    assert!(a.curve.is_none());
    assert!(a.dirty);
}

#[test]
fn a_curve_session_can_undo_even_when_the_history_was_already_full() {
    let mut a = crate::tests::app(SCENE, "curve");
    a.undo = (0..256).map(|i| format!("older snapshot {i}")).collect();
    a.start_curve(a.selected.unwrap(), Shape::Points, "points");
    let before = a.text.clone();
    let ctx = egui::Context::default();
    click_label(&mut a, &ctx, "Add point");
    assert_eq!(a.undo.len(), 256);
    click_label(&mut a, &ctx, "Undo");
    assert_eq!(a.text, before);
    assert_eq!(a.curve.as_ref().unwrap().history_start, a.undo.len());
}

#[test]
fn curve_focus_uses_rig_coordinates_and_fits_small_panels() {
    let mut a = crate::tests::app(SCENE, "curve");
    a.camera.zoom = Some(0.75);
    a.camera.pan = (17.0, -9.0);
    let before = a.camera;
    a.start_curve(a.selected.unwrap(), Shape::Points, "points");
    a.frame_curve((1000, 400), (700, 0), (200.0, 140.0));
    let v = a.camera.view((1000, 400), (0.0, 0.0), (200.0, 140.0));
    let centre = v.to_screen(copilot::Point { x: 900, y: 200 });
    assert!((centre.0 - 100.0).abs() < 0.01);
    assert!((centre.1 - 70.0).abs() < 0.01);
    let corner = v.to_screen(copilot::Point { x: 800, y: 100 });
    assert!(corner.0 >= 14.9 && corner.1 >= 14.9);
    a.finish_curve();
    assert_eq!(a.camera, before);
}

#[test]
fn narrow_toolbar_wraps_all_actions_inside_the_window() {
    let mut a = editor();
    let ctx = egui::Context::default();
    for _ in 0..2 {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(240.0, 500.0),
                )),
                ..Default::default()
            },
            |ctx| a.curve_toolbar(ctx),
        );
    }
    let out = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(240.0, 500.0),
            )),
            ..Default::default()
        },
        |ctx| a.curve_toolbar(ctx),
    );
    for label in ["Done", "Add point", "Remove point", "Undo", "Redo"] {
        let bounds = out
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == label => {
                    Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!(
                    "missing {label}; visible labels: {:?}",
                    out.shapes
                        .iter()
                        .filter_map(|s| match &s.shape {
                            egui::Shape::Text(t) => Some((&t.galley.job.text, t.pos)),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                )
            });
        assert!(
            bounds.min.x >= 0.0 && bounds.max.x <= 240.0,
            "{label}: {bounds:?}"
        );
        assert!(bounds.max.y <= 300.0, "{label}: {bounds:?}");
    }
}

#[test]
fn delete_does_not_take_a_focused_text_fields_key() {
    let mut a = editor();
    a.curve.as_mut().unwrap().picked = Some(1);
    let ctx = egui::Context::default();
    let input = egui::Id::new("curve test text");
    let mut text = String::from("name");
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut text).id(input))
                .request_focus();
        });
    });
    let _ = frame(
        &mut a,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Delete,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(a.curve_handles().len(), 3);
}
