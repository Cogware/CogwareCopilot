// SPDX-License-Identifier: GPL-3.0-only
//! Setting a display's pin count, and saying what each pin is wired to.
//!
//! Apart from [`crate::pins_ui`] for the reason [`crate::modes`] is apart from
//! the dialog that asks for one: what a pin edit *is* can be tested without a
//! window, and is, while the other half is the buttons.
//!
//! # Why these write to disk immediately
//!
//! Same as a mode edit: these change the *rig*, not the scene in front, and
//! the rig is not what Ctrl+Z undoes. The alternative -- a pin map that is
//! only real once somebody remembers to save -- is how a loom ends up wired
//! to a file nobody wrote down.
//!
//! # Why the rig is spliced rather than rewritten
//!
//! A rig file is hand-written and carries comments saying which screen is
//! which and which wire is which. Rewriting it from the parsed [`Rig`] would
//! lose them, so the edits go through [`copilot::scene::edit`], exactly as a
//! widget's properties do.
//!
//! [`Rig`]: copilot::rig::Rig

use copilot::gpio::{Dir, Level, MAX_PINS, Pin, Pull, What};
use copilot::scene::Step;

use crate::App;

/// A pin as the dialog holds it, before it is written.
///
/// Every field of both kinds at once, rather than one or the other, so that
/// switching a pin from a lamp to a button and back does not lose what was
/// picked on the way past -- the same reason
/// [`crate::touch_ui::Draft`](crate::touch_ui) keeps all four actions.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Draft {
    /// Which pin, 1..=count.
    pub pin: u8,
    /// What a person calls the wire.
    pub label: String,
    pub dir: Dir,
    pub active: Level,
    pub pull: Pull,
    /// Whether it carries a tell-tale rather than firing an action.
    pub is_lamp: bool,
    /// The tell-tale, by the spec's name for it.
    pub indicator: String,
    /// The action, as the touch rows write one.
    pub action: crate::touch_ui::Draft,
}

impl Draft {
    /// A pin wired to nothing yet.
    ///
    /// Opens on a tell-tale, because that is what all but one or two of a
    /// display's pins turn out to be.
    pub(crate) fn fresh(pin: u8) -> Self {
        Self {
            pin,
            label: String::new(),
            dir: Dir::In,
            active: Level::High,
            pull: Pull::None,
            is_lamp: true,
            indicator: String::new(),
            action: crate::touch_ui::Draft::default(),
        }
    }

    /// The draft an existing pin opens on.
    pub(crate) fn of(p: &Pin) -> Self {
        let mut d = Self::fresh(p.pin);
        d.label = p.label.clone().unwrap_or_default();
        d.dir = p.dir;
        d.active = p.active;
        d.pull = p.pull;
        match &p.what {
            What::Lamp(i) => {
                d.is_lamp = true;
                d.indicator = i.name().into();
            }
            What::Press(a) => {
                d.is_lamp = false;
                d.action = crate::touch_ui::Draft::of(a);
            }
        }
        d
    }

    /// Whether it names everything its kind needs.
    ///
    /// A pin with no tell-tale picked would fail to parse, and the rig would
    /// be refused on the way back in with the dialog still open over it.
    pub(crate) fn complete(&self) -> bool {
        if self.is_lamp {
            return !self.indicator.is_empty();
        }
        // An output has nothing to read from an action.
        self.dir == Dir::In && self.action.complete_for_pin()
    }

    /// The `pins` entry as it should be written into the rig.
    ///
    /// Only the keys that differ from what the parser assumes, so a hand-
    /// written rig and an edited one look like the same file.
    pub(crate) fn to_rig(&self) -> String {
        let mut out = format!(r#"{{ "pin": {}"#, self.pin);
        if !self.label.trim().is_empty() {
            out.push_str(&format!(r#", "label": {}"#, quoted(self.label.trim())));
        }
        if self.dir != Dir::In {
            out.push_str(&format!(r#", "dir": "{}""#, self.dir.name()));
        }
        // Both are the board's business on an output, so writing them there
        // would put a field in the file that nothing ever reads.
        if self.dir == Dir::In {
            if self.active != Level::High {
                out.push_str(&format!(r#", "active": "{}""#, self.active.name()));
            }
            if self.pull != Pull::None {
                out.push_str(&format!(r#", "pull": "{}""#, self.pull.name()));
            }
        }
        if self.is_lamp {
            out.push_str(&format!(r#", "indicator": {}"#, quoted(&self.indicator)));
        } else {
            out.push_str(&self.action.to_pin_fields());
        }
        out.push_str(" }");
        out
    }
}

impl crate::touch_ui::Draft {
    /// The action's fields, for a `pins` entry rather than a `touch` object.
    ///
    /// The same field names in the same shapes -- it is the same action -- but
    /// spliced into the pin's own object rather than nested in one of its own.
    pub(crate) fn to_pin_fields(&self) -> String {
        let inner = self.to_scene();
        // `to_scene` writes `{ "action": ... }`; a pin wants the middle of it.
        let inner = inner
            .strip_prefix("{ ")
            .and_then(|s| s.strip_suffix(" }"))
            .unwrap_or(&inner);
        format!(", {inner}")
    }

    /// Whether it names everything a *pin* needs, which is one thing more
    /// than a widget: a pin turning a page has to say which pager, because it
    /// is not in the tree and has nothing to look up from.
    pub(crate) fn complete_for_pin(&self) -> bool {
        self.complete() && (self.action() != "page" || !self.pager().trim().is_empty())
    }
}

/// A string as JSON, with the characters that would end it escaped.
fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl App {
    /// Set how many pins display `d` has.
    ///
    /// Lowering it unwires every pin past the new count, because a rig that
    /// lists pin 9 on an eight-pin display will not open at all. The dialog
    /// says how many that is before it asks.
    pub(crate) fn set_pin_count(&mut self, d: usize, count: u8) -> Result<(), String> {
        if count > MAX_PINS {
            return Err(format!("a display cannot have more than {MAX_PINS} pins"));
        }
        let mut text = self.rig_text_for_pins()?;
        // Dropped highest first, so each removal's index is still the index
        // the one before it left behind.
        for i in self.pin_indices_past(d, count)?.into_iter().rev() {
            text = copilot::scene::remove(
                &text,
                &[
                    Step::Key("displays"),
                    Step::Index(d),
                    Step::Key("pins"),
                    Step::Index(i),
                ],
            )
            .ok_or("could not take that pin off the rig")?;
        }
        text = if count == 0 {
            // A display with no pins says nothing rather than saying zero.
            let text =
                copilot::scene::unset(&text, &[Step::Key("displays"), Step::Index(d)], "pins")
                    .unwrap_or(text);
            copilot::scene::unset(&text, &[Step::Key("displays"), Step::Index(d)], "gpio")
                .unwrap_or(text)
        } else {
            copilot::scene::set(
                &text,
                &[Step::Key("displays"), Step::Index(d)],
                "gpio",
                &count.to_string(),
            )
            .ok_or("could not write the pin count")?
        };
        self.commit_rig_pins(text)?;
        self.status = match count {
            0 => "that display has no pins now".into(),
            n => format!("that display has {n} pins"),
        };
        Ok(())
    }

    /// Write `draft` onto display `d`, replacing whatever was on that pin.
    pub(crate) fn set_pin(&mut self, d: usize, draft: &Draft) -> Result<(), String> {
        if !draft.complete() {
            return Err("that pin is not wired to anything yet".into());
        }
        let body = draft.to_rig();
        let mut text = self.rig_text_for_pins()?;
        let at = [Step::Key("displays"), Step::Index(d), Step::Key("pins")];

        text = match self.pin_index(d, draft.pin)? {
            // Replacing in place keeps the list in the order somebody put it
            // in, which on a loom is the order of the wires in the plug.
            Some(i) => {
                let mut path = at.to_vec();
                path.push(Step::Index(i));
                copilot::scene::replace(&text, &path, &body)
                    .ok_or("could not write that pin onto the rig")?
            }
            None => match copilot::scene::append(&text, &at, &body) {
                Some(t) => t,
                // The display has no `pins` key yet.
                None => copilot::scene::set(
                    &text,
                    &[Step::Key("displays"), Step::Index(d)],
                    "pins",
                    &format!("[{body}]"),
                )
                .ok_or("could not start that display's pin list")?,
            },
        };
        self.commit_rig_pins(text)?;
        self.status = format!("pin {} is wired", draft.pin);
        Ok(())
    }

    /// Take pin `n` off display `d`, leaving the pin itself in the count.
    pub(crate) fn clear_pin(&mut self, d: usize, n: u8) -> Result<(), String> {
        let Some(i) = self.pin_index(d, n)? else {
            return Ok(());
        };
        let text = copilot::scene::remove(
            &self.rig_text_for_pins()?,
            &[
                Step::Key("displays"),
                Step::Index(d),
                Step::Key("pins"),
                Step::Index(i),
            ],
        )
        .ok_or("could not take that pin off the rig")?;
        self.commit_rig_pins(text)?;
        self.status = format!("pin {n} is wired to nothing");
        Ok(())
    }

    /// Where pin `n` sits in display `d`'s list, if it is in it.
    fn pin_index(&self, d: usize, n: u8) -> Result<Option<usize>, String> {
        let session = self.rig.as_ref().ok_or("no rig is open")?;
        let display = session
            .rig
            .displays
            .get(d)
            .ok_or("there is no such display")?;
        Ok(display.pins.mapped().iter().position(|p| p.pin == n))
    }

    /// The list positions of display `d`'s pins numbered above `count`.
    fn pin_indices_past(&self, d: usize, count: u8) -> Result<Vec<usize>, String> {
        let session = self.rig.as_ref().ok_or("no rig is open")?;
        let display = session
            .rig
            .displays
            .get(d)
            .ok_or("there is no such display")?;
        Ok(display
            .pins
            .mapped()
            .iter()
            .enumerate()
            .filter(|(_, p)| p.pin > count)
            .map(|(i, _)| i)
            .collect())
    }

    /// How many of display `d`'s pins a count of `count` would unwire.
    ///
    /// For the dialog to say so before it asks, rather than after.
    pub(crate) fn pins_lost_by(&self, d: usize, count: u8) -> usize {
        self.pin_indices_past(d, count).map_or(0, |v| v.len())
    }

    /// The rig's text, or the reason there is none.
    fn rig_text_for_pins(&self) -> Result<String, String> {
        self.rig
            .as_ref()
            .map(|r| r.text.clone())
            .ok_or_else(|| "no rig is open".into())
    }

    /// Write `text` as the rig and re-read it.
    ///
    /// The re-parse is the check, exactly as it is for a mode edit: a splice
    /// that produced something the rig parser will not take is refused here,
    /// with the file on disk left as it was. Unlike a mode edit, nothing
    /// reloads: a pin map changes no scene and no display is showing anything
    /// different because of it.
    fn commit_rig_pins(&mut self, text: String) -> Result<(), String> {
        let rig =
            copilot::rig::parse(&text).map_err(|e| format!("that would break the rig: {e:?}"))?;
        let session = self.rig.as_mut().ok_or("no rig is open")?;
        std::fs::write(&session.path, &text)
            .map_err(|e| format!("cannot write {}: {e}", session.path.display()))?;
        session.text = text;
        session.rig = rig;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use copilot::cogware_can::indicators::Indicator;
    use copilot::gpio::PinError;

    /// A rig on disk with one display, in a directory of its own.
    fn rig_on_disk(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("copilot-pins-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        std::fs::write(
            dir.join("a.scene"),
            r#"{ "width": 64, "height": 64, "node": "0x01",
                 "root": { "type": "panel", "rect": [0, 0, 64, 64] } }"#,
        )
        .expect("the scene");
        std::fs::write(
            dir.join("r.rig"),
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
        dir.join("r.rig")
    }

    fn open(path: std::path::PathBuf) -> App {
        let mut a = App::new(None, &[]);
        a.open_rig(path);
        assert!(a.rig.is_some(), "{}", a.status);
        a
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
        let out = d.to_rig();
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
        let out = d.to_rig();
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
            is_lamp: false,
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
            is_lamp: false,
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
            is_lamp: false,
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
        assert!(err.contains("break the rig"), "{err}");
        let after = std::fs::read_to_string(&a.rig.as_ref().unwrap().path).expect("read");
        assert_eq!(before, after);
        // And the reason survives, so the dialog can say which lamp.
        assert!(
            format!("{:?}", PinError::UnknownIndicator("WARP_CORE".into())).contains("WARP_CORE")
        );
    }
}
