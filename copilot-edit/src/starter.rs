// SPDX-License-Identifier: MIT OR Apache-2.0
//! What a new document and a new widget start as, what each kind is called,
//! and the finished scenes the editor can open without a file dialog.
//!
//! Apart from the editor's state because none of it changes at run time: it
//! is the vocabulary the rest of the editor speaks in, and a table is easier
//! to check against the scene format than a table buried in a struct's impl.

use copilot::widget::Kind;

use crate::{App, Driver};

/// One element wrapped as a single-element array.
///
/// Its own function only so the brackets are written once; getting them wrong
/// produces a document that still parses and has lost a widget.
pub(crate) fn alloc_array(one: &str) -> String {
    format!("[{one}]")
}

/// A new widget of `kind`, small and visible so it can be found and dragged.
///
/// Deliberately not centred or sized to its parent: a new widget appearing at
/// a predictable spot is easier to grab than one that lands somewhere derived
/// from a layout the author has not written yet.
pub(crate) fn starter_widget(kind: &str) -> String {
    let body = match kind {
        "label" => r##""text": "text", "color": "#ffffff""##,
        "bar" => r##""value": 0.5, "fill": "#58a6ff", "track": "#161b22""##,
        "frame" => r##""color": "#8b949e""##,
        "led" => r##""color": "#3fb950", "level": 1.0, "glow": 0.15"##,
        "roundrect" => r##""background": "#30363d", "radius": 8"##,
        "arc" => r##""value": 0.6, "thickness": 8, "fill": "#58a6ff", "track": "#161b22""##,
        "needle" => r##""value": 0.6, "width": 3, "color": "#f85149", "hub": 5"##,
        "scale" => {
            r##""ticks": 11, "major_every": 5, "length": 8, "width": 2, "color": "#8b949e", "major_color": "#e6edf3""##
        }
        "line" => {
            r##""points": [[0.0, 1.0], [0.35, 0.3], [0.7, 0.1], [1.0, 0.5]], "width": 2, "color": "#58a6ff""##
        }
        "chart" => {
            r##""values": [0.2, 0.5, 0.4, 0.8, 0.6, 0.9], "width": 2, "stroke": "#58a6ff", "fill": "#1f6feb""##
        }
        "sevenseg" => r##""text": "88", "color": "#f0f6fc", "ghost": "#22262c", "thickness": 5"##,
        "gradient" => r##""from": "#1f6feb", "to": "#0d1117", "vertical": true"##,
        "polygon" => r##""points": [[0.0, 0.5], [1.0, 0.0], [1.0, 1.0]], "color": "#3fb950""##,
        "ruler" => {
            r##""ticks": 11, "major_every": 5, "length": 8, "width": 1, "color": "#8b949e", "major_color": "#e6edf3""##
        }
        "segbar" => {
            r##""value": 0.6, "segments": 20, "gap": 2, "fill": "#58a6ff", "track": "#161b22", "warn": 0.7, "warn_fill": "#d29922", "danger": 0.9, "danger_fill": "#f85149""##
        }
        "grid" => r##""pitch_x": 16, "pitch_y": 16, "width": 1, "color": "#1f2630""##,
        _ => r##""background": "#30363d""##,
    };
    // The dial kinds want a square that a sweep can actually fill; the default
    // strip would hand the author a needle two pixels long.
    let rect = match kind {
        "arc" | "needle" | "scale" => "[8, 8, 96, 96]",
        "line" | "chart" => "[8, 8, 128, 64]",
        "sevenseg" => "[8, 8, 96, 72]",
        "polygon" => "[8, 8, 64, 64]",
        "ruler" => "[8, 8, 160, 20]",
        "segbar" => "[8, 8, 160, 32]",
        _ => "[8, 8, 64, 24]",
    };
    format!(r#"{{ "type": "{kind}", "rect": {rect}, {body} }}"#)
}

/// The name a widget kind goes by in the outliner and the scene format.
pub(crate) fn kind_name(k: &Kind) -> &'static str {
    match k {
        Kind::Panel { .. } => "panel",
        Kind::Frame { .. } => "frame",
        Kind::Label { .. } => "label",
        Kind::Image { .. } => "image",
        Kind::Anim { .. } => "anim",
        Kind::Bar { .. } => "bar",
        Kind::Led { .. } => "led",
        Kind::RoundRect { .. } => "roundrect",
        Kind::Arc { .. } => "arc",
        Kind::Needle { .. } => "needle",
        Kind::Scale { .. } => "scale",
        Kind::Line { .. } => "line",
        Kind::Chart { .. } => "chart",
        Kind::SevenSeg { .. } => "sevenseg",
        Kind::Gradient { .. } => "gradient",
        Kind::Polygon { .. } => "polygon",
        Kind::Ruler { .. } => "ruler",
        Kind::SegBar { .. } => "segbar",
        Kind::Grid { .. } => "grid",
        // Kind is #[non_exhaustive], so a newer copilot can add a widget this
        // editor build has never heard of. Showing it as unknown beats
        // refusing to compile against a library that grew.
        _ => "unknown",
    }
}

/// The scenes offered under File > Examples.
///
/// Compiled in rather than read from `examples/` at run time, because an
/// installed editor is a binary somewhere with no source tree beside it, and
/// an example that is only there for people who built from a checkout is not
/// an example. Each is a label, a line of description, and what it opens.
///
/// Only scenes that draw with no external asset are listed: an embedded copy
/// has no directory to resolve `images` against, so `image.scene` would open
/// showing placeholders and teach the wrong lesson.
pub(crate) const EXAMPLES: &[(&str, &str, Example)] = &[
    (
        "Instrument cluster",
        "A 320x160 bench panel: labels, bars and an animated reading",
        Example::Scene(include_str!("../../examples/cluster.scene")),
    ),
    (
        "Z31 300ZX replica",
        "The full 2400x900 dashboard, every widget the toolkit has",
        Example::Scene(include_str!("../../examples/z31.scene")),
    ),
    (
        "Z31 rig: cluster and two gauges",
        "The cluster with a MAP and a water gauge beside it, every instrument \
         bound to the bus, in normal, sport and track modes",
        Example::Rig(Z31_RIG),
    ),
];

/// What an entry of [`EXAMPLES`] opens.
pub(crate) enum Example {
    /// One scene, opened with no path so that Save asks where.
    Scene(&'static str),
    /// A rig and its scenes, by file name. A rig is files that name each
    /// other, so these are written to a directory of their own and opened
    /// from there; the status bar says where.
    Rig(&'static [(&'static str, &'static str)]),
}

/// The files of the Z31 rig example, the rig itself first.
const Z31_RIG: &[(&str, &str)] = &[
    ("z31.rig", include_str!("../../examples/z31.rig")),
    (
        "z31-normal.scene",
        include_str!("../../examples/z31-normal.scene"),
    ),
    (
        "z31-sport.scene",
        include_str!("../../examples/z31-sport.scene"),
    ),
    (
        "z31-track.scene",
        include_str!("../../examples/z31-track.scene"),
    ),
    (
        "gauge-left-normal.scene",
        include_str!("../../examples/gauge-left-normal.scene"),
    ),
    (
        "gauge-left-sport.scene",
        include_str!("../../examples/gauge-left-sport.scene"),
    ),
    (
        "gauge-left-track.scene",
        include_str!("../../examples/gauge-left-track.scene"),
    ),
    (
        "gauge-right-normal.scene",
        include_str!("../../examples/gauge-right-normal.scene"),
    ),
    (
        "gauge-right-sport.scene",
        include_str!("../../examples/gauge-right-sport.scene"),
    ),
    (
        "gauge-right-track.scene",
        include_str!("../../examples/gauge-right-track.scene"),
    ),
];

impl App {
    /// Load one of the compiled-in examples as the current document.
    ///
    /// A scene's path is cleared rather than pointed at `examples/`: an
    /// example is something to take apart, and the first Ctrl+S must ask
    /// where to put it instead of writing over the copy the editor ships.
    /// That also makes the document unsaved, which is what the modified
    /// marker then says. A rig is unpacked to a directory of its own instead,
    /// because its files have to be able to find each other.
    ///
    /// The history goes with the old document, for the same reason it does in
    /// [`App::start`]: an undo that resurrected the scene the person had just
    /// left would be a surprise.
    pub(crate) fn open_example(&mut self, label: &str, example: &Example) {
        match example {
            Example::Scene(text) => {
                self.close_rig();
                self.text = (*text).to_string();
                self.path = None;
                self.undo.clear();
                self.redo.clear();
                self.gesture = None;
                self.dirty = true;
                self.selected = None;
                self.extra.clear();
                self.driver = Driver::default();
                self.reload();
                // Fitted rather than left at whatever the last scene wanted:
                // the Z31 is 2400 wide and opens off the side of the window
                // otherwise.
                self.camera.fit();
                self.status = format!("opened the {label} example");
            }
            Example::Rig(files) => {
                let dir =
                    std::env::temp_dir().join(format!("copilot-example-{}", std::process::id()));
                let unpacked = std::fs::create_dir_all(&dir).and_then(|()| {
                    files
                        .iter()
                        .try_for_each(|(name, text)| std::fs::write(dir.join(name), text))
                });
                if let Err(e) = unpacked {
                    self.status = format!("could not unpack the example: {e}");
                    return;
                }
                let Some((rig, _)) = files.iter().find(|(n, _)| n.ends_with(".rig")) else {
                    self.status = "the example has no rig file".into();
                    return;
                };
                self.driver = Driver::default();
                self.open_rig(dir.join(rig));
                if self.rig.is_some() {
                    self.status =
                        format!("opened the {label} example, unpacked to {}", dir.display());
                }
            }
        }
    }
}

/// The scene a new document starts from.
pub(crate) const STARTER: &str = r##"{
  "width": 480,
  "height": 480,
  "root": {
    "type": "panel",
    "rect": [0, 0, 320, 160],
    "background": "#0d1117",
    "children": [
      { "type": "label", "rect": [16, 16, 200, 20], "name": "title",
        "text": "copilot", "color": "#f0f6fc" },
      { "type": "bar", "rect": [16, 60, 288, 24], "name": "demo",
        "value": 0.4, "fill": "#58a6ff", "track": "#161b22" }
    ]
  }
}
"##;

#[cfg(test)]
mod tests {
    use super::*;

    /// The examples are compiled in, so a scene that stops parsing takes the
    /// editor's Examples menu with it silently. This is what makes that loud.
    #[test]
    fn every_example_parses_and_builds() {
        let build = |label: &str, text: &str| {
            let doc = copilot::scene::parse(text)
                .unwrap_or_else(|e| panic!("the {label} example does not parse: {e:?}"));
            let scene = copilot::scene::build_scene(&doc)
                .unwrap_or_else(|e| panic!("the {label} example does not build: {e:?}"));
            assert!(
                scene.tree.len() > 1,
                "the {label} example built an empty tree"
            );
            assert!(
                scene.requests.is_empty(),
                "the {label} example wants an asset, which a compiled-in copy \
                 has no directory to find"
            );
            scene
        };
        for (label, hint, ex) in EXAMPLES {
            assert!(!hint.is_empty(), "the {label} example has no description");
            match ex {
                Example::Scene(text) => {
                    build(label, text);
                }
                Example::Rig(files) => {
                    let (_, rig_text) = files
                        .iter()
                        .find(|(n, _)| n.ends_with(".rig"))
                        .unwrap_or_else(|| panic!("the {label} example has no rig"));
                    let rig = copilot::rig::parse(rig_text)
                        .unwrap_or_else(|e| panic!("the {label} rig does not parse: {e:?}"));
                    for d in &rig.displays {
                        for (m, scene) in d.scenes.iter().enumerate() {
                            let (_, text) = files
                                .iter()
                                .find(|(n, _)| n == scene)
                                .unwrap_or_else(|| panic!("{label}: {scene} is not packed"));
                            let s = build(&format!("{label} / {scene}"), text);
                            assert_eq!(rig.check(d.node, m, &s), Ok(()), "{scene}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn opening_an_example_leaves_nowhere_to_save_over() {
        let (label, text) = EXAMPLES
            .iter()
            .find_map(|(l, _, e)| match e {
                Example::Scene(t) => Some((*l, *t)),
                Example::Rig(_) => None,
            })
            .expect("a scene example");
        let mut a = crate::App::new(None, &[]);
        a.path = Some("somewhere.scene".into());
        a.checkpoint();
        a.open_example(label, &Example::Scene(text));
        assert!(
            a.path.is_none(),
            "Ctrl+S must ask, not overwrite the example"
        );
        assert!(a.undo.is_empty() && a.redo.is_empty());
        assert!(a.dirty, "an example is not saved anywhere yet");
        assert!(a.preview.error.is_none(), "{:?}", a.preview.error);
        assert_eq!(a.text, text);
    }

    #[test]
    fn the_rig_example_opens_as_a_rig_of_three() {
        let (label, _, ex) = EXAMPLES
            .iter()
            .find(|(_, _, e)| matches!(e, Example::Rig(_)))
            .expect("a rig example");
        let mut a = crate::App::new(None, &[]);
        a.open_example(label, ex);
        let rig = a.rig.as_ref().unwrap_or_else(|| panic!("{}", a.status));
        assert_eq!(rig.docs.len(), 3);
        assert_eq!(rig.rig.modes.len(), 3);
        assert!(a.preview.error.is_none(), "{:?}", a.preview.error);
        for d in rig.docs.iter().skip(1) {
            assert!(d.preview.error.is_none(), "{:?}", d.preview.error);
        }
        assert!(!a.dirty, "unpacked files are saved files");
    }
}
