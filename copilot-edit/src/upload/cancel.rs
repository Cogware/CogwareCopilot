// SPDX-License-Identifier: GPL-3.0-only
//! An upload cancellation stays connected until the controller acknowledges it.

use std::time::{Duration, Instant};

use cogware_can::wire::{self, Report};

use super::{Error, Push, Transport};

pub(super) struct Cancel {
    link: Box<dyn Transport>,
    push: Push,
    started: Instant,
    queued: bool,
    incoming: Vec<u8>,
    decoder: wire::Decoder,
}

impl Cancel {
    pub fn new(link: Box<dyn Transport>, push: Push) -> Self {
        Self {
            link,
            push,
            started: Instant::now(),
            queued: false,
            incoming: Vec::new(),
            decoder: wire::Decoder::new(),
        }
    }

    pub fn step(&mut self) -> Result<Option<String>, Error> {
        self.link.poll(&mut self.incoming)?;
        if !self.queued {
            self.queued = self.push.cancel(self.link.as_mut())?;
        }
        for byte in std::mem::take(&mut self.incoming) {
            let Some((kind, len)) = self.decoder.push(byte) else {
                continue;
            };
            if kind != wire::KIND_XFER_REPORT {
                continue;
            }
            let Some(report) = Report::decode(&self.decoder.payload()[..len]) else {
                continue;
            };
            if self.queued && report.finished() && report.reason == 0x83 {
                return Ok(Some("Upload cancelled.".into()));
            }
            if report.phase == wire::PHASE_DONE {
                return Ok(Some(
                    "The upload finished before it could be cancelled.".into(),
                ));
            }
        }
        if self.started.elapsed() >= Duration::from_secs(3) {
            return Err(Error::Link(
                "Upload stopped locally, but the controller did not confirm cancellation. Wait for the display to leave update mode before trying again.".into(),
            ));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct BusyOnce(Arc<AtomicUsize>);
    impl Transport for BusyOnce {
        fn send(&mut self, _: &[u8]) -> Result<bool, Error> {
            Ok(self.0.fetch_add(1, Ordering::SeqCst) > 0)
        }
        fn poll(&mut self, _: &mut Vec<u8>) -> Result<(), Error> {
            Ok(())
        }
        fn describe(&self) -> String {
            "test controller".into()
        }
    }

    #[test]
    fn cancellation_retries_back_pressure_without_sending_more_data() {
        let sent = Arc::new(AtomicUsize::new(0));
        let mut cancel = Cancel::new(
            Box::new(BusyOnce(Arc::clone(&sent))),
            Push::new(2, 1, 0, vec![7; 100]),
        );
        assert!(cancel.step().expect("first attempt").is_none());
        assert!(!cancel.queued);
        assert!(cancel.step().expect("retry").is_none());
        assert!(cancel.queued);
        cancel.step().expect("wait for acknowledgement");
        assert_eq!(sent.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn an_unconfirmed_cancellation_is_not_reported_as_success() {
        let mut cancel = Cancel::new(
            Box::new(BusyOnce(Arc::new(AtomicUsize::new(1)))),
            Push::new(2, 1, 0, vec![]),
        );
        cancel.started = Instant::now() - Duration::from_secs(4);
        assert!(matches!(cancel.step(), Err(Error::Link(_))));
    }

    #[test]
    fn the_controllers_abort_report_confirms_cancellation() {
        let mut link = super::super::tests_support::Fake::default();
        link.report(wire::PHASE_FAILED, 0x83, 100, 0);
        let mut cancel = Cancel::new(Box::new(link), Push::new(2, 1, 0, vec![7; 100]));
        assert_eq!(cancel.step(), Ok(Some("Upload cancelled.".into())));
    }

    #[test]
    fn a_completed_transfer_is_not_claimed_to_have_been_cancelled() {
        let mut link = super::super::tests_support::Fake::default();
        link.report(wire::PHASE_DONE, 0, 100, 100);
        let mut cancel = Cancel::new(Box::new(link), Push::new(2, 1, 0, vec![7; 100]));
        assert_eq!(
            cancel.step(),
            Ok(Some(
                "The upload finished before it could be cancelled.".into()
            )),
        );
    }
}
