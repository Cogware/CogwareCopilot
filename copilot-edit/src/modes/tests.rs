// SPDX-License-Identifier: GPL-3.0-only
//! Regression tests for immediate rig edits.

use super::*;

/// A rig of two displays in one mode, in a directory of its own, with a
/// comment in it so the splicing can be shown not to eat it.
fn rig_on_disk(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("copilot-modes-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    for (node, size, name) in [(1u8, 200u32, "cluster"), (2, 100, "gauge")] {
        std::fs::write(
            dir.join(format!("{name}-normal.scene")),
            format!(
                r#"{{"width":{size},"height":{size},"node":{node},
                    "root":{{"type":"panel","rect":[0,0,{size},{size}],"name":"{name}"}}}}"#
            ),
        )
        .expect("scene");
    }
    std::fs::write(
        dir.join("test.rig"),
        r#"{
            // The cluster is the big one.
            "modes": ["normal"],
            "displays": [
                {"name":"cluster","node":1,"scenes":{"normal":"cluster-normal.scene"}},
                {"name":"gauge","node":2,"scenes":{"normal":"gauge-normal.scene"}},
            ],
        }"#,
    )
    .expect("rig");
    dir
}

fn open(dir: &std::path::Path) -> App {
    let mut a = App::new(None, &[]);
    a.open_rig(dir.join("test.rig"));
    assert!(a.rig.is_some(), "{}", a.status);
    a
}

#[test]
fn a_mode_can_be_added_with_any_name_the_person_likes() {
    let dir = rig_on_disk("add");
    let mut a = open(&dir);
    a.add_mode("rockcrawl", Some(0))
        .unwrap_or_else(|e| panic!("{e}"));

    let r = a.rig.as_ref().expect("a rig");
    assert_eq!(r.rig.modes, ["normal", "rockcrawl"]);
    assert_eq!(r.mode, 1, "the new mode is the one being shown");
    // One scene file per display, copied from normal.
    for f in ["cluster-rockcrawl.scene", "gauge-rockcrawl.scene"] {
        assert!(dir.join(f).exists(), "{f} was not written");
    }
    assert_eq!(r.rig.scene_path(1, 1), Some("cluster-rockcrawl.scene"));
    assert_eq!(r.rig.scene_path(2, 1), Some("gauge-rockcrawl.scene"));
    // The copy is the layout it came from, and still builds.
    assert!(a.preview.error.is_none(), "{:?}", a.preview.error);
    assert_eq!(a.preview.scene_size(), (200, 200));
    assert_eq!(a.preview.node, Some(1));
    // And the comment in the rig survived being spliced.
    assert!(r.text.contains("The cluster is the big one"), "{}", r.text);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_empty_mode_is_a_bare_panel_of_each_displays_size() {
    let dir = rig_on_disk("empty");
    let mut a = open(&dir);
    a.add_mode("valet", None).unwrap_or_else(|e| panic!("{e}"));

    assert!(a.preview.error.is_none(), "{:?}", a.preview.error);
    assert_eq!(a.preview.scene_size(), (200, 200), "the cluster's size");
    assert_eq!(a.preview.node, Some(1), "and its node");
    let r = a.rig.as_ref().expect("a rig");
    let gauge = &r.docs[1];
    assert!(gauge.preview.error.is_none(), "{:?}", gauge.preview.error);
    assert_eq!(gauge.preview.scene_size(), (100, 100));
    // Empty means empty: the panel has nothing in it to lay out yet.
    let t = a.preview.tree.as_ref().expect("a tree");
    let doc_root = t.get(copilot::widget::ROOT).unwrap().children[0];
    assert!(t.get(doc_root).unwrap().children.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_bad_or_repeated_name_is_refused_before_anything_is_written() {
    let dir = rig_on_disk("bad");
    let mut a = open(&dir);
    for bad in ["", "  ", "normal", "a/b", "a.b"] {
        let before = std::fs::read_to_string(dir.join("test.rig")).unwrap();
        assert!(a.add_mode(bad, Some(0)).is_err(), "{bad} was accepted");
        assert_eq!(
            std::fs::read_to_string(dir.join("test.rig")).unwrap(),
            before,
            "{bad} changed the rig on its way to being refused"
        );
    }
    assert_eq!(a.rig.as_ref().unwrap().rig.modes, ["normal"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_mode_can_be_renamed_and_the_files_keep_their_names() {
    let dir = rig_on_disk("rename");
    let mut a = open(&dir);
    a.add_mode("sport", Some(0))
        .unwrap_or_else(|e| panic!("{e}"));
    a.rename_mode(1, "track").unwrap_or_else(|e| panic!("{e}"));

    let r = a.rig.as_ref().expect("a rig");
    assert_eq!(r.rig.modes, ["normal", "track"]);
    // The file is still the one that was written; renaming a mode is not
    // a reason to break a card that points at it.
    assert_eq!(r.rig.scene_path(1, 1), Some("cluster-sport.scene"));
    assert!(dir.join("cluster-sport.scene").exists());
    assert!(a.preview.error.is_none(), "{:?}", a.preview.error);
    // Renaming to something already taken is refused.
    assert!(a.rename_mode(1, "normal").is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn removing_a_mode_unlists_it_and_leaves_its_scenes_alone() {
    let dir = rig_on_disk("remove");
    let mut a = open(&dir);
    a.add_mode("tow", Some(0)).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(a.rig.as_ref().unwrap().mode, 1);

    a.remove_mode(1).unwrap_or_else(|e| panic!("{e}"));
    let r = a.rig.as_ref().expect("a rig");
    assert_eq!(r.rig.modes, ["normal"]);
    assert_eq!(r.mode, 0, "back to a mode that still exists");
    assert!(
        dir.join("cluster-tow.scene").exists(),
        "a panel somebody drew must not vanish with the list entry"
    );
    assert!(a.preview.error.is_none(), "{:?}", a.preview.error);
    // The last mode cannot go.
    assert!(a.remove_mode(0).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_mode_edit_waits_for_unsaved_work() {
    let dir = rig_on_disk("dirty");
    let mut a = open(&dir);
    a.dirty = true;
    assert!(a.add_mode("tow", Some(0)).is_err());
    assert!(a.remove_mode(0).is_err());
    assert_eq!(a.rig.as_ref().unwrap().rig.modes, ["normal"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_mode_name_has_to_work_as_a_key_and_as_a_file_name() {
    let have = ["normal".to_string(), "sport".to_string()];
    assert!(check_name("rockcrawl", &have).is_ok());
    assert!(check_name("rock crawl", &have).is_ok());
    assert!(check_name("wet-weather", &have).is_ok());
    assert!(check_name("Tow_2", &have).is_ok());

    assert!(check_name("", &have).is_err());
    assert!(check_name("   ", &have).is_err());
    assert!(check_name("sport", &have).is_err(), "already there");
    for bad in ["a/b", "a.b", "a\"b", "a\\b", "a:b"] {
        assert!(check_name(bad, &have).is_err(), "{bad} should be refused");
    }
}

#[test]
fn a_new_scene_is_named_after_the_one_it_came_from() {
    assert_eq!(
        scene_name_for("z31-normal.scene", "normal", "rockcrawl"),
        "z31-rockcrawl.scene"
    );
    assert_eq!(
        scene_name_for("gauge-left-normal.scene", "normal", "tow"),
        "gauge-left-tow.scene"
    );
    // A space is a hyphen in a file name, whatever the tab says.
    assert_eq!(
        scene_name_for("z31-normal.scene", "normal", "rock crawl"),
        "z31-rock-crawl.scene"
    );
    // Not following the convention: the mode goes on the end instead.
    assert_eq!(
        scene_name_for("cluster.scene", "normal", "tow"),
        "cluster-tow.scene"
    );
    // Copying from nothing, so there is no name to swap out.
    assert_eq!(
        scene_name_for("z31-normal.scene", "", "tow"),
        "z31-normal-tow.scene"
    );
    // An extension that is not .scene is kept.
    assert_eq!(
        scene_name_for("panel-normal.json", "normal", "tow"),
        "panel-tow.json"
    );
}

#[test]
fn an_invalid_copy_source_is_refused_without_writing_files() {
    let dir = rig_on_disk("invalid-source");
    let mut app = open(&dir);
    let original = std::fs::read_to_string(dir.join("test.rig")).unwrap();
    assert!(app.add_mode("Tow", Some(usize::MAX)).is_err());
    assert_eq!(
        std::fs::read_to_string(dir.join("test.rig")).unwrap(),
        original
    );
    assert!(!dir.join("cluster-Tow.scene").exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn mode_filenames_replace_only_the_final_mode_suffix() {
    assert_eq!(
        scene_name_for("normal-panels/abnormal.scene", "normal", "tow"),
        "normal-panels/abnormal-tow.scene"
    );
    assert_eq!(
        scene_name_for("cluster-rock-crawl.scene", "rock crawl", "tow"),
        "cluster-tow.scene"
    );
    assert_eq!(scene_name_for("normal.scene", "normal", "tow"), "tow.scene");
}

#[test]
fn failed_creation_removes_only_files_created_by_that_attempt() {
    let dir = rig_on_disk("rollback");
    let created = dir.join("new.scene");
    let existing = dir.join("cluster-normal.scene");
    let original = std::fs::read_to_string(&existing).unwrap();
    assert!(
        files::with_new_scenes(
            &[
                (created.clone(), "new".into()),
                (existing.clone(), "must not overwrite".into()),
            ],
            || Ok(())
        )
        .is_err()
    );
    assert!(!created.exists());
    assert_eq!(std::fs::read_to_string(existing).unwrap(), original);
    assert!(
        files::with_new_scenes(&[(created.clone(), "new".into())], || Err(
            "rig cannot be saved".into()
        ))
        .is_err()
    );
    assert!(!created.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn mode_dialog_renders_without_writing_at_narrow_and_wide_widths() {
    let dir = rig_on_disk("windowless");
    let mut app = open(&dir);
    let original = std::fs::read_to_string(dir.join("test.rig")).unwrap();
    app.modes = Some(crate::modes_ui::ModesDialog::new(0));
    let ctx = egui::Context::default();
    for width in [360.0, 1100.0] {
        for _ in 0..2 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 900.0),
                    )),
                    ..Default::default()
                },
                |ctx| app.modes_window(ctx),
            );
        }
    }
    assert_eq!(
        std::fs::read_to_string(dir.join("test.rig")).unwrap(),
        original
    );
    std::fs::remove_dir_all(dir).unwrap();
}

fn mode_frame(app: &mut App, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 1000.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| app.modes_window(ctx),
    )
}

fn mode_click(app: &mut App, ctx: &egui::Context, label: &str) {
    let _ = mode_frame(app, ctx, vec![]);
    let output = mode_frame(app, ctx, vec![]);
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {label}"));
    for pressed in [true, false] {
        let _ = mode_frame(
            app,
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
        );
    }
}

#[test]
fn duplicate_and_remove_dialog_actions_require_explicit_confirmation() {
    let dir = rig_on_disk("mode-confirm");
    let mut app = open(&dir);
    let original = std::fs::read_to_string(dir.join("test.rig")).unwrap();
    app.modes = Some(crate::modes_ui::ModesDialog::new(0));
    let ctx = egui::Context::default();
    mode_click(&mut app, &ctx, "Duplicate…");
    mode_click(&mut app, &ctx, "Review new mode…");
    assert_eq!(
        std::fs::read_to_string(dir.join("test.rig")).unwrap(),
        original
    );
    assert!(!dir.join("cluster-normal-copy.scene").exists());
    mode_click(&mut app, &ctx, "Create mode and scenes");
    assert!(dir.join("cluster-normal-copy.scene").exists());
    assert_eq!(
        app.rig.as_ref().unwrap().rig.modes,
        ["normal", "normal copy"]
    );
    mode_click(&mut app, &ctx, "Remove…");
    assert_eq!(app.rig.as_ref().unwrap().rig.modes.len(), 2);
    mode_click(&mut app, &ctx, "Remove mode from rig");
    assert_eq!(app.rig.as_ref().unwrap().rig.modes, ["normal copy"]);
    assert!(dir.join("cluster-normal.scene").exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_pending_mode_removal_is_not_applied_to_another_rig() {
    let dir = rig_on_disk("old-pending");
    let other = rig_on_disk("new-pending");
    let mut app = open(&dir);
    app.add_mode("tow", Some(0)).unwrap();
    app.modes = Some(crate::modes_ui::ModesDialog::new(0));
    let ctx = egui::Context::default();
    mode_click(&mut app, &ctx, "Remove…");
    let next = open(&other);
    app.rig = next.rig;
    let _ = mode_frame(&mut app, &ctx, vec![]);
    let output = mode_frame(&mut app, &ctx, vec![]);
    assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.job.text == "Remove mode from rig")));
    assert_eq!(app.rig.as_ref().unwrap().rig.modes, ["normal"]);
    std::fs::remove_dir_all(dir).unwrap();
    std::fs::remove_dir_all(other).unwrap();
}
