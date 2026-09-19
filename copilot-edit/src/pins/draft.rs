// SPDX-License-Identifier: GPL-3.0-only
//! Pin drafts retain inactive fields and incomplete numeric text until an explicit save.

use copilot::gpio::{Comparison, Dir, Level, Pin, Pull, Threshold, What};
use copilot::scene::bind::{gauge_by_name, indicator_by_name};

use super::quoted;

/// The source or action assigned to a logical pin.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    /// An individual indicator, read on an input or commanded on an output.
    Indicator,
    /// A numeric gauge condition, available only on an output.
    Threshold,
    /// A button action, available only on an input.
    Button,
}

/// Editable pin fields, including inactive choices preserved across source changes.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Draft {
    /// Logical pin number, starting at one.
    pub pin: u8,
    /// Optional human-readable wire label.
    pub label: String,
    /// Whether the display reads or commands the pin.
    pub dir: Dir,
    /// The asserted electrical level for an input.
    pub active: Level,
    /// The board's bias for an undriven input.
    pub pull: Pull,
    /// Which retained fields are used when saving.
    pub kind: Kind,
    /// The indicator's canonical name.
    pub indicator: String,
    /// A button action with its own retained draft fields.
    pub action: crate::touch_ui::Draft,
    /// The numeric gauge's canonical name.
    pub gauge: String,
    /// The comparison applied to the numeric reading.
    pub comparison: Comparison,
    /// Raw input, preserving incomplete numbers without silently substituting a value.
    pub value: String,
}

impl Draft {
    /// Start an unmapped pin; the threshold example is inactive until selected.
    pub(crate) fn fresh(pin: u8) -> Self {
        Self {
            pin,
            label: String::new(),
            dir: Dir::In,
            active: Level::High,
            pull: Pull::None,
            kind: Kind::Indicator,
            indicator: String::new(),
            action: crate::touch_ui::Draft::default(),
            gauge: "RPM".into(),
            comparison: Comparison::Above,
            value: "5000".into(),
        }
    }

    /// Open a saved mapping without changing its source or condition.
    pub(crate) fn of(pin: &Pin) -> Self {
        let mut draft = Self::fresh(pin.pin);
        draft.label = pin.label.clone().unwrap_or_default();
        draft.dir = pin.dir;
        draft.active = pin.active;
        draft.pull = pin.pull;
        match &pin.what {
            What::Lamp(indicator) => draft.indicator = indicator.name().into(),
            What::Press(action) => {
                draft.kind = Kind::Button;
                draft.action = crate::touch_ui::Draft::of(action);
            }
            What::Threshold(threshold) => {
                draft.kind = Kind::Threshold;
                draft.gauge = threshold.gauge.name.into();
                draft.comparison = threshold.comparison;
                draft.value = threshold.value.to_string();
            }
        }
        draft
    }

    /// Choose a source and its required direction without losing inactive fields.
    pub(crate) fn set_kind(&mut self, kind: Kind) {
        self.kind = kind;
        match kind {
            Kind::Indicator => {}
            Kind::Threshold => self.dir = Dir::Out,
            Kind::Button => self.dir = Dir::In,
        }
    }

    /// Change direction, returning incompatible sources to the indicator choice.
    pub(crate) fn set_dir(&mut self, dir: Dir) {
        self.dir = dir;
        if matches!(
            (dir, self.kind),
            (Dir::In, Kind::Threshold) | (Dir::Out, Kind::Button)
        ) {
            self.kind = Kind::Indicator;
        }
    }

    /// Resolve a numeric condition, refusing bank gauges and incomplete or non-finite values.
    pub(crate) fn threshold(&self) -> Result<Threshold, &'static str> {
        if self.dir != Dir::Out {
            return Err("Value thresholds require an output.");
        }
        let gauge =
            gauge_by_name(&self.gauge).ok_or("Choose a known numeric gauge before saving.")?;
        if matches!(gauge.name, "INDICATORS1" | "INDICATORS2") {
            return Err(
                "Indicator banks are not numeric sources. Choose an individual Indicator instead.",
            );
        }
        let value = self
            .value
            .trim()
            .parse::<f32>()
            .map_err(|_| "Enter a complete finite threshold number before saving.")?;
        if !value.is_finite() {
            return Err("The threshold must be finite; NaN and infinity cannot be saved.");
        }
        Ok(Threshold {
            gauge,
            comparison: self.comparison,
            value,
        })
    }

    /// Explain why a draft cannot yet be saved, before any file is touched.
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        match self.kind {
            Kind::Indicator => indicator_by_name(&self.indicator)
                .map(|_| ())
                .ok_or("Choose a known indicator before saving."),
            Kind::Threshold => self.threshold().map(|_| ()),
            Kind::Button if self.dir != Dir::In => Err("Button actions require an input."),
            Kind::Button if self.action.complete_for_pin() => Ok(()),
            Kind::Button => Err(match self.action.action() {
                "page" if self.action.pager().trim().is_empty() => {
                    "Enter the pager's name before saving."
                }
                "page" => {
                    "Choose a page destination: enter a non-empty name or a valid zero-based index."
                }
                "menu" => "Enter the rig menu's name before saving.",
                "mode" => "Enter the rig mode's name before saving.",
                _ => "Enter the host event name before saving.",
            }),
        }
    }

    /// Whether all fields required by the selected source are valid.
    pub(crate) fn complete(&self) -> bool {
        self.validate().is_ok()
    }

    /// Compare editable saved fields without losing incomplete threshold text.
    pub(crate) fn same_mapping(&self, other: &Self) -> bool {
        self.pin == other.pin
            && self.label.trim() == other.label.trim()
            && self.dir == other.dir
            && self.kind == other.kind
            && (self.dir == Dir::Out || (self.active == other.active && self.pull == other.pull))
            && match self.kind {
                Kind::Indicator => self.indicator == other.indicator,
                Kind::Threshold => {
                    self.gauge == other.gauge
                        && self.comparison == other.comparison
                        && self.value == other.value
                }
                Kind::Button => self.action.to_pin_fields() == other.action.to_pin_fields(),
            }
    }

    /// The canonical rig entry, containing only fields for the selected source.
    pub(crate) fn to_rig(&self) -> Result<String, &'static str> {
        self.validate()?;
        let mut out = format!(r#"{{ "pin": {}"#, self.pin);
        if !self.label.trim().is_empty() {
            out.push_str(&format!(r#", "label": {}"#, quoted(self.label.trim())));
        }
        if self.dir != Dir::In {
            out.push_str(&format!(r#", "dir": "{}""#, self.dir.name()));
        }
        if self.dir == Dir::In {
            if self.active != Level::High {
                out.push_str(&format!(r#", "active": "{}""#, self.active.name()));
            }
            if self.pull != Pull::None {
                out.push_str(&format!(r#", "pull": "{}""#, self.pull.name()));
            }
        }
        match self.kind {
            Kind::Indicator => {
                out.push_str(&format!(r#", "indicator": {}"#, quoted(&self.indicator)))
            }
            Kind::Button => out.push_str(&self.action.to_pin_fields()),
            Kind::Threshold => {
                let threshold = self.threshold()?;
                out.push_str(&format!(
                    r#", "gauge": {}, "{}": {}"#,
                    quoted(threshold.gauge.name),
                    threshold.comparison.name(),
                    threshold.value
                ));
            }
        }
        out.push_str(" }");
        Ok(out)
    }

    /// A reviewable description of the source and logical output command.
    pub(crate) fn summary(&self) -> String {
        match self.kind {
            Kind::Threshold => match self.threshold() {
                Ok(threshold) => format!(
                    "{} {}{} {} → HIGH command, otherwise LOW command",
                    threshold.gauge.name,
                    comparison_symbol(threshold.comparison),
                    threshold.value,
                    threshold.gauge.unit.symbol
                ),
                Err(reason) => format!("Value threshold: {reason}"),
            },
            Kind::Indicator => format!("Indicator {}", self.indicator),
            Kind::Button => format!("Button action: {}", self.action.action()),
        }
    }
}

fn comparison_symbol(comparison: Comparison) -> &'static str {
    match comparison {
        Comparison::Above => ">",
        Comparison::AtLeast => ">=",
        Comparison::Below => "<",
        Comparison::AtMost => "<=",
    }
}
