// SPDX-License-Identifier: GPL-3.0-only
//! Document changes must be deliberate, and a failed save must leave work reachable.

use super::*;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

struct Directory(PathBuf);

#[test]
fn exporting_a_scene_copy_keeps_the_original_document_and_unsaved_state() {
    let dir = Directory::new();
    let path = dir.scene("original.scene");
    let mut app = App::new(Some(path.clone()), &[]);
    app.text.push_str("\n// unsaved edit\n");
    app.dirty = true;
    let copy = dir.0.join("copy.scene");
    app.export_scene_copy(&copy);
    assert_eq!(app.path, Some(path.clone()));
    assert!(app.dirty);
    assert!(
        std::fs::read_to_string(copy)
            .expect("copy")
            .contains("// unsaved edit")
    );
    assert!(
        !std::fs::read_to_string(path)
            .expect("original")
            .contains("// unsaved edit")
    );
}

#[test]
fn reopening_a_rig_example_does_not_overwrite_the_previous_saved_copy() {
    let (label, _, example) = crate::EXAMPLES
        .iter()
        .find(|(_, _, example)| matches!(example, crate::starter::Example::Rig(_)))
        .expect("rig example");
    let mut app = App::new(None, &[]);
    app.open_example(label, example);
    let first = app.path.clone().expect("unpacked scene");
    app.text.push_str("\n// saved custom layout\n");
    app.dirty = true;
    app.save();
    app.open_example(label, example);
    assert_ne!(app.path.as_ref(), Some(&first));
    assert!(
        std::fs::read_to_string(&first)
            .expect("saved copy")
            .contains("// saved custom layout")
    );
    std::fs::remove_dir_all(first.parent().expect("first directory"))
        .expect("remove first fixture");
    std::fs::remove_dir_all(
        app.path
            .as_ref()
            .expect("current scene")
            .parent()
            .expect("directory"),
    )
    .expect("remove second fixture");
}

impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "copilot-document-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).expect("test directory");
        Self(path)
    }

    fn scene(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, crate::STARTER).expect("scene");
        path
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn open_new_and_examples_all_protect_unsaved_changes() {
    for action in [
        Action::New(NewDoc::default()),
        Action::Open(PathBuf::from("another.scene")),
        Action::Example(0),
    ] {
        let mut app = App::new(None, &[]);
        app.dirty = true;
        let before = app.text.clone();
        app.request_document(action);
        assert!(app.pending_action.is_some());
        assert!(app.document_blocked());
        assert_eq!(app.text, before);
    }
}

#[test]
fn discard_is_explicit_and_opening_a_file_resets_old_history() {
    let dir = Directory::new();
    let path = dir.scene("next.scene");
    let mut app = App::new(None, &[]);
    app.dirty = true;
    app.checkpoint();
    app.request_document(Action::Open(path.clone()));
    app.finish_document_prompt(false);
    assert_eq!(app.path.as_deref(), Some(path.as_path()));
    assert!(!app.dirty);
    assert!(app.undo.is_empty() && app.redo.is_empty());
    assert!(app.pending_action.is_none());
}

#[test]
fn a_failed_save_does_not_continue_to_the_next_document() {
    let dir = Directory::new();
    let mut app = App::new(None, &[]);
    app.path = Some(dir.0.join("missing").join("old.scene"));
    app.dirty = true;
    let before = app.text.clone();
    app.request_document(Action::New(NewDoc::default()));
    app.finish_document_prompt(true);
    assert!(app.pending_action.is_some());
    assert!(app.dirty);
    assert_eq!(app.text, before);
}

#[test]
fn successful_save_then_new_keeps_the_previous_work_on_disk() {
    let dir = Directory::new();
    let path = dir.scene("old.scene");
    let mut app = App::new(Some(path.clone()), &[]);
    app.text.push_str("\n// edited\n");
    app.dirty = true;
    let before = app.text.clone();
    app.request_document(Action::New(NewDoc::default()));
    app.finish_document_prompt(true);
    assert_eq!(std::fs::read_to_string(path).expect("saved"), before);
    assert!(app.path.is_none());
    assert!(app.dirty);
    assert!(app.pending_action.is_none());
}

#[test]
fn unreadable_files_leave_the_current_document_and_path_unchanged() {
    let dir = Directory::new();
    let path = dir.scene("old.scene");
    let mut app = App::new(Some(path.clone()), &[]);
    let before = app.text.clone();
    app.open_document(dir.0.join("absent.scene"));
    assert_eq!(app.text, before);
    assert_eq!(app.path, Some(path));
}

#[test]
fn failed_save_as_keeps_the_old_destination() {
    let dir = Directory::new();
    let old = dir.scene("old.scene");
    let mut app = App::new(Some(old.clone()), &[]);
    app.dirty = true;
    assert!(!app.write_scene(&dir.0.join("absent").join("new.scene")));
    assert_eq!(app.path.as_deref(), Some(old.as_path()));
    assert!(app.dirty);
}

#[test]
fn invalid_files_open_in_source_without_showing_the_previous_scene() {
    let dir = Directory::new();
    let bad = dir.0.join("bad.scene");
    std::fs::write(&bad, "{ broken").expect("invalid scene");
    let mut app = App::new(None, &[]);
    app.open_document(bad);
    assert!(app.preview.tree.is_none());
    assert!(app.preview.error.is_some());
    assert!(app.pane == Pane::Source);
}

#[test]
fn formatting_can_restore_comments_with_one_undo() {
    let mut app = App::new(None, &[]);
    app.text.push_str("\n// keep this comment\n");
    let before = app.text.clone();
    app.tidy();
    assert!(!app.text.contains("keep this comment"));
    app.undo();
    assert_eq!(app.text, before);
}

#[test]
fn document_titles_identify_unsaved_work() {
    let mut app = App::new(None, &[]);
    assert!(app.document_title().contains("Untitled scene"));
    app.path = Some(Path::new("my-scene.scene").into());
    app.dirty = true;
    assert!(app.document_title().starts_with("* my-scene.scene"));
}

#[test]
fn save_all_preserves_a_failure_even_when_another_display_saves() {
    let dir = Directory::new();
    dir.scene("first.scene");
    dir.scene("second.scene");
    let path = dir.0.join("car.rig");
    std::fs::write(
        &path,
        r#"{
        "modes":["normal"],"displays":[
        {"name":"first","node":1,"scenes":{"normal":"first.scene"}},
        {"name":"second","node":2,"scenes":{"normal":"second.scene"}}]}"#,
    )
    .expect("rig");
    let mut app = App::new(Some(path), &[]);
    app.text = "{ invalid".into();
    app.dirty = true;
    app.reload();
    let other = &mut app.rig.as_mut().expect("rig").docs[1];
    other.text.push_str("\n// changed\n");
    other.dirty = true;
    app.save_all();
    assert_eq!(app.unsaved(), 1);
    assert!(app.status.contains("remain unsaved"));
    assert!(app.status.contains("Saved 1 scene"));
    assert!(
        std::fs::read_to_string(dir.0.join("second.scene"))
            .expect("saved other display")
            .contains("// changed")
    );
}

#[test]
fn a_new_untitled_scene_cannot_inherit_the_previous_property_drafts_or_curve() {
    let mut app = App::new(None, &[]);
    app.text = r##"{
        "width": 100, "height": 100,
        "root": {"type": "line", "rect": [0, 0, 100, 100],
                 "points": [[0, 0], [1, 1]], "color": "#ffffff"}
    }"##
    .into();
    app.reload();
    let selected = app.doc_root().expect("root");
    app.selected = Some(selected);
    let (shape, field) = crate::curve::Shape::of(
        &app.preview
            .tree
            .as_ref()
            .unwrap()
            .get(selected)
            .unwrap()
            .kind,
    )
    .expect("editable line");
    app.start_curve(selected, shape, field);
    let old_scope = app.properties_scope();
    app.start(&NewDoc::default());
    app.selected = app.doc_root();
    assert_eq!(
        app.selected,
        Some(selected),
        "this fixture reuses the node ID"
    );
    assert_ne!(app.properties_scope(), old_scope);
    assert!(app.curve.is_none());
    assert!(app.drag.is_none());
}

#[test]
fn opening_another_rig_dismisses_configuration_for_the_previous_document() {
    let mut app = App::new(None, &[]);
    app.modes = Some(crate::modes_ui::ModesDialog::new(0));
    app.pins_dialog = Some(crate::pins_ui::PinsDialog::new(0, 8));
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/z31.rig");
    app.open_document(path);
    assert!(app.rig.is_some());
    assert!(app.modes.is_none());
    assert!(app.pins_dialog.is_none());
}

#[test]
fn frame_rate_widgets_keep_their_actual_kind_and_friendly_name() {
    let mut app = App::new(None, &[]);
    app.text = r##"{
        "width": 100, "height": 100,
        "root": {"type": "fps", "rect": [0, 0, 100, 20]}
    }"##
    .into();
    app.reload();
    let root = app.doc_root().expect("frame-rate widget");
    let node = app.preview.tree.as_ref().unwrap().get(root).unwrap();
    assert_eq!(crate::kind_name(&node.kind), "fps");
    assert_eq!(
        crate::describe_node(node),
        crate::starter::widget_title("fps")
    );
}
