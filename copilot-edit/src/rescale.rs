// SPDX-License-Identifier: GPL-3.0-only
//! Moving a whole scene to a different resolution.
//!
//! A cluster authored for one panel is worth keeping when the panel changes,
//! and retyping four numbers per widget is how a layout quietly stops being
//! the layout that was designed. This rewrites the file in place, so the
//! comments and the key order the author chose survive the move.
//!
//! Text splicing rather than a round trip through the value tree, for the same
//! reason [`crate::App::set_field`] does it: a document rebuilt from its parsed
//! form comes back without a single comment in it.

use copilot::scene::{Step, locate};
use copilot::widget::{Kind, NodeId, Tree};

/// A field holding a pixel measurement, and how it scales.
#[derive(Clone, Copy)]
enum Measure {
    /// A thickness, which has no axis of its own.
    Thickness,
    /// An integer font magnification, which must stay at least 1.
    Magnification,
}

/// The pixel-valued fields of each widget kind.
///
/// Listed rather than inferred: leaving a ring's thickness or a label's
/// magnification behind is what makes a rescaled cluster look wrong in a way
/// that is hard to name -- everything is in the right place and none of it is
/// the right weight.
fn measures(kind: &Kind) -> &'static [(&'static str, Measure)] {
    match kind {
        Kind::Label { .. } => &[("scale", Measure::Magnification)],
        Kind::Arc { .. } => &[("thickness", Measure::Thickness)],
        Kind::Needle { .. } => &[("width", Measure::Thickness), ("hub", Measure::Thickness)],
        Kind::Scale { .. } => &[
            ("length", Measure::Thickness),
            ("width", Measure::Thickness),
        ],
        Kind::Line { .. } | Kind::Chart { .. } => &[("width", Measure::Thickness)],
        Kind::RoundRect { .. } => &[("radius", Measure::Thickness)],
        _ => &[],
    }
}

/// Scale `v` by `num / den`, rounding to nearest rather than truncating.
///
/// Truncation would shrink every widget by up to a pixel and, applied to a
/// column of them, would open a visible gap at the bottom of the panel that
/// was not there before.
fn scaled(v: i64, num: u32, den: u32) -> i64 {
    let den = i64::from(den.max(1));
    let half = den / 2;
    let n = v * i64::from(num);
    if n >= 0 {
        (n + half) / den
    } else {
        (n - half) / den
    }
}

/// Rewrite `src` so a scene authored at `from` fills `to` instead.
///
/// Returns `None` if the document has no root, or if any rect it holds is not
/// the four numbers a rect has to be. Nothing is written on failure: a partly
/// rescaled scene is worse than an unchanged one, because the damage is
/// scattered through a file the author then has to audit by eye.
pub fn rescale(src: &str, tree: &Tree, from: (u32, u32), to: (u32, u32)) -> Option<String> {
    if from.0 == 0 || from.1 == 0 {
        return None;
    }

    // Every edit as a (span, replacement), applied back to front so that
    // splicing one does not shift the offsets of those not yet done.
    let mut edits: Vec<(core::ops::Range<usize>, String)> = Vec::new();

    for (path, id) in walk(tree) {
        let node = tree.get(id)?;

        let mut rect_path = path.clone();
        rect_path.push(Step::Key("rect"));
        if let Some(span) = locate(src, &rect_path) {
            let r = node.rect;
            let x = scaled(i64::from(r.left()), to.0, from.0);
            let y = scaled(i64::from(r.top()), to.1, from.1);
            let w = scaled(i64::from(r.size.w), to.0, from.0).max(1);
            let h = scaled(i64::from(r.size.h), to.1, from.1).max(1);
            edits.push((span.start..span.end, format!("[{x}, {y}, {w}, {h}]")));
        }

        for &(field, how) in measures(&node.kind) {
            let mut p = path.clone();
            p.push(Step::Key(field));
            let Some(span) = locate(src, &p) else {
                continue;
            };
            let current: i64 = src.get(span.start..span.end)?.trim().parse().ok()?;
            let out = match how {
                // A thickness belongs to no axis, so it follows the smaller
                // ratio: a ring that grew with the wider one would spill past
                // the edge it was drawn to sit inside.
                Measure::Thickness => {
                    let a = scaled(current, to.0, from.0);
                    let b = scaled(current, to.1, from.1);
                    a.min(b)
                }
                // Magnification is an integer count of pixels per pixel, so it
                // cannot go below one without the text vanishing.
                Measure::Magnification => {
                    scaled(current, to.0.min(to.1), from.0.min(from.1)).max(1)
                }
            };
            edits.push((span.start..span.end, out.max(0).to_string()));
        }
    }

    for (key, value) in [("width", to.0), ("height", to.1)] {
        if let Some(span) = locate(src, &[Step::Key(key)]) {
            edits.push((span.start..span.end, value.to_string()));
        }
    }

    edits.sort_by_key(|(span, _)| core::cmp::Reverse(span.start));
    let mut out = String::from(src);
    for (span, text) in edits {
        out.replace_range(span, &text);
    }
    Some(out)
}

/// Every node below the root, with the document path that reaches it.
///
/// The root itself is skipped: it is the tree's own frame, not a widget the
/// file declares, and it has no span to rewrite.
fn walk(tree: &Tree) -> Vec<(Vec<Step<'static>>, NodeId)> {
    let mut out = Vec::new();
    for i in 0..tree.len() as u32 {
        let id = NodeId(i);
        let Some(indices) = tree.document_path(id) else {
            continue;
        };
        let mut path = vec![Step::Key("root")];
        for step in indices {
            path.push(Step::Key("children"));
            path.push(Step::Index(step));
        }
        out.push((path, id));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tree from source, the way the editor does.
    fn tree_of(src: &str) -> Tree {
        let doc = copilot::scene::parse(src).expect("test document must parse");
        copilot::scene::build(&doc).expect("test document must build")
    }

    fn go(src: &str, from: (u32, u32), to: (u32, u32)) -> String {
        rescale(src, &tree_of(src), from, to).expect("rescale must succeed")
    }

    const DOC: &str = r#"{
  // a comment that must survive
  "width": 100,
  "height": 50,
  "root": {
    "type": "panel",
    "rect": [0, 0, 100, 50],
    "children": [
      { "type": "label", "rect": [10, 5, 40, 20], "text": "hi", "scale": 2 },
      { "type": "arc", "rect": [50, 0, 50, 50], "thickness": 4 },
    ],
  },
}"#;

    #[test]
    fn doubling_doubles_every_rect() {
        let out = go(DOC, (100, 50), (200, 100));
        assert!(out.contains("[0, 0, 200, 100]"), "{out}");
        assert!(out.contains("[20, 10, 80, 40]"), "{out}");
        assert!(out.contains("[100, 0, 100, 100]"), "{out}");
    }

    #[test]
    fn the_declared_size_is_rewritten_too() {
        let out = go(DOC, (100, 50), (200, 100));
        assert!(out.contains(r#""width": 200"#), "{out}");
        assert!(out.contains(r#""height": 100"#), "{out}");
    }

    #[test]
    fn comments_and_formatting_survive() {
        let out = go(DOC, (100, 50), (200, 100));
        assert!(
            out.contains("// a comment that must survive"),
            "the comment went missing"
        );
        assert!(out.contains("\"type\": \"panel\""), "key order changed");
    }

    #[test]
    fn a_rescaled_document_still_builds() {
        let out = go(DOC, (100, 50), (640, 480));
        let doc = copilot::scene::parse(&out).expect("rescaled scene must parse");
        copilot::scene::build(&doc).expect("rescaled scene must build");
    }

    #[test]
    fn a_label_magnification_grows_with_the_panel() {
        let out = go(DOC, (100, 50), (400, 200));
        assert!(out.contains(r#""scale": 8"#), "{out}");
    }

    #[test]
    fn a_magnification_never_reaches_zero() {
        // Shrinking hard would round a scale of 2 to nothing, and a label
        // magnified zero times is an invisible label.
        let out = go(DOC, (100, 50), (4, 2));
        assert!(out.contains(r#""scale": 1"#), "{out}");
    }

    #[test]
    fn a_thickness_follows_the_smaller_ratio() {
        // Wider but no taller: a ring that grew with the width would spill
        // past the edge it was drawn to sit inside.
        let out = go(DOC, (100, 50), (400, 50));
        assert!(out.contains(r#""thickness": 4"#), "{out}");
    }

    #[test]
    fn a_size_never_rounds_away_to_nothing() {
        let src = r#"{"width":100,"height":100,
                      "root":{"type":"panel","rect":[0,0,100,100],"children":[
                        {"type":"panel","rect":[1,1,1,1]}]}}"#;
        let out = go(src, (100, 100), (10, 10));
        assert!(out.contains("[0, 0, 1, 1]"), "{out}");
    }

    #[test]
    fn rescaling_by_one_changes_nothing_but_stays_valid() {
        let out = go(DOC, (100, 50), (100, 50));
        assert!(out.contains("[10, 5, 40, 20]"), "{out}");
        assert!(out.contains(r#""scale": 2"#), "{out}");
        assert!(out.contains(r#""thickness": 4"#), "{out}");
    }

    #[test]
    fn a_round_trip_returns_to_where_it_started() {
        let up = go(DOC, (100, 50), (300, 150));
        let back = go(&up, (300, 150), (100, 50));
        assert!(back.contains("[10, 5, 40, 20]"), "{back}");
        assert!(back.contains(r#""width": 100"#), "{back}");
    }

    #[test]
    fn a_zero_source_dimension_is_refused() {
        let t = tree_of(DOC);
        assert!(rescale(DOC, &t, (0, 50), (200, 100)).is_none());
        assert!(rescale(DOC, &t, (100, 0), (200, 100)).is_none());
    }

    #[test]
    fn a_rounding_scale_lands_on_the_nearest_pixel() {
        // 10 at 3.5x is 35; 5 at 3.5x is 17.5, which must round to 18 rather
        // than truncate to 17.
        let out = go(DOC, (100, 50), (350, 175));
        assert!(out.contains("[35, 18, 140, 70]"), "{out}");
    }

    #[test]
    fn an_enormous_target_does_not_overflow() {
        let t = tree_of(DOC);
        let _ = rescale(DOC, &t, (1, 1), (u32::MAX, u32::MAX));
        let _ = rescale(DOC, &t, (u32::MAX, u32::MAX), (1, 1));
    }
}
