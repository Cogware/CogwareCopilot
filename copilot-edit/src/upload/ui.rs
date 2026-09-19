// SPDX-License-Identifier: GPL-3.0-only
//! Upload controls with explicit connection, transfer and cancellation progress.

use std::time::{Duration, Instant};

use super::cancel::Cancel;
use super::connection::{Connecting, Connection};
use super::{Error, Push, State, Transport};

pub use super::monitor::{LinkState, Monitor};

pub(super) const WARN: egui::Color32 = egui::Color32::from_rgb(255, 176, 64);

struct Work {
    link: Box<dyn Transport>,
    push: Push,
}

enum Doing {
    Asking,
    Connecting(Connecting, Box<Push>),
    Working(Box<Work>),
    Cancelling(Box<Cancel>),
    Said(Result<String, Error>),
}

/// The upload window and any transfer still finishing after it is closed.
pub struct Uploader {
    open: bool,
    started: Instant,
    connection: Connection,
    node: u8,
    slot: u8,
    destinations: super::destination::Choices,
    rig: bool,
    doing: Doing,
}

impl Default for Uploader {
    fn default() -> Self {
        Self {
            open: false,
            started: Instant::now(),
            connection: Connection::default(),
            node: 0x02,
            slot: 0,
            destinations: super::destination::Choices::default(),
            rig: false,
            doing: Doing::Asking,
        }
    }
}

impl Uploader {
    pub(super) fn configure_destination(
        &mut self,
        node: Option<u8>,
        slot: Option<u8>,
        displays: Vec<(String, u8)>,
        modes: Vec<String>,
    ) {
        if self.busy() {
            return;
        }
        if let Some(node) = node {
            self.node = node;
        }
        self.slot = slot.unwrap_or(0);
        self.destinations = super::destination::Choices { displays, modes };
    }

    /// Show the upload controls without discarding an active transfer.
    pub fn open(&mut self, rig: bool) {
        self.open = true;
        if !self.busy() {
            self.rig = rig;
            self.doing = Doing::Asking;
        }
    }

    /// Whether the upload owns, or is looking for, a controller connection.
    pub fn busy(&self) -> bool {
        self.connection.busy()
            || matches!(
                self.doing,
                Doing::Connecting(..) | Doing::Working(..) | Doing::Cancelling(..)
            )
    }

    /// Describe active upload work for the editor's persistent status bar.
    pub fn activity(&self) -> Option<String> {
        match &self.doing {
            Doing::Connecting(pending, _) => Some(format!("Connecting to {}...", pending.label)),
            Doing::Working(work) => Some(format!(
                "Uploading to display 0x{:02X}: {}%",
                self.node,
                work.push.percent(),
            )),
            Doing::Cancelling(_) => Some("Cancelling upload...".into()),
            _ if self.connection.busy() => Some("Searching for Bluetooth devices...".into()),
            _ => None,
        }
    }

    /// Whether the document being uploaded is a rig rather than a scene.
    pub fn sending_rig(&self) -> bool {
        self.rig
    }

    fn advance(&mut self) {
        self.connection.poll();
        if let Doing::Connecting(pending, _) = &self.doing
            && let Some(result) = pending.job.poll()
        {
            let Doing::Connecting(_, push) = std::mem::replace(&mut self.doing, Doing::Asking)
            else {
                unreachable!()
            };
            self.doing = match result {
                Ok(link) => {
                    self.started = Instant::now();
                    Doing::Working(Box::new(Work { link, push: *push }))
                }
                Err(error) => Doing::Said(Err(error)),
            };
        }
        let finished = match &mut self.doing {
            Doing::Working(work) => {
                match work.push.step(
                    work.link.as_mut(),
                    self.started.elapsed().as_millis() as u64,
                ) {
                    Ok(State::Done) => {
                        Some(Ok("Upload complete. The display confirmed receipt.".into()))
                    }
                    Ok(_) => None,
                    Err(error) => Some(Err(error)),
                }
            }
            Doing::Cancelling(cancel) => match cancel.step() {
                Ok(Some(message)) => Some(Ok(message)),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            },
            _ => None,
        };
        if let Some(result) = finished {
            if result.is_err() {
                self.open = true;
            }
            self.doing = Doing::Said(result);
        }
    }

    fn cancel(&mut self) {
        self.connection.cancel();
        self.doing = match std::mem::replace(&mut self.doing, Doing::Asking) {
            Doing::Working(work) => Doing::Cancelling(Box::new(Cancel::new(work.link, work.push))),
            Doing::Connecting(..) => {
                Doing::Said(Ok("Connection cancelled. Nothing was uploaded.".into()))
            }
            other => other,
        };
    }

    /// Draw the dialog and advance active work without waiting for radio or network I/O.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        monitor_busy: bool,
        bytes: impl FnOnce(bool) -> Option<Vec<u8>>,
    ) {
        self.advance();
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("Upload to display")
            .open(&mut open)
            .default_width(520.0)
            .resizable(true)
            .vscroll(true)
            .show(ctx, |ui| self.body(ui, monitor_busy, bytes));
        if !open {
            self.cancel();
        }
        self.open &= open;
    }

    fn body(
        &mut self,
        ui: &mut egui::Ui,
        monitor_busy: bool,
        bytes: impl FnOnce(bool) -> Option<Vec<u8>>,
    ) {
        match &self.doing {
            Doing::Asking => self.ask(ui, monitor_busy, bytes),
            Doing::Connecting(pending, _) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("Connecting to {}...", pending.label));
                });
                ui.small(format!(
                    "Elapsed: {} s. You can keep editing.",
                    pending.started.elapsed().as_secs()
                ));
                if ui.button("Cancel connection").clicked() {
                    self.cancel();
                }
            }
            Doing::Working(work) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!(
                        "Uploading to display 0x{:02X} via {}",
                        self.node,
                        work.link.describe()
                    ));
                });
                ui.add(
                    egui::ProgressBar::new(f32::from(work.push.percent()) / 100.0)
                        .show_percentage(),
                );
                ui.small("Progress shows the data confirmed by the display, not just sent by this computer.");
                ui.small(format!("Elapsed: {} s", self.started.elapsed().as_secs()));
                if ui.button("Cancel upload").clicked() {
                    self.cancel();
                }
            }
            Doing::Cancelling(_) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Cancelling upload...");
                });
                ui.small("Waiting for the controller to release the transfer.");
            }
            Doing::Said(result) => {
                let retry = result.is_err();
                match result {
                    Ok(message) => {
                        ui.label(message);
                    }
                    Err(error) => {
                        ui.colored_label(WARN, format!("Upload could not finish: {error}"));
                        ui.small("Check the selected device, display address and connection, then try again.");
                    }
                }
                ui.horizontal(|ui| {
                    if ui
                        .button(if retry { "Try again" } else { "New upload" })
                        .clicked()
                    {
                        self.doing = Doing::Asking;
                    }
                    if ui.button("Close").clicked() {
                        self.open = false;
                    }
                });
            }
        }
    }

    fn ask(
        &mut self,
        ui: &mut egui::Ui,
        monitor_busy: bool,
        bytes: impl FnOnce(bool) -> Option<Vec<u8>>,
    ) {
        ui.heading(if self.rig {
            "Upload rig definition"
        } else {
            "Upload current scene"
        });
        if monitor_busy {
            ui.colored_label(WARN, "Disconnect in Connections before uploading. The controller accepts one connection at a time.");
        }
        ui.add_enabled_ui(!monitor_busy, |ui| {
            self.connection.show(ui);
            ui.separator();
            if !self.destinations.displays.is_empty() {
                ui.horizontal(|ui| {
                    ui.label("Upload");
                    ui.selectable_value(&mut self.rig, false, "Current scene");
                    ui.selectable_value(&mut self.rig, true, "Rig definition");
                });
            }
            super::destination::show(ui, &mut self.node, &mut self.slot, self.rig, &self.destinations);
            ui.separator();
            if ui.add_enabled(self.connection.ready() && super::destination::valid(self.node, self.slot, self.rig), egui::Button::new(
                if self.rig { "Upload rig" } else { "Upload scene" }
            )).clicked() {
                let Some(image) = bytes(self.rig) else {
                    self.doing = Doing::Said(Err(Error::Link(
                        "The document is empty or has errors. Fix the errors shown in the editor before uploading.".into(),
                    )));
                    return;
                };
                let kind = if self.rig { cogware_can::xfer::kind::RIG } else { cogware_can::xfer::kind::SCENE };
                let push = Push::new(self.node, kind, if self.rig { 0 } else { self.slot }, image);
                match self.connection.connect() {
                    Ok(pending) => self.doing = Doing::Connecting(pending, Box::new(push)),
                    Err(error) => self.doing = Doing::Said(Err(error)),
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests_support::Fake;
    use super::*;

    #[test]
    fn uploading_and_cancelling_have_distinct_global_progress_labels() {
        let mut uploader = working();
        assert_eq!(
            uploader.activity().as_deref(),
            Some("Uploading to display 0x02: 0%")
        );
        uploader.cancel();
        assert_eq!(uploader.activity().as_deref(), Some("Cancelling upload..."));
    }

    #[test]
    fn a_failure_after_closing_an_upload_is_still_shown() {
        struct Failed;
        impl Transport for Failed {
            fn send(&mut self, _: &[u8]) -> Result<bool, Error> {
                Ok(true)
            }
            fn poll(&mut self, _: &mut Vec<u8>) -> Result<(), Error> {
                Err(Error::Link("Controller disconnected".into()))
            }
            fn describe(&self) -> String {
                "test controller".into()
            }
        }
        let mut uploader = working();
        if let Doing::Working(work) = &mut uploader.doing {
            work.link = Box::new(Failed);
        }
        uploader.cancel();
        uploader.open = false;
        uploader.advance();
        assert!(uploader.open);
        assert!(matches!(uploader.doing, Doing::Said(Err(_))));
    }

    fn working() -> Uploader {
        Uploader {
            doing: Doing::Working(Box::new(Work {
                link: Box::new(Fake::default()),
                push: Push::new(2, 1, 0, vec![1; 10]),
            })),
            ..Uploader::default()
        }
    }

    #[test]
    fn reopening_a_running_upload_does_not_replace_it_or_its_document_kind() {
        let mut uploader = working();
        uploader.open(true);
        assert!(uploader.open);
        assert!(!uploader.sending_rig());
        assert!(matches!(uploader.doing, Doing::Working(_)));
    }

    #[test]
    fn closing_a_transfer_keeps_its_connection_until_cancellation_finishes() {
        let mut uploader = working();
        uploader.cancel();
        assert!(uploader.busy());
        assert!(matches!(uploader.doing, Doing::Cancelling(_)));
    }

    #[test]
    fn the_completion_close_button_really_hides_the_window() {
        fn frame(
            uploader: &mut Uploader,
            ctx: &egui::Context,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 700.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| uploader.show(ctx, false, |_| None),
            )
        }
        let ctx = egui::Context::default();
        let mut uploader = Uploader {
            open: true,
            doing: Doing::Said(Ok("Upload complete.".into())),
            ..Uploader::default()
        };
        let _ = frame(&mut uploader, &ctx, vec![]);
        let output = frame(&mut uploader, &ctx, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Close" => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .expect("Close button");
        for pressed in [true, false] {
            let _ = frame(
                &mut uploader,
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(!uploader.open);
    }
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod destination_tests;
