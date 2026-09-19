// SPDX-License-Identifier: GPL-3.0-only
//! Regression tests for immediate rig edits.

use super::*;
use copilot::cogware_can::indicators::Indicator;
use copilot::gpio::{Level as Active, Pull};

/// A pin wired to `ind`, going `dir`.
fn wired(pin: u8, dir: Dir, ind: Indicator) -> Pin {
    Pin {
        pin,
        label: None,
        dir,
        active: Active::High,
        pull: Pull::None,
        what: What::Lamp(ind),
    }
}

/// An editor whose driver has been told about `lamps`.
fn app(lamps: &[(&'static str, bool)]) -> App {
    let mut a = App::new(None, &[]);
    for (name, on) in lamps {
        a.driver.lamps.insert(name, *on);
    }
    a
}

#[test]
fn an_output_follows_the_tell_tale_driving_it() {
    let ind = Indicator::HighBeam;
    let a = app(&[(ind.name(), true)]);
    assert!(a.pin_level(&wired(6, Dir::Out, ind)) == Level::High);
}

#[test]
fn an_output_whose_tell_tale_is_out_reads_low() {
    let ind = Indicator::HighBeam;
    let a = app(&[(ind.name(), false)]);
    assert!(a.pin_level(&wired(6, Dir::Out, ind)) == Level::Low);
}

#[test]
fn a_lamp_nothing_in_the_scene_binds_is_unsaid_rather_than_low() {
    // The distinction the strip exists to keep: a relay on a lamp no
    // widget draws is not off, it is unknown here, and a dark dot would
    // claim the loom is wired and quiet when it is not wired at all.
    let a = app(&[]);
    assert!(a.pin_level(&wired(6, Dir::Out, Indicator::HighBeam)) == Level::Unsaid);
}

#[test]
fn a_button_has_no_level_between_presses() {
    let a = app(&[]);
    let p = Pin {
        what: What::Press(Action::Emit {
            name: "trip".into(),
        }),
        ..wired(3, Dir::In, Indicator::HighBeam)
    };
    assert!(a.pin_level(&p) == Level::Unsaid);
}

#[test]
fn a_pins_own_label_is_what_the_strip_says() {
    // "left stalk" is what the person holding the loom calls it.
    let mut p = wired(1, Dir::In, Indicator::TurnLeft);
    assert_eq!(describe(&p), Indicator::TurnLeft.name());
    p.label = Some("left stalk".into());
    assert_eq!(describe(&p), "left stalk");
}

#[test]
fn an_input_and_an_output_asserted_do_not_look_alike() {
    // One is this screen's doing and the other is the car's.
    assert_ne!(Level::High.color(true), Level::High.color(false));
}

#[test]
fn a_pin_with_nothing_to_say_is_an_empty_ring_not_a_dark_one() {
    // The rim is drawn regardless, so the pin is visible; it is the fill
    // that goes away. A dark fill would read as "off" on a pin whose
    // state nothing here knows.
    assert_eq!(Level::Unsaid.color(true), egui::Color32::TRANSPARENT);
    assert_ne!(Level::Low.color(true), egui::Color32::TRANSPARENT);
}

#[test]
fn logical_levels_and_tooltips_do_not_claim_a_physical_reading() {
    assert_eq!(Level::High.word(), "On");
    assert_eq!(Level::Low.word(), "Off");
    let mut pin = wired(1, Dir::In, Indicator::TurnLeft);
    pin.active = Active::Low;
    let hint = tooltip(&pin);
    assert!(hint.contains("not a raw input reading"));
    assert!(hint.contains("Active low"));
    assert!(hint.contains("No hardware is read or driven"));
}

#[test]
fn gpio_lights_keep_long_labels_out_of_the_compact_row() {
    let ctx = egui::Context::default();
    let mut texts = Vec::new();
    for _ in 0..2 {
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                dot(
                    ui,
                    3,
                    true,
                    "A deliberately long shift light wire label",
                    Level::High,
                );
                dot(ui, 4, false, "Brake switch", Level::Unsaid);
            });
        });
        texts.extend(output.shapes.iter().filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
            _ => None,
        }));
    }
    assert!(texts.iter().any(|text| text == "3"));
    assert!(texts.iter().any(|text| text == "4"));
    assert!(!texts.iter().any(|text| text.contains("shift light")));
    let mut pin = wired(3, Dir::Out, Indicator::HighBeam);
    pin.label = Some("A deliberately long shift light wire label".into());
    assert!(tooltip(&pin).contains("A deliberately long shift light wire label"));
}
