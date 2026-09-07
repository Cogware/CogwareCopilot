// SPDX-License-Identifier: MIT OR Apache-2.0
//! What a widget's properties are, and what they currently hold.
//!
//! One table rather than a match arm per property in the panel code, because
//! the failure this guards against is silent: a widget gains a field, the
//! parser learns it, the scene format documents it, and the inspector simply
//! never shows it. Nobody notices until someone asks why a needle's hub can
//! only be changed by typing JSON.
//!
//! Descriptions and current values come back together, from the one place, so
//! there is no second match to drift out of step with the first.

use copilot::Color;
use copilot::widget::{Align, Kind, VAlign};

/// One editable property, named as the scene format names it.
#[derive(Clone, PartialEq, Debug)]
pub enum Value {
    /// An RGBA colour.
    Color(Color),
    /// A run of text.
    Text(String),
    /// A 0.0..=1.0 fraction.
    Frac(f32),
    /// A fraction that may sit outside 0.0..=1.0, such as a band threshold
    /// parked above the top of a scale to switch the band off.
    Ratio(f32),
    /// A count or a pixel measurement.
    Count(u32),
    /// A whole number that may be negative, such as an angle.
    Whole(i32),
    /// A yes or no.
    Flag(bool),
    /// One of a fixed set of words.
    Choice(&'static str, &'static [&'static str]),
}

impl Value {
    /// The property as it should be written into the scene file.
    ///
    /// Formatting lives with the value rather than at the call site so that a
    /// colour is spelled the same way everywhere. A colour written as
    /// `#rrggbb` in one place and `#rrggbbaa` in another is a file that looks
    /// like two people wrote it, and the alpha is exactly the part someone
    /// later assumes is there.
    #[must_use]
    pub fn to_scene(&self) -> String {
        match self {
            Value::Color(c) => format!("\"#{:02x}{:02x}{:02x}{:02x}\"", c.r, c.g, c.b, c.a),
            Value::Text(t) => {
                let mut out = String::with_capacity(t.len() + 2);
                out.push('"');
                for ch in t.chars() {
                    match ch {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\t' => out.push_str("\\t"),
                        // A control character would end the document at the
                        // lexer rather than at the field that holds it.
                        c if (c as u32) < 0x20 => {
                            out.push_str(&format!("\\u{:04x}", c as u32));
                        }
                        c => out.push(c),
                    }
                }
                out.push('"');
                out
            }
            // Two decimals: a fraction dragged in a slider is a human's
            // approximation, and writing seven digits of float noise into a
            // file people read is how a scene stops being legible.
            Value::Frac(f) | Value::Ratio(f) => format!("{f:.2}"),
            Value::Count(n) => n.to_string(),
            Value::Whole(n) => n.to_string(),
            Value::Flag(b) => b.to_string(),
            Value::Choice(word, _) => format!("\"{word}\""),
        }
    }
}

/// The words a label's alignment may take, in the scene format's spelling.
const ALIGNMENTS: &[&str] = &["left", "center", "right"];

/// The words a label's vertical alignment may take.
const VALIGNMENTS: &[&str] = &["top", "middle", "bottom"];

fn valign_word(a: VAlign) -> &'static str {
    match a {
        VAlign::Top => "top",
        VAlign::Middle => "middle",
        VAlign::Bottom => "bottom",
    }
}

fn align_word(a: Align) -> &'static str {
    match a {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
    }
}

/// Every editable property of `kind`, with what it holds now.
///
/// `rect` and `name` are absent on purpose: they belong to every widget alike
/// and the panel gives them their own row, where a kind's own properties are
/// what changes from one selection to the next.
#[must_use]
pub fn fields(kind: &Kind) -> Vec<(&'static str, Value)> {
    match kind {
        Kind::Panel { background } => vec![("background", Value::Color(*background))],
        Kind::Frame { color } => vec![("color", Value::Color(*color))],
        Kind::Label {
            text,
            color,
            scale,
            align,
            valign,
        } => vec![
            ("text", Value::Text(text.clone())),
            ("color", Value::Color(*color)),
            ("scale", Value::Count(u32::from(*scale))),
            ("align", Value::Choice(align_word(*align), ALIGNMENTS)),
            ("valign", Value::Choice(valign_word(*valign), VALIGNMENTS)),
        ],
        Kind::Image { image } => vec![("image", Value::Count(*image))],
        Kind::Anim {
            anim,
            frame,
            playing,
            speed,
            ..
        } => vec![
            ("anim", Value::Count(*anim)),
            ("frame", Value::Count(*frame)),
            ("playing", Value::Flag(*playing)),
            // Speed is not a fraction: half is 0.5 and double is 2.0, and
            // clamping it to one would take away the faster half.
            ("speed", Value::Frac(*speed)),
        ],
        Kind::Bar {
            value,
            fill,
            track,
            vertical,
        } => vec![
            ("value", Value::Frac(*value)),
            ("fill", Value::Color(*fill)),
            ("track", Value::Color(*track)),
            ("vertical", Value::Flag(*vertical)),
        ],
        Kind::Arc {
            start,
            end,
            value,
            thickness,
            fill,
            track,
        } => vec![
            ("start", Value::Whole(*start)),
            ("end", Value::Whole(*end)),
            ("value", Value::Frac(*value)),
            ("thickness", Value::Count(*thickness)),
            ("fill", Value::Color(*fill)),
            ("track", Value::Color(*track)),
        ],
        Kind::Needle {
            start,
            end,
            value,
            width,
            color,
            hub,
        } => vec![
            ("start", Value::Whole(*start)),
            ("end", Value::Whole(*end)),
            ("value", Value::Frac(*value)),
            ("width", Value::Count(*width)),
            ("color", Value::Color(*color)),
            ("hub", Value::Count(*hub)),
        ],
        Kind::Scale {
            start,
            end,
            ticks,
            major_every,
            length,
            width,
            color,
            major_color,
        } => vec![
            ("start", Value::Whole(*start)),
            ("end", Value::Whole(*end)),
            ("ticks", Value::Count(*ticks)),
            ("major_every", Value::Count(*major_every)),
            ("length", Value::Count(*length)),
            ("width", Value::Count(*width)),
            ("color", Value::Color(*color)),
            ("major_color", Value::Color(*major_color)),
        ],
        Kind::SevenSeg {
            text,
            color,
            ghost,
            thickness,
            digits,
        } => vec![
            ("text", Value::Text(text.clone())),
            ("color", Value::Color(*color)),
            ("ghost", Value::Color(*ghost)),
            ("thickness", Value::Count(*thickness)),
            ("digits", Value::Count(*digits)),
        ],
        Kind::Gradient { from, to, vertical } => vec![
            ("from", Value::Color(*from)),
            ("to", Value::Color(*to)),
            ("vertical", Value::Flag(*vertical)),
        ],
        Kind::Ruler {
            ticks,
            major_every,
            length,
            width,
            color,
            major_color,
            vertical,
        } => vec![
            ("ticks", Value::Count(*ticks)),
            ("major_every", Value::Count(*major_every)),
            ("length", Value::Count(*length)),
            ("width", Value::Count(*width)),
            ("color", Value::Color(*color)),
            ("major_color", Value::Color(*major_color)),
            ("vertical", Value::Flag(*vertical)),
        ],
        Kind::SegBar {
            value,
            segments,
            gap,
            fill,
            track,
            warn,
            warn_fill,
            danger,
            danger_fill,
            vertical,
            height,
            divisions,
            div_gap,
            ..
        } => vec![
            ("value", Value::Frac(*value)),
            ("segments", Value::Count(*segments)),
            ("gap", Value::Count(*gap)),
            ("fill", Value::Color(*fill)),
            ("track", Value::Color(*track)),
            // Not `Frac`: a band is switched off by putting it above the top
            // of the scale, and a slider capped at 1.0 could not say that.
            ("warn", Value::Ratio(*warn)),
            ("warn_fill", Value::Color(*warn_fill)),
            ("danger", Value::Ratio(*danger)),
            ("danger_fill", Value::Color(*danger_fill)),
            ("vertical", Value::Flag(*vertical)),
            // The second axis: how far up its envelope each lit cell stands.
            ("height", Value::Frac(*height)),
            ("divisions", Value::Count(*divisions)),
            ("div_gap", Value::Count(*div_gap)),
            // `profile` is absent for the reason `points` is: a curve is a row
            // of numbers, and neither a column of drag boxes nor the text pane
            // is a good place to shape one. Both are edited on the preview
            // instead -- see `crate::curve`.
        ],
        Kind::Grid {
            pitch_x,
            pitch_y,
            width,
            color,
        } => vec![
            ("pitch_x", Value::Count(*pitch_x)),
            ("pitch_y", Value::Count(*pitch_y)),
            ("width", Value::Count(*width)),
            ("color", Value::Color(*color)),
        ],
        Kind::Led { color, level, glow } => vec![
            ("color", Value::Color(*color)),
            ("level", Value::Frac(*level)),
            ("glow", Value::Frac(*glow)),
        ],
        Kind::RoundRect { background, radius } => vec![
            ("background", Value::Color(*background)),
            ("radius", Value::Count(*radius)),
        ],
        Kind::Line {
            width,
            color,
            closed,
            ..
        } => vec![
            // `points` is absent: a curve is dozens of numbers and a row of
            // drag boxes is a worse way to edit it than the text pane, which
            // is right there.
            ("width", Value::Count(*width)),
            ("color", Value::Color(*color)),
            ("closed", Value::Flag(*closed)),
        ],
        Kind::Polygon { color, .. } => vec![("color", Value::Color(*color))],
        Kind::Chart {
            width,
            stroke,
            fill,
            ..
        } => vec![
            ("width", Value::Count(*width)),
            ("stroke", Value::Color(*stroke)),
            ("fill", Value::Color(*fill)),
        ],
        // `Kind` is `#[non_exhaustive]`, so a newer copilot can carry a widget
        // this editor has never heard of. Showing no properties beats refusing
        // to build against a library that grew.
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind the editor offers, by the name the scene format uses.
    const KINDS: &[&str] = &[
        "panel",
        "roundrect",
        "frame",
        "label",
        "bar",
        "led",
        "arc",
        "needle",
        "scale",
        "line",
        "chart",
        "image",
        "anim",
        "sevenseg",
        "gradient",
        "polygon",
        "ruler",
        "segbar",
        "grid",
    ];

    /// A minimal document holding one widget of `kind`, with `set` overriding
    /// or adding one field.
    ///
    /// The required fields are supplied only when the override does not
    /// already carry them: a key written twice is read once, and the test
    /// would be measuring the parser's choice of duplicate rather than
    /// whether the inspector's name was understood.
    fn doc(kind: &str, set: Option<(&str, String)>) -> String {
        let required: &[(&str, &str)] = match kind {
            "label" | "sevenseg" => &[("text", "\"x\"")],
            "image" => &[("image", "0")],
            "anim" => &[("anim", "0")],
            _ => &[],
        };
        let mut body = String::new();
        for (k, v) in required {
            if set.as_ref().is_some_and(|(n, _)| n == k) {
                continue;
            }
            body.push_str(&format!(r#","{k}":{v}"#));
        }
        if let Some((k, v)) = &set {
            body.push_str(&format!(r#","{k}":{v}"#));
        }
        format!(
            r#"{{"width":100,"height":100,"root":{{"type":"panel","rect":[0,0,100,100],
               "children":[{{"type":"{kind}","rect":[0,0,50,50]{body}}}]}}}}"#
        )
    }

    /// The kind of the single child, built from source.
    fn kind_of(src: &str) -> Kind {
        let parsed = copilot::scene::parse(src).expect("test document must parse");
        let tree = copilot::scene::build(&parsed).expect("test document must build");
        let root = tree.get(copilot::widget::ROOT).expect("a root");
        let outer = tree.get(root.children[0]).expect("the document's root");
        tree.get(outer.children[0])
            .expect("the widget")
            .kind
            .clone()
    }

    /// A value of the same shape but a different content, so that writing it
    /// and reading it back proves the field name was understood.
    fn distinct(v: &Value) -> Value {
        match v {
            Value::Color(c) => Value::Color(if c.r == 0x11 {
                Color::rgba(0x22, 0x33, 0x44, 0x55)
            } else {
                Color::rgba(0x11, 0x22, 0x33, 0x44)
            }),
            Value::Text(t) => Value::Text(if t == "zz" { "yy".into() } else { "zz".into() }),
            Value::Frac(f) => Value::Frac(if (*f - 0.75).abs() < 1e-6 { 0.25 } else { 0.75 }),
            Value::Ratio(f) => Value::Ratio(if (*f - 0.75).abs() < 1e-6 { 0.25 } else { 0.75 }),
            Value::Count(n) => Value::Count(if *n == 7 { 9 } else { 7 }),
            Value::Whole(n) => Value::Whole(if *n == -37 { 41 } else { -37 }),
            Value::Flag(b) => Value::Flag(!*b),
            Value::Choice(w, all) => {
                let other = all.iter().find(|o| *o != w).expect("two choices");
                Value::Choice(other, all)
            }
        }
    }

    #[test]
    fn every_kind_the_palette_offers_has_properties() {
        for k in KINDS {
            // `frame` has one and `panel` has one; none should have none, or
            // selecting it shows an empty inspector and looks broken.
            assert!(
                !fields(&kind_of(&doc(k, None))).is_empty(),
                "{k} exposes nothing to edit"
            );
        }
    }

    #[test]
    fn every_property_the_inspector_offers_is_one_the_parser_accepts() {
        // The failure this catches is a typo in a field name: the inspector
        // writes it, the parser ignores it, and the value silently does not
        // change. Writing a *different* value and reading it back is what
        // makes that visible.
        for k in KINDS {
            for (name, before) in fields(&kind_of(&doc(k, None))) {
                let want = distinct(&before);
                let src = doc(k, Some((name, want.to_scene())));
                let parsed = copilot::scene::parse(&src)
                    .unwrap_or_else(|e| panic!("{k}.{name} did not parse: {e:?}"));
                let tree = copilot::scene::build(&parsed)
                    .unwrap_or_else(|e| panic!("{k}.{name} did not build: {e:?}"));
                let root = tree.get(copilot::widget::ROOT).expect("a root");
                let outer = tree.get(root.children[0]).expect("the document's root");
                let after = &tree.get(outer.children[0]).expect("the widget").kind;

                let got = fields(after)
                    .into_iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| v)
                    .unwrap_or_else(|| panic!("{k}.{name} vanished after a round trip"));
                assert_eq!(
                    got.to_scene(),
                    want.to_scene(),
                    "{k}.{name} did not take: wrote {}, read {}",
                    want.to_scene(),
                    got.to_scene()
                );
            }
        }
    }

    #[test]
    fn a_colour_is_written_with_its_alpha() {
        // Without it a half-transparent track reads back opaque, and the
        // widget changes the moment anyone touches an unrelated field.
        let v = Value::Color(Color::rgba(0x12, 0x34, 0x56, 0x78));
        assert_eq!(v.to_scene(), "\"#12345678\"");
    }

    #[test]
    fn a_quote_in_a_label_does_not_end_the_string() {
        let v = Value::Text(String::from("say \"hi\"\\ now"));
        let src = format!(
            r#"{{"width":10,"height":10,"root":{{"type":"label",
                              "rect":[0,0,10,10],"text":{}}}}}"#,
            v.to_scene()
        );
        let parsed = copilot::scene::parse(&src).expect("an escaped quote must survive");
        let tree = copilot::scene::build(&parsed).expect("and must build");
        let root = tree.get(copilot::widget::ROOT).expect("a root");
        let Kind::Label { text, .. } = &tree.get(root.children[0]).expect("the label").kind else {
            panic!("expected a label");
        };
        assert_eq!(text, "say \"hi\"\\ now");
    }

    #[test]
    fn a_newline_in_a_label_survives_the_round_trip() {
        let v = Value::Text(String::from("a\nb\tc"));
        assert_eq!(v.to_scene(), "\"a\\nb\\tc\"");
    }

    #[test]
    fn a_fraction_is_written_short_enough_to_read() {
        assert_eq!(Value::Frac(0.123_456_79).to_scene(), "0.12");
        assert_eq!(Value::Frac(1.0).to_scene(), "1.00");
    }

    #[test]
    fn an_unknown_kind_offers_nothing_rather_than_panicking() {
        // `Kind` is non-exhaustive; a library that grew must not take the
        // editor down with it.
        let empty = fields(&Kind::Panel {
            background: Color::TRANSPARENT,
        });
        assert_eq!(empty.len(), 1);
    }
}
