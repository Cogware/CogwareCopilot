// SPDX-License-Identifier: GPL-3.0-only
//! The Bind rows of the properties pane: which gauge a widget shows, in what
//! unit, over what range.
//!
//! Not rows in [`crate::inspect::fields`], because a binding is not a property
//! of the kind: two bars can show two gauges, a widget may have no binding at
//! all, and picking a gauge changes what the other rows mean. The whole `bind`
//! object is rewritten whenever any part of it changes -- it is one short
//! line, and splicing a key inside it would need the editor to know where
//! inside the widget's text it sits.

use copilot::cogware_can::indicators::{ALL_INDICATORS, Indicator};
use copilot::cogware_can::{ALL_GAUGES, GaugeData, Quantity, Unit};
use copilot::scene::bind::indicator_bank;
use copilot::scene::{Binding, units_for};
use copilot::widget::BindTarget;

use crate::canvas::{Gesture, committed, gesture};

/// What the rows asked for.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum BindEdit {
    /// Write this `bind` object onto the widget.
    Set(String),
    /// Take the widget's `bind` off.
    Unset,
}

/// What the picker at the top of an axis is set to.
///
/// An enum rather than the name as a string, because a lamp and a gauge are
/// two different things that both have names and the row has to write two
/// different keys for them.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Pick {
    /// Bound to nothing.
    None,
    /// A reading off the bus.
    Gauge(&'static GaugeData),
    /// One tell-tale: one bit of one of the two bitfield gauges.
    Lamp(Indicator),
}

/// A binding as the rows edit it: the fields of a [`Binding`] without the
/// node, so a fresh one can exist before it is written anywhere.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Draft {
    gauge: &'static GaugeData,
    /// The tell-tale, when the binding names one rather than a gauge.
    ///
    /// A lamp is one *bit*, and `gauge` is only the bank it lives in. Holding
    /// the bank alone was enough to draw the pane and not enough to write it
    /// back: the widget came out bound to the whole of `INDICATORS1`, lit
    /// whenever any of its thirty-two lamps was.
    indicator: Option<Indicator>,
    unit: Unit,
    divide: f32,
    offset: f32,
    min: f32,
    max: f32,
    decimals: u8,
    pad: u8,
    target: BindTarget,
}

impl Draft {
    fn of(b: &Binding) -> Self {
        Self {
            gauge: b.gauge,
            indicator: b.indicator,
            unit: b.unit,
            divide: b.divide,
            offset: b.offset,
            min: b.min,
            max: b.max,
            decimals: b.decimals,
            pad: b.pad,
            target: b.target,
        }
    }

    /// A new binding to `gauge`, in its own unit, over a range a person would
    /// probably want: 0..8000 for a speed in rpm, 8..16 for a voltage.
    pub(crate) fn fresh(gauge: &'static GaugeData, target: BindTarget) -> Self {
        let (min, max) = default_span(gauge.unit.quantity);
        Self {
            gauge,
            indicator: None,
            unit: gauge.unit,
            divide: 1.0,
            offset: 0.0,
            min,
            max,
            decimals: 0,
            pad: 0,
            target,
        }
    }

    /// A new binding to one tell-tale.
    ///
    /// Lit is 1 and out is 0, so there is no range to choose and no unit to
    /// convert; the bank the bit lives in is the spec's business and is
    /// worked out rather than asked for.
    pub(crate) fn fresh_lamp(ind: Indicator, target: BindTarget) -> Self {
        Self {
            gauge: indicator_bank(ind),
            indicator: Some(ind),
            unit: Unit::RAW,
            divide: 1.0,
            offset: 0.0,
            min: 0.0,
            max: 1.0,
            decimals: 0,
            pad: 0,
            target,
        }
    }

    /// What this binding reads, for the picker and the rows below it.
    fn pick(&self) -> Pick {
        match self.indicator {
            Some(i) => Pick::Lamp(i),
            None => Pick::Gauge(self.gauge),
        }
    }

    /// The `bind` object as the scene format wants it. Defaults are left out,
    /// so a file says only what its author chose.
    ///
    /// `natural` is the axis this kind drives when a block does not say, so
    /// the `property` key is written only when it carries information.
    pub(crate) fn json(&self, natural: BindTarget) -> String {
        // A lamp names itself and nothing else. Its range is a bit, its unit
        // is the bank's raw one, and a transform over a 0 or a 1 is not a
        // thing anyone wants -- so none of the keys below apply and writing
        // them would only be a second place for the file to be wrong.
        if let Some(ind) = self.indicator {
            let mut s = format!(r#"{{ "indicator": "{}""#, ind.name());
            if self.target != natural {
                s.push_str(&format!(r#", "property": "{}""#, self.target.name()));
            }
            s.push_str(" }");
            return s;
        }
        let mut s = format!(r#"{{ "gauge": "{}""#, self.gauge.name);
        if self.target != natural {
            s.push_str(&format!(r#", "property": "{}""#, self.target.name()));
        }
        if self.unit != self.gauge.unit {
            s.push_str(&format!(r#", "unit": "{}""#, self.unit.symbol));
        }
        if self.divide != 1.0 {
            s.push_str(&format!(r#", "divide": {}"#, num(self.divide)));
        }
        if self.offset != 0.0 {
            s.push_str(&format!(r#", "offset": {}"#, num(self.offset)));
        }
        if self.min != 0.0 {
            s.push_str(&format!(r#", "min": {}"#, num(self.min)));
        }
        s.push_str(&format!(r#", "max": {}"#, num(self.max)));
        if self.target == BindTarget::Text {
            if self.decimals != 0 {
                s.push_str(&format!(r#", "decimals": {}"#, self.decimals));
            }
            if self.pad != 0 {
                s.push_str(&format!(r#", "pad": {}"#, self.pad));
            }
        }
        s.push_str(" }");
        s
    }

    /// Change the display unit, carrying the range across so 0..100 °C
    /// becomes 32..212 °F rather than 0..100 °F.
    fn set_unit(&mut self, unit: Unit) {
        // The range is in what the widget shows, so it comes back through the
        // transform before it is converted and put through again. Without
        // that, a tachometer in hundreds would have its ends multiplied by
        // the conversion factor and by the divisor.
        let (lo, hi) = (self.unshow(self.min), self.unshow(self.max));
        if let (Some(lo), Some(hi)) = (self.unit.convert(lo, unit), self.unit.convert(hi, unit)) {
            self.min = self.show(lo);
            self.max = self.show(hi);
        }
        self.unit = unit;
    }

    /// A reading in the display unit as the widget would show it.
    fn show(&self, reading: f32) -> f32 {
        reading / self.divide + self.offset
    }

    /// The inverse of [`Self::show`].
    fn unshow(&self, shown: f32) -> f32 {
        (shown - self.offset) * self.divide
    }
}

/// A number the way a person would write it in a scene file: whole when it
/// is whole, two places otherwise.
fn num(v: f32) -> String {
    if v.fract() == 0.0 && v.abs() < 1e9 {
        format!("{v:.0}")
    } else {
        format!("{v:.2}")
    }
}

/// A range worth starting from for a gauge of this dimension, in the
/// canonical unit the gauge table stores it in.
///
/// Exhaustive on purpose: a dimension the bus spec grows should get a
/// considered default here, not fall through to one that fits nothing.
fn default_span(q: Quantity) -> (f32, f32) {
    match q {
        Quantity::AngularSpeed => (0.0, 8000.0),
        Quantity::Temperature => (0.0, 150.0),
        Quantity::Pressure => (0.0, 300.0),
        Quantity::Voltage => (8.0, 16.0),
        Quantity::Mixture => (10.0, 20.0),
        Quantity::Speed => (0.0, 300.0),
        // An odometer's whole life, not a trip: a range that ends at a few
        // thousand km makes the bar on a 200,000 km car sit pinned.
        Quantity::Distance => (0.0, 500_000.0),
        Quantity::Angle => (-10.0, 50.0),
        Quantity::Duration => (0.0, 20.0),
        Quantity::Time => (0.0, 3600.0),
        Quantity::Frequency => (0.0, 200.0),
        Quantity::Volume => (0.0, 80.0),
        Quantity::Data => (0.0, 4096.0),
        Quantity::AngularSpeedRate | Quantity::PressureRate | Quantity::PercentRate => {
            (-100.0, 100.0)
        }
        Quantity::Percent | Quantity::Raw => (0.0, 100.0),
    }
}

/// What the picker shows for a choice.
fn pick_name(p: Pick) -> &'static str {
    match p {
        Pick::None => "none",
        Pick::Gauge(g) => g.name,
        Pick::Lamp(i) => i.name(),
    }
}

/// The symbol a unit is shown by. The raw unit's is empty, which on a button
/// reads as broken rather than as raw.
fn symbol(u: Unit) -> &'static str {
    if u.symbol.is_empty() { "raw" } else { u.symbol }
}

/// The axes a kind can be bound on, in the order a person meets them.
fn axes(kind: &copilot::widget::Kind, current: &[&Binding]) -> Vec<BindTarget> {
    // The kind's own axes, plus any the widget is actually bound on.
    //
    // `Visible` is not offered on its own account: every kind accepts it, so
    // listing it always would put a near-identical group under every widget
    // in the editor. But an axis the pane does not list is an axis the pane
    // drops -- the blocks it writes back are built from this list -- so a
    // `visible` binding somebody authored by hand has to appear here or
    // editing the widget's colour would silently delete it.
    let mut out: Vec<BindTarget> = [BindTarget::Reading, BindTarget::Height, BindTarget::Text]
        .into_iter()
        .filter(|t| kind.accepts(*t))
        .collect();
    for b in current {
        if !out.contains(&b.target) {
            out.push(b.target);
        }
    }
    out
}

/// What an axis is called in the pane when a widget has more than one.
fn axis_name(t: BindTarget) -> &'static str {
    match t {
        BindTarget::Reading => "along",
        BindTarget::Height => "up",
        BindTarget::Text => "text",
        BindTarget::Visible => "shown",
    }
}

/// The Bind rows for `kind`, given every binding the scene has for the widget.
///
/// One group per axis the kind has. A bank has two -- how far along its scale
/// the reading has got, and how tall the lit cells stand -- and both are
/// written back together, because they share one `bind` key: editing either
/// through a pane that knew only about the first would silently drop the
/// second.
pub(crate) fn bind_rows(
    ui: &mut egui::Ui,
    kind: &copilot::widget::Kind,
    current: &[&Binding],
) -> Option<(BindEdit, Option<Gesture>)> {
    let targets = axes(kind, current);
    let natural = kind.bind_target()?;
    let mut drafts: Vec<Option<Draft>> = targets
        .iter()
        .map(|t| {
            current
                .iter()
                .find(|b| b.target == *t)
                .map(|b| Draft::of(b))
        })
        .collect();

    let mut edited: Option<Option<Gesture>> = None;
    for (i, t) in targets.iter().enumerate() {
        // Only worth naming when there is more than one: a bar has one axis
        // and a heading over it would be noise.
        if targets.len() > 1 {
            ui.label(egui::RichText::new(axis_name(*t)).small().weak())
                .on_hover_text(match t {
                    BindTarget::Reading => "How far along its scale the reading has got",
                    BindTarget::Height => "How tall the lit cells stand within their envelope",
                    BindTarget::Text => "The number the readout shows",
                    BindTarget::Visible => "Whether the widget is on the screen at all",
                });
            ui.label("");
            ui.end_row();
        }
        if let Some((d, g)) = axis_rows(ui, *t, drafts[i].as_ref()) {
            drafts[i] = d;
            edited = Some(g);
        }
    }

    let g = edited?;
    // One object when a single axis is bound, an array when several are, and
    // no key at all when none is.
    let blocks: Vec<String> = drafts
        .iter()
        .filter_map(|d| d.as_ref().map(|d| d.json(natural)))
        .collect();
    let edit = match blocks.len() {
        0 => BindEdit::Unset,
        1 => BindEdit::Set(blocks.into_iter().next().unwrap_or_default()),
        _ => BindEdit::Set(format!("[{}]", blocks.join(", "))),
    };
    Some((edit, g))
}

/// One axis's rows. `Some` when the person changed something: the new draft
/// for this axis, or `None` if they unbound it.
fn axis_rows(
    ui: &mut egui::Ui,
    target: BindTarget,
    current: Option<&Draft>,
) -> Option<(Option<Draft>, Option<Gesture>)> {
    let salt = target.name();
    ui.label("reads");
    let now = current.map_or(Pick::None, Draft::pick);
    let mut picked = now;
    egui::ComboBox::from_id_salt(("bind-gauge", salt))
        .selected_text(pick_name(now))
        .width(180.0)
        .height(360.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut picked, Pick::None, "none");
            ui.separator();
            for g in ALL_GAUGES {
                ui.selectable_value(&mut picked, Pick::Gauge(g), g.name)
                    .on_hover_text(format!("{} · {:?}", symbol(g.unit), g.source));
            }
            // The tell-tales below the gauges, in the spec's own table order.
            // They are bits rather than readings, so they are picked by name
            // and never by the bank they happen to sit in -- which is why
            // INDICATORS1 and INDICATORS2 are in the list above as well and
            // mean something quite different.
            ui.separator();
            ui.label(egui::RichText::new("tell-tales").small().weak());
            for ind in ALL_INDICATORS {
                ui.selectable_value(&mut picked, Pick::Lamp(*ind), ind.name())
                    .on_hover_text("One bit. Lit is 1, out is 0.");
            }
        });
    ui.end_row();
    if picked != now {
        let d = match picked {
            Pick::None => None,
            Pick::Gauge(g) => Some(Draft::fresh(g, target)),
            Pick::Lamp(i) => Some(Draft::fresh_lamp(i, target)),
        };
        return Some((d, None));
    }
    let b = current?;

    // A lamp has no unit to convert, no range to set and no transform worth
    // applying to a 0 or a 1, so the rows below are not drawn for one. The
    // picker above is the whole of editing a tell-tale.
    if b.indicator.is_some() {
        return None;
    }

    let mut d = b.clone();
    let mut g: Option<Option<Gesture>> = None;
    let mut note = |r: egui::Response| {
        if committed(&r) {
            g = Some(gesture(&r));
        }
    };

    ui.label("unit");
    let mut unit = d.unit;
    let mut unit_changed = false;
    egui::ComboBox::from_id_salt(("bind-unit", salt))
        .selected_text(symbol(d.unit))
        .show_ui(ui, |ui| {
            for u in units_for(d.gauge.unit.quantity) {
                unit_changed |= ui.selectable_value(&mut unit, u, symbol(u)).changed();
            }
        });
    if unit_changed {
        d.set_unit(unit);
    }
    ui.end_row();

    ui.label("divide")
        .on_hover_text("The reading is divided by this before it is shown");
    let was = d.divide;
    note(
        ui.add(
            egui::DragValue::new(&mut d.divide)
                .speed(1.0)
                .range(-1e9..=1e9),
        )
        .on_hover_text(
            "100 for a tachometer whose face reads x100 r/min. Zero is not a \
             thing to divide by, so it is refused.",
        ),
    );
    // Refused rather than corrected: a zero here is a slip mid-drag, and
    // snapping it to one would fight the hand that is still moving.
    if d.divide == 0.0 {
        d.divide = was;
    }
    ui.end_row();

    ui.label("offset").on_hover_text("Added after the division");
    note(
        ui.add(egui::DragValue::new(&mut d.offset).speed(0.1))
            .on_hover_text(
                "-14.7 on a MAP gauge in psi reads zero at atmospheric, which \
                 is what a boost gauge shows.",
            ),
    );
    ui.end_row();

    // "range" for a fraction, because that is what the ends mean; "sweep" for
    // text, where the reading is shown whole and the ends only say how far
    // the dummy values travel.
    ui.label(match target {
        BindTarget::Reading | BindTarget::Height | BindTarget::Visible => "range",
        BindTarget::Text => "sweep",
    });
    ui.horizontal(|ui| {
        let step = ((d.max - d.min).abs() / 200.0).max(0.01);
        note(ui.add(egui::DragValue::new(&mut d.min).speed(step).prefix("min ")));
        note(ui.add(egui::DragValue::new(&mut d.max).speed(step).prefix("max ")));
    });
    ui.end_row();

    if target == BindTarget::Text {
        ui.label("decimals");
        note(ui.add(egui::DragValue::new(&mut d.decimals).range(0..=6)));
        ui.end_row();
        ui.label("pad");
        note(
            ui.add(egui::DragValue::new(&mut d.pad).range(0..=16))
                .on_hover_text(
                    "Least width, space-padded on the left, so digits stay in their cells",
                ),
        );
        ui.end_row();
    }

    // A pick from the unit list is a click, its own undo step.
    let g = if unit_changed { Some(None) } else { g };
    g.map(|g| (Some(d), g))
}

#[cfg(test)]
mod tests {
    use super::*;
    use copilot::cogware_can::indicators::Indicator;
    use copilot::cogware_can::{BAT_VOL, CLNT, GEAR, MAP, RPM};

    /// The binding a `bind` object produces on a widget of `kind`.
    fn bound(kind: &str, json: &str) -> Binding {
        let text = if kind == "label" || kind == "sevenseg" {
            r#","text":"0""#
        } else {
            ""
        };
        let src = format!(
            r#"{{"width":10,"height":10,"root":{{"type":"{kind}","rect":[0,0,10,10]{text},"bind":{json}}}}}"#
        );
        let doc = copilot::scene::parse(&src).unwrap_or_else(|e| panic!("{e:?}: {src}"));
        let mut s = copilot::scene::build_scene(&doc).unwrap_or_else(|e| panic!("{e:?}: {src}"));
        s.bindings.remove(0)
    }

    #[test]
    fn a_fresh_binding_builds_and_spans_something() {
        for g in [&RPM, &CLNT, &MAP, &BAT_VOL, &GEAR] {
            let d = Draft::fresh(g, BindTarget::Reading);
            let b = bound("bar", &d.json(BindTarget::Reading));
            assert_eq!(b.gauge.name, g.name);
            assert_eq!(b.unit, g.unit, "fresh means the gauge's own unit");
            assert!(b.max > b.min, "{}: {:?}", g.name, d);
        }
    }

    #[test]
    fn a_tell_tale_binding_survives_the_pane() {
        // The bug this catches: a binding to one lamp is a binding to one
        // *bit*, and the pane held only the bank that bit lives in. Opening a
        // led bound to OIL_PRESSURE and touching any row rewrote it as
        // `{"gauge": "INDICATORS1"}` -- which lights the widget whenever any
        // of that bank's thirty-two lamps is on, not when that one is.
        let b = bound("led", r#"{ "indicator": "OIL_PRESSURE" }"#);
        assert_eq!(b.indicator, Some(Indicator::OilPressure));

        let d = Draft::of(&b);
        let written = d.json(BindTarget::Reading);
        assert!(
            !written.contains("INDICATORS"),
            "the pane wrote the bank instead of the lamp: {written}"
        );
        let back = bound("led", &written);
        assert_eq!(
            back.indicator,
            Some(Indicator::OilPressure),
            "the lamp did not survive: {written}"
        );
        assert_eq!(Draft::of(&back), d, "what was written is what is read");
    }

    #[test]
    fn a_lamp_writes_its_name_and_nothing_it_has_no_use_for() {
        // A bit has no unit, no range and no transform. Writing them would
        // only be a second place for the file to disagree with itself.
        let d = Draft::fresh_lamp(Indicator::LowFuel, BindTarget::Reading);
        assert_eq!(
            d.json(BindTarget::Reading),
            r#"{ "indicator": "LOW_FUEL" }"#
        );
        // The bank is worked out, never asked for.
        assert_eq!(d.gauge.name, "INDICATORS1");
        assert_eq!(
            bound("led", &d.json(BindTarget::Reading)).gauge.name,
            "INDICATORS1"
        );

        // A lamp on a second bank lands on the second gauge, still by name.
        let two = Draft::fresh_lamp(Indicator::PitLimiter, BindTarget::Reading);
        assert_eq!(two.gauge.name, "INDICATORS2");
        assert_eq!(
            bound("led", &two.json(BindTarget::Reading)).indicator,
            Some(Indicator::PitLimiter)
        );
    }

    #[test]
    fn a_lamp_that_shows_a_widget_keeps_saying_so() {
        // The property is the one key a lamp still needs: it is what makes a
        // printed symbol appear rather than a rectangle brighten.
        let b = bound(
            "led",
            r#"{ "indicator": "OVERHEAT", "property": "visible" }"#,
        );
        let written = Draft::of(&b).json(BindTarget::Reading);
        assert_eq!(
            written,
            r#"{ "indicator": "OVERHEAT", "property": "visible" }"#
        );
        let back = bound("led", &written);
        assert_eq!(back.target, BindTarget::Visible);
        assert_eq!(back.indicator, Some(Indicator::Overheat));
    }

    #[test]
    fn every_lamp_the_spec_has_survives_being_written_and_read() {
        // Sixty-four bits across two banks, each of which has to come back as
        // itself rather than as its neighbour or as its bank.
        for ind in copilot::cogware_can::indicators::ALL_INDICATORS {
            let d = Draft::fresh_lamp(*ind, BindTarget::Reading);
            let b = bound("led", &d.json(BindTarget::Reading));
            assert_eq!(b.indicator, Some(*ind), "{}", ind.name());
            assert_eq!((b.min, b.max), (0.0, 1.0), "{}", ind.name());
        }
    }

    #[test]
    fn an_axis_the_widget_is_bound_on_is_never_dropped() {
        // The other half of the same bug: the blocks written back are built
        // from the axis list, so an axis missing from it is an axis deleted.
        // A led's own axis is `value`; a `visible` binding is not, and used
        // to vanish the moment anything else on the widget was edited.
        let led = copilot::widget::Kind::Led {
            color: copilot::Color::WHITE,
            level: 0.0,
            glow: 0.0,
            off: copilot::Color::TRANSPARENT,
        };
        let b = bound(
            "led",
            r#"{ "indicator": "LOW_FUEL", "property": "visible" }"#,
        );
        assert_eq!(
            axes(&led, &[&b]),
            vec![BindTarget::Reading, BindTarget::Visible],
            "the visible binding had nowhere to be written back from"
        );
        // And a widget bound the ordinary way grows no extra group.
        let plain = bound("led", r#"{ "indicator": "ABS" }"#);
        assert_eq!(axes(&led, &[&plain]), vec![BindTarget::Reading]);
        assert_eq!(axes(&led, &[]), vec![BindTarget::Reading]);
    }

    #[test]
    fn a_gauge_and_a_lamp_are_two_different_choices() {
        // `INDICATORS1` is a real gauge and a legitimate thing to bind to --
        // it is just not what "the oil lamp" means. The picker has to be able
        // to say both, which a bare name cannot.
        let bank = Draft::fresh(&copilot::cogware_can::INDICATORS1, BindTarget::Reading);
        let lamp = Draft::fresh_lamp(Indicator::OilPressure, BindTarget::Reading);
        assert_ne!(bank.pick(), lamp.pick());
        assert_eq!(bank.gauge.name, lamp.gauge.name, "same bank, all the same");
        assert!(bank.json(BindTarget::Reading).contains(r#""gauge""#));
        assert!(lamp.json(BindTarget::Reading).contains(r#""indicator""#));
    }

    #[test]
    fn json_leaves_defaults_out_and_round_trips() {
        let d = Draft {
            gauge: &CLNT,
            indicator: None,
            unit: Unit::FAHRENHEIT,
            divide: 1.0,
            offset: 0.0,
            min: 120.0,
            max: 270.0,
            decimals: 3,
            pad: 2,
            target: BindTarget::Reading,
        };
        // A reading has no digits to format, so it never writes any.
        assert_eq!(
            d.json(BindTarget::Reading),
            r#"{ "gauge": "CLNT", "unit": "°F", "min": 120, "max": 270 }"#
        );
        let b = bound("segbar", &d.json(BindTarget::Reading));
        assert_eq!((b.unit, b.min, b.max), (Unit::FAHRENHEIT, 120.0, 270.0));

        let t = Draft {
            target: BindTarget::Text,
            min: 0.0,
            unit: Unit::CELSIUS,
            ..d
        };
        assert_eq!(
            t.json(BindTarget::Text),
            r#"{ "gauge": "CLNT", "max": 270, "decimals": 3, "pad": 2 }"#
        );
        let b = bound("sevenseg", &t.json(BindTarget::Text));
        assert_eq!((b.decimals, b.pad, b.min), (3, 2, 0.0));
        assert_eq!(Draft::of(&b), t, "what was written is what is read");
    }

    #[test]
    fn the_property_key_is_written_only_when_it_says_something() {
        // A bank's natural axis is its reading, so a height binding has to
        // name itself and a value binding must not.
        let along = Draft::fresh(&RPM, BindTarget::Reading);
        let up = Draft::fresh(&MAP, BindTarget::Height);
        assert!(!along.json(BindTarget::Reading).contains("property"));
        assert!(
            up.json(BindTarget::Reading)
                .contains(r#""property": "height""#)
        );

        // And both survive the round trip through a two-axis widget.
        let src = format!(
            r#"{{"width":10,"height":10,"root":{{"type":"segbar","rect":[0,0,10,10],
                "bind":[{}, {}]}}}}"#,
            along.json(BindTarget::Reading),
            up.json(BindTarget::Reading)
        );
        let doc = copilot::scene::parse(&src).unwrap_or_else(|e| panic!("{e:?}: {src}"));
        let s = copilot::scene::build_scene(&doc).unwrap_or_else(|e| panic!("{e:?}: {src}"));
        assert_eq!(s.bindings.len(), 2);
        assert_eq!(Draft::of(&s.bindings[0]), along);
        assert_eq!(Draft::of(&s.bindings[1]), up);
    }

    #[test]
    fn changing_the_unit_carries_the_range_across() {
        let mut d = Draft::fresh(&CLNT, BindTarget::Reading);
        d.min = 0.0;
        d.max = 100.0;
        d.set_unit(Unit::FAHRENHEIT);
        assert!(
            (d.min - 32.0).abs() < 1e-3 && (d.max - 212.0).abs() < 1e-3,
            "{d:?}"
        );
        // A raw gauge has nothing to convert to, and keeps its numbers.
        let mut r = Draft::fresh(&GEAR, BindTarget::Text);
        r.set_unit(Unit::RAW);
        assert_eq!((r.min, r.max), default_span(Quantity::Raw));
    }

    #[test]
    fn a_transform_is_written_and_read_back() {
        let mut d = Draft::fresh(&RPM, BindTarget::Text);
        d.divide = 100.0;
        d.max = 80.0;
        assert_eq!(
            d.json(BindTarget::Text),
            r#"{ "gauge": "RPM", "divide": 100, "max": 80 }"#
        );
        assert_eq!(Draft::of(&bound("sevenseg", &d.json(BindTarget::Text))), d);

        // A default transform says nothing at all.
        let plain = Draft::fresh(&RPM, BindTarget::Reading);
        assert!(
            !plain.json(BindTarget::Reading).contains("divide"),
            "{}",
            plain.json(BindTarget::Reading)
        );
        assert!(
            !plain.json(BindTarget::Reading).contains("offset"),
            "{}",
            plain.json(BindTarget::Reading)
        );
    }

    #[test]
    fn changing_the_unit_keeps_the_range_in_what_the_widget_shows() {
        // A coolant gauge reading in tens of °C, switched to °F: 0..15 shown
        // is 0..150 °C, which is 32..302 °F, which is 3.2..30.2 shown.
        let mut d = Draft::fresh(&CLNT, BindTarget::Reading);
        d.divide = 10.0;
        d.min = 0.0;
        d.max = 15.0;
        d.set_unit(Unit::FAHRENHEIT);
        assert!((d.min - 3.2).abs() < 1e-3, "{d:?}");
        assert!((d.max - 30.2).abs() < 1e-3, "{d:?}");
    }

    #[test]
    fn a_number_is_written_the_short_way() {
        assert_eq!(num(8000.0), "8000");
        assert_eq!(num(-15.0), "-15");
        assert_eq!(num(14.7), "14.70");
    }
}
