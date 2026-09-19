// SPDX-License-Identifier: GPL-3.0-only
//! Deterministic socket back-pressure and cancellation checks, without a car.

use super::*;
use std::collections::VecDeque;

struct Writer {
    steps: VecDeque<std::io::Result<usize>>,
    bytes: Vec<u8>,
    calls: usize,
}

impl Writer {
    fn new(steps: impl IntoIterator<Item = std::io::Result<usize>>) -> Self {
        Self {
            steps: steps.into_iter().collect(),
            bytes: Vec::new(),
            calls: 0,
        }
    }
}

impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.calls += 1;
        let n = self.steps.pop_front().expect("unexpected write")?;
        assert!(n <= bytes.len());
        self.bytes.extend_from_slice(&bytes[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn blocked() -> std::io::Result<usize> {
    Err(ErrorKind::WouldBlock.into())
}

#[test]
fn partial_writes_preserve_order_and_rejected_frames_are_not_accepted() {
    let mut pending = Pending::default();
    let mut writer = Writer::new([Ok(2), blocked(), Ok(1), blocked(), Ok(2), Ok(2)]);
    assert!(pending.send(&mut writer, b"abcde").unwrap());
    assert!(!pending.send(&mut writer, b"XY").unwrap());
    pending.progress(&mut writer).unwrap();
    pending.progress(&mut writer).unwrap();
    assert_eq!(writer.bytes, b"abc");
    assert!(pending.send(&mut writer, b"XY").unwrap());
    assert_eq!(writer.bytes, b"abcdeXY");
    assert_eq!(pending.start, pending.end);
}

#[test]
fn a_full_socket_accepts_only_one_bounded_pending_frame() {
    let mut pending = Pending::default();
    let mut writer = Writer::new([blocked(), blocked(), blocked(), Ok(3)]);
    assert!(pending.send(&mut writer, b"abc").unwrap());
    assert!(!pending.send(&mut writer, b"abc").unwrap());
    pending.progress(&mut writer).unwrap();
    assert_eq!(writer.calls, 3);
    assert!(writer.bytes.is_empty());
    pending.progress(&mut writer).unwrap();
    assert_eq!(writer.bytes, b"abc");
}

#[test]
fn interrupted_writes_resume_on_a_later_poll_without_spinning() {
    let mut pending = Pending::default();
    let mut writer = Writer::new([Err(ErrorKind::Interrupted.into()), Ok(3)]);
    assert!(pending.send(&mut writer, b"abc").unwrap());
    assert_eq!(writer.calls, 1);
    pending.progress(&mut writer).unwrap();
    assert_eq!(writer.bytes, b"abc");
}

#[test]
fn closed_and_failed_writes_are_reported() {
    for failure in [Ok(0), Err(ErrorKind::BrokenPipe.into())] {
        let mut pending = Pending::default();
        assert!(pending.send(&mut Writer::new([failure]), b"a").is_err());
    }
}

#[test]
fn oversized_frames_are_rejected_without_touching_the_socket() {
    let mut pending = Pending::default();
    let mut writer = Writer::new([]);
    let oversized = [0; cogware_can::wire::MAX_FRAME + 1];
    assert!(pending.send(&mut writer, &oversized).is_err());
    assert_eq!(writer.calls, 0);
}

#[test]
fn cancellation_prevents_any_connection_attempt() {
    let cancelled = AtomicBool::new(true);
    let addr = "127.0.0.1:3333".parse().unwrap();
    let result = retry_connect::<()>(addr, &cancelled, || panic!("must not connect"));
    assert!(matches!(result, Err(Error::Link(why)) if why.contains("cancelled")));
}

#[test]
fn cancellation_after_a_failed_attempt_prevents_retrying() {
    let cancelled = AtomicBool::new(false);
    let addr = "127.0.0.1:3333".parse().unwrap();
    let mut calls = 0;
    let result = retry_connect::<()>(addr, &cancelled, || {
        calls += 1;
        cancelled.store(true, Ordering::Relaxed);
        Err(ErrorKind::ConnectionRefused.into())
    });
    assert_eq!(calls, 1);
    assert!(matches!(result, Err(Error::Link(why)) if why.contains("cancelled")));
}

#[test]
fn a_connection_completed_after_cancellation_is_dropped() {
    struct Open<'a>(&'a AtomicBool);
    impl Drop for Open<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }
    let cancelled = AtomicBool::new(false);
    let dropped = AtomicBool::new(false);
    let addr = "127.0.0.1:3333".parse().unwrap();
    let result = retry_connect(addr, &cancelled, || {
        cancelled.store(true, Ordering::Relaxed);
        Ok(Open(&dropped))
    });
    assert!(result.is_err());
    assert!(dropped.load(Ordering::Relaxed));
}
