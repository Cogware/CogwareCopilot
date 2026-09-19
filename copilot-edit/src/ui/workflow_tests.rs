// SPDX-License-Identifier: GPL-3.0-only
//! Keyboard focus and document prompts must protect unrelated drafts and edits.

use super::*;

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn frame(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) {
    let _ = ctx.run(
        egui::RawInput {
            events,
            ..Default::default()
        },
        |ctx| app.shortcuts(ctx),
    );
}

fn edited() -> App {
    let mut app = App::new(None, &[]);
    app.checkpoint();
    app.text.push_str("\n// an edit\n");
    app.dirty = true;
    app
}

#[test]
fn undo_does_not_steal_the_keyboard_from_an_unrelated_text_field() {
    let mut app = edited();
    let text = app.text.clone();
    let ctx = egui::Context::default();
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("network_password")));
    frame(
        &mut app,
        &ctx,
        vec![key(egui::Key::Z, egui::Modifiers::COMMAND)],
    );
    assert_eq!(app.text, text);
    assert_eq!(app.undo.len(), 1);
}

#[test]
fn source_field_undo_still_uses_document_history() {
    let mut app = edited();
    let ctx = egui::Context::default();
    ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("scene_source_editor")));
    frame(
        &mut app,
        &ctx,
        vec![key(egui::Key::Z, egui::Modifiers::COMMAND)],
    );
    assert!(!app.text.contains("// an edit"));
    assert!(app.undo.is_empty());
}

#[test]
fn a_document_decision_blocks_editing_shortcuts() {
    let mut app = edited();
    let text = app.text.clone();
    app.request_document(crate::document::Action::Example(0));
    frame(
        &mut app,
        &egui::Context::default(),
        vec![
            key(egui::Key::Z, egui::Modifiers::COMMAND),
            key(egui::Key::Delete, egui::Modifiers::NONE),
        ],
    );
    assert_eq!(app.text, text);
    assert_eq!(app.undo.len(), 1);
    assert!(app.pending_action.is_some());
}

#[test]
fn invalid_source_does_not_accept_canvas_editing_shortcuts() {
    let mut app = App::new(None, &[]);
    app.selected = app.doc_root();
    app.text = "{ invalid".into();
    app.reload();
    let history = app.undo.len();
    frame(
        &mut app,
        &egui::Context::default(),
        vec![
            key(egui::Key::ArrowRight, egui::Modifiers::NONE),
            key(egui::Key::Delete, egui::Modifiers::NONE),
        ],
    );
    assert_eq!(app.text, "{ invalid");
    assert_eq!(app.undo.len(), history);
}

#[test]
fn escape_cancels_a_document_replacement_without_discarding_work() {
    let mut app = edited();
    let text = app.text.clone();
    app.request_document(crate::document::Action::Example(0));
    let ctx = egui::Context::default();
    let _ = ctx.run(
        egui::RawInput {
            events: vec![key(egui::Key::Escape, egui::Modifiers::NONE)],
            ..Default::default()
        },
        |ctx| app.document_prompt(ctx),
    );
    assert!(app.pending_action.is_none());
    assert_eq!(app.text, text);
    assert!(app.dirty);
}

#[test]
fn menus_separate_document_view_and_preview_controls() {
    let mut app = App::new(None, &[]);
    let ctx = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1000.0, 700.0),
        )),
        ..Default::default()
    };
    let _ = ctx.run(input.clone(), |ctx| app.toolbar(ctx));
    let output = ctx.run(input, |ctx| app.toolbar(ctx));
    let texts: Vec<&str> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
            _ => None,
        })
        .collect();
    for label in [
        "File",
        "Edit",
        "Add widget",
        "View",
        "Help",
        "Canvas",
        "Preview",
        "Save",
        "Undo",
    ] {
        assert!(texts.contains(&label), "Missing {label}");
    }
}
