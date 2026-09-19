// SPDX-License-Identifier: GPL-3.0-only
//! Identity and worker-lifetime checks that never use Bluetooth hardware.

use super::*;

fn device(id: &str, name: &str) -> Device {
    Device {
        id: id.into(),
        name: name.into(),
        address: "same".into(),
    }
}

#[test]
fn a_selected_identity_never_falls_back_to_another_named_car() {
    let selected = device("adapter-a:first", NAME);
    assert!(!matches_selection(
        &device("adapter-a:second", NAME),
        Some(&selected)
    ));
    assert!(!matches_selection(
        &device("adapter-b:first", NAME),
        Some(&selected)
    ));
    assert!(matches_selection(
        &device("adapter-a:first", "renamed"),
        Some(&selected)
    ));
    assert!(matches_selection(&selected, None));
    assert!(!matches_selection(&device("other", "Beacon"), None));
}

#[test]
fn cancelling_before_scan_or_connect_does_not_open_a_radio() {
    let cancelled = Arc::new(AtomicBool::new(true));
    assert!(matches!(scan(&cancelled), Err(Error::Link(why)) if why.contains("cancelled")));
    let result = Bluetooth::connect_to(device("unused", NAME), cancelled);
    assert!(matches!(result, Err(Error::Link(why)) if why.contains("cancelled")));
}

#[test]
fn dropping_a_transport_stops_its_worker_without_joining() {
    let (to_radio, _orders) = std::sync::mpsc::sync_channel(QUEUE);
    let (_events, from_radio) = std::sync::mpsc::channel();
    let stopped = Arc::new(AtomicBool::new(false));
    drop(Bluetooth {
        to_radio,
        from_radio,
        name: NAME.into(),
        stopped: Arc::clone(&stopped),
    });
    assert!(stopped.load(Ordering::Relaxed));
}

#[test]
fn a_finished_setup_job_does_not_cancel_the_returned_transport() {
    let cancelled = AtomicBool::new(false);
    let stopped = AtomicBool::new(false);
    let setup = Cancellation::new(&cancelled, &stopped);
    let active = setup.transport();
    cancelled.store(true, Ordering::Relaxed);
    assert!(setup.check().is_err());
    assert!(active.check().is_ok());
    stopped.store(true, Ordering::Relaxed);
    assert!(active.check().is_err());
}

#[test]
fn a_cancelled_operation_is_not_started() {
    let cancelled = AtomicBool::new(true);
    let stopped = AtomicBool::new(false);
    let cancellation = Cancellation::new(&cancelled, &stopped);
    let called = AtomicBool::new(false);
    let result = runtime()
        .unwrap()
        .block_on(cancellation.run("test", IO_TIMEOUT, async {
            called.store(true, Ordering::Relaxed);
            Ok(())
        }));
    assert!(result.is_err());
    assert!(!called.load(Ordering::Relaxed));
}

#[test]
fn an_in_flight_operation_observes_cancellation() {
    struct Running<'a>(&'a AtomicBool);
    impl Drop for Running<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    let cancelled = AtomicBool::new(false);
    let stopped = AtomicBool::new(false);
    let abandoned = AtomicBool::new(false);
    let cancellation = Cancellation::new(&cancelled, &stopped);
    let result = runtime().unwrap().block_on(async {
        let (result, ()) = tokio::join!(
            cancellation.run::<()>("test", IO_TIMEOUT, async {
                let _running = Running(&abandoned);
                std::future::pending().await
            }),
            async {
                tokio::time::sleep(Duration::from_millis(1)).await;
                cancelled.store(true, Ordering::Relaxed);
            }
        );
        result
    });
    assert!(matches!(result, Err(Error::Link(why)) if why.contains("cancelled")));
    assert!(abandoned.load(Ordering::Relaxed));
}

#[test]
fn a_stalled_operation_reports_its_timeout() {
    let flag = AtomicBool::new(false);
    let cancellation = Cancellation::new(&flag, &flag);
    let result = runtime().unwrap().block_on(cancellation.run::<()>(
        "service discovery",
        Duration::from_millis(1),
        std::future::pending(),
    ));
    assert!(matches!(result, Err(Error::Link(why)) if why.contains("service discovery timed out")));
}

#[test]
fn failed_setup_still_runs_cleanup_and_preserves_the_failure() {
    let cleaned = AtomicBool::new(false);
    let original = Error::NotFound("missing report characteristic".into());
    let result = runtime().unwrap().block_on(finish::<(), Error>(
        Err(original.clone()),
        "disconnect",
        async {
            cleaned.store(true, Ordering::Relaxed);
            Ok(())
        },
    ));
    assert!(cleaned.load(Ordering::Relaxed));
    assert_eq!(result, Err(original));
}

#[test]
fn cleanup_errors_are_not_silently_discarded() {
    let result = runtime().unwrap().block_on(finish::<(), Error>(
        Err(Error::Link("setup failed".into())),
        "disconnect",
        async { Err(Error::Link("radio refused cleanup".into())) },
    ));
    assert!(
        matches!(result, Err(Error::Link(why)) if why.contains("setup failed") && why.contains("radio refused cleanup"))
    );
}
