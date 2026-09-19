// SPDX-License-Identifier: GPL-3.0-only
//! Pointer regressions driven through a real egui frame without a window.

use crate::{App, tests::app};

#[test]
fn document_dialogs_block_canvas_edits_and_camera_gestures() {
    let mut a = editor();
    let ctx = egui::Context::default();
    let out = frame(&mut a, &ctx, vec![]);
    let origin = image_rect(&a, &out).min;
    let before = a.text.clone();
    let selected = a.selected;
    let camera = a.camera;
    a.resize_to = Some((400, 200));
    let from = origin + egui::vec2(70.0, 80.0);
    drag(
        &mut a,
        &ctx,
        from,
        from + egui::vec2(30.0, 10.0),
        egui::PointerButton::Primary,
    );
    drag(
        &mut a,
        &ctx,
        from,
        from + egui::vec2(30.0, 10.0),
        egui::PointerButton::Middle,
    );
    click(
        &mut a,
        &ctx,
        origin + egui::vec2(180.0, 80.0),
        egui::Modifiers::NONE,
    );
    assert_eq!(a.text, before);
    assert_eq!(a.selected, selected);
    assert_eq!(a.camera, camera);
    assert!(a.undo.is_empty());
}

#[test]
fn a_dialog_ends_an_interrupted_canvas_drag_without_an_extra_edit() {
    let mut a = editor();
    let ctx = egui::Context::default();
    let out = frame(&mut a, &ctx, vec![]);
    let from = image_rect(&a, &out).min + egui::vec2(70.0, 80.0);
    let _ = frame(
        &mut a,
        &ctx,
        button(
            from,
            egui::PointerButton::Primary,
            true,
            egui::Modifiers::NONE,
        ),
    );
    let _ = frame(
        &mut a,
        &ctx,
        vec![egui::Event::PointerMoved(from + egui::vec2(20.0, 10.0))],
    );
    assert!(a.drag.is_some());
    assert!(a.gesture.is_some());
    let edited = a.text.clone();
    a.asking_close = true;
    let _ = frame(
        &mut a,
        &ctx,
        vec![egui::Event::PointerMoved(from + egui::vec2(50.0, 30.0))],
    );
    assert_eq!(a.text, edited);
    assert!(a.drag.is_none());
    assert!(a.gesture.is_none());
}

#[test]
fn long_selection_readouts_do_not_move_the_canvas_in_a_narrow_window() {
    let mut a = editor();
    let ctx = egui::Context::default();
    let mut top = 0.0;
    let mut height = |a: &mut App| {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(240.0, 600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    a.canvas_header(ui);
                    top = ui.available_rect_before_wrap().top();
                });
            },
        );
        top
    };
    let _ = height(&mut a);
    let before = height(&mut a);
    let selected = a.selected.unwrap();
    a.set_field_of(
        selected,
        None,
        "name",
        r#""A very long widget name that must not wrap the canvas while dragging""#,
    );
    a.set_field_of(selected, None, "rect", "[-10000, 12345, 4321, 9876]");
    let after = height(&mut a);
    assert_eq!(before, after);
    a.select(None);
    assert_eq!(height(&mut a), before);
}

#[test]
fn a_marquee_still_selects_multiple_widgets_without_editing() {
    let mut a = editor();
    let before = a.text.clone();
    let ctx = egui::Context::default();
    let out = frame(&mut a, &ctx, vec![]);
    let origin = image_rect(&a, &out).min;
    drag(
        &mut a,
        &ctx,
        origin + egui::vec2(30.0, 40.0),
        origin + egui::vec2(220.0, 120.0),
        egui::PointerButton::Primary,
    );
    assert_eq!(a.members().len(), 2);
    assert_eq!(a.text, before);
    assert!(a.undo.is_empty());
}

#[test]
fn a_resize_handle_in_the_letterbox_has_the_resize_cursor() {
    let mut a = editor();
    a.select(a.doc_root());
    let ctx = egui::Context::default();
    let out = frame(&mut a, &ctx, vec![]);
    let corner = image_rect(&a, &out).min - egui::vec2(4.0, 4.0);
    let out = frame(&mut a, &ctx, vec![egui::Event::PointerMoved(corner)]);
    assert_eq!(
        out.platform_output.cursor_icon,
        egui::CursorIcon::ResizeNwSe
    );
}

const SCENE: &str = r#"{
 "width":400,"height":200,
 "root":{"type":"panel","rect":[0,0,400,200],"name":"root","children":[
 {"type":"bar","rect":[40,50,60,60],"name":"one"},
 {"type":"bar","rect":[150,50,60,60],"name":"two"}
 ]}}"#;

fn editor() -> App {
    let mut a = app(SCENE, "one");
    a.camera.actual_size();
    a.snapping = false;
    a.grid = 0;
    a
}

fn frame(a: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let modifiers = events
        .iter()
        .find_map(|e| match e {
            egui::Event::PointerButton { modifiers, .. } => Some(*modifiers),
            _ => None,
        })
        .unwrap_or_default();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            modifiers,
            ..Default::default()
        },
        |ctx| a.canvas(ctx, 0, 0),
    )
}

fn image_rect(a: &App, out: &egui::FullOutput) -> egui::Rect {
    let id = a.texture.as_ref().unwrap().id();
    fn find(shape: &egui::Shape, id: egui::TextureId) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Rect(rect) if rect.fill_texture_id == id => Some(rect.rect),
            egui::Shape::Mesh(mesh) if mesh.texture_id == id => Some(egui::Rect::from_points(
                &mesh.vertices.iter().map(|v| v.pos).collect::<Vec<_>>(),
            )),
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, id)),
            _ => None,
        }
    }
    out.shapes
        .iter()
        .find_map(|s| find(&s.shape, id))
        .expect("the scene is drawn as an image")
}

fn button(
    pos: egui::Pos2,
    button: egui::PointerButton,
    pressed: bool,
    modifiers: egui::Modifiers,
) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers,
        },
    ]
}

fn click(a: &mut App, ctx: &egui::Context, pos: egui::Pos2, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        let _ = frame(
            a,
            ctx,
            button(pos, egui::PointerButton::Primary, pressed, modifiers),
        );
    }
}

fn drag(
    a: &mut App,
    ctx: &egui::Context,
    from: egui::Pos2,
    to: egui::Pos2,
    which: egui::PointerButton,
) {
    let _ = frame(a, ctx, button(from, which, true, egui::Modifiers::NONE));
    let _ = frame(a, ctx, vec![egui::Event::PointerMoved(to)]);
    let _ = frame(a, ctx, button(to, which, false, egui::Modifiers::NONE));
}

#[test]
fn middle_drag_and_wheel_work_in_the_letterbox_without_editing() {
    let mut a = editor();
    let ctx = egui::Context::default();
    let out = frame(&mut a, &ctx, vec![]);
    let placed = image_rect(&a, &out);
    let from = placed.min - egui::vec2(30.0, 30.0);
    let before = a.text.clone();
    let selected = a.selected;
    drag(
        &mut a,
        &ctx,
        from,
        from + egui::vec2(64.0, -24.0),
        egui::PointerButton::Middle,
    );
    assert_eq!(a.camera.pan, (64.0, -24.0));
    assert_eq!(a.text, before);
    assert_eq!(a.selected, selected);
    let _ = frame(
        &mut a,
        &ctx,
        vec![
            egui::Event::PointerMoved(from),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 100.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(a.camera.zoom.unwrap() > 1.0);
    assert_eq!(a.text, before);
}

#[test]
fn a_group_move_is_still_one_undo_step() {
    let mut a = editor();
    let one = a.selected.unwrap();
    let two = a.preview.tree.as_ref().unwrap().find("two").unwrap();
    a.select_many(vec![one, two]);
    let before = a.text.clone();
    let ctx = egui::Context::default();
    let out = frame(&mut a, &ctx, vec![]);
    let origin = image_rect(&a, &out).min;
    let from = origin + egui::vec2(70.0, 80.0);
    drag(
        &mut a,
        &ctx,
        from,
        from + egui::vec2(17.0, 9.0),
        egui::PointerButton::Primary,
    );
    let tree = a.preview.tree.as_ref().unwrap();
    assert_eq!(
        tree.get(one).unwrap().rect,
        copilot::Rect::new(57, 59, 60, 60)
    );
    assert_eq!(
        tree.get(two).unwrap().rect,
        copilot::Rect::new(167, 59, 60, 60)
    );
    assert_eq!(a.members(), vec![one, two]);
    assert_eq!(a.undo.len(), 1);
    assert!(a.gesture.is_none());
    a.undo();
    assert_eq!(a.text, before);
}

#[test]
fn shift_click_keeps_multi_selection_and_reports_the_remaining_widget() {
    let mut a = editor();
    let ctx = egui::Context::default();
    let out = frame(&mut a, &ctx, vec![]);
    let origin = image_rect(&a, &out).min;
    click(
        &mut a,
        &ctx,
        origin + egui::vec2(180.0, 80.0),
        egui::Modifiers::SHIFT,
    );
    assert_eq!(a.members().len(), 2);
    click(
        &mut a,
        &ctx,
        origin + egui::vec2(70.0, 80.0),
        egui::Modifiers::SHIFT,
    );
    assert_eq!(a.members().len(), 1);
    assert!(a.status.contains("two"), "{}", a.status);
    assert!(!a.status.contains("one"), "{}", a.status);
    assert!(a.undo.is_empty());
}

#[test]
fn hovering_an_unselected_widget_offers_a_move_cursor() {
    let mut a = editor();
    let ctx = egui::Context::default();
    let out = frame(&mut a, &ctx, vec![]);
    let origin = image_rect(&a, &out).min;
    let out = frame(
        &mut a,
        &ctx,
        vec![egui::Event::PointerMoved(origin + egui::vec2(180.0, 80.0))],
    );
    assert_eq!(out.platform_output.cursor_icon, egui::CursorIcon::Grab);
    let _ = frame(
        &mut a,
        &ctx,
        vec![egui::Event::PointerMoved(origin - egui::vec2(20.0, 20.0))],
    );
    assert!(a.hover.is_none());
}

#[test]
fn an_invalid_preview_preserves_the_source_typing_gesture() {
    let mut a = editor();
    let gesture = egui::Id::new("source edit");
    a.gesture = Some(gesture);
    a.text.push('{');
    a.reload();
    let ctx = egui::Context::default();
    let _ = frame(&mut a, &ctx, vec![]);
    assert_eq!(a.gesture, Some(gesture));
}

#[test]
fn a_stale_preview_explains_itself_and_cannot_edit_the_buffer() {
    let mut a = editor();
    a.text.push('{');
    a.reload();
    assert!(a.preview.error.is_some());
    let before = a.text.clone();
    let ctx = egui::Context::default();
    let out = frame(&mut a, &ctx, vec![]);
    assert!(out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text.contains("Preview out of date"))));
    let origin = image_rect(&a, &out).min;
    let from = origin + egui::vec2(70.0, 80.0);
    drag(
        &mut a,
        &ctx,
        from,
        from + egui::vec2(30.0, 20.0),
        egui::PointerButton::Primary,
    );
    assert_eq!(a.text, before);
    assert!(a.undo.is_empty());
    let out = frame(&mut a, &ctx, vec![]);
    let source = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == "Show source" => {
                Some(t.pos + t.galley.size() * 0.5)
            }
            _ => None,
        })
        .expect("the preview offers a route to the source");
    click(&mut a, &ctx, source, egui::Modifiers::NONE);
    assert!(matches!(a.pane, crate::Pane::Source));
}
