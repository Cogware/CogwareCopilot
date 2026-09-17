// SPDX-License-Identifier: GPL-3.0-only
//! Uploading over Bluetooth, from Windows or Linux.
//!
//! The same frames again: written to the characteristic the car takes an
//! image on, and read back from the one it reports through.
//!
//! Both directions are needed, not just the write. A push is paced by what
//! the car says it has room for -- it streams an image through a buffer a
//! fraction of its size -- so a link that only wrote would be an editor
//! guessing, and guessing wrong in the direction that loses bytes.
//!
//! # Why a thread
//!
//! Every cross-platform Bluetooth library is async, and the editor is a
//! redraw loop that must not be held up. So the radio lives on a thread of
//! its own with its own runtime, and the two sides pass whole frames through
//! channels. The editor never awaits anything and never blocks on a radio
//! that has wandered off.

use std::sync::mpsc::{Receiver, Sender, SyncSender, TryRecvError};
use std::time::Duration;

use super::{Error, Transport};

/// What the car calls itself in an advert.
pub const NAME: &str = "Cogware";

/// The characteristic an image is written to.
///
/// Written out the way a person reads it; the car holds the same value
/// byte-reversed, which is how the wire carries it.
pub const VALUE_UUID: &str = "00000002-c067-4761-9e5d-436f67776172";

/// The characteristic the car reports through.
pub const REPORT_UUID: &str = "00000003-c067-4761-9e5d-436f67776172";

/// Frames that may be waiting on the radio at once.
///
/// Enough that the radio always has work, few enough that "handed over" and
/// "written" stay close together. See where the queue is made.
const QUEUE: usize = 2;

/// How long to look for a car before giving up.
const SCAN_FOR: Duration = Duration::from_secs(8);

/// A link to the car over Bluetooth.
pub struct Bluetooth {
    to_radio: SyncSender<Vec<u8>>,
    from_radio: Receiver<Result<Vec<u8>, Error>>,
    name: String,
}

impl Bluetooth {
    /// Find a car, connect, and be ready to write.
    ///
    /// Blocks while it looks, because there is nothing for the editor to draw
    /// until there is a car; everything after this is asynchronous.
    pub fn connect() -> Result<Self, Error> {
        // Bounded, and that bound is load-bearing. A push counts a frame as
        // sent the moment it is handed over, and paces itself by how far
        // ahead of the car it has got. An unbounded queue lets it run
        // thousands of bytes ahead of a radio that has written none of them:
        // the car then starves waiting for image that is sitting in this
        // queue, and gives the push up as abandoned. Measured doing exactly
        // that, four percent into a firmware image.
        let (to_radio, orders) = std::sync::mpsc::sync_channel::<Vec<u8>>(QUEUE);
        let (events, from_radio) = std::sync::mpsc::channel::<Result<Vec<u8>, Error>>();
        let (ready, started) = std::sync::mpsc::channel::<Result<String, Error>>();

        std::thread::Builder::new()
            .name("cogware-ble".into())
            .spawn(move || run(orders, events, ready))
            .map_err(|e| Error::Link(format!("no thread for the radio: {e}")))?;

        // The scan's own limit plus a little, so the answer is the radio's
        // rather than this one's.
        match started.recv_timeout(SCAN_FOR + Duration::from_secs(4)) {
            Ok(Ok(name)) => Ok(Self { to_radio, from_radio, name }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(Error::NotFound("the radio never answered".into())),
        }
    }
}

impl Transport for Bluetooth {
    fn send(&mut self, frame: &[u8]) -> Result<bool, Error> {
        use std::sync::mpsc::TrySendError;
        match self.to_radio.try_send(frame.to_vec()) {
            Ok(()) => Ok(true),
            // The radio has not caught up. Said rather than waited on, so the
            // caller goes back to reading reports -- which is the only thing
            // that will tell it to send more.
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
                    return Err(Error::Link("the radio thread has gone".into()))
                }
            }
        }
    }

    fn describe(&self) -> String {
        format!("{} over Bluetooth", self.name)
    }
}

/// The radio thread: its own runtime, and a loop that writes what it is given.
fn run(
    orders: Receiver<Vec<u8>>,
    events: Sender<Result<Vec<u8>, Error>>,
    ready: Sender<Result<String, Error>>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            let _ = ready.send(Err(Error::Link(format!("no runtime: {e}"))));
            return;
        }
    };
    runtime.block_on(async move {
        match talk(orders, &events, &ready).await {
            Ok(()) => {}
            Err(e) => {
                // Reported down whichever channel is still being watched: the
                // editor is waiting on one of them and must not be left to
                // time out when the reason is known.
                let _ = ready.send(Err(e.clone()));
                let _ = events.send(Err(e));
            }
        }
    });
}

/// Find the car, connect, and pump frames until the editor stops sending.
async fn talk(
    orders: Receiver<Vec<u8>>,
    events: &Sender<Result<Vec<u8>, Error>>,
    ready: &Sender<Result<String, Error>>,
) -> Result<(), Error> {
    use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter, WriteType};
    use btleplug::platform::Manager;
    use futures::StreamExt;

    let manager = Manager::new()
        .await
        .map_err(|e| Error::Link(format!("no Bluetooth: {e}")))?;
    let adapters = manager
        .adapters()
        .await
        .map_err(|e| Error::Link(format!("no Bluetooth: {e}")))?;
    let central = adapters
        .into_iter()
        .next()
        .ok_or_else(|| Error::NotFound("this computer has no Bluetooth adapter".into()))?;

    central
        .start_scan(ScanFilter::default())
        .await
        .map_err(|e| Error::Link(format!("the scan would not start: {e}")))?;
    tokio::time::sleep(SCAN_FOR).await;

    let mut found = None;
    for peripheral in central
        .peripherals()
        .await
        .map_err(|e| Error::Link(e.to_string()))?
    {
        let named = peripheral
            .properties()
            .await
            .ok()
            .flatten()
            .and_then(|p| p.local_name);
        if named.as_deref() == Some(NAME) {
            found = Some((peripheral, named.unwrap_or_default()));
            break;
        }
    }
    let (car, name) = found.ok_or_else(|| {
        Error::NotFound(format!("no car called {NAME} answered in {SCAN_FOR:?}"))
    })?;

    car.connect()
        .await
        .map_err(|e| Error::Link(format!("would not connect: {e}")))?;
    car.discover_services()
        .await
        .map_err(|e| Error::Link(format!("would not describe itself: {e}")))?;

    let found = car.characteristics();
    let wanted = found
        .iter()
        .find(|c| c.uuid.to_string() == VALUE_UUID)
        .cloned()
        .ok_or_else(|| {
            Error::NotFound("that device has no Cogware upload characteristic".into())
        })?;
    let reports = found
        .iter()
        .find(|c| c.uuid.to_string() == REPORT_UUID)
        .cloned()
        .ok_or_else(|| {
            Error::NotFound("that device has no Cogware report characteristic".into())
        })?;

    // Subscribed before anything is written, or the answer to the opening
    // frame arrives before anybody is listening for it and the push waits out
    // its whole patience on a car that did reply.
    car.subscribe(&reports)
        .await
        .map_err(|e| Error::Link(format!("would not report back: {e}")))?;
    let mut notifications = car
        .notifications()
        .await
        .map_err(|e| Error::Link(format!("no reports: {e}")))?;

    let _ = ready.send(Ok(name));

    // Unacknowledged, which means a write can be dropped without either end
    // being told: the car's radio fills faster than its loop drains it, and
    // the packets that do not fit are simply gone. Measured: seven thousand
    // bytes written and never arrived.
    //
    // That is not corrected here, and deliberately. Asking for an
    // acknowledgement per write was tried and is worse -- it turns every
    // packet into a round trip through a board that is also driving a bus,
    // and the replies time out under load. The recovery belongs where the
    // truth is: the car reports what it has actually taken, and `Push`
    // rewinds to that figure rather than trusting its own count. Losing a
    // packet then costs one rewind, which is what the bus protocol does about
    // the same problem one layer down.
    loop {
        // Writing comes first, and reports are taken whenever there is a
        // pause. That is not a compromise: the editor stops writing the
        // moment it has filled the room the car last reported, so a pause is
        // exactly when a fresh report is worth having -- and is the only time
        // the editor is waiting on one.
        match orders.try_recv() {
            Ok(frame) => {
                for piece in frame.chunks(WRITE_MAX) {
                    car.write(&wanted, piece, WriteType::WithoutResponse)
                        .await
                        .map_err(|e| Error::Link(format!("a write failed: {e}")))?;
                    // A breath between packets. Nothing acknowledges these,
                    // so the only thing stopping the car's radio being
                    // overrun is not filling it faster than its loop empties
                    // it -- and every packet lost costs a rewind of
                    // everything after it. At this rate the radio still
                    // carries far more than the bus beyond it can take, so
                    // the pause is free.
                    tokio::time::sleep(BETWEEN_WRITES).await;
                }
                continue;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
        // Waited on rather than polled in a spin, so a quiet moment costs the
        // radio nothing.
        match tokio::time::timeout(IDLE, notifications.next()).await {
            Ok(Some(note)) if note.uuid == reports.uuid => {
                if events.send(Ok(note.value)).is_err() {
                    break;
                }
            }
            // Another characteristic, or simply nothing said in the while.
            Ok(Some(_)) | Err(_) => {}
            // The stream ended, which means the connection has.
            Ok(None) => return Ok(()),
        }
    }
    let _ = car.disconnect().await;
    Ok(())
}

/// How long to wait on a quiet radio before looking for work again.
const IDLE: Duration = Duration::from_millis(20);

/// How long to leave between packets.
///
/// Sized against the car rather than the radio: what matters is that its loop
/// drains its receive queue between one packet and the next, and that loop is
/// also driving a CAN bus.
const BETWEEN_WRITES: Duration = Duration::from_millis(3);

/// Most bytes to put in one write.
///
/// Under the payload the car agreed to, with room for the attribute header
/// that rides in front of it. A write larger than the link allows is refused
/// whole rather than split for us.
const WRITE_MAX: usize = 180;
