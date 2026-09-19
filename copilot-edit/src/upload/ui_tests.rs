// SPDX-License-Identifier: GPL-3.0-only
//! Upload configuration follows the document without mutating an in-flight transfer.

use super::*;

#[test]
fn a_scene_supplies_its_display_address_and_defaults_to_a_scene_upload() {
    let mut app = crate::App::new(None, &[]);
    app.preview.node = Some(7);
    app.open_upload();
    assert_eq!(app.uploader.node, 7);
    assert_eq!(app.uploader.slot, 0);
    assert!(!app.uploader.rig);
    assert!(app.uploader.destinations.displays.is_empty());
}

#[test]
fn a_rig_supplies_the_active_display_mode_and_readable_choices() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/z31.rig");
    let mut app = crate::App::new(Some(path), &[]);
    let rig = app.rig.as_ref().expect("example rig");
    let node = rig.display().node;
    let slot = rig.mode;
    let names = rig.rig.modes.clone();
    let displays: Vec<_> = rig
        .rig
        .displays
        .iter()
        .map(|display| (display.name.clone(), display.node))
        .collect();
    app.open_upload();
    assert_eq!(app.uploader.node, node);
    assert_eq!(usize::from(app.uploader.slot), slot);
    assert_eq!(app.uploader.destinations.displays, displays);
    assert_eq!(app.uploader.destinations.modes, names);
    assert!(!app.uploader.rig);
}

#[test]
fn opening_another_document_cannot_retarget_an_active_upload() {
    use super::super::tests_support::Fake;
    let mut uploader = Uploader {
        doing: Doing::Working(Box::new(Work {
            link: Box::new(Fake::default()),
            push: Push::new(2, 1, 0, vec![1; 10]),
        })),
        node: 2,
        slot: 3,
        ..Default::default()
    };
    uploader.configure_destination(
        Some(9),
        Some(6),
        vec![("Other".into(), 9)],
        vec!["New".into()],
    );
    uploader.open(true);
    assert_eq!(uploader.node, 2);
    assert_eq!(uploader.slot, 3);
    assert!(!uploader.rig);
    assert!(uploader.destinations.displays.is_empty());
    assert!(matches!(uploader.doing, Doing::Working(_)));
}

fn frame(
    uploader: &mut Uploader,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    requested: &mut Option<bool>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 800.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                uploader.ask(ui, false, |rig| {
                    *requested = Some(rig);
                    None
                });
            });
        },
    )
}

fn click(uploader: &mut Uploader, ctx: &egui::Context, label: &str, requested: &mut Option<bool>) {
    let _ = frame(uploader, ctx, vec![], requested);
    let output = frame(uploader, ctx, vec![], requested);
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing {label}"));
    for pressed in [true, false] {
        let _ = frame(
            uploader,
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            requested,
        );
    }
}

#[test]
fn upload_requests_bytes_for_the_kind_selected_at_click_time() {
    for rig in [false, true] {
        let mut uploader = Uploader::default();
        uploader.configure_destination(
            Some(2),
            Some(1),
            vec![("Dashboard".into(), 2)],
            vec!["Road".into(), "Track".into()],
        );
        let ctx = egui::Context::default();
        let mut requested = None;
        click(&mut uploader, &ctx, "Controller Wi-Fi", &mut requested);
        uploader.rig = rig;
        click(
            &mut uploader,
            &ctx,
            if rig { "Upload rig" } else { "Upload scene" },
            &mut requested,
        );
        assert_eq!(requested, Some(rig));
        assert!(matches!(uploader.doing, Doing::Said(Err(_))));
    }
}
