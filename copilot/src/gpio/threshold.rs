// SPDX-License-Identifier: GPL-3.0-only
//! Numeric output rules, evaluated in the gauge table's canonical units.

use alloc::string::ToString;
use cogware_can::GaugeData;

use super::{Dir, PinError};
use crate::scene::Value;
use crate::scene::bind::gauge_by_name;

/// The comparison that commands a numeric GPIO output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    /// Strictly greater than the threshold.
    Above,
    /// Greater than or equal to the threshold.
    AtLeast,
    /// Strictly less than the threshold.
    Below,
    /// Less than or equal to the threshold.
    AtMost,
}

impl Comparison {
    /// Parse a comparison field name from a rig.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "above" => Some(Self::Above),
            "at_least" => Some(Self::AtLeast),
            "below" => Some(Self::Below),
            "at_most" => Some(Self::AtMost),
            _ => None,
        }
    }

    /// The comparison field name used in a rig.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Above => "above",
            Self::AtLeast => "at_least",
            Self::Below => "below",
            Self::AtMost => "at_most",
        }
    }
}

/// A numeric gauge rule that commands an output when its comparison matches.
/// The board applies output polarity; this rule only produces a logical command.
///
/// ```
/// use copilot::gpio::{Comparison, Threshold};
///
/// let shift = Threshold {
///     gauge: &copilot::cogware_can::RPM,
///     comparison: Comparison::Above,
///     value: 5000.0,
/// };
/// assert!(!shift.matches(5000.0));
/// assert!(shift.matches(5001.0));
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Threshold {
    /// The source gauge; indicator banks are not numeric sources.
    pub gauge: &'static GaugeData,
    /// How the canonical reading is compared with [`Self::value`].
    pub comparison: Comparison,
    /// The threshold in the gauge's canonical unit, not fixed-point counts.
    pub value: f32,
}

impl Threshold {
    /// Compare a reading in the gauge's canonical unit.
    /// A non-finite reading or threshold never commands an output.
    #[must_use]
    pub fn matches(&self, reading: f32) -> bool {
        if !reading.is_finite() || !self.value.is_finite() {
            return false;
        }
        match self.comparison {
            Comparison::Above => reading > self.value,
            Comparison::AtLeast => reading >= self.value,
            Comparison::Below => reading < self.value,
            Comparison::AtMost => reading <= self.value,
        }
    }
}

pub(super) fn parse(val: &Value, pin: u8, dir: Dir) -> Result<Option<Threshold>, PinError> {
    let Value::Object(fields) = val else {
        return Ok(None);
    };
    let mut comparisons = fields
        .iter()
        .filter_map(|(name, value)| Comparison::parse(name).map(|comparison| (comparison, value)));
    let first = comparisons.next();
    let Some(source) = val.get("gauge") else {
        return if first.is_some() {
            Err(PinError::MissingGauge(pin))
        } else {
            Ok(None)
        };
    };
    if val.get("indicator").is_some() || val.get("action").is_some() {
        return Err(PinError::NotWired(pin));
    }
    let name = source
        .as_str()
        .ok_or(PinError::BadField { field: "gauge" })?;
    if fields.iter().filter(|(key, _)| key == "gauge").count() != 1 {
        return Err(PinError::BadField { field: "gauge" });
    }
    if dir != Dir::Out {
        return Err(PinError::InputGauge(pin));
    }
    let gauge = gauge_by_name(name).ok_or_else(|| PinError::UnknownGauge(name.to_string()))?;
    if gauge == &cogware_can::INDICATORS1 || gauge == &cogware_can::INDICATORS2 {
        return Err(PinError::NotNumericGauge(name.to_string()));
    }
    u8::try_from(gauge.id).map_err(|_| PinError::BadGaugeId(gauge.id))?;
    let (comparison, value) = first.ok_or(PinError::BadComparison(pin))?;
    if comparisons.next().is_some() {
        return Err(PinError::BadComparison(pin));
    }
    let value = value
        .as_f64()
        .map(|value| value as f32)
        .filter(|value| value.is_finite())
        .ok_or(PinError::BadField {
            field: comparison.name(),
        })?;
    Ok(Some(Threshold {
        gauge,
        comparison,
        value,
    }))
}
