// SPDX-License-Identifier: GPL-3.0-only
//! Uploading over the car's own network, without waiting in the editor's frame.
//!
//! A bounded pending frame preserves TCP's partial writes while the caller
//! keeps reading the reports that pace the transfer.

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::{Error, Transport};

#[cfg(test)]
mod tests;

/// Where the server listens once its access point is up.
pub const DEFAULT_ADDR: &str = "192.168.4.1:3333";

// Short attempts bound cancellation latency even when a host silently drops SYNs.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const TRIES: u32 = 6;
const BETWEEN_TRIES: Duration = Duration::from_millis(400);
const CANCEL_POLL: Duration = Duration::from_millis(20);

/// A link to the car over its own access point.
pub struct Wifi {
    stream: TcpStream,
    addr: SocketAddr,
    pending: Pending,
}

impl Wifi {
    /// Join a server already listening at `addr`.
    pub fn connect(addr: &str) -> Result<Self, Error> {
        Self::connect_cancellable(addr, &AtomicBool::new(false))
    }

    /// Join `addr` on a worker, checking cancellation between bounded attempts.
    pub fn connect_cancellable(addr: &str, cancelled: &AtomicBool) -> Result<Self, Error> {
        check_cancelled(cancelled)?;
        let addr: SocketAddr = addr
            .parse()
            .map_err(|_| Error::NotFound(format!("{addr} is not an address")))?;
        let stream = retry_connect(addr, cancelled, || {
            TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
        })?;
        stream
            .set_nonblocking(true)
            .map_err(|e| Error::Link(e.to_string()))?;
        stream
            .set_nodelay(true)
            .map_err(|e| Error::Link(e.to_string()))?;
        check_cancelled(cancelled)?;
        Ok(Self {
            stream,
            addr,
            pending: Pending::default(),
        })
    }
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), Error> {
    if cancelled.load(Ordering::Relaxed) {
        Err(Error::Link("network connection cancelled".into()))
    } else {
        Ok(())
    }
}

fn retry_connect<T>(
    addr: SocketAddr,
    cancelled: &AtomicBool,
    mut connect: impl FnMut() -> std::io::Result<T>,
) -> Result<T, Error> {
    let mut refused = None;
    for attempt in 0..TRIES {
        check_cancelled(cancelled)?;
        match connect() {
            Ok(stream) => {
                check_cancelled(cancelled)?;
                return Ok(stream);
            }
            Err(e) => refused = Some(e),
        }
        check_cancelled(cancelled)?;
        if attempt + 1 < TRIES {
            // The car may still be closing its previous connection, but a
            // cancelled request must not wait out the whole retry interval.
            let deadline = Instant::now() + BETWEEN_TRIES;
            while Instant::now() < deadline {
                check_cancelled(cancelled)?;
                std::thread::sleep(
                    CANCEL_POLL.min(deadline.saturating_duration_since(Instant::now())),
                );
            }
        }
    }
    let why = match refused {
        Some(e) if e.kind() == ErrorKind::ConnectionRefused => {
            "it is not listening -- either it is not there, or it \
             is already talking to something else"
                .to_string()
        }
        Some(e) => e.to_string(),
        None => "no answer".to_string(),
    };
    Err(Error::NotFound(format!("{addr}: {why}")))
}

struct Pending {
    bytes: [u8; cogware_can::wire::MAX_FRAME],
    start: usize,
    end: usize,
}

impl Default for Pending {
    fn default() -> Self {
        Self {
            bytes: [0; cogware_can::wire::MAX_FRAME],
            start: 0,
            end: 0,
        }
    }
}

impl Pending {
    fn send(&mut self, stream: &mut impl Write, frame: &[u8]) -> Result<bool, Error> {
        if frame.len() > self.bytes.len() {
            return Err(Error::Link("network frame exceeds the wire limit".into()));
        }
        self.progress(stream)?;
        if self.start != self.end {
            return Ok(false);
        }
        self.bytes[..frame.len()].copy_from_slice(frame);
        self.start = 0;
        self.end = frame.len();
        self.progress(stream)?;
        Ok(true)
    }

    fn progress(&mut self, stream: &mut impl Write) -> Result<(), Error> {
        if self.start == self.end {
            return Ok(());
        }
        // One syscall bounds work even if a socket repeatedly interrupts or
        // accepts only one byte. The next poll resumes at exactly this offset.
        match stream.write(&self.bytes[self.start..self.end]) {
            Ok(0) => Err(Error::Link("the car closed the socket".into())),
            Ok(n) => {
                self.start += n;
                Ok(())
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => Ok(()),
            Err(e) => Err(Error::Link(e.to_string())),
        }
    }
}

impl Transport for Wifi {
    fn send(&mut self, frame: &[u8]) -> Result<bool, Error> {
        self.pending.send(&mut self.stream, frame)
    }

    fn poll(&mut self, out: &mut Vec<u8>) -> Result<(), Error> {
        self.pending.progress(&mut self.stream)?;
        let mut buf = [0u8; 1024];
        // A continuously streaming peer must still leave time to draw a frame.
        for _ in 0..16 {
            match self.stream.read(&mut buf) {
                Ok(0) => return Err(Error::Link("the car closed the socket".into())),
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {
                    return Ok(());
                }
                Err(e) => return Err(Error::Link(e.to_string())),
            }
        }
        Ok(())
    }

    fn describe(&self) -> String {
        format!("the car's network at {}", self.addr)
    }
}
