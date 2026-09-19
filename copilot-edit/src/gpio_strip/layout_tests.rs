// SPDX-License-Identifier: GPL-3.0-only
//! A full bank stays one compact row even in a narrow editor.

use super::*;
use crate::rig::{Doc, RigSession};

fn rig_app(count: u8) -> App {
    let mappings: Vec<_> = (1..=count).map(|pin| format!(
        r#"{{"pin":{pin},"label":"A long user-defined wire label which must not wrap","dir":"out","indicator":"HIGH_BEAM"}}"#
    )).collect();
    let text = format!(
        r#"{{"modes":["normal"],"displays":[{{"name":"Cluster","node":"0x01","gpio":{count},"pins":[{}],"scenes":{{"normal":"a.scene"}}}}]}}"#,
        mappings.join(","),
    );
    let mut app = App::new(None, &[]);
    app.rig = Some(RigSession {
        rig: copilot::rig::parse(&text).expect("test rig"),
        text,
        path: "preview-layout.rig".into(),
        mode: 0,
        active: 0,
        docs: vec![Doc {
            path: None,
            text: String::new(),
            dirty: false,
            preview: crate::preview::Preview::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            texture: None,
        }],
    });
    app
}

fn frame(
    app: &mut App,
    ctx: &egui::Context,
    width: f32,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, f32) {
    let mut height = 0.0;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 700.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            app.gpio_strip(ctx);
            height = ctx.available_rect().top();
        },
    );
    (output, height)
}

#[test]
fn thirty_two_pins_never_wrap_or_increase_the_preview_height() {
    for width in [280.0, 400.0, 1000.0] {
        let mut heights = Vec::new();
        for count in [8, 32] {
            let mut app = rig_app(count);
            let ctx = egui::Context::default();
            let _ = frame(&mut app, &ctx, width, vec![]);
            let (output, height) = frame(&mut app, &ctx, width, vec![]);
            assert!(
                height <= 44.0,
                "{count} pins at {width} used {height} points"
            );
            heights.push(height);
            let mut lights = Vec::new();
            for shape in &output.shapes {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.job.text.parse::<u8>().is_ok()
                {
                    let visible = egui::Rect::from_min_size(text.pos, text.galley.size())
                        .intersect(shape.clip_rect);
                    if visible.is_positive() {
                        lights.push(visible);
                    }
                }
            }
            assert!(!lights.is_empty());
            for pair in lights.windows(2) {
                assert!((pair[0].center().y - pair[1].center().y).abs() < 1.0);
                assert!(pair[0].right() <= pair[1].left(), "number labels overlap");
            }
        }
        assert!((heights[0] - heights[1]).abs() <= 1.0);
    }
}

#[test]
fn clicking_a_light_opens_its_pin_without_changing_the_rig() {
    let mut app = rig_app(32);
    let before = app.rig.as_ref().unwrap().text.clone();
    let ctx = egui::Context::default();
    let _ = frame(&mut app, &ctx, 600.0, vec![]);
    let (output, _) = frame(&mut app, &ctx, 600.0, vec![]);
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "2" => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .expect("second pin");
    for pressed in [true, false] {
        frame(
            &mut app,
            &ctx,
            600.0,
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
    assert!(app.pins_dialog.is_some());
    assert_eq!(app.rig.as_ref().unwrap().text, before);
}

#[test]
fn horizontal_scrolling_reaches_the_last_pin_without_wrapping() {
    let mut app = rig_app(32);
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&mut app, &ctx, 320.0, vec![]);
    }
    let _ = frame(
        &mut app,
        &ctx,
        320.0,
        vec![
            egui::Event::PointerMoved(egui::pos2(240.0, 12.0)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(-2000.0, 0.0),
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    for _ in 0..30 {
        frame(&mut app, &ctx, 320.0, vec![]);
    }
    let (output, height) = frame(&mut app, &ctx, 320.0, vec![]);
    let last_visible = output.shapes.iter().any(|shape| {
        if let egui::Shape::Text(text) = &shape.shape
            && text.galley.job.text == "32"
        {
            let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
            return shape.clip_rect.contains_rect(rect);
        }
        false
    });
    assert!(last_visible, "horizontal scroll must expose pin 32");
    assert!(height <= 44.0);
}
