// SPDX-License-Identifier: MIT OR Apache-2.0
//! The panels, and the keys that reach them first.
//!
//! Separated from the commands in [`crate::main`] and the menus in
//! [`crate::menu`] so that each panel is a method with a name rather than a
//! block at some depth inside one very long closure, where the only way to
//! tell the source pane from the values pane is to count braces.

use std::ops::Range;

use copilot::widget::NodeId;

use crate::canvas::Gesture;
use crate::{App, COARSE_NUDGE, Pane};

/// The tint behind the selected widget's source.
///
/// The same magenta as the outline on the preview, faint enough to read
/// through: the two are the same fact shown in two places.
const SOURCE_TINT: egui::Color32 = egui::Color32::from_rgba_premultiplied(64, 0, 64, 64);

impl App {
    /// Where `id` lives in the text, as a byte range.
    fn span_of(&self, id: NodeId) -> Option<Range<usize>> {
        let path = self.node_path(id)?;
        let span = copilot::scene::locate(&self.text, &path)?;
        Some(span.start..span.end)
    }

    /// The scene file, as text, with the selection's source tinted.
    ///
    /// A widget picked on the preview is the same widget in the text, and
    /// the pane shows where: every selected widget's object is tinted, and a
    /// fresh selection scrolls the primary's into view. That is what makes
    /// the text pane a place to *read* about a widget rather than a place to
    /// go looking for it.
    fn source_pane(&mut self, ui: &mut egui::Ui) {
        let marks: Vec<Range<usize>> = self
            .members()
            .into_iter()
            .filter_map(|id| self.span_of(id))
            .collect();
        let reveal = self.reveal.then(|| marks.first().cloned()).flatten();
        // Scrolls both ways and never wraps: a wrapped line of JSON reads
        // as two lines, and the indentation that makes a scene file
        // legible is the first thing wrapping destroys.
        egui::ScrollArea::both().show(ui, |ui| {
            // Kept from before the field runs, because by the time it reports
            // a change the text is already the new one, and undo needs the
            // old one. A few kilobytes a frame, which is nothing.
            let before = self.text.clone();
            let mut layouter = |ui: &egui::Ui, text: &str, _wrap_width: f32| {
                let font = egui::TextStyle::Monospace.resolve(ui.style());
                let color = ui.visuals().text_color();
                let mut job = egui::text::LayoutJob::default();
                job.wrap.max_width = f32::INFINITY;
                for (range, tinted) in pieces(text.len(), &marks) {
                    let Some(piece) = text.get(range) else {
                        continue;
                    };
                    job.append(
                        piece,
                        0.0,
                        egui::TextFormat {
                            font_id: font.clone(),
                            color,
                            background: if tinted {
                                SOURCE_TINT
                            } else {
                                egui::Color32::TRANSPARENT
                            },
                            ..Default::default()
                        },
                    );
                }
                ui.fonts(|f| f.layout_job(job))
            };
            let out = egui::TextEdit::multiline(&mut self.text)
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(40)
                .layouter(&mut layouter)
                .show(ui);
            if let Some(range) = reveal {
                // The galley counts characters, and the span counts bytes.
                let at = egui::text::CCursor::new(
                    before[..range.start.min(before.len())].chars().count(),
                );
                let row = out
                    .galley
                    .pos_from_ccursor(at)
                    .translate(out.galley_pos.to_vec2());
                // Vertically only: the rect asked for hugs the left margin,
                // so the pane does not also slide sideways to the
                // indentation.
                let target = egui::Rect::from_min_max(
                    egui::pos2(out.galley_pos.x, row.min.y - 48.0),
                    egui::pos2(out.galley_pos.x + 1.0, row.max.y + 48.0),
                );
                ui.scroll_to_rect(target, Some(egui::Align::Center));
            }
            let r = out.response;
            if r.changed() {
                // Typing is undone a word at a time: every keystroke as its
                // own step made Ctrl+Z useless, and the whole session as one
                // step made it dangerous.
                let ends = word_boundary(&before, &self.text);
                self.begin_edit_from(Some(Gesture { id: r.id, ends }), before);
                self.dirty = true;
                // Re-parsed on every keystroke. The scene files this targets
                // are a few kilobytes; the cost is invisible and it is what
                // makes the preview never stale.
                self.reload();
            }
        });
    }

    /// The dummy values: one row per named reading, each with the value it
    /// holds, the range its sweep covers, and whether it sweeps at all.
    ///
    /// Per reading rather than one sweep for everything, because the
    /// question a sweep answers is "does this gauge read right across its
    /// range", and a coolant gauge's range is not a tachometer's. The other
    /// readings can be held still while one is watched.
    fn values_pane(&mut self, ui: &mut egui::Ui) {
        ui.heading("Dummy values");
        ui.label(
            egui::RichText::new(
                "Preview only. Never written to the file, so sweeping a gauge \
                 to check its geometry cannot become what ships. Bind a widget \
                 to a gauge, or name a bar, arc, needle, segbar, led or label, \
                 and it appears here.",
            )
            .small()
            .weak(),
        );
        ui.separator();
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.driver.enabled, "Apply");
            ui.add_enabled_ui(self.driver.enabled, |ui| {
                ui.checkbox(&mut self.driver.sweep, "Sweep");
                ui.label("period");
                ui.add(
                    egui::DragValue::new(&mut self.driver.period_s)
                        .range(0.5..=60.0)
                        .speed(0.1)
                        .suffix(" s"),
                )
                .on_hover_text("Seconds for one trip from min to max and back");
            });
            if ui
                .button("Reset to file")
                .on_hover_text("Every value back to what the scene says")
                .clicked()
            {
                self.driver.reset();
            }
        });
        let sweeping = self.driver.enabled && self.driver.sweep;
        let sweep = self.driver.sweep_value();
        if sweeping {
            ui.label(format!("sweep {sweep:.2}"));
        }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            let d = &self.driver;
            if d.gauges.is_empty() && d.channels.is_empty() && d.labels.is_empty() {
                ui.weak("No bound or named widgets to drive.");
                return;
            }
            if !self.driver.gauges.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "Gauges, in the unit the bus carries each in. One slider \
                         moves every widget bound to it; the range is the widgets' own.",
                    )
                    .small()
                    .weak(),
                );
                egui::Grid::new("gauges")
                    .num_columns(4)
                    .spacing([8.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for head in ["gauge", "value", "range", "sweep"] {
                            ui.label(egui::RichText::new(head).small().weak());
                        }
                        ui.end_row();
                        for (name, ch) in &mut self.driver.gauges {
                            let unit = copilot::scene::bind::gauge_by_name(name)
                                .map_or("", |g| g.unit.symbol);
                            ui.label(*name).on_hover_text(format!(
                                "the file draws {:.1} {unit}",
                                ch.authored()
                            ));
                            let (lo, hi) = ordered(ch.min, ch.max);
                            let suffix = format!(" {unit}");
                            if sweeping && ch.sweep {
                                let mut now = ch.current(true, sweep);
                                ui.add_enabled(
                                    false,
                                    egui::Slider::new(&mut now, lo..=hi)
                                        .fixed_decimals(1)
                                        .suffix(suffix),
                                );
                            } else {
                                ui.add(
                                    egui::Slider::new(&mut ch.value, lo..=hi)
                                        .fixed_decimals(1)
                                        .suffix(suffix),
                                );
                            }
                            ui.weak(format!("{lo:.0}..{hi:.0}"));
                            ui.checkbox(&mut ch.sweep, "")
                                .on_hover_text("Follow the sweep, or hold at the value");
                            ui.end_row();
                        }
                    });
                if !self.driver.channels.is_empty() || !self.driver.labels.is_empty() {
                    ui.separator();
                }
            }
            if !self.driver.channels.is_empty() {
                egui::Grid::new("channels")
                    .num_columns(5)
                    .spacing([8.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for head in ["name", "value", "min", "max", "sweep"] {
                            ui.label(egui::RichText::new(head).small().weak());
                        }
                        ui.end_row();
                        for (name, ch) in &mut self.driver.channels {
                            ui.label(name.as_str())
                                .on_hover_text(format!("the file says {:.2}", ch.authored()));
                            let (lo, hi) = ordered(ch.min, ch.max);
                            if sweeping && ch.sweep {
                                // Shown, not editable: what the widget reads
                                // right now is the sweep's business until the
                                // channel is held.
                                let mut now = ch.current(true, sweep);
                                ui.add_enabled(
                                    false,
                                    egui::Slider::new(&mut now, lo..=hi).fixed_decimals(2),
                                );
                            } else {
                                ui.add(egui::Slider::new(&mut ch.value, lo..=hi).fixed_decimals(2));
                            }
                            ui.add(
                                egui::DragValue::new(&mut ch.min)
                                    .speed(0.01)
                                    .fixed_decimals(2),
                            );
                            ui.add(
                                egui::DragValue::new(&mut ch.max)
                                    .speed(0.01)
                                    .fixed_decimals(2),
                            );
                            ui.checkbox(&mut ch.sweep, "")
                                .on_hover_text("Follow the sweep, or hold at the value");
                            ui.end_row();
                        }
                    });
            }
            if !self.driver.labels.is_empty() {
                ui.separator();
                egui::Grid::new("labels")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for (name, t) in &mut self.driver.labels {
                            ui.label(name.as_str())
                                .on_hover_text(format!("the file says {:?}", t.authored()));
                            ui.text_edit_singleline(&mut t.text);
                            ui.end_row();
                        }
                    });
            }
        });
    }

    /// Keys handled before any panel sees them.
    ///
    /// Checked first because a focused text field swallows what it is given,
    /// and undo has to work while the caret is in the source pane.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        // Ctrl+Z / Ctrl+Shift+Z, checked before the panels so a text field
        // holding focus does not swallow them.
        let (want_undo, want_redo) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z),
                i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                ) | i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y),
            )
        });
        if want_redo {
            self.redo();
        } else if want_undo {
            self.undo();
        }

        let unfocused = ctx.memory(|m| m.focused().is_none());
        if unfocused && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
            self.select_all();
        }
        // Arrow keys nudge the selection, and Shift makes the step coarse.
        // Only when no text field has focus: the source pane is a text editor
        // and its caret has the better claim on an arrow key.
        if self.selected.is_some() && unfocused {
            let step = if ctx.input(|i| i.modifiers.shift) {
                COARSE_NUDGE
            } else {
                1
            };
            let (mut dx, mut dy) = (0, 0);
            ctx.input_mut(|i| {
                dx -= i32::from(i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft));
                dx += i32::from(i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight));
                dy -= i32::from(i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
                dy += i32::from(i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
                dx -= i32::from(i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowLeft));
                dx += i32::from(i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowRight));
                dy -= i32::from(i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowUp));
                dy += i32::from(i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowDown));
            });
            if dx != 0 || dy != 0 {
                self.nudge(dx * step, dy * step);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
                self.delete_selected();
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::D)) {
                self.duplicate_selected();
            }
            // Escape clears the selection, as it does in every other editor.
            // A text field with focus takes Escape for itself first, which is
            // why this sits inside the no-focus check.
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                self.select(None);
            }
        }
        // Saving is not gated on a selection, and belongs wherever the caret
        // is: losing an edit because a text field had focus is the one thing a
        // save shortcut exists to prevent.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.save();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::N)) {
            self.new_doc = Some(crate::newdoc::NewDoc::default());
        }
    }

    /// The line along the bottom: what just happened, and what is missing.
    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                for m in &self.preview.missing {
                    ui.colored_label(egui::Color32::from_rgb(255, 160, 0), format!("missing {m}"));
                }
            });
        });
    }

    /// The tabbed pane holding the properties, the source and the dummy
    /// values.
    fn side_pane(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("side")
            .default_width(460.0)
            .resizable(true)
            .show(ctx, |ui| {
                if self.curve.is_some() {
                    ui.disable();
                }
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.pane, Pane::Props, "Properties");
                    ui.selectable_value(&mut self.pane, Pane::Source, "Scene");
                    ui.selectable_value(&mut self.pane, Pane::Values, "Dummy values");
                });
                ui.separator();
                match self.pane {
                    Pane::Props => self.properties_pane(ui),
                    Pane::Source => self.source_pane(ui),
                    Pane::Values => self.values_pane(ui),
                }
            });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        let now_us = self.started.elapsed().as_micros() as u64;
        let delta_us = now_us.saturating_sub(self.last_us);
        self.last_us = now_us;
        self.driver.tick(delta_us);

        // Order matters: the shortcuts run before any panel can take focus,
        // and the docked panels claim their edges before the canvas is told
        // what space is left. The outliner runs before the side pane so a
        // hovered row is known to the preview, and the preview last so it
        // has the final say on what is hovered.
        // Before anything else: a close request has to be vetoed in the
        // frame it arrives, or the window is already gone.
        self.close_guard(ctx);
        // While a curve is being shaped the toolbar is that mode's own, and
        // the shortcuts below it -- delete, nudge, duplicate -- belong to a
        // selection the mode has taken over.
        if self.curve.is_some() {
            self.curve_toolbar(ctx);
        } else {
            self.shortcuts(ctx);
            self.toolbar(ctx);
        }
        self.status_bar(ctx);
        self.outliner(ctx);
        self.side_pane(ctx);
        // A fresh selection has now been shown to the tree and the text;
        // the preview, which runs last, is where the next one is made.
        self.reveal = false;
        self.resolution_dialog(ctx);
        self.new_dialog(ctx);
        self.modes_window(ctx);
        self.canvas(ctx, now_us, delta_us);

        // Animation and sweeping both need a steady repaint; egui is
        // otherwise event-driven and would show a frozen frame.
        ctx.request_repaint();
    }
}

/// A slider's ends, the right way round and apart.
///
/// A person typing a range gets there through both of the states a slider
/// cannot show -- ends reversed, and ends equal -- and must not be refused
/// on the way.
fn ordered(min: f32, max: f32) -> (f32, f32) {
    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
    let hi = if hi - lo < 1e-6 { lo + 0.01 } else { hi };
    (lo, hi)
}

/// Whether an edit to the source text ends the word being typed.
///
/// Inserting one letter or digit continues it; anything else -- a space, a
/// bracket, a paste, a deletion -- ends it, so the next keystroke starts a
/// new undo step.
pub(crate) fn word_boundary(before: &str, after: &str) -> bool {
    if after.len() != before.len() + 1 {
        return true;
    }
    let at = before
        .bytes()
        .zip(after.bytes())
        .position(|(a, b)| a != b)
        .unwrap_or(before.len());
    !after
        .get(at..)
        .and_then(|s| s.chars().next())
        .is_some_and(char::is_alphanumeric)
}

/// `0..len` cut into pieces, each saying whether it lies inside one of
/// `marks`.
///
/// Marks may overlap -- a selected panel and a selected child of it -- and
/// a piece inside any of them is tinted once, not twice. Every cut lands on
/// a mark's edge, so a piece is never half inside a mark.
pub(crate) fn pieces(len: usize, marks: &[Range<usize>]) -> Vec<(Range<usize>, bool)> {
    let mut cuts: Vec<usize> = vec![0, len];
    cuts.extend(
        marks
            .iter()
            .flat_map(|m| [m.start, m.end])
            .filter(|&c| c <= len),
    );
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .filter(|w| w[1] > w[0])
        .map(|w| {
            let tinted = marks.iter().any(|m| m.start <= w[0] && w[1] <= m.end);
            (w[0]..w[1], tinted)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_cut_exactly_at_the_marks() {
        assert_eq!(
            pieces(10, std::slice::from_ref(&(2..5))),
            vec![(0..2, false), (2..5, true), (5..10, false)]
        );
    }

    #[test]
    fn overlapping_marks_tint_once_and_cut_at_every_edge() {
        assert_eq!(
            pieces(10, &[2..8, 4..6]),
            vec![
                (0..2, false),
                (2..4, true),
                (4..6, true),
                (6..8, true),
                (8..10, false)
            ]
        );
    }

    #[test]
    fn no_marks_is_one_plain_piece_and_nothing_runs_past_the_end() {
        assert_eq!(pieces(4, &[]), vec![(0..4, false)]);
        // A mark running past the end, which a stale span can, is cut at
        // the end rather than sending a piece off the text.
        assert_eq!(
            pieces(4, std::slice::from_ref(&(2..9))),
            vec![(0..2, false), (2..4, true)]
        );
        assert_eq!(pieces(0, &[]), vec![]);
    }
}
