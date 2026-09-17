// SPDX-License-Identifier: GPL-3.0-only
//! Uploading over the car's own network.
//!
//! The server puts up a private access point; a laptop joins it and talks to
//! a socket. No router, no addresses to arrange, and nothing of the car on
//! anybody's home network -- which matters, because what this link does is
//! write firmware.
//!
//! The frames are the same ones the cable carries. A socket has no message
//! boundaries of its own, so a format that finds its own way back into step
//! is exactly what it needs; see `cogware_can::wire`.

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use super::{Error, Transport};

/// Where the server listens once its access point is up.
///
/// Fixed rather than discovered: the car is its own network with one thing on
/// it, so there is nothing to discover and a name service would be a second
/// thing to go wrong.
pub const DEFAULT_ADDR: &str = "192.168.4.1:3333";

/// How long to wait for the socket to come up.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Attempts at the socket before the car is called absent.
const TRIES: u32 = 6;

/// How long to leave between them.
///
/// Comfortably longer than the close handshake the car is finishing, so a
/// second attempt lands after it rather than during it.
const BETWEEN_TRIES: Duration = Duration::from_millis(400);

/// A link to the car over its own access point.
pub struct Wifi {
    stream: TcpStream,
    addr: SocketAddr,
}

impl Wifi {
    /// Join a server already listening at `addr`.
    pub fn connect(addr: &str) -> Result<Self, Error> {
        let addr: SocketAddr = addr
            .parse()
            .map_err(|_| Error::NotFound(format!("{addr} is not an address")))?;
        // Tried a few times, because a refusal here is usually not a car
        // that is absent but one that has not finished putting the phone
        // down. It closes the previous connection, waits out the handshake,
        // and only then listens again -- and a push sent during that gap is
        // refused by a server that is otherwise perfectly well. Measured:
        // pushing a rig, which is several files one after another, the second
        // was refused and the third went through.
        let mut refused = None;
        let mut stream = None;
        for _ in 0..TRIES {
            match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
                Ok(open) => {
                    stream = Some(open);
                    break;
                }
                Err(e) => {
                    refused = Some(e);
                    std::thread::sleep(BETWEEN_TRIES);
                }
            }
        }
        let stream = match stream {
            Some(open) => open,
            None => {
                // A refusal after all of those is not necessarily an absent
                // car. It takes one connection at a time -- one socket, and
                // an image being written to it is not a thing to interleave
                // -- so a window left watching it will refuse a push, and the
                // two look identical from here. Said as both, because being
                // told the wrong one sends somebody to check the wrong thing.
                let why = match refused {
                    Some(e) if e.kind() == ErrorKind::ConnectionRefused => {
                        "it is not listening -- either it is not there, or it \
                         is already talking to something else"
                            .to_string()
                    }
                    Some(e) => e.to_string(),
                    None => "no answer".to_string(),
                };
                return Err(Error::NotFound(format!("{addr}: {why}")));
            }
        };
        // Reads must not block the editor's frame, and writes must not sit in
        // a buffer waiting for company: an image is paced by what the car
        // says it has room for, so holding frames back would deadlock against
        // a car waiting to be fed.
        stream
            .set_nonblocking(true)
            .map_err(|e| Error::Link(e.to_string()))?;
        stream
            .set_nodelay(true)
            .map_err(|e| Error::Link(e.to_string()))?;
        Ok(Self { stream, addr })
    }

}

impl Transport for Wifi {
    fn send(&mut self, frame: &[u8]) -> Result<bool, Error> {
        let mut at = 0;
        while at < frame.len() {
            match self.stream.write(&frame[at..]) {
                Ok(0) => return Err(Error::Link("the car closed the socket".into())),
                Ok(n) => at += n,
                // A full send buffer is the ordinary back-pressure of a slow
                // link, not a fault: wait for it rather than lose the frame.
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    std::thread::yield_now();
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(Error::Link(e.to_string())),
            }
        }
        Ok(true)
    }

    fn poll(&mut self, out: &mut Vec<u8>) -> Result<(), Error> {
        let mut buf = [0u8; 1024];
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => return Err(Error::Link("the car closed the socket".into())),
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(Error::Link(e.to_string())),
            }
        }
    }

    fn describe(&self) -> String {
        format!("the car's network at {}", self.addr)
    }
}
