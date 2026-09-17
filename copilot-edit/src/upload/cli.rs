// SPDX-License-Identifier: GPL-3.0-only
//! Pushing from a terminal, with no window.
//!
//! The same code the window drives, so what is tested here is what a person
//! gets when they press Send. It exists because a link is worth testing in a
//! script -- over and over, from a machine with no screen -- and because an
//! editor that can also be a command is an editor that fits a build.

use std::path::Path;

use super::watch::{self, Reading, Watch};
use super::{bluetooth::Bluetooth, wifi::Wifi, Push, State, Transport};

/// Open whichever link the command line names.
fn open(args: &[String]) -> Result<Box<dyn Transport>, String> {
    let after = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .map(String::as_str)
    };
    match after("--link").unwrap_or("bluetooth") {
        "bluetooth" | "ble" => {
            println!("looking for the car over Bluetooth…");
            Ok(Box::new(Bluetooth::connect().map_err(|e| e.to_string())?))
        }
        "wifi" | "net" | "lan" => {
            let addr = after("--addr").unwrap_or(super::wifi::DEFAULT_ADDR);
            Ok(Box::new(Wifi::connect(addr).map_err(|e| e.to_string())?))
        }
        other => Err(format!("--link does not know {other}")),
    }
}

/// Print what the car says about itself until interrupted.
///
/// The same code the editor's window drives, so what is checked here is what
/// somebody sees there.
///
/// ```text
/// copilot-edit --watch [--link bluetooth|wifi|lan] [--addr HOST:PORT] [--for SECONDS]
/// ```
pub fn watch(args: &[String]) -> Result<(), String> {
    let seconds: u64 = args
        .iter()
        .position(|a| a == "--for")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let mut link = open(args)?;
    println!("watching {}", link.describe());

    let mut view = Watch::new();
    let started = std::time::Instant::now();
    let mut shown = None;
    while started.elapsed().as_secs() < seconds {
        view.step(link.as_mut()).map_err(|e| e.to_string())?;
        if let Some((status, _)) = view.latest() {
            // Printed on change, so a car sitting still does not fill the
            // terminal with the same line.
            let line = format!(
                "up {}  bus {} f/s  tx {}  rx {}  host {}  crc {}  subs {}  [{}{}{}]",
                watch::uptime(status.uptime_ms),
                status.can_tx_rate,
                status.can_tx,
                status.can_rx,
                status.host_frames,
                status.crc_errors,
                status.subscribers,
                if status.can_up() { "CAN " } else { "" },
                if status.panel_up() { "OLED " } else { "" },
                if status.loopback() { "LOOPBACK" } else { "" },
            );
            if shown.as_deref() != Some(line.as_str()) {
                println!("{line}");
                shown = Some(line);
                if args.iter().any(|a| a == "--gauges") {
                    // Every gauge in the table, raw count first, including
                    // the ones nothing is filling: those print the sentinel
                    // the car actually sent and what it means, so the number
                    // reads the same here as it does in a packet dump.
                    for gauge in cogware_can::ALL_GAUGES {
                        let (raw, note) = match view.reading(gauge.id) {
                            Reading::Value(raw) => (
                                format!("{raw:>10}"),
                                format!(
                                    "{:.2} {}",
                                    raw as f32 / gauge.unit.scale as f32,
                                    gauge.unit.symbol
                                ),
                            ),
                            Reading::NoData => {
                                ("  CAFEBABE".into(), "no sensor data".into())
                            }
                        };
                        println!("    {:<22} {raw}  {note}", gauge.name);
                    }
                    println!("    ({} reading)", view.live_gauges());
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if view.latest().is_none() {
        return Err("the car never said anything".into());
    }
    Ok(())
}

/// Run a push described by the command line, and report what happened.
///
/// ```text
/// copilot-edit --push FILE --node 0x02 --kind scene --slot 0 [--link bluetooth|wifi|lan]
///              [--addr HOST:PORT]
/// ```
pub fn run(args: &[String]) -> Result<(), String> {
    let after = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .map(String::as_str)
    };
    let file = after("--push").ok_or("--push wants a file")?;
    let node = number(after("--node").unwrap_or("0x02"))? as u8;
    let slot = number(after("--slot").unwrap_or("0"))? as u8;
    let kind = match after("--kind").unwrap_or("scene") {
        "scene" => cogware_can::xfer::kind::SCENE,
        "rig" => cogware_can::xfer::kind::RIG,
        "firmware" => cogware_can::xfer::kind::FIRMWARE,
        other => return Err(format!("--kind does not know {other}")),
    };

    let image = std::fs::read(Path::new(file)).map_err(|e| format!("{file}: {e}"))?;
    println!("{file}: {} bytes -> node {node:#04x} kind {kind} slot {slot}", image.len());

    let mut link: Box<dyn Transport> = match after("--link").unwrap_or("bluetooth") {
        "bluetooth" | "ble" => {
            println!("looking for the car over Bluetooth…");
            Box::new(Bluetooth::connect().map_err(|e| e.to_string())?)
        }
        "wifi" | "net" | "lan" => {
            let addr = after("--addr").unwrap_or(super::wifi::DEFAULT_ADDR);
            Box::new(Wifi::connect(addr).map_err(|e| e.to_string())?)
        }
        other => return Err(format!("--link does not know {other}")),
    };
    println!("connected to {}", link.describe());

    let mut push = Push::new(node, kind, slot, image);
    let started = std::time::Instant::now();
    let mut shown = u8::MAX;
    loop {
        let now_ms = started.elapsed().as_millis() as u64;
        match push.step(link.as_mut(), now_ms) {
            Ok(State::Done) => {
                println!("done in {}s", started.elapsed().as_secs());
                return Ok(());
            }
            Ok(_) => {
                if push.percent() != shown {
                    shown = push.percent();
                    println!("  {shown}%");
                }
            }
            Err(e) => {
                // Told to stop, so the car does not sit holding the transfer
                // open and refusing every later one.
                let _ = push.cancel(link.as_mut());
                return Err(format!(
                    "{e} (heard {} bytes, {} frames)",
                    push.heard(),
                    push.frames()
                ));
            }
        }
        // The car is paced by what it says it has room for, and asking again
        // immediately only burns the link.
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// A decimal or `0x`-prefixed number.
fn number(text: &str) -> Result<u32, String> {
    let (body, radix) = match text.strip_prefix("0x") {
        Some(hex) => (hex, 16),
        None => (text, 10),
    };
    u32::from_str_radix(body, radix).map_err(|_| format!("{text} is not a number"))
}
