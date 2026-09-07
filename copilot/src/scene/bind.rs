// SPDX-License-Identifier: MIT OR Apache-2.0
//! Binding a widget to a gauge.
//!
//! A scene can say what a widget *shows* -- `"bind": { "gauge": "RPM", "max":
//! 8000 }` -- and not only what it looks like at one authored value. The gauge
//! is resolved here, at build time, against the table in [`cogware_can`], so a
//! name the bus has never heard of fails on the desk rather than drawing a
//! blank in the car.
//!
//! # What a binding does at run time
//!
//! Nothing, on its own. The host reads the bus into the gauge table
//! ([`cogware_can::feed_frame`]) and then asks each binding to
//! [`apply`](Binding::apply) itself. One reading goes through four steps, in
//! this order:
//!
//! 1. **unit** -- converted from the gauge's own unit to the one the scene
//!    asked for, if it asked for one. kPa to psi, °C to °F.
//! 2. **transform** -- `reading / divide + offset`. A tachometer face printed
//!    `x100 r/min` shows hundreds; a boost gauge reads zero at atmospheric
//!    rather than 14.7. Neither is something the bus should carry, and
//!    neither should be a reason to write code.
//! 3. **range or format** -- mapped onto the widget's 0..1 fraction over
//!    `min..max`, or formatted to `decimals` places and padded to `pad`.
//! 4. **written** through the tree's setters, so only a reading that actually
//!    changed marks damage.
//!
//! [`Scene::wanted`] is the list of gauge ids the display should subscribe
//! to, derived from the same bindings, so a scene cannot show a gauge it
//! forgot to ask for.
//!
//! # Bindings and animations
//!
//! Both may sit on one widget. A host applies bindings *after* ticking the
//! animator, so live data wins where there is a source and the authored
//! animation runs where there is none: the same file demonstrates itself in
//! the simulator and reads the engine in the car.
//!
//! # Why `max` is required for a reading
//!
//! A bar bound to RPM with no range would be pinned full from one rev per
//! minute, which looks like a rendering bug and is a scene-file one. Refusing
//! the file is the kinder failure. Text has no range to be wrong about, so
//! there `min` and `max` only describe the span a dummy-value sweep should
//! cover, and default to 0..100.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use cogware_can::{ALL_GAUGES, GaugeData, Quantity, Unit};

use crate::asset::Scene;
pub use crate::widget::BindTarget;
use crate::widget::{Kind, NodeId, Tree};

use super::build::BuildError;
use super::value::Value;

/// The outline of the panel a scene is drawn on.
///
/// Metadata rather than geometry: the compositor still paints the whole
/// rectangle, because a round panel's driver clips for it. What the shape
/// changes is how a preview masks the corners and how a host describes the
/// display.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    /// The whole `width` by `height` box is visible.
    #[default]
    Rect,
    /// The inscribed circle is visible; the corners are behind the bezel.
    Round,
}

impl Shape {
    /// Parse the name a scene file uses.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "rect" => Some(Self::Rect),
            "round" => Some(Self::Round),
            _ => None,
        }
    }

    /// The name a scene file uses.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Rect => "rect",
            Self::Round => "round",
        }
    }
}

/// One widget bound to one gauge.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    /// The widget this drives.
    pub node: NodeId,
    /// The gauge it reads, straight from the spec's table.
    pub gauge: &'static GaugeData,
    /// The unit the reading is shown in. The gauge's own unless the scene
    /// asked for another of the same dimension.
    pub unit: Unit,
    /// What the reading is divided by before it is shown. Never zero.
    ///
    /// A tachometer whose face is printed `x100 r/min` shows hundreds, and a
    /// gauge that reads in thousands shows thousands; the bus carries neither.
    /// This is where a scene says so, rather than the display having to.
    pub divide: f32,
    /// Added after the division, in the units that leaves.
    ///
    /// What turns an absolute pressure into a gauge one: MAP in psi with an
    /// offset of -14.7 reads zero at atmospheric, which is what a boost gauge
    /// is for.
    pub offset: f32,
    /// The shown value that maps to a fraction of 0.0.
    pub min: f32,
    /// The shown value that maps to a fraction of 1.0.
    pub max: f32,
    /// Digits after the point when the reading is shown as text.
    pub decimals: u8,
    /// Least width of the text, space-padded on the left, so a readout's
    /// digits stay in their cells as the number shrinks. 0 for none.
    pub pad: u8,
    /// Whether the widget takes a fraction or a string.
    pub target: BindTarget,
}

impl Binding {
    /// Where a shown value sits between `min` and `max`, clamped.
    ///
    /// A collapsed range reads as empty rather than dividing by zero, and a
    /// NaN -- which a conversion cannot produce but a caller might -- does
    /// the same, so a gauge never draws garbage for a bad number.
    #[must_use]
    pub fn fraction(&self, shown: f32) -> f32 {
        let span = self.max - self.min;
        if span.abs() <= f32::EPSILON || shown.is_nan() {
            return 0.0;
        }
        ((shown - self.min) / span).clamp(0.0, 1.0)
    }

    /// A shown value formatted with this binding's decimals, padded to width.
    #[must_use]
    pub fn text(&self, shown: f32) -> String {
        alloc::format!(
            "{shown:>w$.p$}",
            w = usize::from(self.pad),
            p = usize::from(self.decimals)
        )
    }

    /// A reading in `unit` as the widget shows it: `reading / divide + offset`.
    ///
    /// The division comes first so that `offset` is in the units the reader
    /// sees. On a tachometer showing hundreds, an offset of 1 moves the
    /// needle by one hundred rpm, which is the number printed on the face.
    #[must_use]
    pub fn show(&self, reading: f32) -> f32 {
        reading / self.divide + self.offset
    }

    /// The inverse of [`show`](Self::show): what reading would be shown as
    /// `shown`.
    ///
    /// For an editor working backwards from what a widget draws to the
    /// reading that would put it there.
    #[must_use]
    pub fn unshow(&self, shown: f32) -> f32 {
        (shown - self.offset) * self.divide
    }

    /// The gauge's reading in the display unit, before the transform.
    ///
    /// The gauge's own unit goes through `as_f32` rather than a conversion
    /// so that a `RAW` gauge -- a gear number, a status byte -- reads as
    /// itself; `Unit::convert` refuses raw quantities on purpose.
    #[must_use]
    pub fn reading(&self) -> Option<f32> {
        if self.unit == self.gauge.unit {
            self.gauge.as_f32()
        } else {
            self.gauge.to(self.unit)
        }
    }

    /// What the widget should show now, or `None` if nothing has written the
    /// gauge yet.
    #[must_use]
    pub fn current(&self) -> Option<f32> {
        self.reading().map(|v| self.show(v))
    }

    /// Put `shown` -- a value already through [`show`](Self::show) -- on the
    /// widget, however it takes it.
    ///
    /// This is the half of [`apply`](Self::apply) that does not consult the
    /// bus, for an editor that wants to show a reading the engine is not
    /// producing. `None` if the node has gone or changed kind underneath.
    pub fn apply_value(&self, tree: &mut Tree, shown: f32) -> Option<()> {
        match self.target {
            BindTarget::Reading => tree.set_reading(self.node, self.fraction(shown)),
            BindTarget::Height => tree.set_height(self.node, self.fraction(shown)),
            BindTarget::Text => tree.set_text(self.node, &self.text(shown)),
        }
    }

    /// Show the gauge's current reading on the widget.
    ///
    /// Returns whether there was a reading to show. An unset gauge leaves the
    /// widget exactly as it was -- at its authored value, or wherever an
    /// animation put it -- which is what lets a scene without a source still
    /// demonstrate itself.
    pub fn apply(&self, tree: &mut Tree) -> bool {
        self.current()
            .and_then(|v| self.apply_value(tree, v))
            .is_some()
    }
}

impl Scene {
    /// The gauge ids this scene shows, lowest first and without repeats: the
    /// list a display hands to [`cogware_can::subscribe::Subscription::new`].
    ///
    /// Gauge ids are asserted to fit a byte where the table is defined, so
    /// the conversion here cannot drop one; the `filter_map` is belt and
    /// braces against a table that someday relaxes that.
    #[must_use]
    pub fn wanted(&self) -> Vec<u8> {
        let mut ids: Vec<u8> = self
            .bindings
            .iter()
            .filter_map(|b| u8::try_from(b.gauge.id).ok())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    /// Show every bound gauge's current reading on its widget.
    ///
    /// Returns how many bindings had a reading to show, so a display can tell
    /// "the ECU is silent" from "everything reads zero" and say so.
    pub fn apply_gauges(&mut self) -> usize {
        let tree = &mut self.tree;
        self.bindings.iter().filter(|b| b.apply(tree)).count()
    }
}

/// Look a gauge up by the name the spec's table gives it, exactly.
///
/// Exact rather than case-folded because the table's names are the ones a
/// scene author sees in the editor's list and in the bus logs; accepting
/// `rpm` for `RPM` would put two spellings of every gauge into the world.
#[must_use]
pub fn gauge_by_name(name: &str) -> Option<&'static GaugeData> {
    ALL_GAUGES.iter().copied().find(|g| g.name == name)
}

/// Every unit the spec defines, canonical ones first.
///
/// Listed here rather than derived because `cogware_can` exposes them as
/// constants, not as a table. If the spec grows a unit, it goes here too, and
/// the test at the bottom of this file notices when the two disagree in the
/// one way it can check.
const UNITS: &[Unit] = &[
    Unit::RAW,
    Unit::SECONDS,
    Unit::HZ,
    Unit::RPM,
    Unit::RPM_PER_S,
    Unit::CELSIUS,
    Unit::KPA,
    Unit::KPA_PER_S,
    Unit::PERCENT,
    Unit::PERCENT_PER_S,
    Unit::VOLT,
    Unit::AFR,
    Unit::DEGREES,
    Unit::MS,
    Unit::KMH,
    Unit::BYTES,
    Unit::LITRES,
    Unit::FAHRENHEIT,
    Unit::KELVIN,
    Unit::PSI,
    Unit::BAR,
    Unit::INHG,
    Unit::MPH,
    Unit::LAMBDA,
    Unit::MILLIVOLT,
    Unit::MICROSECONDS,
    Unit::MINUTES,
    Unit::GALLONS_US,
    Unit::GALLONS_UK,
];

/// Look a unit up by the symbol the spec prints for it, with the spellings a
/// person types on a plain keyboard accepted as well: `F` for `°F`, `lambda`
/// for `λ`, `us` for `µs`, `kph` for `km/h`, `deg` for `°`.
#[must_use]
pub fn unit_by_symbol(symbol: &str) -> Option<Unit> {
    let canonical = match symbol {
        "C" => "°C",
        "F" => "°F",
        "deg" => "°",
        "kph" => "km/h",
        "lambda" => "λ",
        "us" => "µs",
        other => other,
    };
    UNITS.iter().copied().find(|u| u.symbol == canonical)
}

/// Every unit of one physical dimension, canonical first: what a unit picker
/// should offer for a gauge of that dimension.
pub fn units_for(quantity: Quantity) -> impl Iterator<Item = Unit> {
    UNITS
        .iter()
        .copied()
        .filter(move |u| u.quantity == quantity)
}

/// Read a widget's `bind`: one block, or an array of them.
///
/// An array because a widget can have more than one axis and each needs its
/// own gauge, range and unit: a bank whose columns light with the revs and
/// stand as tall as the boost is two readings on one widget, and nothing
/// less than two blocks can say so.
pub(super) fn bindings(
    node: NodeId,
    kind: &Kind,
    spec: &Value,
    out: &mut Vec<Binding>,
) -> Result<(), BuildError> {
    match spec {
        Value::Array(items) => {
            for item in items {
                out.push(binding(node, kind, item)?);
            }
        }
        _ => out.push(binding(node, kind, spec)?),
    }
    Ok(())
}

/// Read one `bind` block for the widget of `kind` at `node`.
fn binding(node: NodeId, kind: &Kind, spec: &Value) -> Result<Binding, BuildError> {
    // Which axis this block drives. Unstated is the kind's own: a bar's fill,
    // a label's text. Named, it has to be an axis the kind actually has, or
    // the scene is asking a bar to show something it cannot.
    let target = match spec.get("property") {
        None | Some(Value::Null) => kind
            .bind_target()
            .ok_or(BuildError::BadField { field: "bind" })?,
        Some(v) => v
            .as_str()
            .and_then(BindTarget::parse)
            .ok_or(BuildError::BadField { field: "property" })?,
    };
    if !kind.accepts(target) {
        return Err(BuildError::BadField { field: "property" });
    }

    let name = spec
        .get("gauge")
        .and_then(Value::as_str)
        .ok_or(BuildError::Missing { field: "gauge" })?;
    let gauge = gauge_by_name(name).ok_or_else(|| BuildError::UnknownGauge(name.to_string()))?;

    let unit = match spec.get("unit") {
        None | Some(Value::Null) => gauge.unit,
        Some(v) => {
            let s = v.as_str().ok_or(BuildError::BadField { field: "unit" })?;
            let unit = unit_by_symbol(s).ok_or_else(|| BuildError::UnknownUnit(s.to_string()))?;
            if unit.quantity != gauge.unit.quantity {
                return Err(BuildError::WrongUnit {
                    gauge: gauge.name,
                    unit: s.to_string(),
                });
            }
            unit
        }
    };

    let min = number(spec, "min")?.unwrap_or(0.0);
    let max = match (number(spec, "max")?, target) {
        (Some(m), _) => m,
        // Both axes map a reading onto a fraction, so both need to know what
        // the top of the scale is; only text can do without one.
        (None, BindTarget::Reading | BindTarget::Height) => {
            return Err(BuildError::Missing { field: "max" });
        }
        (None, BindTarget::Text) => 100.0,
    };

    // A divisor of zero is every reading becoming infinity, which draws as a
    // pinned gauge rather than as the mistake it is.
    let divide = number(spec, "divide")?.unwrap_or(1.0);
    if divide == 0.0 {
        return Err(BuildError::BadField { field: "divide" });
    }

    Ok(Binding {
        node,
        gauge,
        unit,
        divide,
        offset: number(spec, "offset")?.unwrap_or(0.0),
        min,
        max,
        decimals: small(spec, "decimals")?,
        pad: small(spec, "pad")?,
        target,
    })
}

/// Read an optional small count, such as a digit count, defaulting to zero.
fn small(val: &Value, field: &'static str) -> Result<u8, BuildError> {
    match val.get(field) {
        None | Some(Value::Null) => Ok(0),
        Some(v) => {
            let n = v.as_i64().ok_or(BuildError::BadField { field })?;
            u8::try_from(n).map_err(|_| BuildError::BadField { field })
        }
    }
}

/// Read an optional number, which may be written as an integer.
fn number(val: &Value, field: &'static str) -> Result<Option<f32>, BuildError> {
    match val.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let f = v.as_f64().ok_or(BuildError::BadField { field })? as f32;
            if f.is_nan() {
                return Err(BuildError::BadField { field });
            }
            Ok(Some(f))
        }
    }
}

/// Read a `node` address: an integer, or a string such as `"0x01"`.
///
/// The string form exists because JSON has no hex literals and node addresses
/// are thought about, printed and wired up in hex. `0x00` is the gateway and
/// `0xFF` is every node at once; a scene claiming either is a mistake worth
/// stopping on.
pub(crate) fn node_address(v: &Value) -> Result<u8, BuildError> {
    let field = "node";
    let n = match v {
        Value::Int(i) => *i,
        Value::Str(s) => {
            let digits = s
                .strip_prefix("0x")
                .or_else(|| s.strip_prefix("0X"))
                .ok_or(BuildError::BadField { field })?;
            i64::from_str_radix(digits, 16).map_err(|_| BuildError::BadField { field })?
        }
        _ => return Err(BuildError::BadField { field }),
    };
    let n = u8::try_from(n).map_err(|_| BuildError::BadField { field })?;
    if n == cogware_can::protocol::NODE_MASTER || n == cogware_can::protocol::NODE_BROADCAST {
        return Err(BuildError::ReservedNode(n));
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{build_scene, parse};
    use crate::widget::ROOT;
    use cogware_can::{CLNT, EGT1, MAP, RPM};

    fn scene(root: &str) -> Result<Scene, BuildError> {
        let src = alloc::format!(r#"{{"width":100,"height":50,"root":{root}}}"#);
        build_scene(&parse(&src).expect("must parse"))
    }

    fn top(s: &Scene) -> NodeId {
        s.tree.get(ROOT).unwrap().children[0]
    }

    // --- resolving ---

    #[test]
    fn a_reading_binds_in_the_gauges_own_unit() {
        let s = scene(
            r#"{"type":"bar","rect":[0,0,10,10],
                "bind":{"gauge":"RPM","max":8000}}"#,
        )
        .unwrap();
        assert_eq!(s.bindings.len(), 1);
        let b = &s.bindings[0];
        assert_eq!(b.node, top(&s));
        assert_eq!(b.gauge.name, "RPM");
        assert_eq!(b.unit, Unit::RPM);
        assert_eq!((b.min, b.max, b.decimals), (0.0, 8000.0, 0));
        assert_eq!(b.target, BindTarget::Reading);
    }

    #[test]
    fn text_binds_with_a_display_unit_and_decimals() {
        let s = scene(
            r#"{"type":"sevenseg","rect":[0,0,10,10],"text":"0",
                "bind":{"gauge":"CLNT","unit":"°F","decimals":1}}"#,
        )
        .unwrap();
        let b = &s.bindings[0];
        assert_eq!(b.unit, Unit::FAHRENHEIT);
        assert_eq!(b.decimals, 1);
        assert_eq!(b.target, BindTarget::Text);
        // No range is needed to show a number, so the sweep span defaults.
        assert_eq!((b.min, b.max), (0.0, 100.0));
    }

    #[test]
    fn keyboard_spellings_of_a_unit_are_accepted() {
        for (typed, want) in [
            ("F", Unit::FAHRENHEIT),
            ("C", Unit::CELSIUS),
            ("lambda", Unit::LAMBDA),
            ("kph", Unit::KMH),
            ("psi", Unit::PSI),
        ] {
            assert_eq!(unit_by_symbol(typed), Some(want), "{typed}");
        }
        assert_eq!(unit_by_symbol("furlongs"), None);
    }

    #[test]
    fn every_unit_the_spec_has_is_in_the_table() {
        // The gauges' own units are the one enumeration the spec exposes; if
        // one of them is not in UNITS the table has fallen behind.
        for g in ALL_GAUGES {
            assert!(
                UNITS.contains(&g.unit),
                "{} is stored in {:?}, which unit_by_symbol cannot name",
                g.name,
                g.unit
            );
        }
        assert!(units_for(Quantity::Pressure).count() >= 4);
    }

    #[test]
    fn bindings_on_children_are_collected_in_document_order() {
        let s = scene(
            r#"{"type":"panel","rect":[0,0,10,10],"children":[
                 {"type":"bar","rect":[0,0,5,5],"bind":{"gauge":"RPM","max":1}},
                 {"type":"led","rect":[5,0,5,5],"bind":{"gauge":"MAP","max":1}}]}"#,
        )
        .unwrap();
        let names: Vec<&str> = s.bindings.iter().map(|b| b.gauge.name).collect();
        assert_eq!(names, ["RPM", "MAP"]);
    }

    // --- rejecting ---

    #[test]
    fn a_reading_without_a_range_is_refused() {
        assert_eq!(
            scene(r#"{"type":"bar","rect":[0,0,10,10],"bind":{"gauge":"RPM"}}"#).unwrap_err(),
            BuildError::Missing { field: "max" }
        );
    }

    #[test]
    fn a_gauge_the_spec_does_not_have_is_refused_by_name() {
        assert_eq!(
            scene(r#"{"type":"bar","rect":[0,0,10,10],"bind":{"gauge":"RMP","max":1}}"#)
                .unwrap_err(),
            BuildError::UnknownGauge("RMP".into())
        );
        assert_eq!(
            scene(r#"{"type":"bar","rect":[0,0,10,10],"bind":{"max":1}}"#).unwrap_err(),
            BuildError::Missing { field: "gauge" }
        );
    }

    #[test]
    fn a_unit_of_the_wrong_dimension_is_refused() {
        assert_eq!(
            scene(
                r#"{"type":"bar","rect":[0,0,10,10],
                    "bind":{"gauge":"RPM","unit":"psi","max":1}}"#
            )
            .unwrap_err(),
            BuildError::WrongUnit {
                gauge: "RPM",
                unit: "psi".into()
            }
        );
        assert_eq!(
            scene(
                r#"{"type":"bar","rect":[0,0,10,10],
                    "bind":{"gauge":"RPM","unit":"furlongs","max":1}}"#
            )
            .unwrap_err(),
            BuildError::UnknownUnit("furlongs".into())
        );
    }

    #[test]
    fn a_widget_with_nothing_to_show_cannot_be_bound() {
        assert_eq!(
            scene(r#"{"type":"panel","rect":[0,0,10,10],"bind":{"gauge":"RPM","max":1}}"#)
                .unwrap_err(),
            BuildError::BadField { field: "bind" }
        );
    }

    // --- node and shape ---

    fn with_top(extra: &str) -> Result<Scene, BuildError> {
        let src = alloc::format!(
            r#"{{"width":10,"height":10,{extra}"root":{{"type":"panel","rect":[0,0,1,1]}}}}"#
        );
        build_scene(&parse(&src).expect("must parse"))
    }

    #[test]
    fn a_node_address_is_read_as_a_number_or_as_hex() {
        assert_eq!(with_top(r#""node":1,"#).unwrap().node, Some(1));
        assert_eq!(with_top(r#""node":"0x01","#).unwrap().node, Some(1));
        assert_eq!(with_top(r#""node":"0X2a","#).unwrap().node, Some(42));
        assert_eq!(with_top("").unwrap().node, None);
    }

    #[test]
    fn the_reserved_node_addresses_are_refused() {
        assert_eq!(
            with_top(r#""node":0,"#).unwrap_err(),
            BuildError::ReservedNode(0)
        );
        assert_eq!(
            with_top(r#""node":"0xff","#).unwrap_err(),
            BuildError::ReservedNode(0xff)
        );
    }

    #[test]
    fn a_malformed_node_address_is_refused() {
        for bad in [
            r#""node":"0x1G","#,
            r#""node":"7","#,
            r#""node":300,"#,
            r#""node":true,"#,
        ] {
            assert_eq!(
                with_top(bad).unwrap_err(),
                BuildError::BadField { field: "node" },
                "{bad}"
            );
        }
    }

    #[test]
    fn a_shape_is_read_and_defaults_to_a_rectangle() {
        assert_eq!(with_top(r#""shape":"round","#).unwrap().shape, Shape::Round);
        assert_eq!(with_top("").unwrap().shape, Shape::Rect);
        assert_eq!(
            with_top(r#""shape":"hexagon","#).unwrap_err(),
            BuildError::BadField { field: "shape" }
        );
        assert_eq!(Shape::parse(Shape::Round.name()), Some(Shape::Round));
    }

    // --- at run time ---

    #[test]
    fn the_wanted_list_is_sorted_and_has_no_repeats() {
        let s = scene(
            r#"{"type":"panel","rect":[0,0,10,10],"children":[
                 {"type":"bar","rect":[0,0,5,5],"bind":{"gauge":"RPM","max":1}},
                 {"type":"bar","rect":[0,0,5,5],"bind":{"gauge":"MAP","max":1}},
                 {"type":"led","rect":[5,0,5,5],"bind":{"gauge":"RPM","max":1}}]}"#,
        )
        .unwrap();
        assert_eq!(s.wanted(), alloc::vec![0x24, 0x2D]);
    }

    #[test]
    fn a_fraction_is_clamped_and_a_collapsed_range_reads_empty() {
        let s = scene(
            r#"{"type":"bar","rect":[0,0,10,10],"bind":{"gauge":"RPM","min":1000,"max":5000}}"#,
        )
        .unwrap();
        let b = &s.bindings[0];
        assert_eq!(b.fraction(3000.0), 0.5);
        assert_eq!(b.fraction(0.0), 0.0);
        assert_eq!(b.fraction(9000.0), 1.0);
        assert_eq!(b.fraction(f32::NAN), 0.0);
        let flat = Binding {
            min: 3.0,
            max: 3.0,
            ..b.clone()
        };
        assert_eq!(flat.fraction(3.0), 0.0);
        assert_eq!(b.text(1234.567), "1235");
        assert_eq!(
            Binding {
                decimals: 2,
                ..b.clone()
            }
            .text(1234.567),
            "1234.57"
        );
        // Padded on the left, so a three-cell speedo reads "  7" and not "7  ".
        assert_eq!(
            Binding {
                pad: 3,
                ..b.clone()
            }
            .text(7.0),
            "  7"
        );
        assert_eq!(
            Binding {
                pad: 4,
                decimals: 1,
                ..b.clone()
            }
            .text(5.3),
            " 5.3"
        );
    }

    // The gauge table is one per process and these tests share it, so each
    // one below owns a gauge nothing else here writes to.

    #[test]
    fn a_reading_is_converted_and_mapped_onto_the_widget() {
        let mut s = scene(
            r#"{"type":"needle","rect":[0,0,10,10],"value":0.1,
                "bind":{"gauge":"MAP","unit":"psi","min":0,"max":30}}"#,
        )
        .unwrap();
        MAP.set(1450); // 145.0 kPa, which is 21.03 psi
        assert_eq!(s.apply_gauges(), 1);
        let got = s.tree.get(top(&s)).unwrap().kind.reading().unwrap();
        assert!((got - 21.03 / 30.0).abs() < 1e-3, "got {got}");
    }

    #[test]
    fn text_is_converted_and_formatted() {
        let mut s = scene(
            r#"{"type":"label","rect":[0,0,10,10],"text":"--",
                "bind":{"gauge":"CLNT","unit":"F"}}"#,
        )
        .unwrap();
        CLNT.set(870); // 87.0 °C is 188.6 °F
        assert_eq!(s.apply_gauges(), 1);
        assert_eq!(s.tree.get(top(&s)).unwrap().kind.text(), Some("189"));
    }

    #[test]
    fn a_divisor_puts_a_reading_in_the_units_the_face_is_printed_in() {
        // The tachometer digits beside "x100r/min": the bus carries 3500 rpm
        // and the readout has to say 35.
        let mut s = scene(
            r#"{"type":"sevenseg","rect":[0,0,10,10],"text":"0",
                "bind":{"gauge":"RPM","divide":100}}"#,
        )
        .unwrap();
        let b = &s.bindings[0];
        assert_eq!((b.divide, b.offset), (100.0, 0.0));
        assert_eq!(b.show(3500.0), 35.0);
        assert_eq!(b.unshow(35.0), 3500.0);

        RPM.set(3500);
        assert_eq!(s.apply_gauges(), 1);
        assert_eq!(s.tree.get(top(&s)).unwrap().kind.text(), Some("35"));
        RPM.clear();
    }

    #[test]
    fn an_offset_turns_an_absolute_pressure_into_a_gauge_one() {
        // A boost gauge reads zero at atmospheric, not 14.7.
        let s = scene(
            r#"{"type":"needle","rect":[0,0,10,10],
                "bind":{"gauge":"MAP","unit":"psi","offset":-14.7,"min":-15,"max":30}}"#,
        )
        .unwrap();
        let b = &s.bindings[0];
        // 101.3 kPa is 14.7 psi, which is no boost at all.
        assert!(b.show(14.7).abs() < 1e-4, "{}", b.show(14.7));
        // The transform runs before the range, so 0 sits a third of the way
        // up a -15..30 sweep.
        assert!((b.fraction(b.show(14.7)) - 1.0 / 3.0).abs() < 1e-3);
        assert!((b.unshow(0.0) - 14.7).abs() < 1e-4);
    }

    #[test]
    fn the_transform_round_trips_and_a_zero_divisor_is_refused() {
        let s = scene(
            r#"{"type":"bar","rect":[0,0,10,10],
                "bind":{"gauge":"RPM","divide":1000,"offset":0.5,"max":8}}"#,
        )
        .unwrap();
        let b = &s.bindings[0];
        for v in [0.0, 1234.0, 7999.0] {
            assert!((b.unshow(b.show(v)) - v).abs() < 1e-2, "{v}");
        }
        assert_eq!(
            scene(r#"{"type":"bar","rect":[0,0,10,10],"bind":{"gauge":"RPM","divide":0,"max":1}}"#)
                .unwrap_err(),
            BuildError::BadField { field: "divide" }
        );
    }

    #[test]
    fn a_binding_without_a_transform_shows_the_reading_itself() {
        let s = scene(r#"{"type":"bar","rect":[0,0,10,10],"bind":{"gauge":"RPM","max":8000}}"#)
            .unwrap();
        let b = &s.bindings[0];
        assert_eq!((b.divide, b.offset), (1.0, 0.0));
        assert_eq!(b.show(4321.0), 4321.0);
    }

    #[test]
    fn a_bank_can_be_bound_on_both_axes_at_once() {
        // The instrument this exists for: columns light with the revs, and
        // stand as tall as the boost.
        let mut s = scene(
            r#"{"type":"segbar","rect":[0,0,10,10],"segments":10,
                "bind":[{"gauge":"RPM","max":7000},
                        {"gauge":"MAP","unit":"psi","max":20,"property":"height"}]}"#,
        )
        .unwrap();
        assert_eq!(s.bindings.len(), 2);
        assert_eq!(s.bindings[0].target, BindTarget::Reading);
        assert_eq!(s.bindings[1].target, BindTarget::Height);
        // Two gauges, so two subscriptions.
        assert_eq!(s.wanted(), alloc::vec![0x24, 0x2D]);

        RPM.set(3500);
        MAP.set(1030); // 103.0 kPa, near enough 15 psi
        assert_eq!(s.apply_gauges(), 2);
        let kind = &s.tree.get(top(&s)).unwrap().kind;
        assert_eq!(kind.reading(), Some(0.5), "half the revs, half the columns");
        let tall = kind.height().unwrap();
        assert!(tall > 0.7 && tall < 0.8, "boost sets the height: {tall}");
        RPM.clear();
        MAP.clear();
    }

    #[test]
    fn an_axis_a_widget_does_not_have_is_refused() {
        // Only a bank has a second axis; asking a bar for one is a scene
        // that would silently do nothing.
        assert_eq!(
            scene(
                r#"{"type":"bar","rect":[0,0,10,10],
                    "bind":{"gauge":"MAP","max":20,"property":"height"}}"#
            )
            .unwrap_err(),
            BuildError::BadField { field: "property" }
        );
        assert_eq!(
            scene(
                r#"{"type":"segbar","rect":[0,0,10,10],
                    "bind":{"gauge":"MAP","max":20,"property":"sideways"}}"#
            )
            .unwrap_err(),
            BuildError::BadField { field: "property" }
        );
        // And a height binding still needs to know the top of its scale.
        assert_eq!(
            scene(
                r#"{"type":"segbar","rect":[0,0,10,10],
                    "bind":{"gauge":"MAP","property":"height"}}"#
            )
            .unwrap_err(),
            BuildError::Missing { field: "max" }
        );
    }

    #[test]
    fn a_bank_with_no_second_reading_stands_at_full_height() {
        // What every scene written before the axis existed relies on.
        let s = scene(r#"{"type":"segbar","rect":[0,0,10,10],"value":0.5}"#).unwrap();
        assert_eq!(s.tree.get(top(&s)).unwrap().kind.height(), Some(1.0));
    }

    #[test]
    fn an_unset_gauge_leaves_the_widget_as_authored() {
        let mut s = scene(
            r#"{"type":"bar","rect":[0,0,10,10],"value":0.4,
                "bind":{"gauge":"EGT1","max":900}}"#,
        )
        .unwrap();
        EGT1.clear();
        s.tree.clear_damage();
        assert_eq!(s.apply_gauges(), 0);
        assert_eq!(s.tree.get(top(&s)).unwrap().kind.reading(), Some(0.4));
        assert!(s.tree.damage().is_empty(), "nothing changed, nothing dirty");
    }
}
