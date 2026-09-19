// SPDX-License-Identifier: GPL-3.0-only
//! Connection controls and live readings, independent of the window's visibility.

use std::time::{Duration, Instant};

use super::connection::{Connecting, Connection};
use super::ui::WARN;
use super::watch::Watch;
use super::{Error, Transport, diagnostics};

/// Connection progress and freshness for the editor's status bar.
pub enum LinkState {
    /// No connection is open.
    Off,
    /// Discovery or connection is running in the background.
    Connecting {
        /// The operation currently in progress.
        what: String,
    },
    /// A transport is open, but the controller may not yet have answered.
    Open {
        /// The selected device or network address.
        what: String,
        /// Time since the latest controller status, if one has arrived.
        heard: Option<Duration>,
    },
}

/// The connection dialog and its live controller session.
#[derive(Default)]
pub struct Monitor {
    open: bool,
    connection: Connection,
    connecting: Option<Connecting>,
    watching: Option<(Box<dyn Transport>, Watch)>,
    connected_at: Option<Instant>,
    last_feed: Option<Instant>,
    trouble: Option<Error>,
    notice: String,
    all_gauges: bool,
    ssid: String,
    password: String,
    stored: Option<Result<String, Error>>,
}

impl Monitor {
    /// Show the connection controls without interrupting an existing session.
    pub fn open(&mut self) {
        self.open = true;
    }

    /// Whether another dialog must wait before using the controller.
    pub fn busy(&self) -> bool {
        self.watching.is_some() || self.connecting.is_some() || self.connection.busy()
    }

    /// The connection's progress or the age of its latest status report.
    pub fn link_state(&self) -> LinkState {
        if let Some(pending) = &self.connecting {
            return LinkState::Connecting {
                what: format!("Connecting to {}...", pending.label),
            };
        }
        if self.connection.busy() {
            return LinkState::Connecting {
                what: "Searching for Bluetooth devices...".into(),
            };
        }
        match &self.watching {
            Some((link, watch)) => LinkState::Open {
                what: link.describe(),
                heard: watch.latest().map(|(_, age)| age),
            },
            None => LinkState::Off,
        }
    }

    /// Send preview readings through the existing connection, at most 25 times a second.
    pub fn feed(&mut self, readings: &[(u16, i32)]) {
        if self
            .last_feed
            .is_some_and(|at| at.elapsed() < Duration::from_millis(40))
        {
            return;
        }
        let Some((link, _)) = self.watching.as_mut() else {
            return;
        };
        self.last_feed = Some(Instant::now());
        if let Err(error) = super::feed_readings(link.as_mut(), readings) {
            self.trouble = Some(error);
            self.watching = None;
            self.open = true;
        }
    }

    fn advance(&mut self) {
        self.connection.poll();
        if let Some(result) = self
            .connecting
            .as_ref()
            .and_then(|pending| pending.job.poll())
        {
            self.connecting = None;
            match result {
                Ok(link) => {
                    self.watching = Some((link, Watch::new()));
                    self.connected_at = Some(Instant::now());
                    self.last_feed = None;
                    self.notice.clear();
                }
                Err(error) => self.trouble = Some(error),
            }
        }
        if let Some((link, watch)) = self.watching.as_mut()
            && let Err(error) = watch.step(link.as_mut())
        {
            self.watching = None;
            self.trouble = Some(error);
            self.open = true;
        }
    }

    fn hide(&mut self) {
        self.open = false;
        self.connecting = None;
        self.connection.cancel();
    }

    /// Draw the dialog and service its connection even when the window is hidden.
    pub fn show(&mut self, ctx: &egui::Context, upload_busy: bool) {
        self.advance();
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(if self.watching.is_some() {
                100
            } else {
                50
            }));
        }
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("Connections")
            .open(&mut open)
            .default_width(520.0)
            .resizable(true)
            .vscroll(true)
            .show(ctx, |ui| self.body(ui, upload_busy));
        if !open {
            self.hide();
        }
    }

    fn body(&mut self, ui: &mut egui::Ui, upload_busy: bool) {
        if let Some(error) = &self.trouble {
            ui.colored_label(WARN, format!("Connection problem: {error}"));
            ui.separator();
        }
        if !self.notice.is_empty() {
            ui.label(&self.notice);
        }
        if let Some(pending) = &self.connecting {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("Connecting to {}...", pending.label));
            });
            ui.small(format!(
                "Elapsed: {} s. You can keep editing while this connects.",
                pending.started.elapsed().as_secs()
            ));
            if ui.button("Cancel connection").clicked() {
                self.connecting = None;
                self.notice = "Connection cancelled.".into();
            }
            return;
        }
        if let Some((link, watch)) = &self.watching {
            ui.heading(format!("Connected to {}", link.describe()));
            if watch.latest().is_none() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Waiting for controller status...");
                });
                if self
                    .connected_at
                    .is_some_and(|at| at.elapsed() > Duration::from_secs(5))
                {
                    ui.colored_label(WARN, "The connection is open, but the controller has not replied. Check its power and firmware, or disconnect and try again.");
                }
            }
            diagnostics::show(ui, watch, &mut self.all_gauges);
            self.network_box(ui);
            ui.separator();
            if ui.button("Disconnect").clicked() {
                self.watching = None;
                self.connected_at = None;
                self.last_feed = None;
                self.password.clear();
                self.stored = None;
                self.notice = "Disconnected.".into();
            }
            ui.small("Closing this window keeps the connection open. Use Disconnect to stop.");
            return;
        }
        ui.label("Connect to view live readings or send preview values to your displays.");
        if upload_busy {
            ui.colored_label(
                WARN,
                "An upload is using the controller. Finish or cancel it before connecting here.",
            );
        }
        ui.add_enabled_ui(!upload_busy, |ui| {
            self.connection.show(ui);
            ui.separator();
            if ui
                .add_enabled(self.connection.ready(), egui::Button::new("Connect"))
                .clicked()
            {
                self.trouble = None;
                self.notice.clear();
                self.stored = None;
                match self.connection.connect() {
                    Ok(pending) => self.connecting = Some(pending),
                    Err(error) => self.trouble = Some(error),
                }
            }
        });
    }

    fn network_box(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.collapsing("Configure controller Wi-Fi", |ui| {
            if !self.connection.bluetooth() {
                ui.small("Connect over Bluetooth to send Wi-Fi settings to the controller.");
                return;
            }
            ui.small("Send the network the controller should join after its next restart.");
            ui.horizontal(|ui| {
                ui.label("Network name (SSID)");
                ui.text_edit_singleline(&mut self.ssid);
            });
            ui.horizontal(|ui| {
                ui.label("Password");
                ui.add(egui::TextEdit::singleline(&mut self.password).password(true));
            });
            ui.small("Leave the password empty only for an open network.");
            if ui.add_enabled(!self.ssid.is_empty(), egui::Button::new("Send Wi-Fi settings")).clicked()
                && let Some((link, _)) = self.watching.as_mut()
            {
                self.stored = Some(
                    super::set_network(link.as_mut(), &self.ssid, &self.password).map(|()| {
                        self.password.clear();
                        "Wi-Fi settings queued for sending. Keep the connection open briefly, then restart the controller to join the network. This protocol does not confirm that settings were saved.".into()
                    }),
                );
            }
            match &self.stored {
                Some(Ok(message)) => { ui.label(message); }
                Some(Err(error)) => { ui.colored_label(WARN, error.to_string()); }
                None => {}
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests_support::Fake;
    use super::*;

    #[test]
    fn opening_and_hiding_controls_preserves_the_live_connection() {
        let mut monitor = Monitor {
            watching: Some((Box::new(Fake::default()), Watch::new())),
            ..Monitor::default()
        };
        monitor.open();
        monitor.open();
        assert!(monitor.open);
        monitor.hide();
        assert!(!monitor.open);
        assert!(monitor.busy());
        assert!(matches!(monitor.link_state(), LinkState::Open { .. }));
    }

    #[test]
    fn a_hidden_monitor_still_services_its_transport() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        struct Recording(Arc<AtomicBool>);
        impl Transport for Recording {
            fn send(&mut self, _: &[u8]) -> Result<bool, Error> {
                Ok(true)
            }
            fn poll(&mut self, _: &mut Vec<u8>) -> Result<(), Error> {
                self.0.store(true, Ordering::SeqCst);
                Ok(())
            }
            fn describe(&self) -> String {
                "test controller".into()
            }
        }
        let polled = Arc::new(AtomicBool::new(false));
        let mut monitor = Monitor {
            watching: Some((Box::new(Recording(Arc::clone(&polled))), Watch::new())),
            ..Monitor::default()
        };
        monitor.advance();
        assert!(polled.load(Ordering::SeqCst));
        assert!(monitor.watching.is_some());
        assert!(!monitor.open);
    }

    #[test]
    fn a_hidden_connection_failure_is_shown_without_waiting_for_a_click() {
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
        let mut monitor = Monitor {
            watching: Some((Box::new(Failed), Watch::new())),
            ..Monitor::default()
        };
        monitor.advance();
        assert!(monitor.open);
        assert!(monitor.trouble.is_some());
        assert!(!monitor.busy());
    }

    #[test]
    fn connection_progress_renders_before_the_background_connection_finishes() {
        let (release, wait) = std::sync::mpsc::channel();
        let job = super::super::job::Job::<Box<dyn Transport>>::start(
            "connection-render-test",
            move |_| {
                wait.recv_timeout(Duration::from_secs(3))
                    .expect("released after drawing");
                Ok(Box::new(Fake::default()))
            },
        )
        .expect("worker");
        let mut monitor = Monitor {
            open: true,
            connecting: Some(Connecting {
                label: "Test controller".into(),
                started: Instant::now(),
                job,
            }),
            ..Monitor::default()
        };
        let ctx = egui::Context::default();
        let draw = |monitor: &mut Monitor| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 700.0),
                    )),
                    ..Default::default()
                },
                |ctx| monitor.show(ctx, false),
            )
        };
        let _ = draw(&mut monitor);
        let output = draw(&mut monitor);
        assert!(
            monitor.connecting.is_some(),
            "drawing did not wait for the worker"
        );
        assert!(output.shapes.iter().any(|shape| matches!(
            &shape.shape, egui::Shape::Text(text)
                if text.galley.job.text == "Connecting to Test controller..."
        )));
        drop(monitor);
        release.send(()).expect("release cancelled worker");
    }
}
