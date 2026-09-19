// SPDX-License-Identifier: GPL-3.0-only
//! Bluetooth discovery and frame transport, isolated from the editor's redraw loop.
//!
//! Discovery returns durable identities rather than runtime-bound peripherals;
//! a connection owns its radio worker until cancellation or transport drop.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, TryRecvError};
use std::time::{Duration, Instant};

use btleplug::api::{Peripheral as _, WriteType};
use btleplug::platform::Peripheral;
use futures::StreamExt;

use super::{Error, Transport};
use lifecycle::{Cancellation, finish, runtime};

mod discovery;
mod lifecycle;
#[cfg(test)]
mod tests;

/// What the car calls itself in an advert.
pub const NAME: &str = "Cogware";

/// The upload characteristic, in the usual UUID byte order.
pub const VALUE_UUID: &str = "00000002-c067-4761-9e5d-436f67776172";

/// The characteristic the car reports through.
pub const REPORT_UUID: &str = "00000003-c067-4761-9e5d-436f67776172";

const QUEUE: usize = 2;
const SCAN_FOR: Duration = Duration::from_secs(8);
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const READY_TIMEOUT: Duration = Duration::from_secs(60);
const IDLE: Duration = Duration::from_millis(20);
const BETWEEN_WRITES: Duration = Duration::from_millis(3);
const WRITE_MAX: usize = 180;

/// An advertised BLE device, independent of the runtime used to discover it.
#[derive(Clone, Debug)]
pub struct Device {
    /// Opaque peripheral identity scoped to its adapter, or the Windows system service.
    pub id: String,
    /// Advertised name, or an explicit unnamed-device label.
    pub name: String,
    /// Bluetooth address for distinguishing similarly named devices in the picker.
    pub address: String,
}

/// Scan usable adapters on a worker and stop their scans before returning.
pub fn scan(cancelled: &AtomicBool) -> Result<Vec<Device>, Error> {
    let cancellation = Cancellation::new(cancelled, cancelled);
    cancellation.check()?;
    runtime()?.block_on(async {
        let found = discovery::discover(&cancellation, None).await?;
        Ok(found
            .devices
            .into_iter()
            .map(|(device, _)| device)
            .collect())
    })
}

/// A link to the car over Bluetooth.
pub struct Bluetooth {
    to_radio: SyncSender<Vec<u8>>,
    from_radio: Receiver<Result<Vec<u8>, Error>>,
    name: String,
    stopped: Arc<AtomicBool>,
}

impl Bluetooth {
    /// Find the first advertised Cogware device and connect, for blocking CLI use.
    pub fn connect() -> Result<Self, Error> {
        Self::open(None, Arc::new(AtomicBool::new(false)))
    }

    /// Re-resolve and connect only the selected identity, on a background job.
    pub fn connect_to(device: Device, cancelled: Arc<AtomicBool>) -> Result<Self, Error> {
        Self::open(Some(device), cancelled)
    }

    fn open(selected: Option<Device>, cancelled: Arc<AtomicBool>) -> Result<Self, Error> {
        let stopped = Arc::new(AtomicBool::new(false));
        let cancellation = Cancellation::new(&cancelled, &stopped);
        cancellation.check()?;
        // A push counts accepted frames as sent, so an unbounded queue would
        // let it run far ahead of the radio and starve the car of real bytes.
        let (to_radio, orders) = std::sync::mpsc::sync_channel(QUEUE);
        let (events, from_radio) = std::sync::mpsc::channel();
        let (ready, started) = std::sync::mpsc::channel();
        let worker_cancelled = Arc::clone(&cancelled);
        let worker_stopped = Arc::clone(&stopped);
        std::thread::Builder::new()
            .name("cogware-ble".into())
            .spawn(move || {
                run(
                    selected,
                    worker_cancelled,
                    worker_stopped,
                    orders,
                    events,
                    ready,
                )
            })
            .map_err(|e| Error::Link(format!("no thread for the radio: {e}")))?;

        let deadline = Instant::now() + READY_TIMEOUT;
        let result = loop {
            if let Err(error) = cancellation.check() {
                break Err(error);
            }
            match started.recv_timeout(IDLE) {
                Ok(answer) => break answer,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    break Err(Error::Link(
                        "the radio worker stopped during connection".into(),
                    ));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) if Instant::now() >= deadline => {
                    break Err(Error::Link("Bluetooth connection timed out".into()));
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
        };
        match result.and_then(|name| cancellation.check().map(|()| name)) {
            Ok(name) => Ok(Self {
                to_radio,
                from_radio,
                name,
                stopped,
            }),
            Err(error) => {
                stopped.store(true, Ordering::Relaxed);
                Err(error)
            }
        }
    }
}

impl Drop for Bluetooth {
    fn drop(&mut self) {
        // Joining here would make closing a panel wait for the Bluetooth stack.
        self.stopped.store(true, Ordering::Relaxed);
    }
}

impl Transport for Bluetooth {
    fn send(&mut self, frame: &[u8]) -> Result<bool, Error> {
        use std::sync::mpsc::TrySendError;
        match self.to_radio.try_send(frame.to_vec()) {
            Ok(()) => Ok(true),
            Err(TrySendError::Full(_)) => Ok(false),
            Err(TrySendError::Disconnected(_)) => {
                Err(Error::Link("the radio thread has gone".into()))
            }
        }
    }

    fn poll(&mut self, out: &mut Vec<u8>) -> Result<(), Error> {
        loop {
            match self.from_radio.try_recv() {
                Ok(Ok(bytes)) => out.extend_from_slice(&bytes),
                Ok(Err(e)) => return Err(e),
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => {
                    return Err(Error::Link("the radio thread has gone".into()));
                }
            }
        }
    }

    fn describe(&self) -> String {
        format!("{} over Bluetooth", self.name)
    }
}

fn run(
    selected: Option<Device>,
    cancelled: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    orders: Receiver<Vec<u8>>,
    events: Sender<Result<Vec<u8>, Error>>,
    ready: Sender<Result<String, Error>>,
) {
    let result = runtime().and_then(|runtime| {
        let cancellation = Cancellation::new(&cancelled, &stopped);
        runtime.block_on(talk(selected, &cancellation, orders, &events, &ready))
    });
    if let Err(error) = result {
        let _ = ready.send(Err(error.clone()));
        let _ = events.send(Err(error));
    }
}

async fn talk(
    selected: Option<Device>,
    cancellation: &Cancellation<'_>,
    orders: Receiver<Vec<u8>>,
    events: &Sender<Result<Vec<u8>, Error>>,
    ready: &Sender<Result<String, Error>>,
) -> Result<(), Error> {
    let mut found = discovery::discover(cancellation, selected.as_ref()).await?;
    let index = found
        .devices
        .iter()
        .position(|(device, _)| matches_selection(device, selected.as_ref()))
        .ok_or_else(|| match selected {
            Some(device) => Error::NotFound(format!(
                "{} ({}) is no longer nearby",
                device.name, device.address
            )),
            None => Error::NotFound(format!("no car called {NAME} answered in {SCAN_FOR:?}")),
        })?;
    let (device, car) = found.devices.remove(index);
    let result = session(&car, &device, cancellation, orders, events, ready).await;
    // Even a failed or cancelled connect can have reached the OS. Keep the
    // adapter alive until disconnect has completed, including failed setup.
    let result = finish(result, "Bluetooth disconnect", car.disconnect()).await;
    drop(found);
    result
}

fn matches_selection(device: &Device, selected: Option<&Device>) -> bool {
    match selected {
        Some(selected) => device.id == selected.id,
        None => device.name == NAME,
    }
}

async fn session(
    car: &Peripheral,
    device: &Device,
    cancellation: &Cancellation<'_>,
    orders: Receiver<Vec<u8>>,
    events: &Sender<Result<Vec<u8>, Error>>,
    ready: &Sender<Result<String, Error>>,
) -> Result<(), Error> {
    cancellation
        .bluetooth("Bluetooth connection", car.connect())
        .await?;
    cancellation
        .bluetooth("Bluetooth service discovery", car.discover_services())
        .await?;
    let characteristics = car.characteristics();
    let wanted = characteristics
        .iter()
        .find(|c| c.uuid.to_string() == VALUE_UUID)
        .ok_or_else(|| {
            Error::NotFound("that device has no Cogware upload characteristic".into())
        })?;
    let reports = characteristics
        .iter()
        .find(|c| c.uuid.to_string() == REPORT_UUID)
        .ok_or_else(|| {
            Error::NotFound("that device has no Cogware report characteristic".into())
        })?;

    // The opening frame can be answered immediately, so subscribe before
    // advertising readiness or permitting any write.
    cancellation
        .bluetooth("Bluetooth report subscription", car.subscribe(reports))
        .await?;
    let mut notifications = cancellation
        .bluetooth("Bluetooth notifications", car.notifications())
        .await?;
    cancellation.check()?;
    ready
        .send(Ok(device.name.clone()))
        .map_err(|_| Error::Link("Bluetooth connection request was abandoned".into()))?;

    let cancellation = cancellation.transport();
    loop {
        cancellation.check()?;
        match orders.try_recv() {
            Ok(frame) => {
                for piece in frame.chunks(WRITE_MAX) {
                    cancellation
                        .bluetooth(
                            "Bluetooth write",
                            car.write(wanted, piece, WriteType::WithoutResponse),
                        )
                        .await?;
                    // The car drains its radio alongside CAN traffic; pacing
                    // avoids overrunning it without a round trip per packet.
                    tokio::time::sleep(BETWEEN_WRITES).await;
                }
                continue;
            }
            Err(TryRecvError::Disconnected) => return Ok(()),
            Err(TryRecvError::Empty) => {}
        }
        match tokio::time::timeout(IDLE, notifications.next()).await {
            Ok(Some(note)) if note.uuid == reports.uuid => {
                if events.send(Ok(note.value)).is_err() {
                    return Ok(());
                }
            }
            Ok(Some(_)) | Err(_) => {}
            Ok(None) => return Err(Error::Link("the Bluetooth device disconnected".into())),
        }
    }
}
