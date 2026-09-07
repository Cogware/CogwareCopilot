// SPDX-License-Identifier: MIT OR Apache-2.0
//! The two dialogs about the whole document: starting a scene, and resizing
//! one.
//!
//! Together because they ask the same question -- how big -- and offer the
//! same answers. A new scene is also the one place the editor writes a
//! document from nothing rather than splicing one that exists, so the text
//! it produces is assembled here, where it can be tested without a window.

use crate::{App, Driver};

/// The sizes both dialogs offer as presets.
///
/// The small one is a bench display; the rest are the panels a cluster is
/// actually built for, ending with the wide strip a dash carries.
pub(crate) const PRESETS: &[(&str, (u32, u32))] = &[
    ("320x160", (320, 160)),
    ("480x480", (480, 480)),
    ("800x480", (800, 480)),
    ("1280x720", (1280, 720)),
    ("1920x1080", (1920, 1080)),
    ("2400x900", (2400, 900)),
];

/// What a new scene is made of.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct NewDoc {
    /// Scene width in pixels.
    pub width: u32,
    /// Scene height in pixels.
    pub height: u32,
    /// The root panel's colour.
    pub background: [u8; 3],
    /// Whether the scene draws its edges blended.
    pub antialias: bool,
    /// Whether to start with a label and a bar to grab, rather than an
    /// empty panel.
    pub sample: bool,
    /// The CAN node address of the display this scene is for, or 0 for a
    /// scene with no bus behind it.
    pub node: u8,
    /// Whether the panel is round, its corners behind a bezel.
    pub round: bool,
}

impl Default for NewDoc {
    fn default() -> Self {
        Self {
            width: 800,
            height: 480,
            background: [0x0d, 0x11, 0x17],
            antialias: true,
            sample: false,
            node: 0,
            round: false,
        }
    }
}

/// A drag box for a CAN node address, shown in hex the way addresses are
/// written on a wiring diagram, with 0 meaning "none".
pub(crate) fn node_field(ui: &mut egui::Ui, node: &mut u8) -> egui::Response {
    ui.add(
        egui::DragValue::new(node)
            .range(0..=254)
            .speed(0.1)
            .custom_formatter(|v, _| {
                let n = v.round() as u8;
                if n == 0 {
                    "none".to_string()
                } else {
                    format!("0x{n:02X}")
                }
            })
            .custom_parser(parse_node),
    )
}

/// `0x01`, `1` or `none`, as a person types them.
fn parse_node(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("none") {
        return Some(0.0);
    }
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u8::from_str_radix(hex, 16).ok(),
        None => s.parse::<u8>().ok(),
    }
    .map(f64::from)
}

impl NewDoc {
    /// The scene file this describes.
    ///
    /// Laid out the way a person would write it, since a person is about to
    /// read and edit it. `antialias` is written only when on, so a scene
    /// that does not want it does not carry a key saying so.
    pub fn compose(&self) -> String {
        let (w, h) = (self.width.max(1), self.height.max(1));
        let [r, g, b] = self.background;
        let antialias = if self.antialias {
            "  \"antialias\": true,\n"
        } else {
            ""
        };
        // Written in hex because that is how a node address is thought about
        // and wired; the format takes either spelling.
        let node = if self.node != 0 {
            format!("  \"node\": \"0x{:02X}\",\n", self.node)
        } else {
            String::new()
        };
        let shape = if self.round {
            "  \"shape\": \"round\",\n"
        } else {
            ""
        };
        let children = if self.sample {
            format!(
                "\n      {{ \"type\": \"label\", \"rect\": [16, 16, 200, 20], \"name\": \"title\",\n        \
                 \"text\": \"copilot\", \"color\": \"#f0f6fc\" }},\n      \
                 {{ \"type\": \"bar\", \"rect\": [16, 60, {}, 24], \"name\": \"demo\",\n        \
                 \"value\": 0.4, \"fill\": \"#58a6ff\", \"track\": \"#161b22\" }}\n    ",
                w.saturating_sub(32).max(8)
            )
        } else {
            String::new()
        };
        format!(
            "{{\n  \"width\": {w},\n  \"height\": {h},\n{node}{shape}{antialias}  \"root\": {{\n    \
             \"type\": \"panel\",\n    \"rect\": [0, 0, {w}, {h}],\n    \
             \"background\": \"#{r:02x}{g:02x}{b:02x}\",\n    \"children\": [{children}]\n  }}\n}}\n"
        )
    }
}

impl App {
    /// Replace the document with a fresh scene.
    ///
    /// The history goes with the old document: an undo that brought back a
    /// scene the person had just decided to leave would be a surprise, and
    /// the way back is the file on disk, which is untouched.
    pub(crate) fn start(&mut self, spec: &NewDoc) {
        self.close_rig();
        self.text = spec.compose();
        self.path = None;
        self.undo.clear();
        self.redo.clear();
        self.gesture = None;
        self.dirty = true;
        self.selected = None;
        self.extra.clear();
        self.driver = Driver::default();
        self.camera.fit();
        self.reload();
        self.status = format!("new {}x{} scene", spec.width, spec.height);
    }

    /// The new-scene dialog, while it is open.
    pub(crate) fn new_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut spec) = self.new_doc.clone() else {
            return;
        };
        let mut open = true;
        let mut go = false;
        let mut cancel = false;
        egui::Window::new("New scene")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(ctx, |ui| {
                size_fields(ui, &mut spec.width, &mut spec.height);
                ui.separator();
                egui::Grid::new("new-settings")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("background");
                        ui.color_edit_button_srgb(&mut spec.background);
                        ui.end_row();
                        ui.label("antialias");
                        ui.checkbox(&mut spec.antialias, "").on_hover_text(
                            "Blend the edges of needles, rings, curves, bars and cells",
                        );
                        ui.end_row();
                        ui.label("contents");
                        ui.checkbox(&mut spec.sample, "a label and a bar to start from")
                            .on_hover_text("Otherwise the root panel is empty; use Add to fill it");
                        ui.end_row();
                        ui.label("node");
                        node_field(ui, &mut spec.node).on_hover_text(
                            "CAN address of the display this scene is for. 0x00 is \
                             the gateway; leave it at none for a scene with no bus.",
                        );
                        ui.end_row();
                        ui.label("shape");
                        egui::ComboBox::from_id_salt("new-shape")
                            .selected_text(if spec.round { "round" } else { "rect" })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut spec.round, false, "rect");
                                ui.selectable_value(&mut spec.round, true, "round")
                                    .on_hover_text("A panel whose corners sit behind a bezel");
                            });
                        ui.end_row();
                    });
                if self.dirty {
                    ui.separator();
                    ui.colored_label(
                        egui::Color32::YELLOW,
                        "The current scene has unsaved changes, which a new scene discards.",
                    );
                }
                ui.separator();
                ui.horizontal(|ui| {
                    go = ui.button("Create").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if go {
            self.start(&spec);
        }
        self.new_doc = if go || cancel || !open {
            None
        } else {
            Some(spec)
        };
    }

    /// The resolution dialog, while it is open.
    pub(crate) fn resolution_dialog(&mut self, ctx: &egui::Context) {
        if let Some((mut w, mut h)) = self.resize_to {
            let mut open = true;
            let mut go = false;
            egui::Window::new("Scene resolution")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    size_fields(ui, &mut w, &mut h);
                    ui.label(
                        egui::RichText::new(
                            "Every rect, thickness and text magnification is scaled to \
                             match. Comments and key order are left alone.",
                        )
                        .small()
                        .weak(),
                    );
                    ui.separator();
                    go = ui.button("Rescale").clicked();
                });
            self.resize_to = if go || !open { None } else { Some((w, h)) };
            if go {
                self.apply_rescale(w, h);
            }
        }
    }
}

/// A width, a height, and the presets that fill them in.
fn size_fields(ui: &mut egui::Ui, w: &mut u32, h: &mut u32) {
    ui.horizontal(|ui| {
        ui.label("Width");
        ui.add(egui::DragValue::new(w).range(1..=16384));
        ui.label("Height");
        ui.add(egui::DragValue::new(h).range(1..=16384));
    });
    ui.horizontal(|ui| {
        for (label, preset) in PRESETS {
            if ui.button(*label).clicked() {
                (*w, *h) = *preset;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use copilot::widget::{Kind, ROOT};

    fn build(spec: &NewDoc) -> copilot::widget::Tree {
        let text = spec.compose();
        let doc = copilot::scene::parse(&text).unwrap_or_else(|e| panic!("{e:?}\n{text}"));
        copilot::scene::build_scene(&doc)
            .unwrap_or_else(|e| panic!("{e:?}\n{text}"))
            .tree
    }

    #[test]
    fn a_new_scene_is_the_size_and_colour_it_was_asked_for() {
        let spec = NewDoc {
            width: 640,
            height: 200,
            background: [0x10, 0x20, 0x30],
            antialias: false,
            sample: false,
            ..NewDoc::default()
        };
        let tree = build(&spec);
        let root = tree.get(ROOT).expect("root");
        let doc = tree.get(root.children[0]).expect("document root");
        assert_eq!((doc.rect.size.w, doc.rect.size.h), (640, 200));
        assert_eq!(root.rect.size.w, 640);
        let Kind::Panel { background } = doc.kind else {
            panic!("the root is not a panel");
        };
        assert_eq!(background, copilot::Color::rgb(0x10, 0x20, 0x30));
        assert_eq!(doc.antialias, Some(false));
        assert!(doc.children.is_empty());
        assert!(
            !spec.compose().contains("antialias"),
            "off is the default and not written"
        );
    }

    #[test]
    fn antialiasing_on_is_written_and_inherited() {
        let spec = NewDoc {
            antialias: true,
            sample: true,
            ..NewDoc::default()
        };
        let tree = build(&spec);
        let root = tree.get(ROOT).expect("root");
        let doc = tree.get(root.children[0]).expect("document root");
        assert_eq!(doc.antialias, Some(true));
        assert_eq!(doc.children.len(), 2, "the sample label and bar");
        assert!(tree.find("demo").is_some());
    }

    #[test]
    fn a_node_and_a_round_panel_are_written_only_when_asked_for() {
        let plain = NewDoc::default().compose();
        assert!(
            !plain.contains("node") && !plain.contains("shape"),
            "{plain}"
        );

        let spec = NewDoc {
            node: 2,
            round: true,
            ..NewDoc::default()
        };
        let text = spec.compose();
        let doc = copilot::scene::parse(&text).unwrap_or_else(|e| panic!("{e:?}\n{text}"));
        let scene = copilot::scene::build_scene(&doc).unwrap_or_else(|e| panic!("{e:?}\n{text}"));
        assert_eq!(scene.node, Some(2));
        assert_eq!(scene.shape, copilot::scene::Shape::Round);
        assert!(
            text.contains("\"node\": \"0x02\""),
            "hex, as it is wired: {text}"
        );
    }

    #[test]
    fn a_node_address_parses_the_ways_people_type_it() {
        assert_eq!(parse_node("0x1A"), Some(26.0));
        assert_eq!(parse_node(" 0X02 "), Some(2.0));
        assert_eq!(parse_node("7"), Some(7.0));
        assert_eq!(parse_node("none"), Some(0.0));
        assert_eq!(parse_node("0x1G"), None);
        assert_eq!(parse_node("300"), None);
    }

    #[test]
    fn an_empty_root_still_has_a_list_to_add_into() {
        // `put_inside` finds `children` and inserts; without the key it
        // would have to create one, which it can, but a fresh file should
        // already read the way the editor will leave it.
        assert!(NewDoc::default().compose().contains("\"children\": []"));
    }

    #[test]
    fn a_zero_size_is_not_written() {
        let spec = NewDoc {
            width: 0,
            height: 0,
            ..NewDoc::default()
        };
        let tree = build(&spec);
        assert_eq!(tree.get(ROOT).expect("root").rect.size.w, 1);
    }

    #[test]
    fn starting_over_drops_the_history_and_the_path() {
        let mut a = crate::App::new(None, &[]);
        a.path = Some("somewhere.scene".into());
        a.checkpoint();
        a.text.push(' ');
        a.start(&NewDoc::default());
        assert!(a.path.is_none());
        assert!(a.undo.is_empty() && a.redo.is_empty());
        assert!(a.dirty, "a new scene is not yet saved anywhere");
        assert!(a.preview.error.is_none(), "{:?}", a.preview.error);
        assert_eq!(a.preview.scene_size(), (800, 480));
    }
}
