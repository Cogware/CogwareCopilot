// SPDX-License-Identifier: MIT OR Apache-2.0
//! The widget tree, and the pane that edits whichever widget is selected.
//!
//! Two panes on opposite sides of the preview: the tree on the left says
//! what is there, and the properties on the right say what the selected one
//! is made of. They were one panel once, with the properties squeezed in
//! under the tree, and the properties were the part nobody could find.

use copilot::scene::Shape;
use copilot::widget::{NodeId, ROOT, Tree};

use crate::bind_ui::{self, BindEdit};
use crate::canvas::{Gesture, committed, gesture};
use crate::menu::{self, Command};
use crate::select::toggle_in;
use crate::{App, describe_node, inspect, kind_name};

/// Where a dragged row is about to land, relative to the row under it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Drop {
    /// Just above it, as a sibling.
    Before(NodeId),
    /// Just below it, as a sibling.
    After(NodeId),
    /// Inside it, as its last child.
    Into(NodeId),
}

/// The colour of the marker showing where a drop will land.
const DROP_MARK: egui::Color32 = egui::Color32::from_rgb(80, 200, 255);

impl App {
    /// The widget tree.
    pub(crate) fn outliner(&mut self, ctx: &egui::Context) {
        // Starts each frame empty. A row fills it in when the pointer is over
        // it, and the preview overrides it when the pointer is there instead,
        // so whichever the hand is over is the one outlined.
        self.hover = None;
        let mut cmd = None;
        let mut dropped: Option<(NodeId, Drop)> = None;
        egui::SidePanel::left("outline")
            .default_width(260.0)
            .resizable(true)
            .show(ctx, |ui| {
                // Held, not hidden: the tree is still worth reading while a
                // curve is being shaped, just not worth clicking.
                if self.curve.is_some() {
                    ui.disable();
                }
                ui.heading("Widgets");
                ui.label(
                    egui::RichText::new(
                        "Click to select, Shift-click to select more, drag to move a \
                         widget into or beside another, right-click for commands.",
                    )
                    .small()
                    .weak(),
                );
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let Some(tree) = &self.preview.tree else {
                        ui.weak("scene does not parse");
                        return;
                    };
                    let mut rows = Rows {
                        selected: &mut self.selected,
                        extra: &mut self.extra,
                        hover: &mut self.hover,
                        reveal: &mut self.reveal,
                        shift: ui.input(|i| i.modifiers.shift || i.modifiers.command),
                        cmd: &mut cmd,
                        dropped: &mut dropped,
                    };
                    for child in tree
                        .get(ROOT)
                        .map(|n| n.children.clone())
                        .unwrap_or_default()
                    {
                        rows.show(ui, tree, child);
                    }
                });
            });
        if let Some(c) = cmd {
            self.run(c);
        }
        if let Some((dragged, target)) = dropped {
            self.drop_node(dragged, target);
        }
    }

    /// Put `dragged` where a drop on `target` asked for.
    fn drop_node(&mut self, dragged: NodeId, target: Drop) {
        let Some(tree) = self.preview.tree.as_ref() else {
            return;
        };
        let slot = match target {
            Drop::Into(t) => Some((t, tree.get(t).map_or(0, |n| n.children.len()))),
            Drop::Before(t) => tree
                .get(t)
                .and_then(|n| Some((n.parent?, tree.index_in_parent(t)?))),
            Drop::After(t) => tree
                .get(t)
                .and_then(|n| Some((n.parent?, tree.index_in_parent(t)? + 1))),
        };
        // A slot beside the document root is in the tree's own frame, which
        // the file has no words for.
        let Some((into, at)) = slot.filter(|(into, _)| *into != ROOT) else {
            self.status = "nothing can sit beside the document root".into();
            return;
        };
        if self.move_node(dragged, into, at) {
            self.status = format!("moved {}", self.describe(dragged));
        }
    }

    /// The selected widget's properties: its rect, its name, and the fields
    /// its kind has.
    pub(crate) fn properties_pane(&mut self, ui: &mut egui::Ui) {
        let Some((id, node)) = self
            .selected
            .and_then(|id| Some((id, self.preview.tree.as_ref()?.get(id)?.clone())))
        else {
            self.scene_pane(ui);
            return;
        };
        ui.heading(describe_node(&node));
        let others = self.extra.len();
        if others > 0 {
            ui.label(
                egui::RichText::new(format!(
                    "and {others} more selected. Property edits go to every \
                     selected widget of the same kind; the rect and name to this one."
                ))
                .small()
                .weak(),
            );
        }
        let on_screen = self.preview.tree.as_ref().and_then(|t| t.absolute_rect(id));

        // At most one edit per frame, because each is spliced into the text
        // and the text is reparsed: two in the same frame would have the
        // second computing its span against a document the first had moved.
        // The gesture comes along so a dragged box is one undo step however
        // long it is dragged.
        let mut edit: Option<(&'static str, String, Option<Gesture>)> = None;
        egui::Grid::new("props-common")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.label("kind");
                ui.monospace(kind_name(&node.kind));
                ui.end_row();

                // Editable, because a name is not decoration: it is how the
                // dummy-value driver finds a widget, and how anything driving
                // this cluster for real will find it too.
                ui.label("name");
                let mut name = node.name.clone().unwrap_or_default();
                let r = ui.text_edit_singleline(&mut name);
                if committed(&r) {
                    edit = Some(("name", inspect::Value::Text(name).to_scene(), gesture(&r)));
                }
                ui.end_row();

                let r = node.rect;
                let (mut x, mut y) = (r.left(), r.top());
                let (mut w, mut h) = (r.size.w as i32, r.size.h as i32);
                let mut changed: Option<Option<Gesture>> = None;
                let mut note = |r: egui::Response| {
                    if committed(&r) {
                        changed = Some(gesture(&r));
                    }
                };
                ui.label("position");
                ui.horizontal(|ui| {
                    note(ui.add(egui::DragValue::new(&mut x).speed(1.0).prefix("x ")));
                    note(ui.add(egui::DragValue::new(&mut y).speed(1.0).prefix("y ")));
                });
                ui.end_row();
                ui.label("size");
                ui.horizontal(|ui| {
                    note(ui.add(size_box(&mut w, "w ")));
                    note(ui.add(size_box(&mut h, "h ")));
                });
                ui.end_row();
                if let Some(g) = changed {
                    edit = Some(("rect", format!("[{x}, {y}, {w}, {h}]"), g));
                }
                if let Some(abs) = on_screen {
                    ui.label("on screen");
                    ui.monospace(format!("{}, {}", abs.left(), abs.top()));
                    ui.end_row();
                }

                // Three states, not two: a widget usually draws the way its
                // parent does, and "inherit" is how it goes back to that
                // after being set one way.
                ui.label("antialias");
                let now = match node.antialias {
                    None => "inherit",
                    Some(true) => "on",
                    Some(false) => "off",
                };
                let mut picked = now;
                egui::ComboBox::from_id_salt("antialias")
                    .selected_text(now)
                    .show_ui(ui, |ui| {
                        for option in ["inherit", "on", "off"] {
                            ui.selectable_value(&mut picked, option, option);
                        }
                    });
                if picked != now {
                    edit = Some(("antialias", picked.to_string(), None));
                }
                ui.end_row();
            });
        ui.separator();
        egui::Grid::new("props-kind")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                if let Some(e) = properties(ui, &node.kind) {
                    edit = Some(e);
                }
            });
        // A binding is per widget, not per kind -- two bars can show two
        // gauges -- so it has its own rows and goes to the primary only.
        let mut bind_edit = None;
        if node.kind.bind_target().is_some() {
            ui.separator();
            // Every binding the widget has, not the first: a bank is bound on
            // two axes and they share one `bind` key, so the pane has to write
            // both back or it would drop whichever it did not know about.
            let current: Vec<&copilot::scene::Binding> = self
                .preview
                .bindings
                .iter()
                .filter(|b| b.node == id)
                .collect();
            egui::Grid::new("props-bind")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    bind_edit = bind_ui::bind_rows(ui, &node.kind, &current)
                });
        }
        // A curve is a row of numbers, and the place to shape one is on the
        // widget rather than in a column of drag boxes or the text pane.
        let mut shape_curve = None;
        if let Some((shape, field)) = crate::curve::Shape::of(&node.kind) {
            ui.add_space(6.0);
            if ui
                .button(shape.verb())
                .on_hover_text(
                    "Drag its handles on the preview. Everything else is held \
                     until Done.",
                )
                .clicked()
            {
                shape_curve = Some((shape, field));
            }
        }
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Edits splice the text, so comments and formatting survive.")
                .small()
                .weak(),
        );
        if let Some((shape, field)) = shape_curve {
            self.start_curve(id, shape, field);
            return;
        }
        if let Some((b, g)) = bind_edit {
            match b {
                BindEdit::Set(v) => self.set_field_of(id, g, "bind", &v),
                BindEdit::Unset => self.unset_field_of(id, "bind"),
            }
            return;
        }
        if let Some((field, value, g)) = edit {
            if field == "antialias" {
                match value.as_str() {
                    "inherit" => self.unset_field_of(id, "antialias"),
                    on => self.set_field_of(id, None, "antialias", &(on == "on").to_string()),
                }
                return;
            }
            // A kind's own field goes to every selected widget of that kind:
            // that is what selecting five leds and picking a colour means.
            // The rect and the name are each widget's own.
            let targets: Vec<NodeId> = if field == "rect" || field == "name" {
                vec![id]
            } else {
                let kind = kind_name(&node.kind);
                self.members()
                    .into_iter()
                    .filter(|&m| {
                        self.preview
                            .tree
                            .as_ref()
                            .and_then(|t| t.get(m))
                            .is_some_and(|n| kind_name(&n.kind) == kind)
                    })
                    .collect()
            };
            self.edit_each(&targets, g, field, &value);
        }
    }

    /// The scene's own settings, shown while nothing is selected: the things
    /// that belong to the whole picture rather than to one widget.
    fn scene_pane(&mut self, ui: &mut egui::Ui) {
        ui.heading("Scene");
        ui.label(
            egui::RichText::new(
                "Nothing selected. Click a widget in the preview or in the \
                 tree; right-click either for commands. These are the scene's \
                 own settings.",
            )
            .small()
            .weak(),
        );
        ui.separator();
        let (w, h) = self.preview.scene_size();
        // `Some(value)` writes the key; `None` takes it off, back to default.
        let mut edit: Option<(&'static str, Option<String>)> = None;
        egui::Grid::new("scene-props")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.label("size");
                ui.horizontal(|ui| {
                    ui.monospace(format!("{w}x{h}"));
                    if ui.small_button("change…").clicked() {
                        self.resize_to = Some((w, h));
                    }
                });
                ui.end_row();

                ui.label("node");
                let was = self.preview.node.unwrap_or(0);
                let mut node = was;
                let r = crate::newdoc::node_field(ui, &mut node).on_hover_text(
                    "CAN address of the display this scene is for. 0x00 is the \
                     gateway and 0xFF is everyone, so neither can be chosen.",
                );
                if committed(&r) && node != was {
                    edit = Some(("node", (node != 0).then(|| format!("\"0x{node:02X}\""))));
                }
                ui.end_row();

                ui.label("shape");
                let round = self.preview.shape == Shape::Round;
                let mut pick = round;
                egui::ComboBox::from_id_salt("scene-shape")
                    .selected_text(self.preview.shape.name())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut pick, false, "rect");
                        ui.selectable_value(&mut pick, true, "round")
                            .on_hover_text("A panel whose corners sit behind a bezel");
                    });
                if pick != round {
                    edit = Some(("shape", pick.then(|| "\"round\"".to_string())));
                }
                ui.end_row();
            });
        match edit {
            Some((field, Some(value))) => self.set_scene_field(field, &value),
            Some((field, None)) => self.unset_scene_field(field),
            None => {}
        }
    }
}

/// What drawing a row needs to reach, gathered so the recursion has one
/// thing to pass on rather than six.
struct Rows<'a> {
    selected: &'a mut Option<NodeId>,
    extra: &'a mut Vec<NodeId>,
    hover: &'a mut Option<NodeId>,
    reveal: &'a mut bool,
    shift: bool,
    cmd: &'a mut Option<Command>,
    dropped: &'a mut Option<(NodeId, Drop)>,
}

impl Rows<'_> {
    /// One row of the outliner, and its subtree.
    fn show(&mut self, ui: &mut egui::Ui, tree: &Tree, id: NodeId) {
        let Some(node) = tree.get(id) else { return };
        let label = describe_node(node);
        let text = if node.visible {
            egui::RichText::new(label.as_str())
        } else {
            egui::RichText::new(format!("{label}  (hidden)")).weak()
        };
        let primary = *self.selected == Some(id);
        let picked = primary || self.extra.contains(&id);

        // A parent's header is a selectable label of its own rather than the
        // collapsing header's text, so that a selected panel is highlighted
        // the way a selected leaf is. Without it the outliner showed no
        // selection at all for anything with children.
        let movable = tree.get(id).and_then(|n| n.parent) != Some(ROOT);
        let row = |ui: &mut egui::Ui| {
            let r = draggable_row(ui, picked, text.clone());
            // The document root is not offered a payload, having nowhere to
            // go; it still takes clicks like any other row.
            if movable {
                r.dnd_set_drag_payload(id);
                // The only sign a row is travelling: the grab cursor is
                // suppressed for anything that also takes clicks.
                if r.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                }
            }
            r
        };
        let r = if node.children.is_empty() {
            row(ui)
        } else {
            egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                ui.id().with(id.0),
                true,
            )
            .show_header(ui, row)
            .body(|ui| {
                for child in &node.children {
                    self.show(ui, tree, *child);
                }
            })
            .1
            .inner
        };
        self.drop_target(ui, &r, id);

        if r.hovered() {
            *self.hover = Some(id);
        }
        if r.clicked() && self.shift {
            toggle_in(self.selected, self.extra, id);
        } else if r.clicked() || (r.secondary_clicked() && !picked) {
            *self.selected = Some(id);
            self.extra.clear();
            // So the text pane scrolls to it as well.
            *self.reveal = true;
        }
        if primary && *self.reveal {
            r.scroll_to_me(None);
        }
        let about = match 1 + self.extra.len() {
            n if n > 1 && picked => format!("{n} selected"),
            _ => label.clone(),
        };
        r.context_menu(|ui| {
            if let Some(c) = menu::context_menu(ui, Some(about.as_str())) {
                *self.cmd = Some(c);
            }
        });
    }

    /// Let a dragged row land on this one: above it, below it, or inside.
    ///
    /// The row is three targets in one. Its top and bottom quarters mean
    /// "beside me, on that side"; the middle means "inside me". A marker
    /// shows which while the drag hovers, so the hand knows what letting go
    /// will do before it does it.
    fn drop_target(&mut self, ui: &egui::Ui, r: &egui::Response, id: NodeId) {
        let Some(carried) = r.dnd_hover_payload::<NodeId>() else {
            return;
        };
        if *carried == id {
            return;
        }
        let Some(pos) = ui.input(|i| i.pointer.interact_pos()) else {
            return;
        };
        let rect = r.rect;
        let quarter = rect.height() / 4.0;
        let target = if pos.y < rect.top() + quarter {
            Drop::Before(id)
        } else if pos.y > rect.bottom() - quarter {
            Drop::After(id)
        } else {
            Drop::Into(id)
        };
        let paint = ui.painter();
        let stroke = egui::Stroke::new(2.0_f32, DROP_MARK);
        match target {
            Drop::Before(_) => paint.hline(rect.x_range(), rect.top(), stroke),
            Drop::After(_) => paint.hline(rect.x_range(), rect.bottom(), stroke),
            Drop::Into(_) => paint.rect_stroke(rect, 2.0_f32, stroke),
        };
        if let Some(dragged) = r.dnd_release_payload::<NodeId>() {
            *self.dropped = Some((*dragged, target));
        }
    }
}

/// Draw the selected widget's own properties, as rows of a two-column grid,
/// and report the one that changed, with the gesture it belongs to.
fn properties(
    ui: &mut egui::Ui,
    kind: &copilot::widget::Kind,
) -> Option<(&'static str, String, Option<Gesture>)> {
    let mut edit = None;
    for (name, value) in inspect::fields(kind) {
        let mut next = value.clone();
        ui.label(name);
        let changed: Option<Option<Gesture>> = match &mut next {
            inspect::Value::Color(c) => {
                ui.horizontal(|ui| {
                    let mut rgba = egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
                    let r = ui.color_edit_button_srgba(&mut rgba);
                    *c = copilot::Color::rgba(rgba.r(), rgba.g(), rgba.b(), rgba.a());
                    ui.monospace(format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a));
                    // The picker is a popup, and every nudge inside it comes
                    // back as a change on the button: one undo step for as
                    // long as the popup stays open.
                    r.changed().then(|| {
                        Some(Gesture {
                            id: r.id,
                            ends: !ui.memory(|m| m.any_popup_open()),
                        })
                    })
                })
                .inner
            }
            inspect::Value::Text(t) => commit(ui.text_edit_singleline(t)),
            // A drag box rather than a slider, because parking a band
            // threshold above the top of the scale is how a scene says it
            // does not want that band, and a slider capped at one could not
            // say it.
            inspect::Value::Ratio(f) => commit(ui.add(egui::DragValue::new(f).speed(0.01))),
            inspect::Value::Frac(f) => {
                commit(ui.add(egui::Slider::new(f, 0.0..=1.0).fixed_decimals(2)))
            }
            inspect::Value::Count(n) => commit(ui.add(egui::DragValue::new(n).speed(1.0))),
            inspect::Value::Whole(n) => commit(ui.add(egui::DragValue::new(n).speed(1.0))),
            inspect::Value::Flag(b) => commit(ui.checkbox(b, "")),
            inspect::Value::Choice(word, all) => {
                let mut picked = *word;
                let r = egui::ComboBox::from_id_salt(name)
                    .selected_text(*word)
                    .show_ui(ui, |ui| {
                        let mut hit = false;
                        for option in all.iter() {
                            hit |= ui.selectable_value(&mut picked, option, *option).changed();
                        }
                        hit
                    });
                *word = picked;
                r.inner.unwrap_or(false).then_some(None)
            }
        };
        ui.end_row();
        if let Some(g) = changed
            && edit.is_none()
        {
            edit = Some((name, next.to_scene(), g));
        }
    }
    edit
}

/// A selectable row that senses drags as well as clicks.
///
/// # Why not `SelectableLabel` inside a `dnd_drag_source`
///
/// That was the first arrangement and it made the tree almost unusable. The
/// helper lays its own `interact` over the label sensing *drag only*, and
/// egui hands a click to the topmost widget only when that widget senses
/// clicks; faced with a drag-only widget over a clickable one it discards the
/// click entirely, reasoning that "it would be confusing if clicking a
/// drag-widget would actually click something else below it"
/// (`egui::hit_test`). So every press on a row started a drag, and a row
/// could be selected only on the sliver of label the overlay did not cover.
///
/// An overlay sensing *both* fixes the click and breaks something quieter:
/// egui counts a widget as hovered only if it is at or above the topmost
/// interactive one, so the label underneath would stop lighting up under the
/// pointer. One widget sensing both is the only arrangement with neither
/// fault.
///
/// The body is `egui::SelectableLabel`'s, with `Sense::click()` widened to
/// `Sense::click_and_drag()`, so a row keeps the size, padding and colours
/// every other selectable in the editor has.
fn draggable_row(ui: &mut egui::Ui, selected: bool, text: egui::RichText) -> egui::Response {
    use egui::{NumExt as _, TextStyle, WidgetInfo, WidgetText, WidgetType};

    let padding = ui.spacing().button_padding;
    let extra = padding + padding;
    let wrap = ui.available_width() - extra.x;
    let galley = WidgetText::from(text).into_galley(ui, None, wrap, TextStyle::Button);

    let mut size = extra + galley.size();
    size.y = size.y.at_least(ui.spacing().interact_size.y);
    let (rect, response) = ui.allocate_at_least(size, egui::Sense::click_and_drag());
    response.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            galley.text(),
        )
    });

    if ui.is_rect_visible(response.rect) {
        let pos = ui
            .layout()
            .align_size_within_rect(galley.size(), rect.shrink2(padding))
            .min;
        let visuals = ui.style().interact_selectable(&response, selected);
        if selected || response.hovered() || response.highlighted() || response.has_focus() {
            let rect = rect.expand(visuals.expansion);
            ui.painter().rect(
                rect,
                visuals.rounding,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
            );
        }
        ui.painter().galley(pos, galley, visuals.text_color());
    }
    response
}

/// A drag box for a width or a height, which cannot go negative.
fn size_box<'a>(v: &'a mut i32, prefix: &str) -> egui::DragValue<'a> {
    egui::DragValue::new(v)
        .speed(1.0)
        .range(0..=i32::MAX)
        .prefix(prefix)
}

/// A change a person made, with the gesture it belongs to.
fn commit(r: egui::Response) -> Option<Option<Gesture>> {
    committed(&r).then(|| gesture(&r))
}
