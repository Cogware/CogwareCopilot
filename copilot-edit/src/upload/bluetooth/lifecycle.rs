// SPDX-License-Identifier: GPL-3.0-only
//! Bounded Bluetooth operations whose cleanup must outlive cancellation.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::{Error, IDLE, IO_TIMEOUT};

const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

/// Both the request and the owning transport may stop a radio worker.
pub(super) struct Cancellation<'a> {
    cancelled: &'a AtomicBool,
    stopped: &'a AtomicBool,
}

impl<'a> Cancellation<'a> {
    /// Observe cancellation without transferring ownership of either flag.
    pub(super) fn new(cancelled: &'a AtomicBool, stopped: &'a AtomicBool) -> Self {
        Self { cancelled, stopped }
    }

    /// Reject work that no longer has a waiting caller.
    pub(super) fn check(&self) -> Result<(), Error> {
        if self.cancelled.load(Ordering::Relaxed) || self.stopped.load(Ordering::Relaxed) {
            Err(Error::Link("Bluetooth operation cancelled".into()))
        } else {
            Ok(())
        }
    }

    /// After handoff, dropping the setup job must not close its returned link.
    pub(super) fn transport(&self) -> Cancellation<'_> {
        Cancellation::new(self.stopped, self.stopped)
    }

    /// Bound an operation by both its deadline and the owner's lifetime.
    pub(super) async fn run<T>(
        &self,
        label: &str,
        timeout: Duration,
        operation: impl Future<Output = Result<T, Error>>,
    ) -> Result<T, Error> {
        tokio::select! {
            biased;
            error = async {
                loop {
                    if let Err(error) = self.check() {
                        return error;
                    }
                    tokio::time::sleep(IDLE).await;
                }
            } => Err(error),
            result = tokio::time::timeout(timeout, operation) => {
                result.map_err(|_| Error::Link(format!("{label} timed out")))?
            }
        }
    }

    /// Apply the shared radio deadline and preserve the failed operation's name.
    pub(super) async fn bluetooth<T>(
        &self,
        label: &str,
        operation: impl Future<Output = btleplug::Result<T>>,
    ) -> Result<T, Error> {
        self.run(label, IO_TIMEOUT, async {
            operation
                .await
                .map_err(|e| Error::Link(format!("{label}: {e}")))
        })
        .await
    }
}

/// Keep platform handles on the worker that owns their async runtime.
pub(super) fn runtime() -> Result<tokio::runtime::Runtime, Error> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Link(format!("no Bluetooth runtime: {e}")))
}

/// Always attempt cleanup, preserving both the original and cleanup failures.
pub(super) async fn finish<T, E: std::fmt::Display>(
    result: Result<T, Error>,
    label: &str,
    cleanup: impl Future<Output = Result<(), E>>,
) -> Result<T, Error> {
    // Cancelling cleanup would strand the OS-side scan or connection.
    let cleanup = match tokio::time::timeout(CLEANUP_TIMEOUT, cleanup).await {
        Ok(result) => result.map_err(|e| Error::Link(format!("{label}: {e}"))),
        Err(_) => Err(Error::Link(format!("{label} timed out"))),
    };
    match (result, cleanup) {
        (result, Ok(())) => result,
        (Ok(_), Err(error)) => Err(error),
        (Err(original), Err(cleanup)) => Err(Error::Link(format!("{original}; {cleanup}"))),
    }
}
