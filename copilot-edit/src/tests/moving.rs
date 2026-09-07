// SPDX-License-Identifier: MIT OR Apache-2.0
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
