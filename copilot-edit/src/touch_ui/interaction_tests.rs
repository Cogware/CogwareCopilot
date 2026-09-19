// SPDX-License-Identifier: GPL-3.0-only
//! Headless regressions for staged touch actions and page destinations.

use super::*;

#[test]
fn a_numeric_page_name_is_not_rewritten_as_an_index() {
    let action = Action::Page {
        pager: Some("pages".into()),
        to: Page::Named("1".into()),
    };
    let draft = Draft::of(&action);
    assert!(draft.complete());
    assert_eq!(tests::round(&draft), action);
}

#[test]
fn invalid_page_indices_stay_pending() {
    for text in ["-1", "4294967296", "1.5", "not a number"] {
        let d = Draft {
            to: text.into(),
            ..Draft::default()
        };
        assert!(!d.complete(), "{text}");
    }
}

#[test]
fn an_incomplete_action_and_typing_survive_repeated_ui_frames() {
    let ctx = egui::Context::default();
    let current = Action::Page {
        pager: None,
        to: Page::Next,
    };
    for frame in 0..3 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::Grid::new("touch-test").show(ui, |ui| {
                    let id = ui.id().with("touch-draft");
                    if frame == 0 {
                        ui.data_mut(|data| {
                            data.insert_temp(
                                id,
                                TouchState {
                                    source: Some(Draft::of(&current).to_scene()),
                                    draft: Some(Draft {
                                        action: "menu",
                                        menu: "partly typed".into(),
                                        ..Draft::default()
                                    }),
                                },
                            )
                        });
                    }
                    assert!(touch_rows(ui, Some(&current)).is_none());
                    let state = ui.data(|data| data.get_temp::<TouchState>(id)).unwrap();
                    let draft = state.draft.unwrap();
                    assert_eq!(draft.action, "menu");
                    assert_eq!(draft.menu, "partly typed");
                });
            });
        });
    }
}

#[test]
fn external_source_edits_replace_a_stale_touch_draft() {
    let original = Action::Page {
        pager: None,
        to: Page::Next,
    };
    let mut state = TouchState::default();
    state.sync(Some(&original));
    state.draft.as_mut().unwrap().action = "menu";
    state.sync(Some(&original));
    assert_eq!(state.draft.as_ref().unwrap().action, "menu");
    let changed = Action::Emit {
        name: "reset".into(),
    };
    state.sync(Some(&changed));
    assert_eq!(state.draft, Some(Draft::of(&changed)));
}

#[test]
fn action_and_button_labels_do_not_change_serialised_names() {
    for action in ACTIONS {
        assert_ne!(action_title(action), *action);
    }
    assert_eq!(button_title(Button::Centre), "Centre / confirm");
    assert_eq!(button_name(Button::Centre), "centre");
}
