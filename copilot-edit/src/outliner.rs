// SPDX-License-Identifier: GPL-3.0-only
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
use crate::inspect::controls;
use crate::menu::{self, Command};
use crate::select::toggle_in;
use crate::touch_ui::TouchEdit;
use crate::{App, describe_node, inspect, kind_name};

mod bands;
mod fields;
#[cfg(test)]
mod interaction_tests;

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
                if self.curve.is_some() || self.document_blocked() {
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
                        ui.weak("The scene cannot be read yet. Correct the error in Scene source to show its widgets.");
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
        if self.document_blocked() {
            return;
        }
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
        if self.document_blocked() {
            ui.weak("Finish or cancel the open dialog before editing widgets.");
        }
        if let Some(error) = &self.preview.error {
            ui.colored_label(ui.visuals().error_fg_color, "Scene source has an error");
            ui.label(error);
            ui.weak("Showing the last valid scene. Fix Scene source before editing properties.");
        }
        // Scrolled both ways: a grid row or a long field name that does not
        // fit scrolls here, rather than pushing the whole side pane wider
        // and eating the canvas.
        egui::ScrollArea::both().show(ui, |ui| {
            ui.push_id(self.properties_scope(), |ui| {
                ui.add_enabled_ui(
                    self.preview.error.is_none() && !self.document_blocked(),
                    |ui| self.selected_properties(ui),
                );
            });
        });
    }

    pub(crate) fn properties_scope(&self) -> egui::Id {
        egui::Id::new((
            "widget-properties",
            self.document_epoch,
            &self.path,
            self.selected.map(|id| id.0),
        ))
    }

    fn selected_properties(&mut self, ui: &mut egui::Ui) {
        let Some((id, node)) = self
            .selected
            .and_then(|id| Some((id, self.preview.tree.as_ref()?.get(id)?.clone())))
        else {
            self.scene_pane(ui);
            return;
        };
        ui.heading(describe_node(&node));
        ui.weak(crate::starter::widget_hint(kind_name(&node.kind)));
        let others = self.extra.len();
        if others > 0 {
            ui.label(
                egui::RichText::new(format!(
                    "{} widgets selected. Values shown belong to this primary widget. \
                     Type-specific fields change all selected widgets of the same type. \
                     Name, geometry, smoothing, data, touch and bands change this widget only.",
                    others + 1
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
        ui.strong("Identity and geometry");
        egui::Grid::new("props-common")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.label("Widget type");
                ui.label(crate::starter::widget_title(kind_name(&node.kind)))
                    .on_hover_text(format!("Scene type: {}", kind_name(&node.kind)));
                ui.end_row();

                // Editable, because a name is not decoration: it is how the
                // dummy-value driver finds a widget, and how anything driving
                // this cluster for real will find it too.
                ui.label("Widget name");
                let mut name = node.name.clone().unwrap_or_default();
                let r = ui.add(egui::TextEdit::singleline(&mut name).hint_text("Optional name"))
                    .on_hover_text("Used by page actions, animations and data drivers. Choose a recognisable, unique name.");
                if committed(&r) {
                    edit = Some(("name", inspect::Value::Text(name).to_scene(), gesture(&r)));
                }
                ui.end_row();

                let r = node.rect;
                let (mut x, mut y) = (r.left(), r.top());
                let (mut w, mut h) = (r.size.w, r.size.h);
                let mut changed: Option<Option<Gesture>> = None;
                let mut note = |r: egui::Response| {
                    if committed(&r) {
                        changed = Some(gesture(&r));
                    }
                };
                ui.label("Position in parent").on_hover_text("Scene pixels from the parent's top-left. Negative positions are allowed.");
                ui.horizontal(|ui| {
                    ui.label("X");
                    note(controls::number(ui, &mut x, 1.0, " px", None));
                    ui.label("Y");
                    note(controls::number(ui, &mut y, 1.0, " px", None));
                });
                ui.end_row();
                ui.label("Width");
                ui.horizontal(|ui| {
                    note(controls::number(ui, &mut w, 1.0, " px", None));
                });
                ui.end_row();
                ui.label("Height");
                note(controls::number(ui, &mut h, 1.0, " px", None));
                ui.end_row();
                if let Some(g) = changed {
                    edit = Some(("rect", format!("[{x}, {y}, {w}, {h}]"), g));
                }
                if let Some(abs) = on_screen {
                    ui.label("Screen position").on_hover_text("Read-only absolute position, including parent offsets.");
                    ui.monospace(format!("X {} px, Y {} px", abs.left(), abs.top()));
                    ui.end_row();
                }

                // Three states, not two: a widget usually draws the way its
                // parent does, and "inherit" is how it goes back to that
                // after being set one way.
                ui.label("Smooth edges");
                let now = match node.antialias {
                    None => "inherit",
                    Some(true) => "on",
                    Some(false) => "off",
                };
                let mut picked = now;
                egui::ComboBox::from_id_salt("antialias")
                    .selected_text(smoothing_title(now))
                    .show_ui(ui, |ui| {
                        for option in ["inherit", "on", "off"] {
                            ui.selectable_value(&mut picked, option, smoothing_title(option));
                        }
                    });
                if picked != now {
                    edit = Some(("antialias", picked.to_string(), None));
                }
                ui.end_row();
            });
        ui.separator();
        if let Some(e) = fields::properties(ui, &node.kind) {
            edit = Some(e);
        }
        let mut remove_field = None;
        let removal = match &node.kind {
            copilot::widget::Kind::Arc {
                cursor: Some(_), ..
            }
            | copilot::widget::Kind::SegBar {
                cursor: Some(_), ..
            } => Some(("cursor", "Remove reading marker")),
            copilot::widget::Kind::Chart {
                fade_to: Some(_), ..
            } => Some(("fade_to", "Do not fade older samples")),
            _ => None,
        };
        if let Some((field, label)) = removal
            && ui.button(label).clicked()
        {
            remove_field = Some(field);
        }
        let asset = match &node.kind {
            copilot::widget::Kind::Image { image } => {
                Some(("image", *image, self.preview.images.len()))
            }
            copilot::widget::Kind::Anim { anim, .. } => {
                Some(("animation", *anim, self.preview.clips.len()))
            }
            _ => None,
        };
        if let Some((kind, index, count)) = asset {
            let key = if kind == "image" { "images" } else { "anims" };
            if let Ok(source) = copilot::scene::parse(&self.text)
                && let Some(paths) = source.get(key).and_then(|value| value.as_array())
                && let Some(path) = paths.get(index as usize).and_then(|value| value.as_str())
            {
                ui.label(format!("Asset: {path}"));
            }
            if let copilot::widget::Kind::Anim { anim, frame, .. } = &node.kind
                && let Some(clip) = self.preview.clips.get(*anim)
                && *frame as usize >= clip.frames.len()
            {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!(
                        "Frame {frame} is outside this animation's {} frames (first frame = 0).",
                        clip.frames.len()
                    ),
                );
            }
            ui.weak(format!(
                "{count} {kind} assets in this scene (indices start at 0)."
            ));
            if index as usize >= count {
                ui.colored_label(ui.visuals().error_fg_color, format!("No {kind} asset at index {index}. Add an asset in Scene source or choose an existing index."));
            }
        }
        // A binding is per widget, not per kind -- two bars can show two
        // gauges -- so it has its own rows and goes to the primary only.
        let mut bind_edit = None;
        if node.kind.bind_target().is_some() || self.preview.bindings.iter().any(|b| b.node == id) {
            ui.separator();
            ui.strong("Live data");
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
        // What the widget does when a finger lands on it. Below the binding
        // because a widget is far more often something that shows a reading
        // than something that is pressed, and above the curve because it is
        // part of what the widget *is* rather than a tool for shaping it.
        let mut touch_edit = None;
        {
            let current = self
                .preview
                .hotspots
                .iter()
                .find(|h| h.node == id)
                .map(|h| h.action.clone());
            ui.separator();
            ui.strong("Touch action");
            egui::Grid::new("props-touch")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    touch_edit = crate::touch_ui::touch_rows(ui, current.as_ref());
                });
        }

        // A gauge's bands are a list, so they get rows of their own below the
        // one-row-per-field grid above.
        let mut zone_edit = None;
        if let Some(zones) = crate::zones_ui::zones_of(&node.kind) {
            let fill = crate::zones_ui::fill_of(&node.kind);
            let marked = crate::zones_ui::has_cursor(&node.kind);
            ui.separator();
            ui.strong("Colour bands");
            zone_edit = crate::zones_ui::zone_rows(ui, zones, fill, marked);
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
        if self.document_blocked() {
            return;
        }
        if let Some(field) = remove_field {
            self.unset_field_of(id, field);
            return;
        }
        if let Some((z, g)) = zone_edit {
            self.set_colour_bands(id, g, z);
            return;
        }
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
        if let Some(t) = touch_edit {
            match t {
                TouchEdit::Set(v) => self.set_field_of(id, None, "touch", &v),
                TouchEdit::Unset => self.unset_field_of(id, "touch"),
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
                ui.label("Display size");
                ui.horizontal(|ui| {
                    ui.monospace(format!("{w} × {h} px"));
                    if ui.small_button("Change size…").clicked() {
                        self.resize_to = Some((w, h));
                    }
                });
                ui.end_row();

                ui.label("Display CAN address");
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

                ui.label("Display shape");
                let round = self.preview.shape == Shape::Round;
                let mut pick = round;
                egui::ComboBox::from_id_salt("scene-shape")
                    .selected_text(if round { "Round" } else { "Rectangular" })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut pick, false, "Rectangular");
                        ui.selectable_value(&mut pick, true, "Round")
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

fn smoothing_title(value: &str) -> &str {
    match value {
        "inherit" => "Use parent setting",
        "on" => "Smooth",
        "off" => "Pixel-sharp",
        other => other,
    }
}
