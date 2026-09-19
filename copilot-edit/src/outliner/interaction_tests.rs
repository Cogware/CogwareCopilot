// SPDX-License-Identifier: GPL-3.0-only
//! Full inspector regressions with an egui context and no native window.

use super::*;

fn draw(app: &mut App) -> egui::FullOutput {
    let ctx = egui::Context::default();
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 2400.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_max_width(460.0);
                app.properties_pane(ui);
            });
        },
    )
}

fn text(output: &egui::FullOutput) -> String {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn inspecting_every_type_keeps_scene_text_and_history_unchanged() {
    for kind in [
        "panel",
        "pager",
        "menu",
        "frame",
        "label",
        "fps",
        "image",
        "anim",
        "bar",
        "arc",
        "needle",
        "scale",
        "sevenseg",
        "gradient",
        "ruler",
        "segbar",
        "grid",
        "led",
        "roundrect",
        "line",
        "polygon",
        "chart",
    ] {
        let required = match kind {
            "menu" => r#","menu":"display""#,
            "label" | "sevenseg" => r#","text":"hello""#,
            "image" => r#","image":0"#,
            "anim" => r#","anim":0,"speed":-2.5"#,
            _ => "",
        };
        let source = format!(
            r#"{{"width":64,"height":64,
            // Preserve this comment and the authored key order.
            "root":{{"name":"selected","rect":[-10,5,40,30],"type":"{kind}"{required}}}}}"#
        );
        let mut app = crate::tests::app(&source, "selected");
        let undo = app.undo.len();
        let _ = draw(&mut app);
        assert_eq!(app.text, source, "{kind}");
        assert_eq!(app.undo.len(), undo, "{kind}");
    }
}

#[test]
fn invalid_scene_text_explains_why_properties_are_unavailable() {
    let source =
        r#"{"width":10,"height":10,"root":{"type":"panel","name":"root","rect":[0,0,10,10]}}"#;
    let mut app = crate::tests::app(source, "root");
    app.text.push_str("broken");
    app.reload();
    assert!(app.preview.error.is_some());
    let before = app.text.clone();
    assert!(text(&draw(&mut app)).contains("Fix Scene source before editing properties"));
    assert_eq!(app.text, before);
}

#[test]
fn a_visibility_binding_on_a_panel_is_not_hidden() {
    let source = r#"{"width":10,"height":10,"root":{"type":"panel","name":"root","rect":[0,0,10,10],
        "bind":{"indicator":"OIL_PRESSURE","property":"visible"}}}"#;
    let mut app = crate::tests::app(source, "root");
    assert!(text(&draw(&mut app)).contains("Oil Pressure"));
    assert_eq!(app.text, source);
}

#[test]
fn a_document_dialog_blocks_tree_selection_and_explains_held_properties() {
    let source = r#"{"width":20,"height":20,"root":{"type":"panel","name":"root","rect":[0,0,20,20],
        "children":[{"type":"bar","name":"other widget","rect":[0,0,10,10]}]}}"#;
    let mut app = crate::tests::app(source, "root");
    let selected = app.selected;
    app.resize_to = Some((40, 40));
    assert!(text(&draw(&mut app)).contains("Finish or cancel the open dialog"));
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| app.outliner(ctx));
    let position = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::epaint::Shape::Text(text) if text.galley.text().contains("other widget") => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .expect("the disabled tree is still visible");
    for pressed in [true, false] {
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ctx| app.outliner(ctx),
        );
    }
    assert_eq!(app.selected, selected);
    assert_eq!(app.text, source);
}
