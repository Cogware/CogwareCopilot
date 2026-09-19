// SPDX-License-Identifier: GPL-3.0-only
//! GPIO drafts and explicitly saved rig edits, independent of scene undo history.
//! Pin fields are spliced rather than rewriting the rig, preserving wiring comments.
//! All validation happens before a file write; saving refreshes preview sources only.

use copilot::gpio::MAX_PINS;
use copilot::scene::Step;

use crate::App;

mod draft;
mod splice;
pub(crate) use draft::{Draft, Kind};

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
pub(crate) fn quoted(s: &str) -> String {
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
            0 => "GPIO disabled for this display".into(),
            n => format!("logical pin count set to {n}"),
        };
        Ok(())
    }

    /// Write `draft` onto display `d`, replacing whatever was on that pin.
    pub(crate) fn set_pin(&mut self, d: usize, draft: &Draft) -> Result<(), String> {
        let body = draft.to_rig()?;
        let mut text = self.rig_text_for_pins()?;
        let at = [Step::Key("displays"), Step::Index(d), Step::Key("pins")];

        text = match self.pin_index(d, draft.pin)? {
            // Replacing in place keeps the list in the order somebody put it
            // in, which on a loom is the order of the wires in the plug.
            Some(i) => {
                let mut path = at.to_vec();
                path.push(Step::Index(i));
                splice::mapping(text, &path, &body)?
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
        self.status = format!("logical pin {} mapping updated", draft.pin);
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
        self.status = format!("logical pin {n} mapping removed");
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
        self.sync_driver();
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod threshold_tests;
