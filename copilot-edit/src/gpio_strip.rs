// SPDX-License-Identifier: GPL-3.0-only
//! The loom, live, along the top of the canvas.
//!
//! The Pins dialog says what a display's pins are *wired to*; this says what
//! they are *doing*. Authoring an output means flicking a tell-tale and
//! trusting that a relay somewhere follows it, and the only way to see that
//! before the board exists is to draw it.
//!
//! Above the canvas rather than in the side pane, because the thing it has to
//! be read against is the scene: a shift light on pin 6 and the lamp drawn at
//! the top of the cluster are one change, and a strip a tab away is a strip
//! nobody looks at while dragging a widget.

use copilot::gpio::{Dir, Pin, Pins, What};
use copilot::scene::Action;

use crate::App;

/// What the editor can say about a pin's level.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    /// Being commanded on.
    High,
    /// Being commanded off.
    Low,
    /// Nothing in this scene drives it, so the editor has no opinion.
    ///
    /// Distinct from `Low` on purpose. A relay on a lamp no widget binds is
    /// not off -- it is unknown here, and a dot drawn dark would say the rig
    /// is wired and quiet when it is really not wired at all.
    Unsaid,
}

impl Level {
    /// What fills the dot. Nothing at all when there is nothing to say.
    fn color(self, out: bool) -> egui::Color32 {
        match self {
            // Green for a commanded output, amber for an input the display
            // believes is asserted: one is this screen's doing and the other
            // is the car's, and they should not look alike.
            Self::High if out => egui::Color32::from_rgb(63, 185, 80),
            Self::High => egui::Color32::from_rgb(210, 153, 34),
            Self::Low => egui::Color32::from_rgb(56, 63, 71),
            // Unfilled rather than dark: a dot the colour of the panel behind
            // it says there is no pin there, and there is.
            Self::Unsaid => egui::Color32::TRANSPARENT,
        }
    }

    /// How it reads in a tooltip.
    fn word(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Low => "low",
            Self::Unsaid => "nothing in this scene drives it",
        }
    }
}

impl App {
    /// What the editor believes pin `p` is doing.
    ///
    /// An output follows its tell-tale, which is what the firmware does with
    /// the real one. An input is the car's to assert, and the nearest the
    /// editor has is the lamp the master would have lit on the way back --
    /// the far end of the round trip, which is the part that is drawn.
    pub(crate) fn pin_level(&self, p: &Pin) -> Level {
        let What::Lamp(ind) = p.what else {
            // A button fires an action on a press; there is no level to show
            // between presses.
            return Level::Unsaid;
        };
        match self.driver.lamps.get(ind.name()) {
            Some(true) => Level::High,
            Some(false) => Level::Low,
            None => Level::Unsaid,
        }
    }

    /// The active display's loom, or nothing when it has none.
    fn loom(&self) -> Option<&Pins> {
        let pins = &self.rig.as_ref()?.display().pins;
        (!pins.mapped().is_empty()).then_some(pins)
    }

    /// The strip along the top of the canvas: one dot per wired pin.
    pub(crate) fn gpio_strip(&mut self, ctx: &egui::Context) {
        // Read out first: the panel closure borrows `self` and the loom lives
        // on the rig inside it.
        let Some(pins) = self.loom() else {
            return;
        };
        let row: Vec<(u8, bool, String, Level)> = pins
            .mapped()
            .iter()
            .map(|p| {
                let out = p.dir == Dir::Out;
                (p.pin, out, describe(p), self.pin_level(p))
            })
            .collect();

        egui::TopBottomPanel::top("gpio").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("GPIO").small().weak());
                ui.separator();
                for (pin, out, what, level) in &row {
                    dot(ui, *pin, *out, what, *level);
                }
            });
        });
    }
}

/// The ring every pin gets, so a wired one is visible whatever it is doing.
const RIM: egui::Color32 = egui::Color32::from_rgb(110, 118, 129);

/// One pin: its dot, its number, and what it is wired to.
///
/// Two channels, kept separate so neither has to be learned from the other.
/// The rim's weight is the direction -- heavy for a pin this display drives,
/// light for one it only listens to -- and the fill is the level. An empty
/// ring is a pin nothing here has an opinion about, which is a third state
/// and not a dark version of off.
fn dot(ui: &mut egui::Ui, pin: u8, out: bool, what: &str, level: Level) {
    let size = egui::vec2(12.0, 12.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let centre = rect.center();
    let painter = ui.painter();
    let fill = level.color(out);
    if fill != egui::Color32::TRANSPARENT {
        painter.circle_filled(centre, 5.0, fill);
    }
    let weight = if out { 2.0_f32 } else { 1.0_f32 };
    painter.circle_stroke(centre, 5.0, egui::Stroke::new(weight, RIM));
    ui.label(egui::RichText::new(format!("{pin}")).small());
    let way = if out { "driven here" } else { "read here" };
    response.on_hover_text(format!(
        "pin {pin} — {what}\n{way}, currently {}",
        level.word()
    ));
}

/// What a pin is wired to, for the strip and its tooltip.
///
/// The author's own label wins where there is one: "left stalk" is what the
/// person holding the loom calls it, and `TURN_LEFT` is what the bus does.
fn describe(p: &Pin) -> String {
    if let Some(label) = &p.label {
        return label.clone();
    }
    match &p.what {
        What::Lamp(i) => i.name().to_string(),
        What::Press(a) => format!("button: {}", action_word(a)),
    }
}

/// The shortest honest name for an action, for a tooltip.
fn action_word(a: &Action) -> String {
    match a {
        Action::Page { pager, .. } => match pager {
            Some(name) => format!("turn {name}"),
            None => "turn the page".to_string(),
        },
        Action::Menu { menu, .. } => format!("menu {menu}"),
        Action::Mode { mode } => format!("mode {mode}"),
        Action::Emit { name } => name.clone(),
    }
}

#[cfg(test)]
mod tests {
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
}
