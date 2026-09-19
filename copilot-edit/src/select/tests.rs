// SPDX-License-Identifier: GPL-3.0-only
//! Selection identity and status must agree after adding and removing members.

use crate::tests::app;

const SCENE: &str = r#"{"width":100,"height":100,"root":{"type":"panel","rect":[0,0,100,100],"name":"root","children":[
{"type":"bar","rect":[0,0,10,10],"name":"one"},{"type":"bar","rect":[20,0,10,10],"name":"two"},{"type":"bar","rect":[40,0,10,10],"name":"three"}]}}"#;

#[test]
fn repeated_non_adjacent_members_are_kept_once_without_reordering() {
    let mut a = app(SCENE, "one");
    let tree = a.preview.tree.as_ref().unwrap();
    let one = tree.find("one").unwrap();
    let two = tree.find("two").unwrap();
    let three = tree.find("three").unwrap();
    a.select_many(vec![two, one, two, three, one]);
    assert_eq!(a.members(), vec![two, one, three]);
}

#[test]
fn removing_either_member_names_the_one_left_selected() {
    for primary in [true, false] {
        let mut a = app(SCENE, "one");
        let one = a.selected.unwrap();
        let two = a.preview.tree.as_ref().unwrap().find("two").unwrap();
        a.select_many(vec![one, two]);
        a.toggle(if primary { one } else { two });
        let remaining = if primary { two } else { one };
        assert_eq!(a.selected, Some(remaining));
        assert_eq!(a.status, format!("selected {}", a.describe(remaining)));
    }
}
