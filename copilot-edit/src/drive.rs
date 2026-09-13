// SPDX-License-Identifier: GPL-3.0-only
//! Dummy values, for seeing a scene at readings it does not hold.
//!
//! A scene file stores one authored value per gauge — whatever the author
//! happened to type. That is almost never the value worth looking at: a fuel
//! bar at 0.62 tells you nothing about whether it reads correctly at 0.0 or
//! 1.0, and a speed label saying "88" hides that "188" overflows its box.
//!
//! A driver overrides named widgets in the *preview only*. Nothing here is
//! ever written to the file, so sweeping a gauge from empty to full to check
//! its geometry cannot accidentally become the value someone ships.
//!
//! A widget bound to a gauge is driven through its binding instead of by
//! name: one channel per *gauge*, in the unit the bus carries it in, so a
//! single RPM slider moves the tachometer bank, the digits beside it, and
//! anything else that reads RPM -- on this display and every other one open.

use std::collections::{BTreeMap, BTreeSet};

use copilot::scene::Binding;
use copilot::widget::{BindTarget, Kind, NodeId, Tree};

/// One driven reading.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    /// The value the preview shows while the sweep is off.
    pub value: f32,
    /// Bottom of the sweep range.
    pub min: f32,
    /// Top of the sweep range.
    pub max: f32,
    /// Whether the sweep moves this channel, or it is held at `value`.
    pub sweep: bool,
    /// What the scene file itself says, as of the last sync.
    authored: f32,
}

/// One driven label text.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    /// The text the preview shows.
    pub text: String,
    /// What the scene file itself says, as of the last sync.
    authored: String,
}

/// What a widget looked like before the driver touched it.
///
/// Both halves, because a binding reaches two: a reading is in the kind, and
/// a `visible` binding -- which is how a tell-tale shows a symbol rather than
/// dimming a rectangle -- is in the flag beside it.
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    node: NodeId,
    kind: Kind,
    visible: bool,
}

impl Saved {
    fn of(node: NodeId, n: &copilot::widget::Node) -> Self {
        Self {
            node,
            kind: n.kind.clone(),
            visible: n.visible,
        }
    }
}

/// Live overrides applied to the preview before it is drawn.
#[derive(Debug)]
pub struct Driver {
    /// Readings, keyed by widget name.
    pub channels: BTreeMap<String, Channel>,
    /// Label texts, keyed by widget name.
    pub labels: BTreeMap<String, Text>,
    /// Readings for bound gauges, keyed by the gauge's name and held in the
    /// gauge's own unit -- the one the bus carries.
    pub gauges: BTreeMap<&'static str, Channel>,
    /// Bound tell-tales, keyed by the name the bus spec gives each lamp.
    ///
    /// One switch per lamp rather than a slider on the bank: the two bitfield
    /// gauges carry thirty-two lamps each, and a slider on `INDICATORS1`
    /// would light or extinguish every one of them together.
    pub lamps: BTreeMap<&'static str, bool>,
    /// Whether overrides are applied at all.
    pub enabled: bool,
    /// Whether the sweep is running.
    pub sweep: bool,
    /// Seconds for one full up-and-down sweep. Default 4.0.
    pub period_s: f32,
    phase: f32,
}

impl Default for Driver {
    fn default() -> Self {
        Self {
            channels: BTreeMap::new(),
            labels: BTreeMap::new(),
            gauges: BTreeMap::new(),
            lamps: BTreeMap::new(),
            enabled: false,
            sweep: false,
            period_s: 4.0,
            phase: 0.0,
        }
    }
}

impl Channel {
    /// What this channel makes the widget show: the sweep position mapped onto
    /// min..=max while `sweeping` and `self.sweep`, otherwise `value`.
    pub fn current(&self, sweeping: bool, sweep: f32) -> f32 {
        if sweeping && self.sweep {
            self.min + sweep * (self.max - self.min)
        } else {
            self.value
        }
    }

    /// The value the file holds.
    pub fn authored(&self) -> f32 {
        self.authored
    }
}

impl Text {
    /// The text the file holds.
    pub fn authored(&self) -> &str {
        &self.authored
    }
}

impl Driver {
    /// Learn the drivable named widgets in `tree`, and the gauges `bindings`
    /// name -- and the gauges of `others`, the displays open beside this
    /// one, so one slider reaches every widget in the rig that reads them.
    ///
    /// A name that is no longer in the tree (or no longer drivable) is dropped.
    /// A new name gets a channel at its authored value with the full 0..=1
    /// sweep range. An existing name keeps its slider value unless the person
    /// never moved it off the authored value, in which case it follows the
    /// new authored value.
    pub fn sync(&mut self, tree: &Tree, bindings: &[Binding], others: &[(&Tree, &[Binding])]) {
        self.sync_gauges(tree, bindings, others);
        self.sync_lamps(tree, bindings, others);
        // A bound widget is the gauge's to drive. Offering it by name as well
        // would put two sliders on one reading, and the second would win by
        // accident of ordering.
        let bound: BTreeSet<NodeId> = bindings.iter().map(|b| b.node).collect();

        let mut seen_channels: BTreeMap<String, f32> = BTreeMap::new();
        let mut seen_labels: BTreeMap<String, String> = BTreeMap::new();

        for i in 0..tree.len() {
            let Some(id) = u32::try_from(i).ok().map(NodeId) else {
                continue;
            };
            if bound.contains(&id) {
                continue;
            }
            let Some(node) = tree.get(id) else { continue };
            let Some(name) = node.name.as_deref() else {
                continue;
            };
            if let Some(v) = node.kind.reading() {
                seen_channels.insert(name.to_string(), v);
            }
            // Labels only: a seven-segment readout shows text too, but the
            // pane offers it a numeric reading through its binding instead.
            if let Kind::Label { text, .. } = &node.kind {
                seen_labels.insert(name.to_string(), text.clone());
            }
        }

        self.channels
            .retain(|name, _| seen_channels.contains_key(name));
        self.labels.retain(|name, _| seen_labels.contains_key(name));

        for (name, authored) in &seen_channels {
            match self.channels.get_mut(name) {
                Some(ch) => {
                    if ch.value == ch.authored && *authored != ch.authored {
                        ch.value = *authored;
                    }
                    ch.authored = *authored;
                }
                None => {
                    self.channels.insert(
                        name.clone(),
                        Channel {
                            value: *authored,
                            min: 0.0,
                            max: 1.0,
                            sweep: true,
                            authored: *authored,
                        },
                    );
                }
            }
        }

        for (name, authored) in &seen_labels {
            match self.labels.get_mut(name) {
                Some(t) => {
                    if t.text == t.authored && *authored != t.authored {
                        t.text = authored.clone();
                    }
                    t.authored = authored.clone();
                }
                None => {
                    self.labels.insert(
                        name.clone(),
                        Text {
                            text: authored.clone(),
                            authored: authored.clone(),
                        },
                    );
                }
            }
        }
    }

    /// Learn the gauges `bindings` name.
    ///
    /// A gauge's channel spans the widest range any binding of it declares,
    /// converted back to the gauge's own unit, because two widgets may show
    /// one gauge in two units and a slider has to mean one thing. The range
    /// follows the file rather than the person: it is the widgets' range, and
    /// the sweep exists to cover exactly that. The starting value is worked
    /// back from what the file draws, so switching the driver on changes
    /// nothing until a slider moves.
    fn sync_gauges(&mut self, tree: &Tree, bindings: &[Binding], others: &[(&Tree, &[Binding])]) {
        let mut seen: BTreeMap<&'static str, (f32, f32, f32)> = BTreeMap::new();
        let all = core::iter::once((tree, bindings)).chain(others.iter().copied());
        for (tree, bindings) in all {
            for b in bindings {
                // The bank a tell-tale lives in is not a reading anybody
                // wants a slider on; `sync_lamps` gives each lamp a switch.
                if b.indicator.is_some() {
                    continue;
                }
                let lo = to_canonical(b, b.min.min(b.max));
                let hi = to_canonical(b, b.min.max(b.max));
                let authored = tree
                    .get(b.node)
                    .map_or(lo, |n| authored_of(b, &n.kind, n.visible));
                let e = seen.entry(b.gauge.name).or_insert((authored, lo, hi));
                e.1 = e.1.min(lo);
                e.2 = e.2.max(hi);
            }
        }
        self.gauges.retain(|name, _| seen.contains_key(name));
        for (name, (authored, min, max)) in seen {
            match self.gauges.get_mut(name) {
                Some(ch) => {
                    if ch.value == ch.authored && authored != ch.authored {
                        ch.value = authored;
                    }
                    ch.authored = authored;
                    ch.min = min;
                    ch.max = max;
                }
                None => {
                    self.gauges.insert(
                        name,
                        Channel {
                            value: authored,
                            min,
                            max,
                            sweep: true,
                            authored,
                        },
                    );
                }
            }
        }
    }

    /// Learn the tell-tales `bindings` name, here and on the displays beside.
    ///
    /// A lamp starts at whatever the file draws -- a led authored at full is a
    /// lamp the author wants to see lit -- so switching the driver on changes
    /// nothing until a switch is flicked, the same as every other channel.
    fn sync_lamps(&mut self, tree: &Tree, bindings: &[Binding], others: &[(&Tree, &[Binding])]) {
        let mut seen: BTreeMap<&'static str, bool> = BTreeMap::new();
        let all = core::iter::once((tree, bindings)).chain(others.iter().copied());
        for (tree, bindings) in all {
            for b in bindings {
                let Some(ind) = b.indicator else { continue };
                let lit = tree.get(b.node).is_some_and(|n| match b.target {
                    copilot::widget::BindTarget::Visible => n.visible,
                    _ => n.kind.reading().is_some_and(|v| v >= 0.5),
                });
                // Lit wins where one lamp drives two widgets and they
                // disagree: the author drew one of them on, so show it on.
                *seen.entry(ind.name()).or_insert(lit) |= lit;
            }
        }
        self.lamps.retain(|name, _| seen.contains_key(name));
        for (name, lit) in seen {
            self.lamps.entry(name).or_insert(lit);
        }
    }

    /// Advance the sweep by `delta_us` microseconds.
    ///
    /// A triangle wave: phase runs 0..2 over `period_s` seconds. A period of
    /// zero or less is treated as 4.0 to avoid division by zero.
    pub fn tick(&mut self, delta_us: u64) {
        if !self.sweep {
            return;
        }
        let period = if self.period_s > 0.0 {
            self.period_s
        } else {
            4.0
        };
        self.phase += delta_us as f32 / (period * 1_000_000.0);
        if self.phase > 2.0 {
            self.phase -= 2.0;
        }
    }

    /// The sweep position, 0.0..=1.0.
    pub fn sweep_value(&self) -> f32 {
        if self.phase <= 1.0 {
            self.phase
        } else {
            2.0 - self.phase
        }
    }

    /// Apply the overrides to `tree` and return what was there, so
    /// [`Self::restore`] can put it back after the frame is drawn.
    ///
    /// Returns an empty `Vec` when `!self.enabled`.
    pub fn apply(&self, tree: &mut Tree, bindings: &[Binding]) -> Vec<Saved> {
        if !self.enabled {
            return Vec::new();
        }
        let sweep = self.sweep_value();
        let mut saved = Vec::new();

        for b in bindings {
            // A tell-tale is a bit, not a reading: it gets a switch of its
            // own rather than a slider on the bank it shares with thirty-one
            // others, which one slider would drive all of at once.
            if let Some(ind) = b.indicator {
                let Some(on) = self.lamps.get(ind.name()) else {
                    continue;
                };
                let Some(node) = tree.get(b.node) else {
                    continue;
                };
                saved.push(Saved::of(b.node, node));
                b.apply_value(tree, if *on { b.max } else { b.min });
                continue;
            }
            let Some(ch) = self.gauges.get(b.gauge.name) else {
                continue;
            };
            let Some(node) = tree.get(b.node) else {
                continue;
            };
            saved.push(Saved::of(b.node, node));
            let v = from_canonical(b, ch.current(self.sweep, sweep));
            b.apply_value(tree, v);
        }

        for (name, ch) in &self.channels {
            let Some(id) = tree.find(name) else { continue };
            let Some(node) = tree.get(id) else { continue };
            let v = ch.current(self.sweep, sweep);
            let Some(new_kind) = node.kind.with_reading(v) else {
                continue;
            };
            saved.push(Saved::of(id, node));
            tree.set_kind(id, new_kind);
        }

        for (name, text) in &self.labels {
            let Some(id) = tree.find(name) else { continue };
            let Some(node) = tree.get(id) else { continue };
            let Kind::Label {
                color,
                scale,
                align,
                valign,
                ..
            } = node.kind
            else {
                continue;
            };
            saved.push(Saved::of(id, node));
            tree.set_kind(
                id,
                Kind::Label {
                    text: text.text.clone(),
                    color,
                    scale,
                    align,
                    valign,
                },
            );
        }

        saved
    }

    /// Put back what [`Self::apply`] replaced.
    pub fn restore(tree: &mut Tree, saved: Vec<Saved>) {
        for s in saved {
            tree.set_kind(s.node, s.kind);
            // Visibility as well as kind: a binding whose property is
            // `visible` moves the flag and leaves the kind alone, and
            // restoring only the kind would leave a widget the preview had
            // hidden hidden for good.
            tree.set_visible(s.node, s.visible);
        }
    }

    /// Every channel value and label text back to what the file says.
    ///
    /// The lamps are left alone: a switch has no authored value to go back to
    /// beyond the one it opened on, and a reset that put every tell-tale out
    /// would undo the one thing a person checking a dark panel is doing.
    pub fn reset(&mut self) {
        for ch in self.channels.values_mut().chain(self.gauges.values_mut()) {
            ch.value = ch.authored;
        }
        for t in self.labels.values_mut() {
            t.text = t.authored.clone();
        }
    }
}

/// `v`, in the binding's display unit, in the gauge's own.
///
/// The gauge's own unit is where a channel lives, because it is the one unit
/// every binding of that gauge shares. A raw gauge has no conversion and
/// `convert` says so; its display unit is then its own and `v` is `v`.
fn to_canonical(b: &Binding, shown: f32) -> f32 {
    // Back through the binding's own transform first: `min` and `max` are in
    // what the widget *shows*, and a tachometer showing hundreds would put a
    // slider 0..70 where the gauge means 0..7000.
    let v = b.unshow(shown);
    if b.unit == b.gauge.unit {
        v
    } else {
        b.unit.convert(v, b.gauge.unit).unwrap_or(v)
    }
}

/// `v`, in the gauge's own unit, as the binding's widget shows it.
fn from_canonical(b: &Binding, v: f32) -> f32 {
    let v = if b.unit == b.gauge.unit {
        v
    } else {
        b.gauge.unit.convert(v, b.unit).unwrap_or(v)
    };
    b.show(v)
}

/// The reading, in the gauge's own unit, that would draw `kind` as the file
/// has it: the fraction mapped back through the range, or the text read as
/// a number. The bottom of the range when neither can be worked out.
fn authored_of(b: &Binding, kind: &Kind, visible: bool) -> f32 {
    let shown = match b.target {
        BindTarget::Reading => kind.reading().map(|f| b.min + f * (b.max - b.min)),
        BindTarget::Height => kind.height().map(|f| b.min + f * (b.max - b.min)),
        BindTarget::Text => kind.text().and_then(|t| t.trim().parse::<f32>().ok()),
        // A hidden widget is at the bottom of its range and a shown one at
        // the top, which is the only reading either state can have come from.
        BindTarget::Visible => Some(if visible { b.max } else { b.min }),
    };
    to_canonical(b, shown.unwrap_or(b.min))
}

#[cfg(test)]
mod tests {
    use super::*;
    use copilot::widget::{Align, Node, ROOT, VAlign};
    use copilot::{Color, Rect};

    fn node(name: &str, kind: Kind) -> Node {
        Node {
            rect: Rect::new(0, 0, 10, 10),
            kind,
            visible: true,
            antialias: None,
            name: Some(name.into()),
            children: Vec::new(),
            parent: None,
        }
    }

    fn bar(value: f32) -> Kind {
        Kind::Bar {
            value,
            fill: Color::rgb(1, 2, 3),
            track: Color::rgb(4, 5, 6),
            vertical: false,
        }
    }

    fn label(text: &str) -> Kind {
        Kind::Label {
            text: text.into(),
            color: Color::rgb(1, 2, 3),
            scale: 1,
            align: Align::Left,
            valign: VAlign::Top,
        }
    }

    /// A tree holding one named bar at `value` and one named label.
    fn tree(value: f32, text: &str) -> Tree {
        let mut t = Tree::new(Rect::new(0, 0, 100, 100));
        t.push(ROOT, node("rpm", bar(value))).expect("a bar");
        t.push(ROOT, node("speed", label(text))).expect("a label");
        t
    }

    fn channel(min: f32, max: f32, sweep: bool) -> Channel {
        Channel {
            value: 0.3,
            min,
            max,
            sweep,
            authored: 0.3,
        }
    }

    #[test]
    fn the_sweep_runs_between_min_and_max_and_a_held_channel_ignores_it() {
        let ch = channel(0.25, 0.75, true);
        assert_eq!(ch.current(true, 0.0), 0.25);
        assert_eq!(ch.current(true, 1.0), 0.75);
        assert_eq!(ch.current(true, 0.5), 0.5);
        assert_eq!(channel(0.0, 1.0, false).current(true, 0.9), 0.3);
        // And a sweep that is not running leaves every channel at its value.
        assert_eq!(ch.current(false, 0.9), 0.3);
    }

    #[test]
    fn a_new_name_starts_at_the_authored_value_with_the_full_range() {
        let mut d = Driver::default();
        d.sync(&tree(0.4, "88"), &[], &[]);
        let ch = d.channels.get("rpm").expect("the bar");
        assert_eq!((ch.value, ch.min, ch.max, ch.sweep), (0.4, 0.0, 1.0, true));
        assert_eq!(d.labels.get("speed").map(|t| t.text.as_str()), Some("88"));
    }

    #[test]
    fn a_vanished_name_is_dropped() {
        let mut d = Driver::default();
        d.sync(&tree(0.4, "88"), &[], &[]);
        let mut t = Tree::new(Rect::new(0, 0, 100, 100));
        t.push(ROOT, node("other", bar(0.1))).expect("a bar");
        d.sync(&t, &[], &[]);
        assert!(!d.channels.contains_key("rpm"), "{:?}", d.channels);
        assert!(d.labels.is_empty());
        assert!(d.channels.contains_key("other"));
    }

    #[test]
    fn an_untouched_slider_follows_the_file() {
        // Editing the value in the inspector must show up in the values
        // pane, or the two disagree about what the gauge reads.
        let mut d = Driver::default();
        d.sync(&tree(0.4, "88"), &[], &[]);
        d.sync(&tree(0.7, "99"), &[], &[]);
        assert_eq!(d.channels["rpm"].value, 0.7);
        assert_eq!(d.labels["speed"].text, "99");
    }

    #[test]
    fn a_moved_slider_survives_an_edit_to_the_file() {
        let mut d = Driver::default();
        d.sync(&tree(0.4, "88"), &[], &[]);
        d.channels.get_mut("rpm").expect("the bar").value = 0.9;
        d.labels.get_mut("speed").expect("the label").text = "123".into();
        d.sync(&tree(0.7, "99"), &[], &[]);
        assert_eq!(d.channels["rpm"].value, 0.9);
        assert_eq!(d.channels["rpm"].authored(), 0.7);
        assert_eq!(d.labels["speed"].text, "123");
        assert_eq!(d.labels["speed"].authored(), "99");
        // Until a reset, which is the way back to what the file says.
        d.reset();
        assert_eq!(d.channels["rpm"].value, 0.7);
        assert_eq!(d.labels["speed"].text, "99");
    }

    #[test]
    fn apply_then_restore_leaves_the_tree_as_it_was() {
        let mut d = Driver::default();
        let mut t = tree(0.4, "88");
        d.sync(&t, &[], &[]);
        d.enabled = true;
        d.channels.get_mut("rpm").expect("the bar").value = 0.9;
        d.labels.get_mut("speed").expect("the label").text = "123".into();

        d.enabled = false;
        assert!(
            d.apply(&mut t, &[]).is_empty(),
            "a disabled driver touched the tree"
        );
        d.enabled = true;
        let saved = d.apply(&mut t, &[]);
        let rpm = t.find("rpm").expect("rpm");
        let speed = t.find("speed").expect("speed");
        assert_eq!(t.get(rpm).expect("node").kind.reading(), Some(0.9));
        assert_eq!(t.get(speed).expect("node").kind.text(), Some("123"));

        Driver::restore(&mut t, saved);
        assert_eq!(t.get(rpm).expect("node").kind.reading(), Some(0.4));
        assert_eq!(t.get(speed).expect("node").kind.text(), Some("88"));
    }

    #[test]
    fn every_gauge_kind_has_a_reading_and_a_panel_does_not() {
        let kinds = [
            bar(0.2),
            Kind::Arc {
                start: 0,
                end: 90,
                value: 0.2,
                thickness: 1,
                fill: Color::rgb(1, 1, 1),
                track: Color::rgb(2, 2, 2),
            },
            Kind::Needle {
                start: 0,
                end: 90,
                value: 0.2,
                width: 1,
                color: Color::rgb(1, 1, 1),
                hub: 0,
            },
            Kind::Led {
                color: Color::rgb(1, 1, 1),
                level: 0.2,
                glow: 0.1,
                off: Color::TRANSPARENT,
            },
        ];
        for k in kinds {
            assert_eq!(k.reading(), Some(0.2), "{k:?}");
            let changed = k.with_reading(0.8).expect("a reading to change");
            assert_eq!(changed.reading(), Some(0.8), "{k:?}");
        }
        let panel = Kind::Panel {
            background: Color::rgb(0, 0, 0),
        };
        assert_eq!(panel.reading(), None);
        assert!(panel.with_reading(0.5).is_none());
    }

    #[test]
    fn a_segbar_keeps_its_profile_when_driven() {
        // The one kind with a heap field: the value must change and the
        // curve must not go missing with it.
        let src = r#"{"width":10,"height":10,"root":{"type":"segbar","rect":[0,0,10,10],
            "value":0.2,"profile":[0.5,1.0]}}"#;
        let doc = copilot::scene::parse(src).expect("parses");
        let t = copilot::scene::build(&doc).expect("builds");
        let root = t.get(ROOT).expect("root");
        let seg = &t.get(root.children[0]).expect("the segbar").kind;
        let driven = seg.with_reading(0.8).expect("a reading");
        assert_eq!(driven.reading(), Some(0.8));
        let Kind::SegBar { profile, .. } = driven else {
            panic!("kind changed");
        };
        assert_eq!(profile, vec![0.5, 1.0]);
    }

    /// A bar bound to RPM and a label bound to coolant in °F.
    fn bound() -> (Tree, Vec<Binding>) {
        let src = r#"{"width":100,"height":100,"root":{"type":"panel","rect":[0,0,100,100],
            "children":[
              {"type":"bar","rect":[0,0,10,10],"name":"rpm","value":0.25,
               "bind":{"gauge":"RPM","max":8000}},
              {"type":"label","rect":[0,20,10,10],"text":"32",
               "bind":{"gauge":"CLNT","unit":"F","min":32,"max":212}}]}}"#;
        let doc = copilot::scene::parse(src).expect("parses");
        let s = copilot::scene::build_scene(&doc).expect("builds");
        (s.tree, s.bindings)
    }

    #[test]
    fn a_slider_reaches_a_widget_that_divides_its_reading() {
        // The tachometer digits: the widget shows hundreds, the slider must
        // still be in rpm, and the two have to agree.
        let src = r#"{"width":100,"height":100,"root":{"type":"panel","rect":[0,0,100,100],
            "children":[
              {"type":"sevenseg","rect":[0,0,10,10],"text":"12",
               "bind":{"gauge":"RPM","divide":100,"min":0,"max":80}}]}}"#;
        let doc = copilot::scene::parse(src).expect("parses");
        let s = copilot::scene::build_scene(&doc).expect("builds");
        let (mut t, b) = (s.tree, s.bindings);

        let mut d = Driver::default();
        d.sync(&t, &b, &[]);
        let ch = &d.gauges["RPM"];
        assert_eq!(
            (ch.min, ch.max),
            (0.0, 8000.0),
            "the slider is in rpm, not hundreds"
        );
        assert_eq!(
            ch.authored(),
            1200.0,
            "the file draws 12, which is 1200 rpm"
        );

        d.enabled = true;
        d.gauges.get_mut("RPM").expect("rpm").value = 4500.0;
        d.apply(&mut t, &b);
        assert_eq!(t.get(b[0].node).expect("node").kind.text(), Some("45"));
    }

    #[test]
    fn a_bound_widget_is_driven_by_its_gauge_in_the_unit_the_bus_carries() {
        let (mut t, b) = bound();
        let mut d = Driver::default();
        d.sync(&t, &b, &[]);
        // Bound widgets are the gauge's business, not a name channel's.
        assert!(!d.channels.contains_key("rpm"), "{:?}", d.channels);
        let rpm = &d.gauges["RPM"];
        assert_eq!((rpm.min, rpm.max), (0.0, 8000.0));
        assert_eq!(rpm.authored(), 2000.0, "a quarter of the bar is 2000 rpm");
        // 32..212 °F is 0..100 °C, and "32" on the label is freezing.
        let clt = &d.gauges["CLNT"];
        assert!(
            clt.min.abs() < 1e-3 && (clt.max - 100.0).abs() < 1e-3,
            "{clt:?}"
        );
        assert!(clt.authored().abs() < 1e-3, "{clt:?}");

        d.enabled = true;
        d.gauges.get_mut("RPM").expect("rpm").value = 4000.0;
        d.gauges.get_mut("CLNT").expect("clnt").value = 100.0;
        let saved = d.apply(&mut t, &b);
        let bar = t.find("rpm").expect("the bar");
        let label = b[1].node;
        assert_eq!(t.get(bar).expect("node").kind.reading(), Some(0.5));
        assert_eq!(t.get(label).expect("node").kind.text(), Some("212"));
        Driver::restore(&mut t, saved);
        assert_eq!(t.get(bar).expect("node").kind.reading(), Some(0.25));
        assert_eq!(t.get(label).expect("node").kind.text(), Some("32"));
    }
}

#[cfg(test)]
mod lamp_tests {
    use super::*;
    use copilot::asset::Scene;
    use copilot::scene::{build_scene, parse};

    /// Build a scene from its root widget, as the preview does.
    fn scene(root: &str) -> Scene {
        let src = format!(r#"{{"width":100,"height":100,"root":{root}}}"#);
        build_scene(&parse(&src).expect("must parse")).expect("must build")
    }

    /// A panel holding a led on `OIL_PRESSURE` and a label on `LOW_FUEL`.
    fn lamps_scene() -> Scene {
        scene(
            r#"{"type":"panel","rect":[0,0,100,100],"children":[
                 {"type":"led","rect":[0,0,10,10],"name":"oil","level":0.0,
                  "bind":{"indicator":"OIL_PRESSURE"}},
                 {"type":"label","rect":[0,20,60,10],"name":"fuel","text":"LOW FUEL",
                  "visible":false,
                  "bind":{"indicator":"LOW_FUEL","property":"visible"}}]}"#,
        )
    }

    #[test]
    fn a_bound_lamp_gets_a_switch_and_its_bank_gets_no_slider() {
        // One slider on INDICATORS1 would drive all thirty-two of its lamps
        // together, which is not a thing anybody wants to do.
        let s = lamps_scene();
        let mut d = Driver::default();
        d.sync(&s.tree, &s.bindings, &[]);
        assert_eq!(d.lamps.len(), 2);
        assert!(d.lamps.contains_key("OIL_PRESSURE"));
        assert!(d.lamps.contains_key("LOW_FUEL"));
        assert!(
            d.gauges.is_empty(),
            "a tell-tale bank turned up as a gauge slider: {:?}",
            d.gauges.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_lamp_opens_at_what_the_file_draws() {
        // Switching the driver on must change nothing until a switch moves.
        let s = scene(
            r#"{"type":"led","rect":[0,0,10,10],"level":1.0,
                "bind":{"indicator":"ABS"}}"#,
        );
        let mut d = Driver::default();
        d.sync(&s.tree, &s.bindings, &[]);
        assert_eq!(d.lamps.get("ABS"), Some(&true), "the file draws it lit");
    }

    #[test]
    fn flicking_a_switch_lights_the_widget_it_drives() {
        let mut s = lamps_scene();
        let mut d = Driver::default();
        d.sync(&s.tree, &s.bindings, &[]);
        d.enabled = true;
        *d.lamps.get_mut("OIL_PRESSURE").expect("a switch") = true;

        let saved = d.apply(&mut s.tree, &s.bindings);
        let oil = s.tree.find("oil").expect("the led");
        assert_eq!(
            s.tree.get(oil).expect("it exists").kind.reading(),
            Some(1.0)
        );
        Driver::restore(&mut s.tree, saved);
        assert_eq!(
            s.tree.get(oil).expect("it exists").kind.reading(),
            Some(0.0)
        );
    }

    #[test]
    fn a_lamp_that_hides_a_widget_is_put_back_when_the_frame_is_done() {
        // The bug this guards against: a `visible` binding moves the flag and
        // leaves the kind alone, so a restore that only put the kind back
        // would leave the widget hidden for good.
        let mut s = lamps_scene();
        let mut d = Driver::default();
        d.sync(&s.tree, &s.bindings, &[]);
        d.enabled = true;
        *d.lamps.get_mut("LOW_FUEL").expect("a switch") = true;

        let fuel = s.tree.find("fuel").expect("the warning");
        assert!(!s.tree.get(fuel).unwrap().visible, "the file hides it");
        let saved = d.apply(&mut s.tree, &s.bindings);
        assert!(s.tree.get(fuel).unwrap().visible, "the switch showed it");
        Driver::restore(&mut s.tree, saved);
        assert!(
            !s.tree.get(fuel).unwrap().visible,
            "the preview kept the driver's visibility after the frame"
        );
    }

    #[test]
    fn a_lamp_that_leaves_the_scene_loses_its_switch() {
        let mut d = Driver::default();
        let s = lamps_scene();
        d.sync(&s.tree, &s.bindings, &[]);
        assert_eq!(d.lamps.len(), 2);

        let fewer =
            scene(r#"{"type":"led","rect":[0,0,10,10],"bind":{"indicator":"OIL_PRESSURE"}}"#);
        d.sync(&fewer.tree, &fewer.bindings, &[]);
        assert_eq!(d.lamps.len(), 1);
        assert!(d.lamps.contains_key("OIL_PRESSURE"));
    }

    #[test]
    fn a_switch_keeps_its_position_across_a_reload() {
        // Editing the file must not put out a lamp somebody is looking at.
        let mut d = Driver::default();
        let s = lamps_scene();
        d.sync(&s.tree, &s.bindings, &[]);
        *d.lamps.get_mut("OIL_PRESSURE").unwrap() = true;
        d.sync(&s.tree, &s.bindings, &[]);
        assert_eq!(d.lamps.get("OIL_PRESSURE"), Some(&true));
    }

    #[test]
    fn resetting_the_channels_leaves_the_lamps_alone() {
        // A reset that put every tell-tale out would undo the one thing a
        // person checking a dark panel is doing.
        let mut d = Driver::default();
        let s = lamps_scene();
        d.sync(&s.tree, &s.bindings, &[]);
        *d.lamps.get_mut("OIL_PRESSURE").unwrap() = true;
        d.reset();
        assert_eq!(d.lamps.get("OIL_PRESSURE"), Some(&true));
    }

    #[test]
    fn a_disabled_driver_touches_neither_lamp_nor_widget() {
        let mut s = lamps_scene();
        let mut d = Driver::default();
        d.sync(&s.tree, &s.bindings, &[]);
        *d.lamps.get_mut("OIL_PRESSURE").unwrap() = true;
        assert!(d.apply(&mut s.tree, &s.bindings).is_empty());
        let oil = s.tree.find("oil").unwrap();
        assert_eq!(s.tree.get(oil).unwrap().kind.reading(), Some(0.0));
    }
}
