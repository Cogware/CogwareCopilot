// SPDX-License-Identifier: GPL-3.0-only
//! Scoped discovery, with scans cleaned up on every return path.
//! Windows exposes one system-wide discovery scope rather than adapter routing.

use std::collections::BTreeSet;

use btleplug::api::{
    Central, CentralEvent, CentralState, Manager as _, Peripheral as _, ScanFilter,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::{FutureExt, Stream, StreamExt, future};

use super::{Cancellation, Device, Error, IO_TIMEOUT, NAME, SCAN_FOR, finish};

/// Runtime-bound peripherals and the adapters that keep them valid.
pub(super) struct Discovery {
    /// Descriptions paired with handles from the current runtime only.
    pub(super) devices: Vec<(Device, Peripheral)>,
    _adapters: Vec<(String, Adapter)>,
}

/// Find current advertisements, optionally only in the selected discovery scope.
pub(super) async fn discover(
    cancellation: &Cancellation<'_>,
    selected: Option<&Device>,
) -> Result<Discovery, Error> {
    let adapters = cancellation
        .run("Bluetooth adapters", IO_TIMEOUT, adapters(selected))
        .await?;
    let result = cancellation
        .run("Bluetooth scan", SCAN_FOR + IO_TIMEOUT * 2, async {
            let results =
                future::join_all(adapters.iter().map(|(key, adapter)| inspect(key, adapter))).await;
            let mut devices = Vec::new();
            let mut errors = Vec::new();
            for result in results {
                match result {
                    Ok(found) => devices.extend(found),
                    Err(error) => errors.push(error.to_string()),
                }
            }
            if !errors.is_empty() {
                return Err(Error::Link(errors.join("; ")));
            }
            devices.sort_by_cached_key(|(device, _)| sort_key(device));
            Ok(devices)
        })
        .await;
    // A cancelled start_scan may already have reached the OS, so every
    // attempted adapter is stopped, not just those whose start returned Ok.
    let devices = finish(result, "stopping Bluetooth scans", async {
        let results = future::join_all(adapters.iter().map(|(key, adapter)| async move {
            adapter.stop_scan().await.map_err(|e| format!("{key}: {e}"))
        }))
        .await;
        let errors: Vec<_> = results.into_iter().filter_map(Result::err).collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    })
    .await?;
    cancellation.check()?;
    Ok(Discovery {
        devices,
        _adapters: adapters,
    })
}

async fn adapters(selected: Option<&Device>) -> Result<Vec<(String, Adapter)>, Error> {
    let manager = Manager::new()
        .await
        .map_err(|e| Error::Link(format!("no Bluetooth: {e}")))?;
    let adapters = manager
        .adapters()
        .await
        .map_err(|e| Error::Link(format!("no Bluetooth adapters: {e}")))?;
    if adapters.is_empty() {
        return Err(Error::NotFound(
            "this computer has no Bluetooth adapter".into(),
        ));
    }
    let mut candidates = Vec::new();
    for adapter in adapters {
        let description = adapter
            .adapter_info()
            .await
            .map_err(|e| Error::Link(format!("Bluetooth adapter information: {e}")))?;
        let state = adapter
            .adapter_state()
            .await
            .map_err(|e| Error::Link(format!("{description}: cannot read adapter state: {e}")))?;
        candidates.push((description, state, adapter));
    }
    select_adapters(candidates, selected, cfg!(target_os = "windows"))
}

fn select_adapters<T>(
    candidates: Vec<(String, CentralState, T)>,
    selected: Option<&Device>,
    system_wide: bool,
) -> Result<Vec<(String, T)>, Error> {
    let mut usable = Vec::new();
    let mut unavailable = Vec::new();
    let mut keys = BTreeSet::new();
    for (description, state, adapter) in candidates {
        if state != CentralState::PoweredOn {
            unavailable.push(format!("{description}: {state:?}"));
            continue;
        }
        // WinRT's watcher and FromBluetoothAddressAsync have no adapter
        // argument. One powered wrapper represents that global service;
        // neither its constant adapter_info nor enumeration order is an ID.
        let key = if system_wide {
            "winrt-system".to_string()
        } else {
            description
        };
        if selected.is_some_and(|device| !device.id.starts_with(&prefix(&key))) {
            continue;
        }
        if !keys.insert(key.clone()) {
            if system_wide {
                continue;
            }
            return Err(Error::Link(format!(
                "Bluetooth adapters have ambiguous identity: {key}"
            )));
        }
        usable.push((key, adapter));
    }
    if usable.is_empty() {
        let why = if unavailable.is_empty() {
            "the selected Bluetooth adapter is no longer available".into()
        } else {
            format!(
                "no powered-on Bluetooth adapter ({})",
                unavailable.join("; ")
            )
        };
        return Err(Error::NotFound(why));
    }
    Ok(usable)
}

async fn inspect(key: &str, adapter: &Adapter) -> Result<Vec<(Device, Peripheral)>, Error> {
    let mut events = adapter
        .events()
        .await
        .map_err(|e| Error::Link(format!("{key}: scan events unavailable: {e}")))?;
    discard_snapshot(key, &mut events).await?;
    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(|e| Error::Link(format!("{key}: scan would not start: {e}")))?;
    let until = tokio::time::sleep(SCAN_FOR);
    tokio::pin!(until);
    let mut seen = BTreeSet::new();
    loop {
        tokio::select! {
            biased;
            _ = &mut until => break,
            event = events.next() => match event {
                Some(CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id)
                    | CentralEvent::ManufacturerDataAdvertisement { id, .. }
                    | CentralEvent::ServiceDataAdvertisement { id, .. }
                    | CentralEvent::ServicesAdvertisement { id, .. }
                    | CentralEvent::RssiUpdate { id, .. }) => { seen.insert(id); }
                Some(CentralEvent::StateUpdate(state)) if state != CentralState::PoweredOn => {
                    return Err(Error::Link(format!("{key}: adapter became {state:?}")));
                }
                Some(_) => {}
                None => return Err(Error::Link(format!("{key}: scan event stream ended"))),
            }
        }
    }
    // Some backends retain old peripherals indefinitely; only advertisements
    // observed during this scan belong in a nearby-device picker.
    let mut devices = Vec::new();
    for id in seen {
        let peripheral = adapter
            .peripheral(&id)
            .await
            .map_err(|e| Error::Link(format!("{key}: cannot resolve {id}: {e}")))?;
        let properties = peripheral
            .properties()
            .await
            .map_err(|e| Error::Link(format!("{key}: cannot describe {id}: {e}")))?;
        let name = properties
            .and_then(|p| p.local_name)
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "Unnamed device".into());
        let device = Device {
            id: identity(key, &peripheral.id().to_string()),
            name,
            address: peripheral.address().to_string(),
        };
        devices.push((device, peripheral));
    }
    Ok(devices)
}

async fn discard_snapshot(key: &str, events: &mut (impl Stream + Unpin)) -> Result<(), Error> {
    // BlueZ prepends an immediately-ready snapshot of cached peripherals.
    // Drain it before start_scan, never after: new advertisements, including
    // those from a cached device, must remain evidence for this scan.
    loop {
        for _ in 0..64 {
            match events.next().now_or_never() {
                Some(Some(_)) => {}
                Some(None) => {
                    return Err(Error::Link(format!("{key}: scan event stream ended")));
                }
                None => return Ok(()),
            }
        }
        // A busy stream must not keep the enclosing timeout or cancellation
        // future from being polled.
        tokio::task::yield_now().await;
    }
}

fn prefix(adapter: &str) -> String {
    format!("{}:{adapter}", adapter.len())
}

fn identity(adapter: &str, peripheral: &str) -> String {
    format!("{}{peripheral}", prefix(adapter))
}

fn sort_key(device: &Device) -> (bool, bool, String, String, String) {
    (
        !device.name.eq_ignore_ascii_case(NAME),
        device.name == "Unnamed device",
        device.name.to_lowercase(),
        device.address.clone(),
        device.id.clone(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_distinguishes_adapters_and_peripherals_without_separator_collisions() {
        assert_ne!(identity("a", "bc"), identity("ab", "c"));
        assert_ne!(
            identity("hci0", "same-address"),
            identity("hci1", "same-address")
        );
        assert_ne!(identity("hci0", "first"), identity("hci0", "second"));
        assert!(!identity("ab", "c").starts_with(&prefix("a")));
    }

    #[test]
    fn sorting_is_useful_and_deterministic_for_duplicate_names() {
        let device = |id: &str, name: &str| Device {
            id: id.into(),
            name: name.into(),
            address: "same".into(),
        };
        let mut devices = [
            device("z", "Unnamed device"),
            device("b", NAME),
            device("x", "Beacon"),
            device("a", NAME),
        ];
        devices.sort_by_cached_key(sort_key);
        assert_eq!(
            devices.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
            ["a", "b", "x", "z"]
        );
    }

    #[test]
    fn windows_adapters_share_one_scope_regardless_of_count_or_order() {
        let on = |adapter| ("WinRT".into(), CentralState::PoweredOn, adapter);
        let first = select_adapters(vec![on(1), on(2)], None, true).unwrap();
        let reversed = select_adapters(vec![on(2), on(1)], None, true).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(reversed.len(), 1);
        assert_eq!(first[0].0, reversed[0].0);
        assert_eq!(
            identity(&first[0].0, "peripheral"),
            identity(&reversed[0].0, "peripheral")
        );
        let selected = Device {
            id: identity(&first[0].0, "peripheral"),
            name: NAME.into(),
            address: "same".into(),
        };
        assert_eq!(
            select_adapters(vec![on(3)], Some(&selected), true).unwrap()[0].0,
            first[0].0
        );
    }

    #[test]
    fn an_off_windows_adapter_does_not_hide_the_powered_adapter() {
        let off = || ("WinRT".into(), CentralState::PoweredOff, 0);
        let on = || ("WinRT".into(), CentralState::PoweredOn, 1);
        for candidates in [vec![off(), on()], vec![on(), off()]] {
            let usable = select_adapters(candidates, None, true).unwrap();
            assert_eq!(usable.len(), 1);
            assert_eq!(usable[0].1, 1);
        }
        assert!(matches!(
            select_adapters(vec![off()], None, true),
            Err(Error::NotFound(why)) if why.contains("PoweredOff")
        ));
    }

    #[test]
    fn routed_adapters_keep_distinct_scopes_and_reject_ambiguous_powered_ids() {
        let on = |name: &str| (name.into(), CentralState::PoweredOn, ());
        assert_eq!(
            select_adapters(vec![on("hci0"), on("hci1")], None, false)
                .unwrap()
                .len(),
            2
        );
        assert!(select_adapters(vec![on("hci0"), on("hci0")], None, false).is_err());
        let usable = select_adapters(
            vec![("hci0".into(), CentralState::PoweredOff, ()), on("hci0")],
            None,
            false,
        )
        .unwrap();
        assert_eq!(usable.len(), 1);
    }

    #[test]
    fn bootstrap_devices_are_discarded_but_current_advertisements_remain() {
        let (sender, live) = futures::channel::mpsc::unbounded();
        let mut events = futures::stream::iter(["cached-discovery", "cached-services"]).chain(live);
        super::super::lifecycle::runtime().unwrap().block_on(async {
            discard_snapshot("adapter", &mut events).await.unwrap();
            assert!(events.next().now_or_never().is_none());
            sender.unbounded_send("new-advertisement").unwrap();
            sender.unbounded_send("cached-discovery").unwrap();
            assert_eq!(events.next().await, Some("new-advertisement"));
            assert_eq!(events.next().await, Some("cached-discovery"));
        });
    }

    #[test]
    fn an_ended_bootstrap_stream_reports_the_adapter_error() {
        let mut events = futures::stream::iter(["cached-discovery"]);
        let result = super::super::lifecycle::runtime()
            .unwrap()
            .block_on(discard_snapshot("adapter", &mut events));
        assert!(
            matches!(result, Err(Error::Link(why)) if why.contains("adapter: scan event stream ended"))
        );
    }

    #[test]
    fn draining_a_busy_stream_still_observes_the_scan_timeout() {
        let flag = std::sync::atomic::AtomicBool::new(false);
        let cancellation = Cancellation::new(&flag, &flag);
        let mut events = futures::stream::repeat("advertisement");
        let result = super::super::lifecycle::runtime()
            .unwrap()
            .block_on(cancellation.run(
                "snapshot",
                std::time::Duration::from_millis(1),
                discard_snapshot("adapter", &mut events),
            ));
        assert!(matches!(result, Err(Error::Link(why)) if why.contains("snapshot timed out")));
    }

    #[test]
    fn draining_a_busy_stream_still_observes_cancellation() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let cancelled = AtomicBool::new(false);
        let stopped = AtomicBool::new(false);
        let cancellation = Cancellation::new(&cancelled, &stopped);
        let mut events = futures::stream::repeat("advertisement").inspect(|_| {
            cancelled.store(true, Ordering::Relaxed);
        });
        let result = super::super::lifecycle::runtime()
            .unwrap()
            .block_on(cancellation.run(
                "snapshot",
                IO_TIMEOUT,
                discard_snapshot("adapter", &mut events),
            ));
        assert!(matches!(result, Err(Error::Link(why)) if why.contains("cancelled")));
    }
}
