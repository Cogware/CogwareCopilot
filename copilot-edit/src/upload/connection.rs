// SPDX-License-Identifier: GPL-3.0-only
//! Shared connection choices and Bluetooth discovery for the editor's dialogs.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use super::bluetooth::{self, Bluetooth, Device};
use super::job::Job;
use super::{Error, Transport, wifi::Wifi};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Link {
    #[default]
    Bluetooth,
    Wifi,
    Lan,
}

enum Target {
    Bluetooth(Device),
    Network(String),
}

impl Target {
    fn description(&self) -> String {
        match self {
            Self::Bluetooth(device) => format!("{} ({})", device.name, device.address),
            Self::Network(addr) => addr.clone(),
        }
    }

    fn connect(self, stop: Arc<AtomicBool>) -> Result<Box<dyn Transport>, Error> {
        match self {
            Self::Bluetooth(device) => {
                Bluetooth::connect_to(device, stop).map(|link| Box::new(link) as Box<dyn Transport>)
            }
            Self::Network(addr) => Wifi::connect_cancellable(&addr, &stop)
                .map(|link| Box::new(link) as Box<dyn Transport>),
        }
    }
}

pub(super) struct Connecting {
    pub label: String,
    pub started: Instant,
    pub job: Job<Box<dyn Transport>>,
}

pub(super) struct Connection {
    link: Link,
    addr: String,
    devices: Vec<Device>,
    selected: Option<String>,
    scan: Option<Job<Vec<Device>>>,
    searched: bool,
    note: String,
    trouble: Option<Error>,
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            link: Link::Bluetooth,
            addr: super::wifi::DEFAULT_ADDR.into(),
            devices: Vec::new(),
            selected: None,
            scan: None,
            searched: false,
            note: String::new(),
            trouble: None,
        }
    }
}

impl Connection {
    pub fn bluetooth(&self) -> bool {
        self.link == Link::Bluetooth
    }

    pub fn busy(&self) -> bool {
        self.scan.is_some()
    }

    pub fn cancel(&mut self) {
        if self.scan.take().is_some() {
            self.note = "Device search cancelled.".into();
        }
    }

    pub fn poll(&mut self) {
        let Some(result) = self.scan.as_ref().and_then(Job::poll) else {
            return;
        };
        self.scan = None;
        self.searched = true;
        match result {
            Ok(devices) => self.found(devices),
            Err(error) => self.trouble = Some(error),
        }
    }

    fn found(&mut self, devices: Vec<Device>) {
        if !devices
            .iter()
            .any(|device| Some(&device.id) == self.selected.as_ref())
        {
            self.selected = None;
        }
        self.devices = devices;
        self.note = if self.devices.is_empty() {
            "No Bluetooth devices found. Check that Bluetooth is on and the controller is nearby and advertising, then search again.".into()
        } else {
            format!(
                "{} device(s) found. Choose your Cogware controller below.",
                self.devices.len()
            )
        };
    }

    fn target(&self) -> Result<Target, Error> {
        if self.bluetooth() {
            return self
                .devices
                .iter()
                .find(|device| Some(&device.id) == self.selected.as_ref())
                .cloned()
                .map(Target::Bluetooth)
                .ok_or_else(|| Error::NotFound("Choose a Bluetooth device first.".into()));
        }
        let addr = self.addr.trim().parse::<SocketAddr>().map_err(|_| {
            Error::NotFound("Enter an IP address and port, for example 192.168.4.1:3333.".into())
        })?;
        if addr.port() == 0 || addr.ip().is_unspecified() {
            return Err(Error::NotFound(
                "Use the controller's IP address and a non-zero port.".into(),
            ));
        }
        Ok(Target::Network(addr.to_string()))
    }

    pub fn ready(&self) -> bool {
        !self.busy() && self.target().is_ok()
    }

    pub fn connect(&self) -> Result<Connecting, Error> {
        let target = self.target()?;
        Ok(Connecting {
            label: target.description(),
            started: Instant::now(),
            job: Job::start("controller connection", move |stop| target.connect(stop))?,
        })
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.poll();
        let before = self.link;
        ui.horizontal_wrapped(|ui| {
            ui.label("Connection type");
            ui.selectable_value(&mut self.link, Link::Bluetooth, "Bluetooth");
            ui.selectable_value(&mut self.link, Link::Wifi, "Controller Wi-Fi");
            ui.selectable_value(&mut self.link, Link::Lan, "Local network");
        });
        if self.link != before {
            self.cancel();
            self.trouble = None;
            self.note.clear();
        }
        ui.add_space(6.0);
        if self.bluetooth() {
            self.devices_ui(ui);
        } else {
            ui.horizontal(|ui| {
                ui.label("IP address : port");
                ui.add(
                    egui::TextEdit::singleline(&mut self.addr)
                        .hint_text("192.168.4.1:3333")
                        .desired_width(200.0),
                );
            });
            ui.small(if self.link == Link::Wifi {
                "Join the controller's Wi-Fi network in your computer's network settings first."
            } else {
                "Your computer and controller must be on the same network. Use the controller's address on that network."
            });
            if self.link == Link::Lan {
                ui.small("To configure the controller's Wi-Fi, connect over Bluetooth and open Configure controller Wi-Fi.");
            }
            if let Err(error) = self.target() {
                ui.colored_label(super::ui::WARN, error.to_string());
            }
        }
        if let Some(error) = &self.trouble {
            ui.colored_label(super::ui::WARN, error.to_string());
        }
    }

    fn devices_ui(&mut self, ui: &mut egui::Ui) {
        ui.small("Search nearby Bluetooth devices, then choose your Cogware controller. No device is connected automatically.");
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.busy(),
                    egui::Button::new(if self.searched {
                        "Refresh devices"
                    } else {
                        "Find devices"
                    }),
                )
                .clicked()
            {
                self.trouble = None;
                self.note.clear();
                match Job::start("Bluetooth device search", |stop| bluetooth::scan(&stop)) {
                    Ok(scan) => self.scan = Some(scan),
                    Err(error) => self.trouble = Some(error),
                }
            }
            if self.busy() {
                ui.spinner();
                ui.label("Searching...");
                if ui.button("Cancel search").clicked() {
                    self.cancel();
                }
                ui.ctx().request_repaint_after(Duration::from_millis(50));
            }
        });
        if !self.note.is_empty() {
            ui.small(&self.note);
        }
        let selected = self
            .devices
            .iter()
            .find(|device| Some(&device.id) == self.selected.as_ref())
            .map(|device| format!("{} ({})", device.name, device.address))
            .unwrap_or_else(|| "Select a Bluetooth device...".into());
        ui.add_enabled_ui(!self.busy() && !self.devices.is_empty(), |ui| {
            egui::ComboBox::from_id_salt("bluetooth_device")
                .width(340.0)
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for device in &self.devices {
                        ui.selectable_value(
                            &mut self.selected,
                            Some(device.id.clone()),
                            format!("{} ({})", device.name, device.address),
                        );
                    }
                });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str) -> Device {
        Device {
            id: id.into(),
            name: "Cogware".into(),
            address: id.into(),
        }
    }

    #[test]
    fn duplicate_names_are_selected_by_identity_and_never_automatically() {
        let mut connection = Connection::default();
        connection.found(vec![device("first"), device("second")]);
        assert!(!connection.ready());
        connection.selected = Some("second".into());
        let Target::Bluetooth(chosen) = connection.target().expect("selected") else {
            panic!("Bluetooth selection");
        };
        assert_eq!(chosen.id, "second");
    }

    #[test]
    fn refresh_keeps_only_a_selection_that_still_exists() {
        let mut connection = Connection {
            selected: Some("second".into()),
            ..Connection::default()
        };
        connection.found(vec![device("second"), device("first")]);
        assert_eq!(connection.selected.as_deref(), Some("second"));
        connection.found(vec![device("first")]);
        assert!(connection.selected.is_none());
        assert!(!connection.ready());
    }

    #[test]
    fn invalid_network_endpoints_cannot_start_a_connection() {
        let mut connection = Connection {
            link: Link::Lan,
            ..Connection::default()
        };
        for addr in [
            "",
            "controller",
            "192.168.4.1",
            "192.168.4.1:0",
            "0.0.0.0:3333",
        ] {
            connection.addr = addr.into();
            assert!(!connection.ready(), "{addr}");
        }
        connection.addr = " 192.168.4.1:3333 ".into();
        assert!(connection.ready());
    }

    #[test]
    fn the_device_popup_selects_a_specific_controller() {
        fn frame(
            connection: &mut Connection,
            ctx: &egui::Context,
            events: Vec<egui::Event>,
        ) -> egui::FullOutput {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700.0, 700.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| connection.show(ui));
                },
            )
        }
        fn label(output: &egui::FullOutput, wanted: &str) -> egui::Pos2 {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == wanted => {
                        Some(text.pos + text.galley.size() * 0.5)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("visible control: {wanted}"))
        }
        fn click(connection: &mut Connection, ctx: &egui::Context, pos: egui::Pos2) {
            for pressed in [true, false] {
                let _ = frame(
                    connection,
                    ctx,
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
        }
        let ctx = egui::Context::default();
        let mut connection = Connection::default();
        connection.found(vec![device("first"), device("second")]);
        let output = frame(&mut connection, &ctx, vec![]);
        click(
            &mut connection,
            &ctx,
            label(&output, "Select a Bluetooth device..."),
        );
        let output = frame(&mut connection, &ctx, vec![]);
        click(&mut connection, &ctx, label(&output, "Cogware (second)"));
        assert_eq!(connection.selected.as_deref(), Some("second"));
        assert!(connection.ready());
        assert!(!connection.busy());
    }
}
