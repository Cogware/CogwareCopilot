// SPDX-License-Identifier: GPL-3.0-only
//! Tests for moving a widget about the tree: the one operation a drag in
//! the outliner, Move inwards and Move outwards are all made of.

use crate::tests::{app, shape};

const DOC: &str = r#"{
  "width": 100, "height": 100,
  "root": { "type": "panel", "rect": [0, 0, 100, 100], "name": "root", "children": [
{ "type": "panel", "rect": [0, 0, 40, 40], "name": "a", "children": [
  { "type": "bar", "rect": [1, 1, 5, 5], "name": "a1" },
  { "type": "bar", "rect": [1, 1, 5, 5], "name": "a2" } ] },
{ "type": "bar", "rect": [50, 0, 10, 10], "name": "b" },
{ "type": "panel", "rect": [0, 50, 40, 40], "name": "c", "children": [
  { "type": "bar", "rect": [1, 1, 5, 5], "name": "c1" } ] } ] } }"#;

fn names(a: &crate::App) -> Vec<String> {
    shape(a)
        .into_iter()
        .map(|(d, n)| format!("{d}:{n}"))
        .collect()
}

/// Where `name` sits on the scene, which is what a move must not change.
fn abs(a: &crate::App, name: &str) -> copilot::Rect {
    let t = a.preview.tree.as_ref().expect("a tree");
    t.absolute_rect(t.find(name).expect(name)).expect(name)
}

fn id(a: &crate::App, name: &str) -> copilot::widget::NodeId {
    a.preview
        .tree
        .as_ref()
        .and_then(|t| t.find(name))
        .expect(name)
}

#[test]
fn a_widget_moves_into_another_parent_and_stays_selected() {
    let mut a = app(DOC, "b");
    let c = id(&a, "c");
    assert!(a.move_node(id(&a, "b"), c, 0), "{}", a.status);
    assert_eq!(
        names(&a),
        ["1:root", "2:a", "3:a1", "3:a2", "2:c", "3:b", "3:c1"]
    );
    assert_eq!(
        a.selected,
        Some(id(&a, "b")),
        "the moved widget lost the selection"
    );
}

#[test]
fn moving_forward_in_the_same_list_lands_where_it_was_aimed() {
    // "after c" is index 3 before the move; taking `a` out makes it 2.
    let mut a = app(DOC, "a");
    let root = id(&a, "root");
    assert!(a.move_node(id(&a, "a"), root, 3), "{}", a.status);
    assert_eq!(
        names(&a),
        ["1:root", "2:b", "2:c", "3:c1", "2:a", "3:a1", "3:a2"]
    );
}

#[test]
fn moving_backward_in_the_same_list_needs_no_correction() {
    let mut a = app(DOC, "c");
    let root = id(&a, "root");
    assert!(a.move_node(id(&a, "c"), root, 0), "{}", a.status);
    assert_eq!(
        names(&a),
        ["1:root", "2:c", "3:c1", "2:a", "3:a1", "3:a2", "2:b"]
    );
}

#[test]
fn moving_into_a_later_sibling_finds_it_after_the_renumbering() {
    // `c` is child 2 of root until `a` is removed, when it becomes 1.
    let mut a = app(DOC, "a");
    let c = id(&a, "c");
    assert!(a.move_node(id(&a, "a"), c, 1), "{}", a.status);
    assert_eq!(
        names(&a),
        ["1:root", "2:b", "2:c", "3:c1", "3:a", "4:a1", "4:a2"]
    );
}

#[test]
fn a_widget_cannot_be_moved_into_itself_or_its_children() {
    let mut a = app(DOC, "a");
    let before = names(&a);
    let (a_id, a1) = (id(&a, "a"), id(&a, "a1"));
    assert!(!a.move_node(a_id, a1, 0));
    assert!(!a.move_node(a_id, a_id, 0));
    assert_eq!(names(&a), before);
    assert!(a.status.contains("into itself"), "{}", a.status);
}

#[test]
fn the_document_root_stays_where_it_is() {
    let mut a = app(DOC, "root");
    let before = names(&a);
    let c = id(&a, "c");
    assert!(!a.move_node(id(&a, "root"), c, 0));
    assert_eq!(names(&a), before);
}

#[test]
fn a_move_is_one_undo_step() {
    let mut a = app(DOC, "b");
    let before = names(&a);
    let c = id(&a, "c");
    a.move_node(id(&a, "b"), c, 0);
    a.undo();
    assert_eq!(names(&a), before);
}

#[test]
fn a_widget_changing_parent_stays_where_it_was_on_screen() {
    // The bug this guards: a rect is relative to its parent, so moving `b`
    // into `c` -- which starts 50 down the scene -- used to carry the numbers
    // over untouched and drop the widget 50 pixels below where the author
    // left it. A deep enough panel put it off the scene entirely.
    let mut a = app(DOC, "b");
    let before = abs(&a, "b");
    let c = id(&a, "c");
    assert!(a.move_node(id(&a, "b"), c, 0), "{}", a.status);
    assert_eq!(abs(&a, "b"), before, "the widget moved on screen");
    assert!(a.text.contains("[50, -50, 10, 10]"), "{}", a.text);
}

#[test]
fn a_widget_moving_outwards_stays_where_it_was_on_screen() {
    // The other direction, and the one that hurts: `c1` sits at [1, 1] inside
    // a panel 50 down, so promoting it to the root has to become [1, 51] or
    // the widget jumps up the scene.
    let mut a = app(DOC, "c1");
    let before = abs(&a, "c1");
    a.reparent(false);
    assert_eq!(abs(&a, "c1"), before, "the widget moved on screen");
    assert!(a.text.contains("[1, 51, 5, 5]"), "{}", a.text);
}

#[test]
fn reordering_within_one_parent_leaves_the_rect_alone() {
    // A reorder changes what a widget is drawn on top of, not where it is.
    // Rewriting the rect here would churn the text on every Raise and Lower.
    let mut a = app(DOC, "a1");
    let parent = id(&a, "a");
    assert!(a.move_node(id(&a, "a1"), parent, 2), "{}", a.status);
    assert_eq!(
        a.text.matches("[1, 1, 5, 5]").count(),
        3,
        "a reorder rewrote a rect: {}",
        a.text
    );
}
