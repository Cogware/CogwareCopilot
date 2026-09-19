// SPDX-License-Identifier: GPL-3.0-only
//! Compact GPIO command lights; preview state is not an electrical measurement.

use copilot::gpio::{Dir, Pin, Pins, What};
use copilot::scene::Action;

use crate::App;

/// The logical indicator state available to the scene preview.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    /// The preview indicator is asserted.
    High,
    /// The preview indicator is inactive.
    Low,
    /// The preview has no state for this mapping.
    Unsaid,
}

impl Level {
    fn color(self, out: bool) -> egui::Color32 {
        match self {
            Self::High if out => egui::Color32::from_rgb(63, 185, 80),
            Self::High => egui::Color32::from_rgb(210, 153, 34),
            Self::Low => egui::Color32::from_rgb(56, 63, 71),
            Self::Unsaid => egui::Color32::TRANSPARENT,
        }
    }

    fn word(self) -> &'static str {
        match self {
            Self::High => "On",
            Self::Low => "Off",
            Self::Unsaid => "Not previewed",
        }
    }
}

impl App {
    /// Read logical indicator state, never infer physical voltage or button state.
    pub(crate) fn pin_level(&self, p: &Pin) -> Level {
        let state = match &p.what {
            What::Lamp(ind) => self.driver.lamps.get(ind.name()).copied(),
            What::Threshold(threshold) if self.driver.enabled => self
                .driver
                .gauges
                .get(threshold.gauge.name)
                .map(|channel| channel.current(self.driver.sweep, self.driver.sweep_value()))
                .filter(|value| value.is_finite())
                .map(|value| threshold.matches(value)),
            What::Threshold(_) | What::Press(_) => None,
        };
        match state {
            Some(true) => Level::High,
            Some(false) => Level::Low,
            None => Level::Unsaid,
        }
    }

    fn loom(&self) -> Option<&Pins> {
        let pins = &self.rig.as_ref()?.display().pins;
        (!pins.is_empty()).then_some(pins)
    }

    /// Show labelled GPIO states and a direct route to their mappings.
    pub(crate) fn gpio_strip(&mut self, ctx: &egui::Context) {
        let Some(pins) = self.loom() else { return };
        let count = pins.count();
        let row: Vec<_> = pins
            .mapped()
            .iter()
            .map(|p| {
                (
                    p.pin,
                    p.dir == Dir::Out,
                    describe(p),
                    self.pin_level(p),
                    tooltip(p),
                )
            })
            .collect();
        let already_open = self.pins_dialog.is_some();
        let mut configure = false;
        let mut select = None;
        egui::TopBottomPanel::top("gpio").show(ctx, |ui| {
            ui.add_enabled_ui(!self.document_blocked(), |ui| {
                ui.horizontal(|ui| {
                    ui.strong("GPIO").on_hover_text(
                        "Preview commands, not measured voltages. Hover a light for its mapping and state.");
                    configure = ui.small_button("Configure...")
                        .on_hover_text("Edit GPIO mappings. Clicking a light opens that pin.").clicked();
                    if row.is_empty() {
                        ui.weak(format!("{count} pins; none mapped"));
                    } else {
                        egui::ScrollArea::horizontal()
                            .id_salt("gpio-lights")
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 3.0;
                                    for (pin, out, what, level, hint) in &row {
                                        if dot(ui, *pin, *out, what, *level)
                                            .on_hover_text(format!(
                                                "{hint}\nPreview: {}. Green = output on; amber = input indicator on; hollow = not previewed.",
                                                level.word()))
                                            .clicked()
                                        {
                                            select = Some(*pin);
                                        }
                                    }
                                });
                            });
                    }
                });
            });
        });
        // Reopening an existing editor must not replace a half-typed GPIO draft.
        if (configure || select.is_some()) && already_open {
            ctx.move_to_top(egui::LayerId::new(
                egui::Order::Middle,
                egui::Id::new("pins"),
            ));
            self.status = "GPIO settings are already open. The current draft has been kept.".into();
        }
        if (configure || select.is_some())
            && self.pins_dialog.is_none()
            && let Some(session) = self.rig.as_ref()
        {
            let mut dialog = crate::pins_ui::PinsDialog::new(session.active, count);
            if let Some(pin) = select {
                dialog.open_pin(pin, &session.display().pins);
            }
            self.pins_dialog = Some(dialog);
        }
    }
}

fn dot(ui: &mut egui::Ui, pin: u8, out: bool, what: &str, level: Level) -> egui::Response {
    ui.push_id(("gpio-preview-pin", pin), |ui| {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(36.0, 24.0), egui::Sense::click());
        let direction = if out { "Output" } else { "Input" };
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                format!("Pin {pin}, {direction}, {what}, {}", level.word()),
            )
        });
        if ui.is_rect_visible(rect) {
            let visuals = ui.style().interact(&response);
            if response.hovered() || response.has_focus() {
                ui.painter()
                    .rect(rect, 3.0_f32, visuals.weak_bg_fill, visuals.bg_stroke);
            }
            let centre = egui::pos2(rect.left() + 9.0, rect.center().y);
            ui.painter().circle(
                centre,
                5.0,
                level.color(out),
                egui::Stroke::new(if out { 1.5_f32 } else { 1.0_f32 }, visuals.text_color()),
            );
            ui.painter().text(
                egui::pos2(rect.right() - 3.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                pin.to_string(),
                egui::FontId::proportional(11.0),
                visuals.text_color(),
            );
        }
        response
    })
    .inner
}

fn describe(p: &Pin) -> String {
    if let Some(label) = &p.label {
        return label.clone();
    }
    match &p.what {
        What::Lamp(i) => i.name().to_string(),
        What::Press(a) => format!("Button: {}", action_word(a)),
        What::Threshold(threshold) => threshold_text(threshold),
    }
}

fn tooltip(p: &Pin) -> String {
    let meaning = match &p.what {
        What::Lamp(i) if p.dir == Dir::In => format!(
            "{} indicator preview, not a raw input reading. Active {} with {} pull is applied by board firmware.",
            i.name(),
            p.active.name(),
            p.pull.name()
        ),
        What::Lamp(i) => format!(
            "{} commands this output. Electrical polarity belongs to the board.",
            i.name()
        ),
        What::Threshold(threshold) => format!(
            "{} commands HIGH when true, otherwise LOW. Enable Use preview values to try it. Electrical polarity belongs to the board.",
            threshold_text(threshold),
        ),
        What::Press(a) => format!(
            "On a physical button press: {}. There is no held button state in this preview.",
            action_word(a)
        ),
    };
    format!(
        "Logical pin {} — {}\n{meaning}\nNo hardware is read or driven here. Click to open GPIO mapping.",
        p.pin,
        describe(p)
    )
}

fn action_word(action: &Action) -> String {
    match action {
        Action::Page { pager, .. } => match pager {
            Some(name) => format!("change page in {name}"),
            None => "change page".into(),
        },
        Action::Menu { menu, button } => format!(
            "{} on menu {menu}",
            copilot::scene::touch::button_name(*button)
        ),
        Action::Mode { mode } => format!("request mode {mode}"),
        Action::Emit { name } => format!("emit {name}"),
    }
}

#[cfg(test)]
mod tests;

fn threshold_text(threshold: &copilot::gpio::Threshold) -> String {
    use copilot::gpio::Comparison;
    let operator = match threshold.comparison {
        Comparison::Above => ">",
        Comparison::AtLeast => ">=",
        Comparison::Below => "<",
        Comparison::AtMost => "<=",
    };
    format!(
        "{} {operator} {} {}",
        threshold.gauge.name, threshold.value, threshold.gauge.unit.symbol
    )
}

#[cfg(test)]
#[path = "gpio_strip/layout_tests.rs"]
mod layout_tests;
