// SPDX-License-Identifier: GPL-3.0-only
//! Regression tests for immediate rig edits.

use super::test_support::{Fixture, TestApp};
use super::*;
use copilot::cogware_can::indicators::Indicator;
use copilot::gpio::{Dir, Level, Pin, PinError, Pull, What};

/// A rig on disk with one display, in a directory of its own.
pub(super) fn rig_on_disk(tag: &str) -> Fixture {
    let files = Fixture::new(tag);
    let dir = files.parent().unwrap();
    std::fs::write(
        dir.join("a.scene"),
        r#"{ "width": 64, "height": 64, "node": "0x01",
             "root": { "type": "panel", "rect": [0, 0, 64, 64] } }"#,
    )
    .expect("the scene");
    std::fs::write(
        &files,
        r#"// Which screen is which, and which wire is which.
{
  "modes": ["normal"],
  "displays": [
// The cluster, behind the wheel.
{ "name": "cluster", "node": "0x01", "gpio": 8,
  "scenes": { "normal": "a.scene" } },
  ],
}"#,
    )
    .expect("the rig");
    files
}

fn open(files: Fixture) -> TestApp {
    TestApp::open(files)
}

/// The display's pins as the rig on disk now reads them.
fn pins_of(a: &App) -> copilot::gpio::Pins {
    let text = std::fs::read_to_string(&a.rig.as_ref().unwrap().path).expect("read back");
    copilot::rig::parse(&text).expect("parse back").displays[0]
        .pins
        .clone()
}

fn lamp(pin: u8, name: &str) -> Draft {
    Draft {
        indicator: name.into(),
        ..Draft::fresh(pin)
    }
}

#[test]
fn a_pin_is_written_and_reads_back_as_what_was_asked_for() {
    let mut a = open(rig_on_disk("write"));
    a.set_pin(0, &lamp(1, "TURN_LEFT")).expect("must write");
    let pins = pins_of(&a);
    assert_eq!(pins.count(), 8);
    assert_eq!(
        pins.get(1).and_then(Pin::indicator),
        Some(Indicator::TurnLeft)
    );
}

#[test]
fn the_rigs_comments_survive_a_pin_edit() {
    // The whole reason these splice rather than rewrite: a rig says which
    // screen is which, and an editor that ate that would not be used.
    let mut a = open(rig_on_disk("comments"));
    a.set_pin(0, &lamp(1, "TURN_LEFT")).expect("must write");
    let text = std::fs::read_to_string(&a.rig.as_ref().unwrap().path).expect("read back");
    assert!(text.contains("Which screen is which"));
    assert!(text.contains("The cluster, behind the wheel."));
}

#[test]
fn every_field_a_pin_carries_survives_the_trip() {
    let mut a = open(rig_on_disk("fields"));
    let d = Draft {
        label: "high beam feed".into(),
        active: Level::Low,
        pull: Pull::Up,
        ..lamp(3, "HIGH_BEAM")
    };
    a.set_pin(0, &d).expect("must write");
    let pins = pins_of(&a);
    let p = pins.get(3).expect("pin 3");
    assert_eq!(p.label.as_deref(), Some("high beam feed"));
    assert_eq!(p.active, Level::Low);
    assert_eq!(p.pull, Pull::Up);
    assert_eq!(Draft::of(p), d, "and the dialog reopens on the same thing");
}

#[test]
fn a_default_pin_writes_none_of_the_keys_it_need_not() {
    // A hand-written rig and an edited one should look like one file.
    let d = lamp(1, "ABS");
    let out = d.to_rig().unwrap();
    assert!(!out.contains("dir"), "{out}");
    assert!(!out.contains("active"), "{out}");
    assert!(!out.contains("pull"), "{out}");
    assert!(!out.contains("label"), "{out}");
}

#[test]
fn an_output_writes_no_level_or_pull_because_neither_means_anything() {
    let d = Draft {
        dir: Dir::Out,
        active: Level::Low,
        pull: Pull::Up,
        ..lamp(6, "SHIFT_LIGHT")
    };
    let out = d.to_rig().unwrap();
    assert!(out.contains(r#""dir": "out""#), "{out}");
    assert!(!out.contains("active"), "{out}");
    assert!(!out.contains("pull"), "{out}");
}

#[test]
fn rewriting_a_pin_replaces_it_rather_than_listing_it_twice() {
    let mut a = open(rig_on_disk("replace"));
    a.set_pin(0, &lamp(1, "TURN_LEFT")).expect("first");
    a.set_pin(0, &lamp(1, "TURN_RIGHT")).expect("second");
    let pins = pins_of(&a);
    assert_eq!(pins.mapped().len(), 1, "the pin was listed twice");
    assert_eq!(
        pins.get(1).and_then(Pin::indicator),
        Some(Indicator::TurnRight)
    );
}

#[test]
fn a_pin_can_be_taken_off_without_changing_the_count() {
    let mut a = open(rig_on_disk("clear"));
    a.set_pin(0, &lamp(1, "TURN_LEFT")).expect("write");
    a.set_pin(0, &lamp(2, "TURN_RIGHT")).expect("write");
    a.clear_pin(0, 1).expect("clear");
    let pins = pins_of(&a);
    assert_eq!(pins.count(), 8, "the display still has eight pins");
    assert!(pins.get(1).is_none());
    assert!(pins.get(2).is_some(), "the other pin was taken too");
}

#[test]
fn clearing_a_pin_that_is_wired_to_nothing_is_not_an_error() {
    let mut a = open(rig_on_disk("clear-none"));
    assert!(a.clear_pin(0, 4).is_ok());
}

#[test]
fn lowering_the_count_unwires_the_pins_that_no_longer_exist() {
    // A rig listing pin 9 on a four-pin display will not open at all, so
    // the count and the list have to move together.
    let mut a = open(rig_on_disk("shrink"));
    for (pin, name) in [(1, "TURN_LEFT"), (5, "ABS"), (8, "BRAKE")] {
        a.set_pin(0, &lamp(pin, name)).expect("write");
    }
    assert_eq!(a.pins_lost_by(0, 4), 2, "pins 5 and 8 would go");
    a.set_pin_count(0, 4).expect("shrink");
    let pins = pins_of(&a);
    assert_eq!(pins.count(), 4);
    assert_eq!(pins.mapped().len(), 1);
    assert!(pins.get(1).is_some());
}

#[test]
fn dropping_the_count_to_nothing_leaves_no_pin_keys_behind() {
    let mut a = open(rig_on_disk("zero"));
    a.set_pin(0, &lamp(1, "TURN_LEFT")).expect("write");
    a.set_pin_count(0, 0).expect("zero");
    let pins = pins_of(&a);
    assert!(pins.is_empty());
    let text = std::fs::read_to_string(&a.rig.as_ref().unwrap().path).expect("read back");
    assert!(!text.contains("gpio"), "{text}");
    assert!(!text.contains("\"pins\""), "{text}");
}

#[test]
fn raising_the_count_leaves_the_wiring_alone() {
    let mut a = open(rig_on_disk("grow"));
    a.set_pin(0, &lamp(1, "TURN_LEFT")).expect("write");
    a.set_pin_count(0, 16).expect("grow");
    let pins = pins_of(&a);
    assert_eq!(pins.count(), 16);
    assert_eq!(pins.mapped().len(), 1);
}

#[test]
fn a_count_past_what_a_word_holds_is_refused_before_anything_is_written() {
    let mut a = open(rig_on_disk("toobig"));
    assert!(a.set_pin_count(0, 33).is_err());
    assert_eq!(pins_of(&a).count(), 8, "the rig was written anyway");
}

#[test]
fn a_pin_wired_to_nothing_is_refused_rather_than_written_broken() {
    let mut a = open(rig_on_disk("empty"));
    let d = Draft::fresh(1);
    assert!(!d.complete());
    assert!(a.set_pin(0, &d).is_err());
    assert!(pins_of(&a).get(1).is_none());
}

#[test]
fn a_button_pin_round_trips_through_the_rig() {
    let mut a = open(rig_on_disk("button"));
    let mut action = crate::touch_ui::Draft::default();
    action.set_action("menu");
    action.set_menu("display");
    let d = Draft {
        kind: Kind::Button,
        action,
        ..Draft::fresh(5)
    };
    assert!(d.complete());
    a.set_pin(0, &d).expect("must write");
    let pins = pins_of(&a);
    assert!(matches!(
        pins.get(5).map(Pin::what),
        Some(What::Press(copilot::scene::Action::Menu { .. }))
    ));
}

#[test]
fn a_button_pin_turning_a_page_is_not_complete_until_it_names_the_pager() {
    // The one thing a pin needs that a widget does not: it is not in the
    // tree, so there is nothing to look up from.
    let mut action = crate::touch_ui::Draft::default();
    action.set_action("page");
    let mut d = Draft {
        kind: Kind::Button,
        action,
        ..Draft::fresh(5)
    };
    assert!(!d.complete(), "a page action with no pager was accepted");
    d.action.set_pager("pages");
    assert!(d.complete());
}

#[test]
fn an_output_cannot_be_wired_to_an_action() {
    let mut action = crate::touch_ui::Draft::default();
    action.set_action("emit");
    action.set_emit("horn");
    let d = Draft {
        dir: Dir::Out,
        kind: Kind::Button,
        action,
        ..Draft::fresh(6)
    };
    assert!(!d.complete(), "an output took an action");
}

#[test]
fn a_splice_the_rig_parser_refuses_leaves_the_file_as_it_was() {
    // The re-parse is the check. Writing first and validating afterwards
    // is how an editor destroys the only good copy.
    let mut a = open(rig_on_disk("refused"));
    a.set_pin(0, &lamp(1, "TURN_LEFT")).expect("write");
    let before = std::fs::read_to_string(&a.rig.as_ref().unwrap().path).expect("read");
    let bad = Draft {
        indicator: "WARP_CORE".into(),
        ..Draft::fresh(2)
    };
    let err = a.set_pin(0, &bad).expect_err("that lamp does not exist");
    assert!(err.contains("known indicator"), "{err}");
    let after = std::fs::read_to_string(&a.rig.as_ref().unwrap().path).expect("read");
    assert_eq!(before, after);
    // And the reason survives, so the dialog can say which lamp.
    assert!(format!("{:?}", PinError::UnknownIndicator("WARP_CORE".into())).contains("WARP_CORE"));
}
