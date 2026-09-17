// SPDX-License-Identifier: GPL-3.0-only
//! The window that sends what is open to the car.
//!
//! Deliberately small. Choosing a link and a display is the whole of the
//! decision; everything else is the car's answer, put in front of the person
//! who asked rather than into a log they will not read.

use super::watch::{self, Reading, Watch};
use super::{bluetooth::Bluetooth, wifi::Wifi, Error, Push, State, Transport};

/// Why a shared network may not answer, said where somebody choosing it reads
/// it rather than in a document nobody opens.
const LAN_NOTE: &str =
    "The master joins a network it has been told about. Send it the name and \
     password over Bluetooth first.";

/// How the link to the car stands.
pub enum LinkState {
    /// Nothing open.
    Off,
    /// A link is open; `heard` is how long ago the car last answered, or
    /// `None` if it has not answered yet.
    Open {
        /// What the link is, as the transport describes itself.
        what: String,
        /// Since the last word from the car.
        heard: Option<std::time::Duration>,
    },
}

/// Which way the editor is talking to the car.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Link {
    /// Over the radio, which needs nothing arranged beforehand.
    #[default]
    Bluetooth,
    /// Over the car's own access point, which the laptop must have joined.
    Wifi,
    /// Over a network both are already on, for a car wired or joined to one.
    ///
    /// The same socket as [`Link::Wifi`]: what differs is whose network it is,
    /// and that the master has to have been told how to join it.
    Lan,
}

/// What the window is doing.
enum Doing {
    /// Waiting to be told what to send and where.
    Asking,
    /// Connecting, or pushing.
    ///
    /// Boxed together: a push carries the image, so this variant is a
    /// kilobyte larger than the others and every `Doing` would otherwise be
    /// sized for it.
    Working(Box<(Box<dyn Transport>, Push)>),
    /// Finished, with something to say.
    Said(Result<String, Error>),
}

/// The window that watches the car.
///
/// Kept apart from the upload window because the two are different jobs: one
/// is a thing somebody does and finishes, the other is a thing left open
/// while working. They share the transports and nothing else.
#[derive(Default)]
pub struct Monitor {
    open: bool,
    link: Link,
    addr: String,
    /// When the drive was last sent, so it goes at a sane rate rather than
    /// once a frame.
    last_feed: Option<std::time::Instant>,
    /// The network to hand the master, and what came of sending it.
    ssid: String,
    password: String,
    stored: Option<Result<String, Error>>,
    /// The link, once opened, and what it has heard.
    watching: Option<(Box<dyn Transport>, Watch)>,
    /// Why the last attempt failed, if it did.
    trouble: Option<Error>,
    /// Whether to list every gauge or only the ones the car is holding.
    ///
    /// Every gauge by default, and that is the useful default: a list that
    /// only shows what is arriving cannot show what has stopped.
    all_gauges: bool,
}

impl Monitor {
    /// Open or close the window.
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if !self.open {
            self.watching = None;
        }
        if self.addr.is_empty() {
            self.addr = super::wifi::DEFAULT_ADDR.to_string();
        }
    }

    /// Draw it, and keep the reading current.
    /// How the link to the car stands, for anything that wants to show it.
    ///
    /// "Open" is not the same as "answering": a socket stays up and a radio
    /// stays paired long after the board behind them has stopped talking, so
    /// what is reported is how long ago the car last said anything.
    pub fn link_state(&self) -> LinkState {
        match &self.watching {
            None => LinkState::Off,
            Some((link, watch)) => LinkState::Open {
                what: link.describe(),
                heard: watch.latest().map(|(_, age)| age),
            },
        }
    }

    /// Send one round of readings down the link this window already has open.
    ///
    /// Reuses the watch connection rather than opening a second one: the car
    /// takes one at a time, and a feed that opened its own would be refused
    /// by a board that is perfectly well.
    pub fn feed(&mut self, car: &copilot::cogware_can::drive::Car) {
        // Twenty-five a second: about what a car's own bus gives a dash, and
        // a hundred and eighty bytes a second, which no link here notices.
        // Ten was tried first and read as laggy -- a needle stepping rather
        // than moving -- and sixty only fills the channel the reports come
        // back on. A set that will not fit is dropped rather than queued: the
        // next one is forty milliseconds away and is newer.
        const EVERY: std::time::Duration = std::time::Duration::from_millis(40);
        if self.last_feed.is_some_and(|at| at.elapsed() < EVERY) {
            return;
        }
        let Some((link, _)) = self.watching.as_mut() else {
            return;
        };
        self.last_feed = Some(std::time::Instant::now());
        if let Err(e) = super::feed_gauges(link.as_mut(), car) {
            self.trouble = Some(e);
            self.watching = None;
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("The car")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| self.body(ui));
        if self.open && !open {
            self.watching = None;
        }
        self.open = open;
        if self.watching.is_some() {
            // A reading a second is no use if the frame only comes when the
            // mouse moves.
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        let Some((link, watch)) = self.watching.as_mut() else {
            self.offer(ui);
            return;
        };
        let described = link.describe();
        if let Err(e) = watch.step(link.as_mut()) {
            self.trouble = Some(e);
            self.watching = None;
            return;
        }
        ui.label(format!("Watching {described}"));
        ui.separator();
        match watch.latest() {
            Some((status, age)) => {
                egui::Grid::new("car").num_columns(2).show(ui, |ui| {
                    let mut row = |name: &str, value: String| {
                        ui.label(name);
                        ui.label(value);
                        ui.end_row();
                    };
                    row("running for", watch::uptime(status.uptime_ms));
                    row("bus", format!("{} frames a second", status.can_tx_rate));
                    row("sent on the bus", status.can_tx.to_string());
                    row("heard on the bus", status.can_rx.to_string());
                    row("gauges asked for", status.subscribers.to_string());
                    row("frames from a host", status.host_frames.to_string());
                    // Shown always, not only when it climbs: a zero here is
                    // worth seeing, because it is what says the link is clean.
                    row("checksums thrown away", status.crc_errors.to_string());
                });
                ui.separator();
                ui.horizontal(|ui| {
                    lamp(ui, "CAN", status.can_up());
                    lamp(ui, "panel", status.panel_up());
                    lamp(ui, "host", status.host_up());
                    if status.loopback() {
                        // Worth saying loudly: in loopback the car answers
                        // itself and nothing reaches a display, which looks
                        // exactly like everything working.
                        ui.colored_label(WARN, "loopback -- nothing reaches a display");
                    }
                });
                if watch.silent() {
                    ui.colored_label(WARN, format!("nothing for {}s", age.as_secs()));
                }
                ui.separator();
                gauges(ui, watch, &mut self.all_gauges);
            }
            None => {
                ui.spinner();
                ui.label("waiting for the car to say something");
            }
        }
        ui.separator();
        if ui.button("Stop watching").clicked() {
            self.watching = None;
        }
    }

    /// Where the master is told which network to join.
    ///
    /// Only over Bluetooth, and only there: these are the credentials for the
    /// network, so a link that already needs it cannot carry them.
    fn network_box(&mut self, ui: &mut egui::Ui) {
        ui.collapsing("Tell the master its network", |ui| {
            if self.link != Link::Bluetooth {
                ui.small("Choose Bluetooth first: a network cannot carry its own password.");
                return;
            }
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut self.ssid);
            });
            ui.horizontal(|ui| {
                ui.label("Password");
                ui.add(egui::TextEdit::singleline(&mut self.password).password(true));
            });
            if ui.button("Send and store").clicked() {
                self.stored = Some(
                    Bluetooth::connect()
                        .and_then(|mut link| {
                            super::set_network(&mut link, &self.ssid, &self.password)
                        })
                        .map(|()| format!("{} stored; restart the master to join it", self.ssid)),
                );
            }
            match &self.stored {
                Some(Ok(said)) => ui.small(said.as_str()),
                Some(Err(e)) => ui.colored_label(WARN, e.to_string()),
                None => ui.small("The master keeps this across a power cycle."),
            };
        });
    }

    fn offer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Link");
            ui.selectable_value(&mut self.link, Link::Bluetooth, "Bluetooth");
            ui.selectable_value(&mut self.link, Link::Wifi, "The car's network");
            ui.selectable_value(&mut self.link, Link::Lan, "A network you both share");
        });
        if self.link != Link::Bluetooth {
            ui.horizontal(|ui| {
                ui.label("Address");
                ui.text_edit_singleline(&mut self.addr);
            });
        }
        if self.link == Link::Lan {
            ui.small(LAN_NOTE);
        }
        self.network_box(ui);
        if let Some(e) = &self.trouble {
            ui.colored_label(WARN, e.to_string());
        }
        if ui.button("Watch").clicked() {
            self.trouble = None;
            let opened: Result<Box<dyn Transport>, Error> = match self.link {
                Link::Bluetooth => Bluetooth::connect().map(|b| Box::new(b) as Box<dyn Transport>),
                // One socket for both: the car's own access point and a network
                // you share differ in whose network it is, not in what this
                // end does with it.
                Link::Wifi | Link::Lan => {
                    Wifi::connect(&self.addr).map(|w| Box::new(w) as Box<dyn Transport>)
                }
            };
            match opened {
                Ok(link) => self.watching = Some((link, Watch::new())),
                Err(e) => self.trouble = Some(e),
            }
        }
    }
}

/// Draw what the car is reading, gauge by gauge.
///
/// Every gauge this build knows, in the table's own order, whether the car is
/// holding a value for it or not. That is the point: a list of only what is
/// arriving cannot show what has stopped arriving, and a sender that has died
/// is exactly what somebody opens this to find.
///
/// The raw count first, because it is what the car actually has. The value
/// beside it is the same number divided by the gauge's scale -- useful, but
/// an interpretation, and a sensor reading nonsense reads as nonsense in the
/// first column and as something plausible in the second.
///
/// A channel nothing is filling is drawn as the sentinel the car actually
/// sent, `CAFEBABE`, and said in words beside it. Spelled out rather than
/// blanked, because the number is what somebody sees in a log or a packet
/// dump and it should mean the same thing everywhere it appears.
fn gauges(ui: &mut egui::Ui, watch: &Watch, all: &mut bool) {
    let held = watch.live_gauges();
    ui.horizontal(|ui| {
        ui.label(format!("{held} gauges reading"));
        ui.checkbox(all, "show every gauge");
    });
    if !watch.has_readings() {
        ui.weak("the car has not sent any readings yet");
        return;
    }
    egui::ScrollArea::vertical()
        .max_height(280.0)
        .show(ui, |ui| {
            egui::Grid::new("gauges").num_columns(3).striped(true).show(ui, |ui| {
                for gauge in cogware_can::ALL_GAUGES {
                    let reading = watch.reading(gauge.id);
                    // Only rows with a value, unless asked for the lot.
                    if !*all && !matches!(reading, Reading::Value(_)) {
                        continue;
                    }
                    ui.label(gauge.name);
                    match reading {
                        Reading::Value(raw) => {
                            ui.monospace(raw.to_string());
                            let scale = gauge.unit.scale as f32;
                            let value = raw as f32 / scale;
                            ui.weak(format!("{value:.2} {}", gauge.unit.symbol));
                        }
                        Reading::NoData => {
                            // The sentinel spelled out, so the number in a
                            // packet dump and the words on the screen are
                            // plainly the same thing.
                            ui.monospace(
                                egui::RichText::new("CAFEBABE").color(egui::Color32::GRAY),
                            );
                            ui.weak("no sensor data");
                        }
                    }
                    ui.end_row();
                }
            });
        });
}

/// The colour a warning is expected in, matching the displays'.
const WARN: egui::Color32 = egui::Color32::from_rgb(255, 176, 64);

/// A lamp that is lit or not, with the name beside it either way.
///
/// Both states drawn, rather than only the lit one: a row that shows nothing
/// when a thing is down reads as a row that has not loaded.
fn lamp(ui: &mut egui::Ui, name: &str, on: bool) {
    let colour = if on {
        egui::Color32::from_rgb(63, 185, 80)
    } else {
        egui::Color32::from_rgb(110, 118, 129)
    };
    ui.colored_label(colour, if on { "●" } else { "○" });
    ui.label(name);
}

/// The upload window.
pub struct Uploader {
    open: bool,
    /// When this window was built, as the origin for the clock a push needs.
    started: std::time::Instant,
    link: Link,
    /// Address of the display to send to, as the rig numbers them.
    node: u8,
    /// Which slot of the kind: a scene's mode index, or zero for a rig.
    slot: u8,
    /// Whether what is open is a rig rather than a scene.
    rig: bool,
    doing: Doing,
    /// The address to use when the car's network is chosen.
    addr: String,
}

impl Default for Uploader {
    fn default() -> Self {
        Self {
            open: false,
            started: std::time::Instant::now(),
            link: Link::Bluetooth,
            node: 0x02,
            slot: 0,
            rig: false,
            doing: Doing::Asking,
            addr: super::wifi::DEFAULT_ADDR.to_string(),
        }
    }
}

impl Uploader {
    /// Open the window for whatever is currently being edited.
    pub fn open(&mut self, rig: bool) {
        self.open = true;
        self.rig = rig;
        self.doing = Doing::Asking;
    }

    /// Whether the next push goes as a rig rather than a scene.
    ///
    /// The caller needs it to hand over the right document: the two are
    /// different files, and sending one under the other's kind writes it over
    /// the wrong one on the card.
    pub fn sending_rig(&self) -> bool {
        self.rig
    }

    /// Draw it, and carry a push along if one is running.
    ///
    /// `bytes` is what would be sent, fetched only when somebody asks to
    /// send: a scene is serialised on demand rather than kept in step with
    /// the canvas, which would cost a redraw of the document every frame.
    pub fn show(&mut self, ctx: &egui::Context, bytes: impl FnOnce() -> Option<Vec<u8>>) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("Send to the car")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| self.body(ui, bytes));
        if self.open && !open {
            // Closing the window mid-push must tell the car, or it sits
            // holding the transfer open and refuses every later one until it
            // times out -- with the bus locked and every screen showing the
            // update notice the whole time.
            if let Doing::Working(work) = &mut self.doing {
                let (link, push) = work.as_mut();
                let _ = push.cancel(link.as_mut());
            }
            self.doing = Doing::Asking;
        }
        self.open = open;
        // A push in flight needs the frame to keep coming even when nothing
        // is being touched, or it advances only when the mouse moves.
        if matches!(self.doing, Doing::Working(..)) {
            ctx.request_repaint();
        }
    }

    fn body(&mut self, ui: &mut egui::Ui, bytes: impl FnOnce() -> Option<Vec<u8>>) {
        match &mut self.doing {
            Doing::Asking => {
                self.ask(ui, bytes);
            }
            Doing::Working(work) => {
                let (link, push) = work.as_mut();
                let described = link.describe();
                // The editor's own clock. A push is paced by the car, and
                // the retry of its opening frame by wall time, so a redraw
                // rate must not be what decides either.
                let now_ms = self.started.elapsed().as_millis() as u64;
                let step = push.step(link.as_mut(), now_ms);
                let percent = push.percent();
                ui.label(format!("Sending over {described}"));
                ui.add(egui::ProgressBar::new(f32::from(percent) / 100.0).show_percentage());
                // What the bus has confirmed, not what the editor has
                // written: the two differ by most of a transfer.
                ui.small("the bar follows what the car has passed to the display");
                match step {
                    Ok(State::Done) => {
                        self.doing = Doing::Said(Ok("The display took it.".into()));
                    }
                    Ok(_) => {}
                    Err(e) => self.doing = Doing::Said(Err(e)),
                }
            }
            Doing::Said(what) => {
                match what {
                    Ok(good) => {
                        ui.label(good.clone());
                    }
                    Err(e) => {
                        ui.colored_label(egui::Color32::from_rgb(255, 176, 64), e.to_string());
                    }
                }
                if ui.button("Again").clicked() {
                    self.doing = Doing::Asking;
                }
            }
        }
    }

    fn ask(&mut self, ui: &mut egui::Ui, bytes: impl FnOnce() -> Option<Vec<u8>>) {
        ui.horizontal(|ui| {
            ui.label("Link");
            ui.selectable_value(&mut self.link, Link::Bluetooth, "Bluetooth");
            ui.selectable_value(&mut self.link, Link::Wifi, "The car's network");
            ui.selectable_value(&mut self.link, Link::Lan, "A network you both share");
        });
        if self.link != Link::Bluetooth {
            ui.horizontal(|ui| {
                ui.label("Address");
                ui.text_edit_singleline(&mut self.addr);
            });
            if self.link == Link::Lan {
                ui.small(LAN_NOTE);
            } else {
                ui.small("Join the car's access point first; it is not on your own network.");
            }
        } else {
            ui.small("The car has to be advertising. It is, unless a display has turned it off.");
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Display");
            ui.add(egui::DragValue::new(&mut self.node).range(1..=0x0F).hexadecimal(2, false, true));
        });
        if !self.rig {
            ui.horizontal(|ui| {
                ui.label("Mode");
                ui.add(egui::DragValue::new(&mut self.slot).range(0..=7));
            });
            ui.small("The mode's place in the rig, which is the slot the scene lands in.");
        }
        ui.separator();
        if ui.button("Send").clicked() {
            let Some(image) = bytes() else {
                self.doing = Doing::Said(Err(Error::Link("there is nothing open to send".into())));
                return;
            };
            let opened: Result<Box<dyn Transport>, Error> = match self.link {
                Link::Bluetooth => Bluetooth::connect().map(|b| Box::new(b) as Box<dyn Transport>),
                // One socket for both: the car's own access point and a network
                // you share differ in whose network it is, not in what this
                // end does with it.
                Link::Wifi | Link::Lan => {
                    Wifi::connect(&self.addr).map(|w| Box::new(w) as Box<dyn Transport>)
                }
            };
            match opened {
                Ok(link) => {
                    let kind = if self.rig {
                        cogware_can::xfer::kind::RIG
                    } else {
                        cogware_can::xfer::kind::SCENE
                    };
                    let slot = if self.rig { 0 } else { self.slot };
                    let push = Push::new(self.node, kind, slot, image);
                    self.doing = Doing::Working(Box::new((link, push)));
                }
                Err(e) => self.doing = Doing::Said(Err(e)),
            }
        }
    }
}
