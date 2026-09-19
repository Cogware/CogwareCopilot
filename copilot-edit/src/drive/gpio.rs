// SPDX-License-Identifier: GPL-3.0-only
//! Include rig-only GPIO sources in preview controls even without a matching widget.

use copilot::gpio::{Pins, What};
use std::collections::BTreeMap;

pub(super) fn gauges(seen: &mut BTreeMap<&'static str, (f32, f32, f32)>, pins: &[&Pins]) {
    for pin in pins.iter().flat_map(|pins| pins.mapped()) {
        if let What::Threshold(threshold) = &pin.what {
            let margin = (threshold.value.abs() * 0.25).max(1.0);
            let lo = (threshold.value - margin).clamp(f32::MIN, 0.0);
            let hi = (threshold.value + margin).clamp(0.0, f32::MAX);
            let entry = seen.entry(threshold.gauge.name).or_insert((0.0, lo, hi));
            entry.1 = entry.1.min(lo);
            entry.2 = entry.2.max(hi);
        }
    }
}

pub(super) fn lamps(seen: &mut BTreeMap<&'static str, bool>, pins: &[&Pins]) {
    for pin in pins.iter().flat_map(|pins| pins.mapped()) {
        if let What::Lamp(indicator) = pin.what {
            seen.entry(indicator.name()).or_insert(false);
        }
    }
}

impl crate::App {
    pub(crate) fn sync_driver(&mut self) {
        let Some(tree) = self.preview.tree.as_ref() else {
            return;
        };
        let others: Vec<_> = self
            .rig
            .iter()
            .flat_map(|rig| &rig.docs)
            .filter_map(|doc| Some((doc.preview.tree.as_ref()?, doc.preview.bindings.as_slice())))
            .collect();
        let pins: Vec<_> = self
            .rig
            .iter()
            .flat_map(|rig| &rig.rig.displays)
            .map(|display| &display.pins)
            .collect();
        self.driver
            .sync(tree, &self.preview.bindings, &others, &pins);
    }
}

#[cfg(test)]
mod tests;
