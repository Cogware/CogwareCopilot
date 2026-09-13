// SPDX-License-Identifier: GPL-3.0-only
//! The Pins dialog: how many pins a display has, and what each is wired to.
//!
//! Apart from [`crate::pins`], which holds the operations, for the reason
//! [`crate::modes_ui`] is apart from [`crate::modes`]: what a pin edit *is*
//! can be tested without a window, and is, while this half is the buttons that
//! ask for one.
//!
//! A list on the left and one pin's details on the right, rather than a row of
//! seven columns per pin. A pin wired to a hard button has a whole action
//! behind it -- the same one a widget's `touch` has -- and there is no honest
//! way to fit that in a cell.

use copilot::cogware_can::indicators::ALL_INDICATORS;
use copilot::gpio::{Dir, Level, MAX_PINS, Pull, What};

use crate::App;
use crate::pins::Draft;

/// The Pins dialog while it is open.
pub(crate) struct PinsDialog {
    /// Which display's pins, by its index in the rig.
    display: usize,
    /// The pin being edited, and the draft so far.
    ///
    /// Held rather than read back each frame because a half-typed label is
    /// not yet anything the rig would accept, and re-reading would throw a
    /// keystroke away every time the parse failed.
    editing: Option<Draft>,
    /// The count being typed, before it is applied.
    count: u8,
    /// What went wrong with the last thing tried.
    error: String,
}

impl PinsDialog {
    /// A fresh dialog on the display being edited.
    pub(crate) fn new(display: usize, count: u8) -> Self {
        Self {
            display,
            editing: None,
            count,
            error: String::new(),
        }
    }
}

/// One thing the dialog asked for. One per frame: each rewrites the rig and
/// re-reads it, and the second would be working from a list the first changed.
enum Act {
    Count(u8),
    Write(Draft),
    Clear(u8),
}

impl App {
    /// The Pins dialog: a display's whole loom.
    pub(crate) fn pins_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.pins_dialog.take() else {
            return;
        };
        let Some(session) = self.rig.as_ref() else {
            return;
        };
        let names: Vec<String> = session
            .rig
            .displays
            .iter()
            .map(|x| x.name.clone())
            .collect();
        d.display = d.display.min(names.len().saturating_sub(1));
        let pins = session.rig.displays[d.display].pins.clone();
        let mut open = true;
        let mut act: Option<Act> = None;

        egui::Window::new("Pins")
            .collapsible(false)
            .resizable(true)
            .default_width(620.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "The spare pins on this display, numbered 1 upwards. Which \
                         physical pin each lands on is the board's business, in the \
                         board's own code, where the schematic is.",
                    )
                    .small()
                    .weak(),
                );
                ui.separator();

                ui.horizontal(|ui| {
                    if names.len() > 1 {
                        ui.label("display");
                        egui::ComboBox::from_id_salt("pins-display")
                            .selected_text(&names[d.display])
                            .show_ui(ui, |ui| {
                                for (i, n) in names.iter().enumerate() {
                                    if ui.selectable_label(i == d.display, n).clicked()
                                        && i != d.display
                                    {
                                        d.display = i;
                                        d.editing = None;
                                    }
                                }
                            });
                        ui.separator();
                    }
                    ui.label("how many");
                    ui.add(egui::DragValue::new(&mut d.count).range(0..=MAX_PINS));
                    let lost = self.pins_lost_by(d.display, d.count);
                    let can = d.count != pins.count();
                    if ui
                        .add_enabled(can, egui::Button::new("Apply"))
                        .on_hover_text(if lost > 0 {
                            format!("{lost} wired pin(s) would come off the rig")
                        } else {
                            "Set how many pins this display has".into()
                        })
                        .clicked()
                    {
                        act = Some(Act::Count(d.count));
                    }
                    if lost > 0 && can {
                        ui.colored_label(egui::Color32::YELLOW, format!("unwires {lost}"));
                    }
                });

                if pins.is_empty() {
                    ui.add_space(8.0);
                    ui.weak("This display has no pins. Say how many above.");
                    return;
                }

                ui.separator();
                ui.columns(2, |col| {
                    // --- the loom ---
                    col[0].label(egui::RichText::new("Pin").strong());
                    egui::ScrollArea::vertical()
                        .id_salt("pin-list")
                        .max_height(320.0)
                        .show(&mut col[0], |ui| {
                            for n in 1..=pins.count() {
                                let wired = pins.get(n);
                                let picked = d.editing.as_ref().is_some_and(|e| e.pin == n);
                                let text = match wired {
                                    Some(p) => {
                                        let what = match &p.what {
                                            What::Lamp(i) => i.name().to_string(),
                                            What::Press(_) => "button".into(),
                                        };
                                        let arrow = if p.dir == Dir::Out { "out" } else { "in" };
                                        match p.label.as_deref() {
                                            Some(l) => format!("{n}  {l} — {what} ({arrow})"),
                                            None => format!("{n}  {what} ({arrow})"),
                                        }
                                    }
                                    None => format!("{n}  —"),
                                };
                                let mut row = ui.selectable_label(picked, text);
                                if wired.is_none() {
                                    row = row.on_hover_text("Wired to nothing yet");
                                }
                                if row.clicked() {
                                    d.editing = Some(match wired {
                                        Some(p) => Draft::of(p),
                                        None => Draft::fresh(n),
                                    });
                                }
                            }
                        });

                    // --- the pin in hand ---
                    let ui = &mut col[1];
                    let Some(draft) = d.editing.as_mut() else {
                        ui.label(egui::RichText::new("Wiring").strong());
                        ui.add_space(4.0);
                        ui.weak("Pick a pin on the left.");
                        return;
                    };
                    ui.label(egui::RichText::new(format!("Pin {}", draft.pin)).strong());
                    egui::Grid::new("pin-detail")
                        .num_columns(2)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("label");
                            ui.add(
                                egui::TextEdit::singleline(&mut draft.label)
                                    .hint_text("left stalk")
                                    .desired_width(160.0),
                            )
                            .on_hover_text(
                                "What the wire is called. Nothing reads it; it is \
                                 for whoever opens the loom in three years.",
                            );
                            ui.end_row();

                            ui.label("direction");
                            ui.horizontal(|ui| {
                                for dir in [Dir::In, Dir::Out] {
                                    if ui
                                        .selectable_label(draft.dir == dir, dir.name())
                                        .on_hover_text(match dir {
                                            Dir::In => "The car drives it, the display reads it",
                                            Dir::Out => "The display drives it: a bulb, a relay",
                                        })
                                        .clicked()
                                    {
                                        draft.dir = dir;
                                        // An output has nothing to read from
                                        // an action, so it can only be a lamp.
                                        if dir == Dir::Out {
                                            draft.is_lamp = true;
                                        }
                                    }
                                }
                            });
                            ui.end_row();

                            // Both belong to the board on an output, where a
                            // driver transistor inverts if the hardware needs it.
                            if draft.dir == Dir::In {
                                ui.label("asserted");
                                ui.horizontal(|ui| {
                                    for level in [Level::High, Level::Low] {
                                        if ui
                                            .selectable_label(draft.active == level, level.name())
                                            .clicked()
                                        {
                                            draft.active = level;
                                        }
                                    }
                                })
                                .response
                                .on_hover_text(
                                    "Which level means yes. A feed off a switch is \
                                     high; anything switched to chassis ground, \
                                     which is most of a car, is low.",
                                );
                                ui.end_row();

                                ui.label("pull");
                                ui.horizontal(|ui| {
                                    for pull in [Pull::None, Pull::Up, Pull::Down] {
                                        if ui
                                            .selectable_label(draft.pull == pull, pull.name())
                                            .clicked()
                                        {
                                            draft.pull = pull;
                                        }
                                    }
                                })
                                .response
                                .on_hover_text(
                                    "What the board ties it to when nothing drives \
                                     it. The board applies this; nothing here does.",
                                );
                                ui.end_row();
                            }

                            ui.label("wired to");
                            ui.horizontal(|ui| {
                                if ui.selectable_label(draft.is_lamp, "tell-tale").clicked() {
                                    draft.is_lamp = true;
                                }
                                let can_press = draft.dir == Dir::In;
                                if ui
                                    .add_enabled(
                                        can_press,
                                        egui::SelectableLabel::new(!draft.is_lamp, "button"),
                                    )
                                    .on_hover_text(if can_press {
                                        "A hard button, firing what a widget's touch fires"
                                    } else {
                                        "An output has nothing to read from an action"
                                    })
                                    .clicked()
                                {
                                    draft.is_lamp = false;
                                }
                            });
                            ui.end_row();

                            if draft.is_lamp {
                                ui.label("lamp");
                                let shown = if draft.indicator.is_empty() {
                                    "pick one"
                                } else {
                                    draft.indicator.as_str()
                                };
                                egui::ComboBox::from_id_salt("pin-lamp")
                                    .selected_text(shown)
                                    .height(320.0)
                                    .show_ui(ui, |ui| {
                                        // In the spec's own table order, which
                                        // groups the road-car lamps first and
                                        // the motorsport ones after.
                                        for ind in ALL_INDICATORS {
                                            let name = ind.name();
                                            if ui
                                                .selectable_label(draft.indicator == name, name)
                                                .clicked()
                                            {
                                                draft.indicator = name.into();
                                            }
                                        }
                                    });
                                ui.end_row();
                            } else {
                                ui.label("action");
                                egui::ComboBox::from_id_salt("pin-action")
                                    .selected_text(draft.action.action())
                                    .show_ui(ui, |ui| {
                                        for a in crate::touch_ui::ACTIONS {
                                            if ui
                                                .selectable_label(draft.action.action() == *a, *a)
                                                .clicked()
                                            {
                                                draft.action.set_action(a);
                                            }
                                        }
                                    });
                                ui.end_row();
                                crate::touch_ui::action_rows(ui, &mut draft.action);
                            }
                        });

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let ready = draft.complete();
                        if ui
                            .add_enabled(ready, egui::Button::new("Wire it"))
                            .on_hover_text(if ready {
                                "Write this pin onto the rig"
                            } else {
                                "Pick what it is wired to first"
                            })
                            .clicked()
                        {
                            act = Some(Act::Write(draft.clone()));
                        }
                        if pins.get(draft.pin).is_some()
                            && ui
                                .button("Unwire")
                                .on_hover_text("Leave the pin, take the wiring off")
                                .clicked()
                        {
                            act = Some(Act::Clear(draft.pin));
                        }
                    });
                });

                if !d.error.is_empty() {
                    ui.separator();
                    ui.colored_label(egui::Color32::RED, &d.error);
                }
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(
                        "Written to the rig at once, like a mode: Ctrl+Z undoes \
                         the scene, not the loom.",
                    )
                    .small()
                    .weak(),
                );
            });

        let display = d.display;
        match act {
            Some(Act::Count(n)) => match self.set_pin_count(display, n) {
                Ok(()) => {
                    d.error.clear();
                    // A pin that has just been unwired is not a pin to be
                    // left sitting in the detail panel.
                    if d.editing.as_ref().is_some_and(|e| e.pin > n) {
                        d.editing = None;
                    }
                }
                Err(e) => d.error = e,
            },
            Some(Act::Write(draft)) => match self.set_pin(display, &draft) {
                Ok(()) => d.error.clear(),
                Err(e) => d.error = e,
            },
            Some(Act::Clear(n)) => match self.clear_pin(display, n) {
                Ok(()) => {
                    d.error.clear();
                    d.editing = Some(Draft::fresh(n));
                }
                Err(e) => d.error = e,
            },
            None => {}
        }
        // Kept only while there is still a rig to edit: closing one with the
        // dialog open must not leave it floating over a bare scene.
        self.pins_dialog = (open && self.rig.is_some()).then_some(d);
    }
}
