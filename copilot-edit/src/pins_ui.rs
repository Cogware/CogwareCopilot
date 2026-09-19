// SPDX-License-Identifier: GPL-3.0-only
//! GPIO mapping drafts, explicit file saves and protection when changing selection.

use copilot::gpio::{MAX_PINS, Pins};

use crate::App;
use crate::pins::Draft;

mod controls;
#[cfg(test)]
mod tests;
mod threshold;

/// GPIO configuration for one display, held locally until explicitly saved.
pub(crate) struct PinsDialog {
    rig_path: Option<std::path::PathBuf>,
    display: usize,
    editing: Option<Draft>,
    baseline: Option<Draft>,
    count: u8,
    pending: Option<Pending>,
    filter: String,
    gauge_filter: String,
    error: String,
    success: String,
}

impl PinsDialog {
    /// Start on the active display with its persisted logical pin count.
    pub(crate) fn new(display: usize, count: u8) -> Self {
        Self {
            rig_path: None,
            display,
            editing: None,
            baseline: None,
            count,
            pending: None,
            filter: String::new(),
            gauge_filter: String::new(),
            error: String::new(),
            success: String::new(),
        }
    }

    fn pin_dirty(&self) -> bool {
        match (&self.editing, &self.baseline) {
            (Some(draft), Some(baseline)) => !draft.same_mapping(baseline),
            (None, None) => false,
            _ => true,
        }
    }

    fn needs_discard(&self, nav: Nav, pins: &Pins) -> bool {
        self.pin_dirty() || (!matches!(nav, Nav::Pin(_)) && self.count != pins.count())
    }

    /// Choose a pin when opening from its preview badge.
    pub(crate) fn open_pin(&mut self, pin: u8, pins: &Pins) {
        self.select_pin(pin, pins);
    }

    fn select_pin(&mut self, pin: u8, pins: &Pins) {
        self.editing = Some(pins.get(pin).map_or_else(|| Draft::fresh(pin), Draft::of));
        self.baseline = self.editing.clone();
        self.filter.clear();
        self.gauge_filter.clear();
        self.error.clear();
        self.success.clear();
    }

    fn select_display(&mut self, display: usize, pins: &Pins) {
        self.display = display;
        self.count = pins.count();
        self.editing = None;
        self.baseline = None;
        self.filter.clear();
        self.gauge_filter.clear();
        self.error.clear();
        self.success.clear();
    }

    fn request(&mut self, nav: Nav, pins: &Pins) -> Option<Nav> {
        if self.needs_discard(nav, pins) {
            self.pending = Some(Pending::Leave(nav));
            None
        } else {
            Some(nav)
        }
    }
}

#[derive(Clone)]
enum Act {
    Count(u8),
    Write(Box<Draft>),
    Clear(u8),
}

#[derive(Clone, Copy)]
enum Nav {
    Display(usize),
    Pin(u8),
    Close,
}

#[derive(Clone)]
enum Pending {
    Save(Act),
    Leave(Nav),
}

impl App {
    /// Edit mappings without saving from text entry, focus loss or panel changes.
    pub(crate) fn pins_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.pins_dialog.take() else {
            return;
        };
        let Some(session) = self.rig.as_ref() else {
            return;
        };
        // Counts and drafts belong to a particular rig, not just a display index.
        if d.rig_path
            .as_ref()
            .is_some_and(|path| *path != session.path)
        {
            d = PinsDialog::new(session.active, session.display().pins.count());
        }
        d.rig_path = Some(session.path.clone());
        let names: Vec<_> = session
            .rig
            .displays
            .iter()
            .map(|x| x.name.clone())
            .collect();
        let Some(display) = session.rig.displays.get(d.display) else {
            return;
        };
        let pins = display.pins.clone();
        let path = session.path.display().to_string();
        let mut open = true;
        let mut wanted = None;
        let mut nav = None;
        let mut act = None;
        let before = d.editing.clone();
        let old_count = d.count;
        egui::Window::new("GPIO — logical pin mapping")
            .id(egui::Id::new("pins"))
            .collapsible(false)
            .resizable(true)
            .default_width(700.0)
            .default_height((ctx.screen_rect().height() - 100.0).clamp(300.0, 760.0))
            .min_width(280.0)
            .max_width((ctx.screen_rect().width() - 24.0).max(280.0))
            .max_height((ctx.screen_rect().height() - 48.0).max(180.0))
            .vscroll(true)
            .open(&mut open)
            .show(ctx, |ui| {
                if ui.button("Close GPIO").clicked() { wanted = Some(Nav::Close); }
                if !d.error.is_empty() { ui.colored_label(ui.visuals().error_fg_color, &d.error); }
                if !d.success.is_empty() { ui.label(&d.success); }
                ui.separator();
                ui.label("Map logical pins 1–32 to indicators, numeric value thresholds or button actions. These are not physical connector or processor GPIO numbers.");
                ui.weak("The board firmware and schematic define the physical mapping. Saving changes the rig file, not connected hardware. Ctrl+Z does not undo it.");
                ui.separator();
                if d.pending.is_none() {
                ui.scope(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Display");
                        egui::ComboBox::from_id_salt("pins-display")
                            .selected_text(&names[d.display])
                            .show_ui(ui, |ui| {
                                for (i, name) in names.iter().enumerate() {
                                    if ui.selectable_label(i == d.display, name).clicked() && i != d.display {
                                        wanted = Some(Nav::Display(i));
                                    }
                                }
                            });
                    });
                    ui.push_id(("pin-count", d.display), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Logical pin count");
                            ui.add(egui::DragValue::new(&mut d.count).range(0..=MAX_PINS))
                                .on_hover_text("0 disables GPIO mapping. The maximum is 32 logical pins per display.");
                            let changed = d.count != pins.count();
                            if ui.add_enabled(changed, egui::Button::new("Review pin count…"))
                                .on_disabled_hover_text("The count already matches the saved rig.")
                                .clicked() {
                                d.pending = Some(Pending::Save(Act::Count(d.count)));
                            }
                            if changed && ui.button("Reset count").clicked() { d.count = pins.count(); }
                        });
                    });
                    let lost = self.pins_lost_by(d.display, d.count);
                    if lost > 0 {
                        ui.colored_label(ui.visuals().warn_fg_color, format!("Reducing the count removes {lost} saved mapping(s). Review before saving."));
                    }
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt(("pin-editor", d.display))
                        .max_height((ui.available_height() - 60.0).max(120.0))
                        .show(ui, |ui| {
                            if pins.is_empty() {
                                ui.label("No logical pins are enabled. Set a count above and save it to begin mapping.");
                            } else if ui.available_width() >= 620.0 {
                                ui.columns(2, |cols| {
                                    controls::pin_list(&mut cols[0], &d, &pins, &mut wanted);
                                    controls::pin_detail(&mut cols[1], &mut d, &pins);
                                });
                            } else {
                                controls::pin_list(ui, &d, &pins, &mut wanted);
                                ui.separator();
                                controls::pin_detail(ui, &mut d, &pins);
                            }
                        });
                });
                }
                if before != d.editing || old_count != d.count { d.success.clear(); }
                if let Some(pending) = d.pending.clone() {
                    ui.separator();
                    ui.group(|ui| {
                        let label = match &pending {
                            Pending::Save(Act::Count(n)) => {
                                ui.strong("Save logical pin count?");
                                let lost = self.pins_lost_by(d.display, *n);
                                ui.label(format!("Set “{}” to {n} logical pins. {lost} saved mapping(s) will be removed.", names[d.display]));
                                if d.editing.as_ref().is_some_and(|e| e.pin > *n) && d.pin_dirty() {
                                    ui.label("The selected pin's unsaved draft will also be discarded.");
                                }
                                "Save pin count"
                            }
                            Pending::Save(Act::Write(draft)) => {
                                ui.strong("Save GPIO mapping?");
                                ui.label(format!("Write logical pin {} on “{}” to the rig file.", draft.pin, names[d.display]));
                                if !draft.label.trim().is_empty() {
                                    ui.label(format!("Wire label: {}", draft.label.trim()));
                                }
                                let dir = if draft.dir == copilot::gpio::Dir::In { "Input" } else { "Output" };
                                ui.label(format!("{dir} · {}", draft.summary()));
                                if draft.kind == crate::pins::Kind::Threshold {
                                    ui.weak("Requires threshold-capable firmware on the controller/display. HIGH and LOW are logical commands, not measured voltages.");
                                }
                                if draft.dir == copilot::gpio::Dir::In {
                                    ui.label(format!("Active {} · {} pull", draft.active.name(), draft.pull.name()));
                                }
                                ui.collapsing("Mapping written to the rig", |ui| {
                                    if let Ok(body) = draft.to_rig() { ui.monospace(body); }
                                });
                                ui.weak("Saving changes this rig file only; it does not drive hardware. Check the board wiring before deploying.");
                                "Save pin mapping"
                            }
                            Pending::Save(Act::Clear(pin)) => {
                                ui.strong("Remove saved mapping?");
                                ui.label(format!("Unmap logical pin {pin} on “{}”. The pin count stays unchanged; Ctrl+Z cannot restore the mapping.", names[d.display]));
                                "Remove pin mapping"
                            }
                            Pending::Leave(_) => {
                                ui.strong("Discard unsaved GPIO draft?");
                                ui.label("These edits have not been written. Keep editing to save them, or discard them to continue.");
                                "Discard draft and continue"
                            }
                        };
                        if matches!(pending, Pending::Save(_)) {
                            ui.weak(format!("Writes immediately: {path}"));
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui.button(label).clicked() {
                                match pending.clone() {
                                    Pending::Save(action) => act = Some(action),
                                    Pending::Leave(next) => nav = Some(next),
                                }
                                d.pending = None;
                            }
                            if ui.button("Keep editing").clicked() { d.pending = None; }
                        });
                    });
                }
                if d.pin_dirty() || d.count != pins.count() {
                    ui.weak("Unsaved GPIO draft — use the review and save buttons to write it.");
                }
            });
        if !open {
            wanted = Some(Nav::Close);
        }
        if let Some(next) = wanted {
            nav = d.request(next, &pins);
        }
        if let Some(action) = act {
            let result = match &action {
                Act::Count(n) => self.set_pin_count(d.display, *n),
                Act::Write(draft) => self.set_pin(d.display, draft),
                Act::Clear(pin) => self.clear_pin(d.display, *pin),
            };
            match result {
                Ok(()) => {
                    let pins =
                        &self.rig.as_ref().expect("a saved rig").rig.displays[d.display].pins;
                    match action {
                        Act::Write(draft) => d.select_pin(draft.pin, pins),
                        Act::Clear(pin) => d.select_pin(pin, pins),
                        Act::Count(n) => {
                            d.count = n;
                            if d.editing.as_ref().is_some_and(|e| e.pin > n) {
                                d.editing = None;
                                d.baseline = None;
                            }
                        }
                    }
                    d.error.clear();
                    d.success = format!("Saved to disk: {}", self.status);
                }
                Err(error) => {
                    d.error = error;
                    d.success.clear();
                }
            }
        }
        match nav {
            Some(Nav::Close) => return,
            Some(Nav::Pin(pin)) => d.select_pin(pin, &pins),
            Some(Nav::Display(display)) => {
                if let Some(pins) = self
                    .rig
                    .as_ref()
                    .and_then(|r| r.rig.displays.get(display))
                    .map(|d| &d.pins)
                {
                    d.select_display(display, pins);
                }
            }
            None => {}
        }
        self.pins_dialog = self.rig.is_some().then_some(d);
    }
}
