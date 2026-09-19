// SPDX-License-Identifier: GPL-3.0-only
//! Apply a colour-band gesture atomically, including migration from legacy thresholds.

use crate::App;
use crate::canvas::Gesture;
use crate::zones_ui::ZoneEdit;
use copilot::widget::NodeId;

impl App {
    /// Apply bands and remove superseded thresholds as one undoable source change.
    pub(super) fn set_colour_bands(
        &mut self,
        id: NodeId,
        gesture: Option<Gesture>,
        edit: ZoneEdit,
    ) {
        let Some(path) = self.node_path(id) else {
            self.status = "The selected widget is no longer available.".into();
            return;
        };
        let mut source = match edit {
            ZoneEdit::Set(value) => {
                let Some(source) = copilot::scene::set(&self.text, &path, "zones", &value) else {
                    self.status = "Could not write colour bands. Check the scene text.".into();
                    return;
                };
                source
            }
            ZoneEdit::Unset => copilot::scene::unset(&self.text, &path, "zones")
                .unwrap_or_else(|| self.text.clone()),
        };
        for field in ["warn", "warn_fill", "danger", "danger_fill"] {
            if let Some(without) = copilot::scene::unset(&source, &path, field) {
                source = without;
            }
        }
        if source == self.text {
            return;
        }
        // Removing legacy thresholds belongs to the band gesture, not four extra undo steps.
        self.begin_edit(gesture);
        self.text = source;
        self.dirty = true;
        self.reload();
        self.status = "Colour bands updated.".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r##"{
      "width":20,"height":20,
      // Keep the document's comments and field order.
      "root":{"name":"gauge","type":"bar","rect":[0,0,20,20],
        "warn":0.7,"warn_fill":"#ffaa00","danger":0.9,"danger_fill":"#ff0000",
        "vertical":false}
    }"##;

    #[test]
    fn migrating_legacy_bands_is_one_gesture_and_one_undo() {
        let mut app = crate::tests::app(SOURCE, "gauge");
        let node = app.selected.unwrap();
        let undo_count = app.undo.len();
        let id = egui::Id::new("band-drag");
        for (at, ends) in [(0.65, false), (0.6, true)] {
            app.set_colour_bands(
                node,
                Some(Gesture { id, ends }),
                ZoneEdit::Set(format!(r##"[{{"at":{at},"color":"#ffaa00"}}]"##)),
            );
        }
        assert_eq!(app.undo.len(), undo_count + 1);
        assert!(!app.text.contains("\"warn\""));
        assert!(!app.text.contains("\"danger\""));
        assert!(app.text.contains("// Keep the document's comments"));
        assert!(app.text.find("\"name\"").unwrap() < app.text.find("\"type\"").unwrap());
        assert!(app.preview.error.is_none());
        app.undo();
        assert_eq!(app.text, SOURCE);
    }

    #[test]
    fn removing_legacy_bands_is_atomic_and_does_not_report_absent_keys() {
        let mut app = crate::tests::app(SOURCE, "gauge");
        let node = app.selected.unwrap();
        let undo_count = app.undo.len();
        app.set_colour_bands(node, None, ZoneEdit::Unset);
        assert_eq!(app.undo.len(), undo_count + 1);
        assert_eq!(app.status, "Colour bands updated.");
        assert!(app.preview.error.is_none());
        app.undo();
        assert_eq!(app.text, SOURCE);
    }
}
