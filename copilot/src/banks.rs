// SPDX-License-Identifier: GPL-3.0-only
//! A lock over the tell-tale banks, for tests only.
//!
//! Every other test in this crate that touches the gauge table gets a gauge to
//! itself, so the table being global is no trouble. The tell-tales are the
//! exception: all sixty-four live in the same two words, [`bind`] reads them
//! through a binding and [`gpio`] drives a pin from them, and the harness runs
//! the lot in parallel. One guard rather than one per module, because two
//! locks over one pair of words is not a lock.
//!
//! A spin lock rather than anything from `std`, because this is a `no_std`
//! crate and the contention is a dozen tests deep.
//!
//! [`bind`]: crate::scene::bind
//! [`gpio`]: crate::gpio

use core::hint::spin_loop;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering::{Acquire, Relaxed, Release};

use cogware_can::indicators::{IndicatorSet, apply};

static HELD: AtomicBool = AtomicBool::new(false);

/// The banks, held for as long as this is alive and every lamp put out when
/// it is taken and when it is dropped.
pub(crate) struct Banks;

impl Banks {
    /// Wait for the banks, then clear them.
    pub(crate) fn take() -> Self {
        while HELD
            .compare_exchange_weak(false, true, Acquire, Relaxed)
            .is_err()
        {
            spin_loop();
        }
        apply(IndicatorSet::new());
        Banks
    }

    /// Light `lamps` and nothing else, while the banks are held.
    pub(crate) fn set(&self, lamps: &[cogware_can::indicators::Indicator]) {
        let mut set = IndicatorSet::new();
        for l in lamps {
            set.set(*l, true);
        }
        apply(set);
    }
}

impl Drop for Banks {
    fn drop(&mut self) {
        // Put them out on the way out as well as on the way in: a test that
        // panicked holding them must not leave a lamp lit for the next one.
        apply(IndicatorSet::new());
        HELD.store(false, Release);
    }
}
