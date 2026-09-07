// SPDX-License-Identifier: MIT OR Apache-2.0
//! Not losing work on the way out.
//!
//! The window manager's close button is the one command in the editor that
//! cannot be undone, and until now it threw away everything unsaved without
//! asking. Closing is vetoed while anything is dirty, and the person is
//! offered the three answers there are: save it, throw it away, or stay.
//!
//! # Why the veto rather than a save-on-exit
//!
//! Saving without being asked is its own kind of data loss: a scene edited by
//! accident -- a stray drag, a slider nudged with the wrong widget selected --
//! would be written over the good copy by the act of closing the window.
//! Asking costs one click and cannot destroy anything.

use crate::App;

/// What the person chose in the prompt.
enum Answer {
    /// Write everything out, then close.
    Save,
    /// Close and lose it.
    Discard,
    /// Stay open.
    Stay,
}

impl App {
    /// How many documents have unsaved changes, the parked ones included.
    ///
    /// A rig's other displays are just as unsaved as the one on screen, and
    /// a prompt that counted only the visible one would be a lie in exactly
    /// the case the prompt exists for.
    pub(crate) fn unsaved(&self) -> usize {
        usize::from(self.dirty)
            + self
                .rig
                .as_ref()
                .map_or(0, |r| r.docs.iter().filter(|d| d.dirty).count())
    }

    /// Hold the window open while there is unsaved work, and ask.
    ///
    /// Called before the panels each frame: the veto has to reach egui in the
    /// same frame the close was requested, or the window is already gone.
    pub(crate) fn close_guard(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.closing || self.unsaved() == 0 {
                // Either already answered, or nothing to lose.
                return;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.asking_close = true;
        }
        if !self.asking_close {
            return;
        }

        let n = self.unsaved();
        let what = match (&self.rig, self.path.as_ref()) {
            (Some(_), _) => format!(
                "{n} display{} of this rig {} unsaved changes.",
                if n == 1 { "" } else { "s" },
                if n == 1 { "has" } else { "have" }
            ),
            (None, Some(p)) => format!("{} has unsaved changes.", p.display()),
            (None, None) => "This scene has never been saved.".to_string(),
        };

        let mut answer = Answer::Stay;
        let mut open = true;
        egui::Window::new("Unsaved changes")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(&what);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Save and close").clicked() {
                        answer = Answer::Save;
                    }
                    if ui
                        .button("Close without saving")
                        .on_hover_text("The changes are lost")
                        .clicked()
                    {
                        answer = Answer::Discard;
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Answer::Stay;
                        self.asking_close = false;
                    }
                });
            });
        // The X on the prompt means the same as Cancel: stay, unsaved.
        if !open {
            self.asking_close = false;
        }

        match answer {
            Answer::Save => {
                if self.rig.is_some() {
                    self.save_all();
                } else {
                    self.save();
                }
                // Still dirty means the save did not happen -- no path was
                // chosen, or the scene does not parse and `save` refused it.
                // Staying open with the reason in the status bar beats
                // closing over the top of work that was never written.
                if self.unsaved() == 0 {
                    self.close_now(ctx);
                } else {
                    self.asking_close = false;
                    self.status = format!("not closed: {}", self.status);
                }
            }
            Answer::Discard => self.close_now(ctx),
            Answer::Stay => {}
        }
    }

    /// Let the next close through, and ask for one.
    fn close_now(&mut self, ctx: &egui::Context) {
        self.closing = true;
        self.asking_close = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsaved_counts_every_display_not_just_the_one_on_screen() {
        // A rig's parked displays are as unsaved as the visible one, and a
        // count that missed them would under-report in the one case the
        // prompt exists for.
        let dir = std::env::temp_dir().join(format!("copilot-close-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        for name in ["a", "b"] {
            std::fs::write(
                dir.join(format!("{name}-normal.scene")),
                r#"{"width":10,"height":10,"root":{"type":"panel","rect":[0,0,10,10]}}"#,
            )
            .expect("scene");
        }
        std::fs::write(
            dir.join("t.rig"),
            r#"{"modes":["normal"],"displays":[
                {"name":"a","node":1,"scenes":{"normal":"a-normal.scene"}},
                {"name":"b","node":2,"scenes":{"normal":"b-normal.scene"}}]}"#,
        )
        .expect("rig");

        let mut a = App::new(None, &[]);
        // A fresh unsaved scene counts on its own.
        a.dirty = true;
        assert_eq!(a.unsaved(), 1);

        a.open_rig(dir.join("t.rig"));
        assert_eq!(a.unsaved(), 0, "just opened: {}", a.status);
        a.dirty = true;
        assert_eq!(a.unsaved(), 1, "the display being edited");
        a.rig.as_mut().expect("a rig").docs[1].dirty = true;
        assert_eq!(a.unsaved(), 2, "and the one parked beside it");

        // The prompt's "Save and close" only closes when this reaches zero,
        // so what it keys off has to actually be cleared by saving.
        a.text.push(' ');
        a.rig.as_mut().expect("a rig").docs[1].text.push(' ');
        a.save_all();
        assert_eq!(
            a.unsaved(),
            0,
            "save_all left something dirty: {}",
            a.status
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_scene_that_will_not_parse_is_not_saved_over_the_good_copy() {
        // The other half of that branch: `save` refuses a broken document, so
        // the prompt must stay open rather than close over unwritten work.
        let dir = std::env::temp_dir().join(format!("copilot-close-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("s.scene");
        let good = r#"{"width":10,"height":10,"root":{"type":"panel","rect":[0,0,10,10]}}"#;
        std::fs::write(&path, good).expect("scene");

        let mut a = App::new(Some(path.clone()), &[]);
        a.text = "{ this is not a scene".into();
        a.dirty = true;
        a.reload();
        a.save();
        assert_eq!(a.unsaved(), 1, "a broken scene was saved anyway");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            good,
            "the good copy on disk was overwritten"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
