// SPDX-License-Identifier: GPL-3.0-only
//! Cancellable background work whose results never hold up an editor frame.

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use super::Error;

pub(crate) struct Job<T> {
    receiver: Receiver<Result<T, Error>>,
    cancelled: Arc<AtomicBool>,
    finished: Cell<bool>,
}

impl<T: Send + 'static> Job<T> {
    pub fn start(
        name: &str,
        work: impl FnOnce(Arc<AtomicBool>) -> Result<T, Error> + Send + 'static,
    ) -> Result<Self, Error> {
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&cancelled);
        std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let result = work(Arc::clone(&stopping));
                // Dropping an abandoned result also closes any connection it owns.
                if !stopping.load(Ordering::Acquire) {
                    let _ = sender.send(result);
                }
            })
            .map_err(|e| Error::Link(format!("Could not start {name}: {e}")))?;
        Ok(Self {
            receiver,
            cancelled,
            finished: Cell::new(false),
        })
    }

    pub fn poll(&self) -> Option<Result<T, Error>> {
        let result = match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(Error::Link(
                "The background task stopped unexpectedly. Please try again.".into(),
            ))),
        };
        if result.is_some() {
            self.finished.set(true);
        }
        result
    }
}

impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        if !self.finished.get() {
            self.cancelled.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn an_unfinished_job_can_be_polled_without_waiting() {
        let (release, wait) = mpsc::channel();
        let job = Job::<i32>::start("pending-test", move |_| {
            wait.recv_timeout(Duration::from_secs(2)).expect("released");
            Ok(7)
        })
        .expect("worker");
        assert!(job.poll().is_none());
        release.send(()).expect("release worker");
        assert_eq!(job.receiver.recv_timeout(Duration::from_secs(2)), Ok(Ok(7)));
    }

    #[test]
    fn dropping_a_job_cancels_it_and_disposes_of_a_late_connection() {
        struct Connection(mpsc::Sender<()>);
        impl Drop for Connection {
            fn drop(&mut self) {
                self.0.send(()).expect("test is waiting for drop");
            }
        }
        let (release, wait) = mpsc::channel();
        let (dropped, disposed) = mpsc::channel();
        let (observed, cancelled) = mpsc::channel();
        let job = Job::<Connection>::start("cancel-test", move |stop| {
            wait.recv_timeout(Duration::from_secs(2)).expect("released");
            observed
                .send(stop.load(Ordering::Acquire))
                .expect("observer");
            Ok(Connection(dropped))
        })
        .expect("worker");
        drop(job);
        release.send(()).expect("release worker");
        assert!(
            cancelled
                .recv_timeout(Duration::from_secs(2))
                .expect("cancellation")
        );
        disposed
            .recv_timeout(Duration::from_secs(2))
            .expect("connection dropped");
    }

    #[test]
    fn a_worker_failure_is_returned_instead_of_becoming_an_empty_result() {
        let job = Job::<()>::start("failure-test", |_| {
            Err(Error::NotFound("No Bluetooth adapter".into()))
        })
        .expect("worker");
        assert_eq!(
            job.receiver.recv_timeout(Duration::from_secs(2)),
            Ok(Err(Error::NotFound("No Bluetooth adapter".into())))
        );
    }

    #[test]
    fn a_lost_worker_is_an_error() {
        let (sender, receiver) = mpsc::channel::<Result<(), Error>>();
        drop(sender);
        let job: Job<()> = Job {
            receiver,
            cancelled: Arc::new(AtomicBool::new(false)),
            finished: Cell::new(false),
        };
        assert!(matches!(job.poll(), Some(Err(Error::Link(_)))));
    }

    #[test]
    fn taking_a_completed_connection_does_not_cancel_its_radio_worker() {
        let (sender, receiver) = mpsc::channel();
        sender.send(Ok(())).expect("result");
        let cancelled = Arc::new(AtomicBool::new(false));
        let job: Job<()> = Job {
            receiver,
            cancelled: Arc::clone(&cancelled),
            finished: Cell::new(false),
        };
        assert_eq!(job.poll(), Some(Ok(())));
        drop(job);
        assert!(!cancelled.load(Ordering::Acquire));
    }
}
