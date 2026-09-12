// SPDX-License-Identifier: GPL-3.0-only
//! Tests for the commands, driven through `App` with no window in sight.
//!
//! The editor's riskiest code is the part that rearranges a document by
//! splicing its text, and none of it needs a context, a surface or a frame to
//! run. Keeping these here rather than in a `mod tests` at the end of one very
//! long file is what stops the file being long.

mod moving;

use super::*;
use copilot::widget::Tree;

/// An editor holding `src`, with the widget called `name` selected.
pub(crate) fn app(src: &str, name: &str) -> App {
    let mut a = App::new(None, &[]);
    a.text = src.to_string();
    a.reload();
    a.selected = a.preview.tree.as_ref().and_then(|t| t.find(name));
    assert!(a.selected.is_some(), "`{name}` is not in the test document");
    a
}

/// The names of every node, depth first, each with its depth.
pub(crate) fn shape(a: &App) -> Vec<(usize, String)> {
    fn walk(t: &Tree, id: NodeId, depth: usize, out: &mut Vec<(usize, String)>) {
        if let Some(n) = t.get(id) {
            if depth > 0 {
                out.push((depth, n.name.clone().unwrap_or_default()));
            }
            for c in &n.children {
                walk(t, *c, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    if let Some(t) = &a.preview.tree {
        walk(t, copilot::widget::ROOT, 0, &mut out);
    }
    out
}

const DOC: &str = r#"{
  "width": 100, "height": 100,
  "root": {
    "type": "panel", "rect": [0, 0, 100, 100], "name": "root",
    "children": [
      { "type": "panel", "rect": [0, 0, 40, 40], "name": "box",
        "children": [
          { "type": "bar", "rect": [1, 1, 5, 5], "name": "inner" },
        ] },
      { "type": "bar", "rect": [50, 0, 10, 10], "name": "loose" },
    ],
  },
}"#;

#[test]
fn moving_inwards_makes_it_a_child_of_the_widget_above() {
    let mut a = app(DOC, "loose");
    a.reparent(true);
    assert_eq!(
        shape(&a),
        vec![
            (1, "root".into()),
            (2, "box".into()),
            (3, "inner".into()),
            (3, "loose".into()),
        ],
        "{}",
        a.status
    );
}

#[test]
fn moving_outwards_makes_it_a_sibling_of_its_parent() {
    let mut a = app(DOC, "inner");
    a.reparent(false);
    assert_eq!(
        shape(&a),
        vec![
            (1, "root".into()),
            (2, "box".into()),
            (2, "inner".into()),
            (2, "loose".into()),
        ],
        "{}",
        a.status
    );
}

#[test]
fn moving_inwards_and_back_out_returns_the_shape_it_started_with() {
    let before = shape(&app(DOC, "loose"));
    let mut a = app(DOC, "loose");
    a.reparent(true);
    a.selected = a.preview.tree.as_ref().and_then(|t| t.find("loose"));
    a.reparent(false);
    assert_eq!(shape(&a), before, "{}", a.status);
}

#[test]
fn moving_inwards_creates_a_child_list_that_was_never_written() {
    // Most widgets in a scene are leaves with no `children` key at all.
    // Refusing to accept a drop until somebody types an empty array by
    // hand would make this useless for exactly the case it is for.
    let src = r#"{"width":100,"height":100,
          "root":{"type":"panel","rect":[0,0,100,100],"name":"root","children":[
            {"type":"bar","rect":[0,0,9,9],"name":"leaf"},
            {"type":"bar","rect":[9,0,9,9],"name":"mover"}]}}"#;
    let mut a = app(src, "mover");
    a.reparent(true);
    assert_eq!(
        shape(&a),
        vec![(1, "root".into()), (2, "leaf".into()), (3, "mover".into())],
        "{}",
        a.status
    );
    assert!(a.text.contains("\"children\""), "no list was written");
}

#[test]
fn the_first_child_has_nothing_above_it_to_move_into() {
    let mut a = app(DOC, "box");
    let before = shape(&a);
    a.reparent(true);
    assert_eq!(shape(&a), before, "it moved anyway");
    assert!(a.status.contains("nothing above"), "{}", a.status);
}

#[test]
fn a_top_level_widget_cannot_move_further_out() {
    let mut a = app(DOC, "box");
    let before = shape(&a);
    a.reparent(false);
    assert_eq!(shape(&a), before, "it moved anyway");
    assert!(a.status.contains("already at the top"), "{}", a.status);
}

#[test]
fn a_move_keeps_the_comments_around_it() {
    let src = r#"{
  // a comment that must survive
  "width": 100, "height": 100,
  "root": { "type": "panel", "rect": [0,0,100,100], "name": "root", "children": [
    { "type": "bar", "rect": [0,0,9,9], "name": "leaf" },
    // and this one
    { "type": "bar", "rect": [9,0,9,9], "name": "mover" },
  ] },
}"#;
    let mut a = app(src, "mover");
    a.reparent(true);
    assert!(
        a.text.contains("// a comment that must survive"),
        "{}",
        a.text
    );
    assert!(a.text.contains("// and this one"), "{}", a.text);
}

#[test]
fn a_move_is_one_undo_away() {
    let mut a = app(DOC, "loose");
    let before = shape(&a);
    a.reparent(true);
    assert_ne!(shape(&a), before);
    a.undo();
    assert_eq!(shape(&a), before, "undo did not put it back");
}

#[test]
fn a_failed_move_leaves_the_text_untouched() {
    let mut a = app(DOC, "box");
    let text = a.text.clone();
    a.reparent(false);
    assert_eq!(a.text, text, "a refused move still edited the file");
}

#[test]
fn adding_a_child_to_a_leaf_creates_its_list() {
    let src = r#"{"width":100,"height":100,
          "root":{"type":"panel","rect":[0,0,100,100],"name":"root","children":[
            {"type":"panel","rect":[0,0,50,50],"name":"leaf"}]}}"#;
    let mut a = app(src, "leaf");
    a.add_child("bar");
    assert_eq!(
        shape(&a).len(),
        3,
        "expected root, leaf and the new bar: {}",
        a.status
    );
}

// --- selection ---

#[test]
fn a_click_on_the_background_selects_nothing() {
    // The document root covers the whole scene, so a bare hit test answers
    // it for every click that lands on nothing else, and there would be no
    // way to end up with nothing selected.
    let a = app(DOC, "loose");
    assert_eq!(a.pick(copilot::Point { x: 90, y: 90 }), None);
    assert_eq!(
        a.pick(copilot::Point { x: 52, y: 5 }),
        a.preview.tree.as_ref().and_then(|t| t.find("loose"))
    );
}

#[test]
fn deselecting_clears_the_selection_and_says_so() {
    let mut a = app(DOC, "loose");
    a.run(menu::Command::Deselect);
    assert_eq!(a.selected, None);
    assert!(a.status.contains("nothing selected"), "{}", a.status);
}

#[test]
fn select_parent_climbs_and_stops_at_the_document_root() {
    let mut a = app(DOC, "inner");
    let name = |a: &App| {
        a.selected
            .and_then(|id| a.preview.tree.as_ref()?.get(id)?.name.clone())
    };
    a.select_parent();
    assert_eq!(name(&a).as_deref(), Some("box"));
    a.select_parent();
    assert_eq!(name(&a).as_deref(), Some("root"));
    a.select_parent();
    assert_eq!(
        name(&a).as_deref(),
        Some("root"),
        "it climbed past the document"
    );
    assert!(a.status.contains("nothing above"), "{}", a.status);
}

#[test]
fn adding_with_nothing_selected_goes_into_the_root_and_selects_it() {
    let mut a = app(DOC, "loose");
    a.selected = None;
    a.add_child("led");
    assert_eq!(shape(&a).len(), 5, "{}", a.status);
    assert_eq!(
        shape(&a)[4].0,
        2,
        "the new widget is not a child of the root"
    );
    let picked = a
        .selected
        .and_then(|id| a.preview.tree.as_ref()?.get(id).map(|n| kind_name(&n.kind)));
    assert_eq!(picked, Some("led"), "the new widget was not selected");
}

#[test]
fn every_kind_the_add_menu_offers_builds() {
    // The failure this catches is a starter whose fields the parser rejects:
    // the menu item would add a widget and the preview would go red.
    for kind in menu::KINDS {
        let mut a = app(DOC, "root");
        a.add_child(kind);
        assert!(a.preview.error.is_none(), "{kind}: {:?}", a.preview.error);
        assert_eq!(shape(&a).len(), 5, "{kind}: {}", a.status);
    }
}

// --- snapping ---

#[test]
fn a_drag_snaps_to_widgets_outside_its_own_panel() {
    // `inner` lives in `box` at (0, 0); `loose` is a sibling of `box` at
    // (50, 0). Converted into box's space, loose is still at (50, 0), and it
    // has to be there for inner to line up with it.
    let a = app(DOC, "inner");
    let targets = a.snap_targets(a.selected.expect("selected"));
    assert!(
        targets.contains(&copilot::Rect::new(50, 0, 10, 10)),
        "loose is missing from {targets:?}"
    );
    assert_eq!(
        targets[0],
        copilot::Rect::new(0, 0, 40, 40),
        "the parent comes first"
    );
}

#[test]
fn snap_targets_are_in_the_parents_space() {
    // `loose` is a child of root; `inner` is at absolute (1, 1), which is
    // (1, 1) in root's space too, but only because root is at the origin.
    let src = r#"{"width":100,"height":100,
          "root":{"type":"panel","rect":[0,0,100,100],"name":"root","children":[
            {"type":"panel","rect":[20,30,50,50],"name":"box","children":[
              {"type":"bar","rect":[5,5,10,10],"name":"a"},
              {"type":"panel","rect":[20,20,20,20],"name":"sub","children":[
                {"type":"bar","rect":[1,1,5,5],"name":"deep"}]}]}]}}"#;
    let a = app(src, "a");
    let targets = a.snap_targets(a.selected.expect("selected"));
    // deep is at absolute (20+20+1, 30+20+1) = (41, 51); in box's space
    // that is (21, 21).
    assert!(
        targets.contains(&copilot::Rect::new(21, 21, 5, 5)),
        "deep is misplaced in {targets:?}"
    );
    assert!(
        !targets.contains(&copilot::Rect::new(5, 5, 10, 10)),
        "a is a target for itself"
    );
}

#[test]
fn a_dragged_widget_does_not_snap_to_its_own_children() {
    // They move with it, so they would snap it to itself and hold it still.
    let a = app(DOC, "box");
    let targets = a.snap_targets(a.selected.expect("selected"));
    assert!(
        !targets.contains(&copilot::Rect::new(1, 1, 5, 5)),
        "inner is a target: {targets:?}"
    );
    assert!(
        targets.contains(&copilot::Rect::new(50, 0, 10, 10)),
        "the sibling is missing: {targets:?}"
    );
}

// --- undo grouping ---

/// A drag's worth of rect edits, all under one gesture.
fn drag(a: &mut App, id: NodeId, rects: &[&str]) {
    let g = Some(canvas::Gesture {
        id: egui::Id::new("test drag"),
        ends: false,
    });
    for r in rects {
        a.set_field_of(id, g, "rect", r);
    }
    a.gesture = None;
}

#[test]
fn a_drag_is_one_undo_step_however_many_frames_it_took() {
    let mut a = app(DOC, "loose");
    let id = a.selected.expect("selected");
    let before = a.text.clone();
    drag(
        &mut a,
        id,
        &["[51, 0, 10, 10]", "[52, 0, 10, 10]", "[53, 0, 10, 10]"],
    );
    assert_eq!(a.undo.len(), 1, "every frame became its own undo step");
    a.undo();
    assert_eq!(a.text, before);
}

#[test]
fn two_drags_are_two_undo_steps() {
    let mut a = app(DOC, "loose");
    let id = a.selected.expect("selected");
    drag(&mut a, id, &["[51, 0, 10, 10]", "[52, 0, 10, 10]"]);
    drag(&mut a, id, &["[60, 0, 10, 10]", "[61, 0, 10, 10]"]);
    assert_eq!(a.undo.len(), 2);
    a.undo();
    assert!(a.text.contains("[52, 0, 10, 10]"), "{}", a.text);
}

#[test]
fn a_one_off_edit_after_a_drag_is_its_own_step() {
    let mut a = app(DOC, "loose");
    let id = a.selected.expect("selected");
    drag(&mut a, id, &["[51, 0, 10, 10]"]);
    a.set_field_of(id, None, "value", "0.9");
    assert_eq!(a.undo.len(), 2);
}

#[test]
fn undo_keeps_a_selection_that_still_means_the_same_widget() {
    let mut a = app(DOC, "loose");
    let id = a.selected.expect("selected");
    drag(&mut a, id, &["[51, 0, 10, 10]"]);
    a.undo();
    assert_eq!(
        a.selected,
        Some(id),
        "a rect edit undone lost the selection"
    );
    a.redo();
    assert_eq!(a.selected, Some(id));
}

#[test]
fn undo_drops_a_selection_the_step_renumbered() {
    // Moving `loose` into `box` renumbers it; undoing that move must not
    // leave an id pointing at whatever now sits where it was.
    let mut a = app(DOC, "loose");
    a.reparent(true);
    a.selected = a.preview.tree.as_ref().and_then(|t| t.find("loose"));
    a.undo();
    assert_eq!(a.selected, None);
}

#[test]
fn typing_in_the_source_is_undone_a_word_at_a_time() {
    assert!(
        !ui::word_boundary("ab", "abc"),
        "a letter continues the word"
    );
    assert!(
        !ui::word_boundary("a1", "a12"),
        "a digit continues the word"
    );
    assert!(ui::word_boundary("ab", "ab "), "a space ends it");
    assert!(ui::word_boundary("ab", "ab,"), "punctuation ends it");
    assert!(ui::word_boundary("abc", "ab"), "a deletion ends it");
    assert!(ui::word_boundary("ab", "abcd"), "a paste ends it");
    assert!(
        !ui::word_boundary("", "a"),
        "the first letter starts a word"
    );
}

#[test]
fn the_source_pane_edit_is_undoable() {
    let mut a = app(DOC, "loose");
    let before = a.text.clone();
    let after = before.replace("\"loose\"", "\"loosed\"");
    a.text = after.clone();
    a.begin_edit_from(
        Some(canvas::Gesture {
            id: egui::Id::new("source"),
            ends: false,
        }),
        before.clone(),
    );
    a.reload();
    assert_eq!(a.undo.len(), 1);
    a.undo();
    assert_eq!(a.text, before);
    a.redo();
    assert_eq!(a.text, after);
}

// --- multi-select ---

fn find(a: &App, name: &str) -> NodeId {
    a.preview
        .tree
        .as_ref()
        .and_then(|t| t.find(name))
        .unwrap_or_else(|| panic!("no widget called {name}"))
}

#[test]
fn shift_click_adds_and_removes_and_promotes() {
    let mut a = app(DOC, "loose");
    let (loose, inner, boxed) = (find(&a, "loose"), find(&a, "inner"), find(&a, "box"));
    a.toggle(inner);
    assert_eq!(a.members(), vec![loose, inner]);
    a.toggle(boxed);
    assert_eq!(a.members(), vec![loose, inner, boxed]);
    a.toggle(inner);
    assert_eq!(a.members(), vec![loose, boxed]);
    // Removing the primary promotes the next, so a selection never has
    // extras but no primary.
    a.toggle(loose);
    assert_eq!((a.selected, a.extra.clone()), (Some(boxed), vec![]));
    a.toggle(boxed);
    assert_eq!(a.members(), vec![]);
}

#[test]
fn a_plain_select_clears_the_extras() {
    let mut a = app(DOC, "loose");
    a.toggle(find(&a, "inner"));
    let boxed = find(&a, "box");
    a.select(Some(boxed));
    assert_eq!(a.members(), vec![boxed]);
}

#[test]
fn select_all_takes_the_siblings() {
    let mut a = app(DOC, "loose");
    a.select_all();
    assert_eq!(a.members(), vec![find(&a, "box"), find(&a, "loose")]);
    let mut a = app(DOC, "inner");
    a.select_all();
    assert_eq!(a.members(), vec![find(&a, "inner")]);
}

#[test]
fn a_band_picks_what_it_wholly_encloses_outermost_only() {
    let a = app(DOC, "loose");
    let t = a.preview.tree.as_ref().expect("tree");
    // Around box (0,0,40,40) and inner (1,1,5,5): just box.
    assert_eq!(
        select::enclosed(t, copilot::Rect::new(0, 0, 45, 45)),
        vec![find(&a, "box")]
    );
    // Around inner only.
    assert_eq!(
        select::enclosed(t, copilot::Rect::new(0, 0, 10, 10)),
        vec![find(&a, "inner")]
    );
    // Touching a leaf is enough; touching its panel is not.
    assert_eq!(
        select::enclosed(t, copilot::Rect::new(3, 3, 60, 4)),
        vec![find(&a, "inner"), find(&a, "loose")]
    );
    // Touching box but not enclosing it, and missing inner: nothing.
    assert_eq!(
        select::enclosed(t, copilot::Rect::new(20, 20, 30, 30)),
        vec![]
    );
    // Everything.
    assert_eq!(
        select::enclosed(t, copilot::Rect::new(0, 0, 100, 100)),
        vec![find(&a, "box"), find(&a, "loose")]
    );
}

#[test]
fn a_band_is_the_same_whichever_corner_it_starts_from() {
    let p = |x, y| copilot::Point { x, y };
    assert_eq!(
        select::span(p(5, 7), p(1, 2)),
        copilot::Rect::new(1, 2, 4, 5)
    );
    assert_eq!(
        select::span(p(1, 2), p(5, 7)),
        copilot::Rect::new(1, 2, 4, 5)
    );
}

#[test]
fn a_band_with_shift_adds_to_the_selection() {
    let mut a = app(DOC, "loose");
    a.marquee_select(
        copilot::Point { x: 0, y: 0 },
        copilot::Point { x: 10, y: 10 },
        true,
    );
    assert_eq!(a.members(), vec![find(&a, "loose"), find(&a, "inner")]);
    a.marquee_select(
        copilot::Point { x: 0, y: 0 },
        copilot::Point { x: 10, y: 10 },
        false,
    );
    assert_eq!(a.members(), vec![find(&a, "inner")]);
}

#[test]
fn a_nudge_moves_every_selected_widget_in_one_undo_step() {
    let mut a = app(DOC, "loose");
    a.toggle(find(&a, "inner"));
    a.nudge(3, 0);
    assert!(a.text.contains("[53, 0, 10, 10]"), "{}", a.text);
    assert!(a.text.contains("[4, 1, 5, 5]"), "{}", a.text);
    assert_eq!(a.undo.len(), 1);
}

#[test]
fn deleting_a_selection_removes_all_of_it_at_once() {
    let mut a = app(DOC, "loose");
    a.toggle(find(&a, "box"));
    a.delete_selected();
    assert_eq!(shape(&a), vec![(1, "root".into())], "{}", a.status);
    assert_eq!(a.undo.len(), 1);
    a.undo();
    assert_eq!(shape(&a).len(), 4);
}

#[test]
fn deleting_a_parent_and_its_child_together_does_not_delete_twice() {
    let mut a = app(DOC, "inner");
    a.toggle(find(&a, "box"));
    a.delete_selected();
    assert_eq!(
        shape(&a),
        vec![(1, "root".into()), (2, "loose".into())],
        "{}",
        a.status
    );
}

#[test]
fn duplicating_a_selection_copies_each_beside_itself() {
    let mut a = app(DOC, "inner");
    a.toggle(find(&a, "loose"));
    a.duplicate_selected();
    assert_eq!(
        shape(&a),
        vec![
            (1, "root".into()),
            (2, "box".into()),
            (3, "inner".into()),
            (3, "inner".into()),
            (2, "loose".into()),
            (2, "loose".into()),
        ],
        "{}",
        a.status
    );
}

#[test]
fn a_property_edit_reaches_every_selected_widget_of_the_kind() {
    let mut a = app(DOC, "loose");
    let (loose, inner) = (find(&a, "loose"), find(&a, "inner"));
    a.edit_each(&[loose, inner], None, "value", "0.25");
    assert_eq!(a.text.matches("\"value\": 0.25").count(), 2, "{}", a.text);
    assert_eq!(a.undo.len(), 1);
}
