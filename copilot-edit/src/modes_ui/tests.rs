// SPDX-License-Identifier: GPL-3.0-only
//! Naming and unsaved-mode-switch regressions.

use super::*;

#[test]
fn suggested_names_are_unique_valid_and_utf8_safe() {
    let existing = vec!["New mode".into(), "New mode 2".into()];
    assert_eq!(suggested_name("New mode", &existing), "New mode 3");
    for base in ["é".repeat(30), "Sport / tow".into(), " ".into()] {
        let name = suggested_name(&base, &[]);
        assert!(crate::modes::check_name(&name, &[]).is_ok(), "{name}");
    }
}

#[test]
fn duplicate_prepares_a_copy_without_a_file_operation() {
    let modes = vec!["Normal".into(), "Normal copy".into()];
    let mut dialog = ModesDialog::new(1);
    dialog.duplicate(0, &modes);
    assert_eq!(dialog.copy_from, Some(0));
    assert_eq!(dialog.name, "Normal copy 2");
    assert!(dialog.pending.is_none());
}

#[test]
fn a_failed_save_keeps_the_switch_question_and_reports_remaining_work() {
    let mut app = App::new(None, &[]);
    app.text = "{".into();
    app.reload();
    app.dirty = true;
    app.path = Some(std::path::PathBuf::from("must-not-be-written.scene"));
    app.switching = Some(1);
    app.save_and_switch(1);
    assert_eq!(app.switching, Some(1));
    assert!(app.dirty);
    assert!(app.status.starts_with("not switched:"));
}
