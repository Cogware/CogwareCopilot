// SPDX-License-Identifier: GPL-3.0-only
//! The Touch rows of the properties pane: what a widget does when a finger
//! lands on it.
//!
//! Shaped after [`crate::bind_ui`] and for the same reasons. A touch action is
//! not a property of the kind -- any widget can be pressed, and two panels can
//! do two different things -- and picking the action changes which of the
//! other rows mean anything, so the whole `touch` object is rewritten whenever
//! any part of it changes. It is one short line, and splicing a key inside it
//! would need the editor to know where inside the widget's text it sits.

use copilot::menu::Button;
use copilot::scene::touch::{Action, Page, button_name};

/// What the rows asked for.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum TouchEdit {
    /// Write this `touch` object onto the widget.
    Set(String),
    /// Take the widget's `touch` off.
    Unset,
}

/// The action names, in the order the picker offers them.
///
/// Page first because it is the one the toolkit carries out itself; the rest
/// are things the host is asked to do.
pub(crate) const ACTIONS: &[&str] = &["page", "menu", "mode", "emit"];

/// A line saying what each action is for, since the word alone does not say.
fn about(action: &str) -> &'static str {
    match action {
        "page" => "Turn a pager on this display. Nothing leaves the screen.",
        "menu" => "Press a button on one of the rig's menus.",
        "mode" => "Ask the bus to put every display into a drive mode.",
        "emit" => "Hand a name to the host, which decides what it means.",
        _ => "",
    }
}

/// Which action an existing one is, by the name the file uses.
fn action_name(a: &Action) -> &'static str {
    match a {
        Action::Page { .. } => "page",
        Action::Menu { .. } => "menu",
        Action::Mode { .. } => "mode",
        Action::Emit { .. } => "emit",
    }
}

/// An action as the rows edit it.
///
/// Every field of every action at once, rather than one variant at a time, so
/// that switching the picker from `page` to `menu` and back does not lose what
/// was typed into the page rows on the way past.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Draft {
    action: &'static str,
    /// `page`: which pager, empty for the nearest one enclosing the widget.
    pager: String,
    /// `page`: "next", "prev", a page's name, or a number.
    to: String,
    /// `menu`: which of the rig's menus.
    menu: String,
    /// `menu`: which button to press.
    button: Button,
    /// `mode`: the mode's name in the rig.
    mode: String,
    /// `emit`: whatever the host and the scene agreed it means.
    emit: String,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            action: "page",
            pager: String::new(),
            to: "next".into(),
            menu: String::new(),
            button: Button::Centre,
            mode: String::new(),
            emit: String::new(),
        }
    }
}

impl Draft {
    /// Which action it is, by the name the file uses.
    pub(crate) fn action(&self) -> &'static str {
        self.action
    }

    /// The pager a `page` action names, which may be empty.
    pub(crate) fn pager(&self) -> &str {
        &self.pager
    }

    /// Pick which action it is.
    ///
    /// The only setter the dialogs need: past this the rows edit the fields
    /// in place behind `&mut self`, and nothing else has any business
    /// reaching into a half-typed draft.
    pub(crate) fn set_action(&mut self, action: &'static str) {
        self.action = action;
    }

    /// Name the pager a `page` action turns.
    #[cfg(test)]
    pub(crate) fn set_pager(&mut self, pager: &str) {
        self.pager = pager.into();
    }

    /// Name the menu a `menu` action presses.
    #[cfg(test)]
    pub(crate) fn set_menu(&mut self, menu: &str) {
        self.menu = menu.into();
    }

    /// Name what an `emit` action hands the host.
    #[cfg(test)]
    pub(crate) fn set_emit(&mut self, name: &str) {
        self.emit = name.into();
    }

    /// The draft an existing action opens on.
    pub(crate) fn of(a: &Action) -> Self {
        let mut d = Draft {
            action: action_name(a),
            ..Draft::default()
        };
        match a {
            Action::Page { pager, to } => {
                d.pager = pager.clone().unwrap_or_default();
                d.to = match to {
                    Page::Next => "next".into(),
                    Page::Prev => "prev".into(),
                    Page::Named(n) => n.clone(),
                    Page::Index(i) => i.to_string(),
                };
            }
            Action::Menu { menu, button } => {
                d.menu = menu.clone();
                d.button = *button;
            }
            Action::Mode { mode } => d.mode = mode.clone(),
            Action::Emit { name } => d.emit = name.clone(),
        }
        d
    }

    /// The `touch` object as it should be written into the scene file.
    ///
    /// Only the keys the chosen action uses, and only those that differ from
    /// what the parser would assume, so an authored file stays as short as the
    /// hand-written ones in `examples/`.
    pub(crate) fn to_scene(&self) -> String {
        let mut out = alloc_string(self.action);
        match self.action {
            "page" => {
                if !self.pager.trim().is_empty() {
                    out.push_str(&format!(r#", "pager": {}"#, quoted(self.pager.trim())));
                }
                // "next" is what a `page` action means with nothing else said.
                if self.to.trim() != "next" && !self.to.trim().is_empty() {
                    let t = self.to.trim();
                    // A number stays a number: `"to": 1` is the index, and
                    // `"to": "1"` would be a page whose name is the digit.
                    match t.parse::<u32>() {
                        Ok(n) => out.push_str(&format!(r#", "to": {n}"#)),
                        Err(_) => out.push_str(&format!(r#", "to": {}"#, quoted(t))),
                    }
                }
            }
            "menu" => {
                out.push_str(&format!(r#", "menu": {}"#, quoted(self.menu.trim())));
                out.push_str(&format!(r#", "button": "{}""#, button_name(self.button)));
            }
            "mode" => out.push_str(&format!(r#", "mode": {}"#, quoted(self.mode.trim()))),
            "emit" => out.push_str(&format!(r#", "name": {}"#, quoted(self.emit.trim()))),
            _ => {}
        }
        out.push_str(" }");
        out
    }

    /// Whether the draft names everything its action needs.
    ///
    /// A `menu` action with no menu would fail to build, and the preview would
    /// go red on a widget the author was still in the middle of setting up.
    pub(crate) fn complete(&self) -> bool {
        match self.action {
            "menu" => !self.menu.trim().is_empty(),
            "mode" => !self.mode.trim().is_empty(),
            "emit" => !self.emit.trim().is_empty(),
            _ => true,
        }
    }
}

/// `{ "action": "page"` -- the head every written object shares.
fn alloc_string(action: &str) -> String {
    format!(r#"{{ "action": "{action}""#)
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

/// The Touch rows for the selected widget, given the action it already has.
///
/// `None` unless something was changed, exactly as
/// [`crate::bind_ui::bind_rows`] works, so the caller only rewrites the file
/// when there is something to write.
pub(crate) fn touch_rows(ui: &mut egui::Ui, current: Option<&Action>) -> Option<TouchEdit> {
    let mut draft = current.map(Draft::of);

    ui.label("touch");
    let mut edit = None;
    ui.horizontal(|ui| {
        let mut on = draft.is_some();
        if ui
            .checkbox(&mut on, "")
            .on_hover_text("Make this widget something a finger can press")
            .changed()
        {
            edit = Some(if on {
                // A fresh action turns the page, because that is the one the
                // display can carry out on its own and the commonest by far.
                TouchEdit::Set(Draft::default().to_scene())
            } else {
                TouchEdit::Unset
            });
        }
        if let Some(d) = draft.as_mut() {
            let was = d.action;
            egui::ComboBox::from_id_salt("touch-action")
                .selected_text(d.action)
                .show_ui(ui, |ui| {
                    for a in ACTIONS {
                        ui.selectable_value(&mut d.action, a, *a)
                            .on_hover_text(about(a));
                    }
                });
            if d.action != was && d.complete() {
                edit = Some(TouchEdit::Set(d.to_scene()));
            }
        }
    });
    ui.end_row();

    let Some(d) = draft.as_mut() else {
        return edit;
    };
    ui.label("").on_hover_text(about(d.action));
    ui.label(egui::RichText::new(about(d.action)).small().weak());
    ui.end_row();

    if action_rows(ui, d) && d.complete() {
        edit = Some(TouchEdit::Set(d.to_scene()));
    }
    edit
}

/// The rows for whichever action a draft is currently set to.
///
/// Returns whether anything was changed. Shared with the pin dialog, which
/// wires the same actions to a hard button: two copies of these rows would be
/// two places to forget a field when the enum grows one.
///
/// Only the rows the chosen action uses. Showing the rest greyed out would
/// make the pane four times as tall for no more information.
pub(crate) fn action_rows(ui: &mut egui::Ui, d: &mut Draft) -> bool {
    let mut changed = false;
    match d.action {
        "page" => {
            ui.label("pager");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut d.pager)
                        .hint_text("nearest")
                        .desired_width(140.0),
                )
                .on_hover_text(
                    "The pager's name. Left empty, the nearest one around \
                     this widget is turned, which is what a button sitting \
                     inside the page it turns wants.",
                )
                .lost_focus();
            ui.end_row();

            ui.label("to");
            ui.horizontal(|ui| {
                for word in ["next", "prev"] {
                    if ui.selectable_label(d.to == word, word).clicked() {
                        d.to = word.into();
                        changed = true;
                    }
                }
                changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut d.to)
                            .hint_text("or a page's name")
                            .desired_width(100.0),
                    )
                    .on_hover_text(
                        "A page's name survives a page being inserted in \
                         front of it, where counting to one does not.",
                    )
                    .lost_focus();
            });
            ui.end_row();
        }
        "menu" => {
            ui.label("menu");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut d.menu)
                        .hint_text("as the rig names it")
                        .desired_width(140.0),
                )
                .lost_focus();
            ui.end_row();

            ui.label("button");
            ui.horizontal(|ui| {
                for b in [
                    Button::Up,
                    Button::Down,
                    Button::Left,
                    Button::Right,
                    Button::Centre,
                ] {
                    if ui.selectable_label(d.button == b, button_name(b)).clicked() {
                        d.button = b;
                        changed = true;
                    }
                }
            });
            ui.end_row();
        }
        "mode" => {
            ui.label("mode");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut d.mode)
                        .hint_text("as the rig names it")
                        .desired_width(140.0),
                )
                .on_hover_text("Every display moves together, and the master may refuse.")
                .lost_focus();
            ui.end_row();
        }
        "emit" => {
            ui.label("name");
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut d.emit)
                        .hint_text("whatever the host expects")
                        .desired_width(140.0),
                )
                .lost_focus();
            ui.end_row();
        }
        _ => {}
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use copilot::scene::{build_scene, parse};

    /// Write a draft out, put it on a widget, and read back what it became.
    ///
    /// The one thing worth testing here is the round trip: the rows produce
    /// text, the parser reads it, and the two have to agree or the editor
    /// writes files it cannot open.
    fn round(d: &Draft) -> Action {
        let src = format!(
            r#"{{"width":10,"height":10,"root":
                {{"type":"panel","rect":[0,0,10,10],"touch":{}}}}}"#,
            d.to_scene()
        );
        let scene = build_scene(&parse(&src).expect("the rows wrote bad JSON"))
            .expect("the rows wrote an action the parser refuses");
        scene.hotspots[0].action.clone()
    }

    #[test]
    fn a_fresh_action_turns_the_next_page_of_the_nearest_pager() {
        assert_eq!(
            round(&Draft::default()),
            Action::Page {
                pager: None,
                to: Page::Next
            }
        );
    }

    #[test]
    fn every_action_the_picker_offers_round_trips() {
        for action in ACTIONS {
            let d = Draft {
                action,
                pager: "pages".into(),
                to: "gps".into(),
                menu: "display".into(),
                button: Button::Left,
                mode: "track".into(),
                emit: "reset trip".into(),
            };
            assert_eq!(action_name(&round(&d)), *action, "{action}");
        }
    }

    #[test]
    fn a_page_named_by_a_number_stays_a_number() {
        // `"to": 1` is the second page; `"to": "1"` is a page called "1".
        let d = Draft {
            to: "1".into(),
            ..Draft::default()
        };
        assert_eq!(
            round(&d),
            Action::Page {
                pager: None,
                to: Page::Index(1)
            }
        );
    }

    #[test]
    fn an_empty_pager_name_is_left_out_rather_than_written_blank() {
        // `"pager": ""` would name a widget called nothing, and find none.
        let d = Draft::default();
        assert!(!d.to_scene().contains("pager"));
        assert_eq!(
            round(&d),
            Action::Page {
                pager: None,
                to: Page::Next
            }
        );
    }

    #[test]
    fn every_button_survives_the_trip() {
        for button in [
            Button::Up,
            Button::Down,
            Button::Left,
            Button::Right,
            Button::Centre,
        ] {
            let d = Draft {
                action: "menu",
                menu: "display".into(),
                button,
                ..Draft::default()
            };
            assert_eq!(
                round(&d),
                Action::Menu {
                    menu: "display".into(),
                    button
                },
                "{button:?}"
            );
        }
    }

    #[test]
    fn a_name_with_a_quote_in_it_does_not_end_the_document() {
        let d = Draft {
            action: "emit",
            emit: r#"say "hi""#.into(),
            ..Draft::default()
        };
        assert_eq!(
            round(&d),
            Action::Emit {
                name: r#"say "hi""#.into()
            }
        );
    }

    #[test]
    fn an_action_missing_what_it_needs_is_not_written() {
        // A `menu` action with no menu would fail to build, and the preview
        // would go red on a widget still being set up.
        let d = Draft {
            action: "menu",
            ..Draft::default()
        };
        assert!(!d.complete());
        assert!(Draft::default().complete(), "a page action needs nothing");
    }

    #[test]
    fn the_draft_keeps_what_was_typed_when_the_picker_moves_past_it() {
        // Switching from menu to mode and back must not lose the menu name.
        let mut d = Draft {
            action: "menu",
            menu: "display".into(),
            ..Draft::default()
        };
        d.action = "mode";
        d.mode = "track".into();
        d.action = "menu";
        assert_eq!(d.menu, "display");
    }

    #[test]
    fn every_action_the_picker_offers_says_what_it_is_for() {
        for a in ACTIONS {
            assert!(!about(a).is_empty(), "{a} has no description");
        }
    }
}
