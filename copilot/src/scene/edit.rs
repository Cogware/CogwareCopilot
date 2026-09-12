// SPDX-License-Identifier: GPL-3.0-only
//! Structural edits to a scene, performed on the text.
//!
//! Widgets are added, removed and replaced by splicing bytes, so comments,
//! blank lines, key order and the author's own indentation survive — which a
//! round trip through [`super::Value`] cannot promise. The delicate part is
//! commas: an element removed from the middle takes the comma after it, and
//! the last element takes the one before instead.

use alloc::string::String;

use super::locate::{Step, locate};

/// Replace the value at `path` with `value`, returning the new document.
pub fn replace(src: &str, path: &[Step<'_>], value: &str) -> Option<String> {
    let span = locate(src, path)?;
    let bytes = src.as_bytes();
    // Validate char boundaries to avoid panics on non-UTF8 splits.
    if !src.is_char_boundary(span.start) || !src.is_char_boundary(span.end) {
        return None;
    }
    if span.start > span.end || span.end > bytes.len() {
        return None;
    }
    let mut out = String::with_capacity(src.len() + value.len());
    out.push_str(&src[..span.start]);
    out.push_str(value);
    out.push_str(&src[span.end..]);
    Some(out)
}

/// Delete the array element at `path`, returning the new document.
///
/// `path` must end in a `Step::Index`. The element and one adjacent comma
/// are removed so the result is still valid.
pub fn remove(src: &str, path: &[Step<'_>]) -> Option<String> {
    let last = path.last()?;
    if !matches!(last, Step::Index(_)) {
        return None;
    }
    let elem_span = locate(src, path)?;
    let bytes = src.as_bytes();
    if elem_span.start > elem_span.end || elem_span.end > bytes.len() {
        return None;
    }

    let mut start = elem_span.start;
    let mut end = elem_span.end;

    // Scan forward for a comma to remove "elem,"
    let mut i = end;
    let mut found_forward = false;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b',' => {
                end = i + 1;
                found_forward = true;
                break;
            }
            _ => break,
        }
    }

    // If no forward comma, scan backward for a comma to remove ", elem"
    if !found_forward {
        let mut j = start;
        while j > 0 {
            j -= 1;
            match bytes[j] {
                b' ' | b'\t' | b'\n' | b'\r' => continue,
                b',' => {
                    start = j;
                    break;
                }
                _ => break,
            }
        }
    }

    if !src.is_char_boundary(start) || !src.is_char_boundary(end) {
        return None;
    }

    let mut out = String::with_capacity(src.len());
    out.push_str(&src[..start]);
    out.push_str(&src[end..]);
    Some(out)
}

/// Remove `key` from the object at `path`, returning the new document, or
/// `None` if there is no such key.
///
/// The other half of [`set`]. A property the editor wrote can be taken back
/// out again, which is how a widget returns to inheriting a setting rather
/// than carrying its own copy of it for ever. The key, the colon and the
/// value go, and one adjacent comma with them, by the same rule [`remove`]
/// uses for an array element.
pub fn unset(src: &str, path: &[Step<'_>], key: &str) -> Option<String> {
    let mut field = alloc::vec::Vec::with_capacity(path.len() + 1);
    field.extend_from_slice(path);
    field.push(Step::Key(key));
    let value = locate(src, &field)?;
    let bytes = src.as_bytes();
    if value.end > bytes.len() {
        return None;
    }

    // Back from the value over the colon to the key's opening quote.
    let mut start = value.start;
    let back = |i: &mut usize| {
        while *i > 0 && matches!(bytes[*i - 1], b' ' | b'\t' | b'\n' | b'\r') {
            *i -= 1;
        }
    };
    back(&mut start);
    if start == 0 || bytes[start - 1] != b':' {
        return None;
    }
    start -= 1;
    back(&mut start);
    if start == 0 || bytes[start - 1] != b'"' {
        return None;
    }
    start -= 1;
    // Keys here are plain names; one holding an escaped quote is not a key
    // this editor writes, and refusing beats splicing it wrongly.
    start = src[..start].rfind('"')?;

    // Forward through the comma. The whitespace after it goes too when there
    // is whitespace before the key, so the key that follows keeps one gap
    // and not both; after a bare brace there is none to double.
    let spaced = start > 0 && matches!(bytes[start - 1], b' ' | b'\t' | b'\n' | b'\r');
    let mut end = value.end;
    let mut i = end;
    let mut found_forward = false;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b',' => {
                i += 1;
                while spaced && i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r')
                {
                    i += 1;
                }
                end = i;
                found_forward = true;
                break;
            }
            _ => break,
        }
    }
    if !found_forward {
        let mut j = start;
        while j > 0 {
            j -= 1;
            match bytes[j] {
                b' ' | b'\t' | b'\n' | b'\r' => continue,
                b',' => {
                    start = j;
                    break;
                }
                _ => break,
            }
        }
    }
    if !src.is_char_boundary(start) || !src.is_char_boundary(end) {
        return None;
    }
    let mut out = String::with_capacity(src.len());
    out.push_str(&src[..start]);
    out.push_str(&src[end..]);
    Some(out)
}

/// Set `key` on the object at `path` to `value`, adding it if it is not there.
///
/// The adding half is the point. A scene file leaves out everything it is
/// happy to take the default of, so an editor that could only rewrite keys
/// already present would be unable to change a track colour on any widget
/// whose author never mentioned one -- which is most of them, and exactly the
/// ones where reaching for the text is least expected.
///
/// A new key goes in immediately after the opening brace rather than at the
/// end. The end of an object is where its `children` array lives, and a
/// property appended after several hundred lines of nested widgets is a
/// property nobody will find again.
pub fn set(src: &str, path: &[Step<'_>], key: &str, value: &str) -> Option<String> {
    let mut field = alloc::vec::Vec::with_capacity(path.len() + 1);
    field.extend_from_slice(path);
    field.push(Step::Key(key));
    if let Some(span) = locate(src, &field) {
        let mut out = String::with_capacity(src.len() + value.len());
        out.push_str(src.get(..span.start)?);
        out.push_str(value);
        out.push_str(src.get(span.end..)?);
        return Some(out);
    }

    let span = locate(src, path)?;
    let bytes = src.as_bytes();
    if bytes.get(span.start) != Some(&b'{') {
        return None;
    }
    let at = span.start + 1;
    if !src.is_char_boundary(at) {
        return None;
    }

    // Whether the object already holds anything decides the comma. An empty
    // one must not gain a trailing comma it did not ask for -- the format
    // tolerates them, but a file the editor touched should not start
    // collecting punctuation nobody typed.
    let empty = src.get(at..span.end.checked_sub(1)?)?.trim().is_empty();

    let mut out = String::with_capacity(src.len() + key.len() + value.len() + 8);
    out.push_str(src.get(..at)?);
    out.push('"');
    out.push_str(key);
    out.push_str("\": ");
    out.push_str(value);
    if !empty {
        out.push(',');
    }
    out.push_str(src.get(at..)?);
    Some(out)
}

/// The insertion point is derived from the located span rather than re-parsed
/// because the span already encodes the exact byte offsets of the array
/// delimiters, avoiding a second traversal of the source.  When appending,
/// the backwards scan over whitespace is necessary because the JSON-superset
/// grammar tolerates arbitrary whitespace before the closing bracket, and the
/// trailing-comma allowance means the last significant byte may be a comma
/// rather than an element.  Inserting before an existing element rather than
/// after the previous one keeps the operation local to a single span lookup
/// and sidesteps the need to distinguish an empty array from a one-element
/// array at the insertion site.  Character-boundary checks guard against
/// multi-byte UTF-8 sequences being split, which would yield an invalid
/// string.  The bounded element count prevents a pathological path from
/// causing an unbounded search.
pub fn insert(src: &str, path: &[Step<'_>], index: usize, value: &str) -> Option<String> {
    let span = locate(src, path)?;

    if span.end == 0 {
        return None;
    }

    let bytes = src.as_bytes();
    if bytes.get(span.start) != Some(&b'[') {
        return None;
    }
    if bytes.get(span.end - 1) != Some(&b']') {
        return None;
    }

    let mut count: usize = 0;
    while count < 100000 {
        let mut extended = path.to_vec();
        extended.push(Step::Index(count));
        if locate(src, &extended).is_none() {
            break;
        }
        count += 1;
    }

    let (insert_offset, prefix, suffix) = if index >= count {
        // One before the ']', not at it: the bracket is not whitespace, so a
        // scan starting there stops immediately and the new element lands
        // outside the array it was meant to join.
        let mut pos = span.end.checked_sub(2)?;
        loop {
            let b = bytes.get(pos)?;
            if b == &b' ' || b == &b'\t' || b == &b'\r' || b == &b'\n' {
                pos = pos.checked_sub(1)?;
            } else {
                break;
            }
        }

        match bytes.get(pos)? {
            b'[' => {
                if !src.is_char_boundary(pos + 1) {
                    return None;
                }
                (pos + 1, "", "")
            }
            b',' => {
                if !src.is_char_boundary(pos + 1) {
                    return None;
                }
                (pos + 1, "", "")
            }
            _ => {
                if !src.is_char_boundary(pos + 1) {
                    return None;
                }
                (pos + 1, ", ", "")
            }
        }
    } else {
        let mut extended = path.to_vec();
        extended.push(Step::Index(index));
        let elem_span = locate(src, &extended)?;
        if !src.is_char_boundary(elem_span.start) {
            return None;
        }
        (elem_span.start, "", ", ")
    };

    let before = src.get(0..insert_offset)?;
    let after = src.get(insert_offset..)?;

    let mut result = String::with_capacity(src.len() + value.len() + 2);
    result.push_str(before);
    result.push_str(prefix);
    result.push_str(value);
    result.push_str(suffix);
    result.push_str(after);

    Some(result)
}

/// Insert `value` as a new last element of the array at `path`.
pub fn append(src: &str, path: &[Step<'_>], value: &str) -> Option<String> {
    let span = locate(src, path)?;
    let bytes = src.as_bytes();
    if span.end == 0 || span.end > bytes.len() {
        return None;
    }
    // The byte at span.end - 1 must be ']'
    if bytes.get(span.end - 1) != Some(&b']') {
        return None;
    }

    // Find the last non-whitespace byte before the ']'
    let mut i = span.end - 1;
    let mut prev_byte: Option<u8> = None;
    let mut prev_pos: Option<usize> = None;
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => continue,
            b => {
                prev_byte = Some(b);
                prev_pos = Some(i);
                break;
            }
        }
    }

    let mut out = String::with_capacity(src.len() + value.len() + 2);
    // `prev_pos` is Some whenever `prev_byte` is, but reading it through `?`
    // rather than `unwrap` keeps the no-panic promise structural instead of
    // dependent on an invariant a later edit could break.
    let after_prev = prev_pos? + 1;
    let insert_pos = match prev_byte {
        // Empty array: before the bracket, not after it. `span.end` is one
        // past the ']', so inserting there produces "[]value".
        Some(b'[') => span.end.checked_sub(1)?,
        _ => after_prev,
    };

    if !src.is_char_boundary(insert_pos) {
        return None;
    }

    out.push_str(&src[..insert_pos]);
    match prev_byte {
        Some(b',') => {
            // Already have a comma, just insert value
            out.push_str(value);
        }
        Some(b'[') => {
            // Empty array, just insert value
            out.push_str(value);
        }
        _ => {
            // Need to add ", " before value
            out.push_str(", ");
            out.push_str(value);
        }
    }
    out.push_str(&src[insert_pos..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::parse;

    const DOC: &str = r#"{
  // this comment must survive every operation below
  "root": {
    "children": [
      { "type": "bar", "name": "a" },
      { "type": "bar", "name": "b" },
      { "type": "bar", "name": "c" }
    ]
  },
  "empty": [],
  "trailing": [1, 2,]
}"#;

    fn kids() -> [Step<'static>; 2] {
        [Step::Key("root"), Step::Key("children")]
    }

    fn kid(n: usize) -> [Step<'static>; 3] {
        [Step::Key("root"), Step::Key("children"), Step::Index(n)]
    }

    /// Every result must still parse, or the operation has produced a file the
    /// editor can no longer open.
    fn valid(s: &str) -> crate::scene::Value {
        parse(s).unwrap_or_else(|e| panic!("result does not parse: {e:?}\n{s}"))
    }

    fn names(v: &crate::scene::Value) -> alloc::vec::Vec<alloc::string::String> {
        v.get("root")
            .and_then(|r| r.get("children"))
            .and_then(crate::scene::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|c| c.get("name").and_then(|n| n.as_str()))
                    .map(alloc::string::String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    // --- replace ---

    #[test]
    fn replace_swaps_only_the_targeted_value() {
        let out = replace(DOC, &kid(1), r#"{ "type": "label", "name": "z" }"#).unwrap();
        let v = valid(&out);
        assert_eq!(names(&v), ["a", "z", "c"]);
        assert!(out.contains("must survive"), "the comment was lost");
    }

    #[test]
    fn replace_on_a_missing_path_is_none() {
        assert!(replace(DOC, &[Step::Key("nope")], "1").is_none());
    }

    // --- remove ---

    #[test]
    fn remove_takes_the_first_element_and_its_comma() {
        let out = remove(DOC, &kid(0)).unwrap();
        assert_eq!(names(&valid(&out)), ["b", "c"]);
    }

    #[test]
    fn remove_takes_a_middle_element() {
        let out = remove(DOC, &kid(1)).unwrap();
        assert_eq!(names(&valid(&out)), ["a", "c"]);
    }

    #[test]
    fn remove_takes_the_last_element_and_the_comma_before_it() {
        // The last element has no comma after it, so the preceding one has to
        // go instead or the array is left ending in a dangling separator.
        let out = remove(DOC, &kid(2)).unwrap();
        assert_eq!(names(&valid(&out)), ["a", "b"]);
    }

    #[test]
    fn removing_every_element_leaves_a_valid_empty_array() {
        let mut doc = alloc::string::String::from(DOC);
        for _ in 0..3 {
            doc = remove(&doc, &kid(0)).expect("remove first");
        }
        let v = valid(&doc);
        assert!(names(&v).is_empty());
        assert!(doc.contains("must survive"), "the comment was lost");
    }

    #[test]
    fn remove_preserves_the_comment() {
        let out = remove(DOC, &kid(1)).unwrap();
        assert!(out.contains("must survive"));
    }

    #[test]
    fn remove_requires_an_index_at_the_end_of_the_path() {
        assert!(remove(DOC, &kids()).is_none(), "a key-terminated path");
        assert!(remove(DOC, &[]).is_none(), "an empty path");
    }

    // --- append ---

    #[test]
    fn append_adds_to_a_populated_array() {
        let out = append(DOC, &kids(), r#"{ "type": "bar", "name": "d" }"#).unwrap();
        assert_eq!(names(&valid(&out)), ["a", "b", "c", "d"]);
    }

    #[test]
    fn append_adds_to_an_empty_array() {
        let out = append(DOC, &[Step::Key("empty")], "42").unwrap();
        let v = valid(&out);
        let arr = v
            .get("empty")
            .and_then(crate::scene::Value::as_array)
            .unwrap();
        assert_eq!(arr.len(), 1);
    }

    #[test]
    fn append_after_a_trailing_comma_does_not_double_it() {
        // "[1, 2,]" already has the separator; adding another makes "[1, 2,,3]"
        // which is not a document any parser will take.
        let out = append(DOC, &[Step::Key("trailing")], "3").unwrap();
        let v = valid(&out);
        let arr = v
            .get("trailing")
            .and_then(crate::scene::Value::as_array)
            .unwrap();
        assert_eq!(arr.len(), 3);
    }

    #[test]
    fn append_to_a_missing_path_is_none() {
        assert!(append(DOC, &[Step::Key("nope")], "1").is_none());
    }

    #[test]
    fn append_to_something_that_is_not_an_array_is_none() {
        assert!(append(DOC, &[Step::Key("root")], "1").is_none());
    }

    // --- round trips ---

    #[test]
    fn append_then_remove_returns_the_original_shape() {
        let with = append(DOC, &kids(), r#"{ "type": "bar", "name": "d" }"#).unwrap();
        let back = remove(&with, &kid(3)).unwrap();
        assert_eq!(names(&valid(&back)), ["a", "b", "c"]);
    }

    #[test]
    fn no_operation_ever_produces_something_that_will_not_parse() {
        // The property that matters: an editor must never write a file it can
        // no longer open.
        for n in 0..3 {
            valid(&remove(DOC, &kid(n)).unwrap());
            valid(&replace(DOC, &kid(n), "true").unwrap());
        }
        valid(&append(DOC, &kids(), "null").unwrap());
        valid(&append(DOC, &[Step::Key("empty")], "null").unwrap());
        valid(&append(DOC, &[Step::Key("trailing")], "null").unwrap());
    }

    // --- set ---

    const OBJ: &str = r#"{"a": 1, "b": {"type": "panel", "rect": [0, 0, 4, 4]}}"#;

    #[test]
    fn setting_an_existing_key_replaces_its_value() {
        let out = set(OBJ, &[Step::Key("b")], "type", r#""label""#).unwrap();
        assert!(out.contains(r#""type": "label""#), "{out}");
        assert!(!out.contains("panel"), "{out}");
    }

    #[test]
    fn setting_a_missing_key_adds_it() {
        let out = set(OBJ, &[Step::Key("b")], "color", r##""#ff0000ff""##).unwrap();
        let parsed = crate::scene::parse(&out).expect("still valid");
        assert_eq!(
            parsed
                .get("b")
                .and_then(|v| v.get("color"))
                .and_then(crate::scene::Value::as_str),
            Some("#ff0000ff"),
            "{out}"
        );
    }

    #[test]
    fn an_added_key_goes_first_where_it_can_be_found() {
        // Not last: the end of a widget is where its `children` array is, and
        // a property after several hundred lines of nested widgets is lost.
        let out = set(OBJ, &[Step::Key("b")], "color", r##""#ff0000ff""##).unwrap();
        let obj = out.find("\"color\"").expect("the new key");
        let existing = out.find("\"type\"").expect("the old key");
        assert!(
            obj < existing,
            "the new key landed after the old ones: {out}"
        );
    }

    #[test]
    fn adding_to_an_empty_object_leaves_no_stray_comma() {
        let src = r#"{"a": {}}"#;
        let out = set(src, &[Step::Key("a")], "x", "1").unwrap();
        assert_eq!(out, r#"{"a": {"x": 1}}"#, "{out}");
        crate::scene::parse(&out).expect("still valid");
    }

    #[test]
    fn setting_a_key_leaves_the_rest_of_the_file_alone() {
        let src = "{\n  // keep me\n  \"a\": {\"n\": 1},\n}";
        let out = set(src, &[Step::Key("a")], "m", "2").unwrap();
        assert!(out.contains("// keep me"), "{out}");
        assert!(out.contains(r#""n": 1"#), "{out}");
        crate::scene::parse(&out).expect("still valid");
    }

    #[test]
    fn setting_on_a_path_that_is_not_an_object_is_refused() {
        // An array has no keys, and quietly writing one into it would produce
        // a document that no longer parses.
        assert!(set(OBJ, &[Step::Key("b"), Step::Key("rect")], "x", "1").is_none());
        assert!(set(OBJ, &[Step::Key("a")], "x", "1").is_none());
    }

    #[test]
    fn setting_on_a_path_that_does_not_exist_is_refused() {
        assert!(set(OBJ, &[Step::Key("nope")], "x", "1").is_none());
    }

    #[test]
    fn a_set_document_still_builds_as_a_scene() {
        let src = r#"{"width":10,"height":10,
                      "root":{"type":"bar","rect":[0,0,10,10]}}"#;
        // `track` is defaulted and absent, which is the case the whole
        // function exists for.
        let out = set(src, &[Step::Key("root")], "track", r##""#102030ff""##).unwrap();
        let doc = crate::scene::parse(&out).expect("parses");
        let tree = crate::scene::build(&doc).expect("builds");
        let root = tree.get(crate::widget::ROOT).unwrap();
        let crate::widget::Kind::Bar { track, .. } = &tree.get(root.children[0]).unwrap().kind
        else {
            panic!("expected a bar");
        };
        assert_eq!(*track, crate::Color::rgba(0x10, 0x20, 0x30, 0xff));
    }

    // --- insert ---

    /// A three-element array, with the surrounding document a scene has.
    const ARR: &str = r#"{"root": {"children": [{"n": 0}, {"n": 1}, {"n": 2}]}}"#;

    /// The path to that array.
    fn arr_path() -> [Step<'static>; 2] {
        [Step::Key("root"), Step::Key("children")]
    }

    /// The `n` of each element, in order, so a move can be read at a glance.
    fn order(src: &str) -> alloc::vec::Vec<i64> {
        let doc = crate::scene::parse(src).expect("must still parse");
        doc.get("root")
            .and_then(|r| r.get("children"))
            .and_then(crate::scene::Value::as_array)
            .expect("an array")
            .iter()
            .map(|e| {
                e.get("n")
                    .and_then(crate::scene::Value::as_i64)
                    .expect("an n")
            })
            .collect()
    }

    #[test]
    fn inserting_at_the_front_puts_it_first() {
        let out = insert(ARR, &arr_path(), 0, r#"{"n": 9}"#).expect("must insert");
        assert_eq!(order(&out), [9, 0, 1, 2]);
    }

    #[test]
    fn inserting_in_the_middle_shifts_the_rest_along() {
        let out = insert(ARR, &arr_path(), 1, r#"{"n": 9}"#).expect("must insert");
        assert_eq!(order(&out), [0, 9, 1, 2]);
    }

    #[test]
    fn inserting_at_the_length_puts_it_last() {
        let out = insert(ARR, &arr_path(), 3, r#"{"n": 9}"#).expect("must insert");
        assert_eq!(order(&out), [0, 1, 2, 9]);
    }

    #[test]
    fn inserting_past_the_end_still_puts_it_last() {
        // Clamping rather than refusing: the caller asked for "after
        // everything", and there is only one place that can mean.
        let out = insert(ARR, &arr_path(), 999, r#"{"n": 9}"#).expect("must insert");
        assert_eq!(order(&out), [0, 1, 2, 9]);
    }

    #[test]
    fn inserting_into_an_empty_array_works() {
        let src = r#"{"root": {"children": []}}"#;
        let out = insert(src, &arr_path(), 0, r#"{"n": 9}"#).expect("must insert");
        assert_eq!(order(&out), [9]);
    }

    #[test]
    fn inserting_after_a_trailing_comma_does_not_double_it() {
        let src = r#"{"root": {"children": [{"n": 0},]}}"#;
        let out = insert(src, &arr_path(), 1, r#"{"n": 9}"#).expect("must insert");
        assert_eq!(order(&out), [0, 9], "{out}");
        assert!(!out.contains(",,"), "{out}");
    }

    #[test]
    fn inserting_leaves_comments_and_layout_alone() {
        let src = "{\n  // keep me\n  \"root\": {\n    \"children\": [\n      {\"n\": 0},\n    ],\n  },\n}";
        let out = insert(src, &arr_path(), 0, r#"{"n": 9}"#).expect("must insert");
        assert!(out.contains("// keep me"), "{out}");
        assert_eq!(order(&out), [9, 0]);
    }

    #[test]
    fn inserting_somewhere_that_is_not_an_array_is_refused() {
        assert!(insert(ARR, &[Step::Key("root")], 0, "1").is_none());
    }

    #[test]
    fn inserting_at_a_path_that_does_not_exist_is_refused() {
        assert!(insert(ARR, &[Step::Key("nope")], 0, "1").is_none());
    }

    #[test]
    fn a_removed_then_inserted_element_lands_where_it_was_asked_to() {
        // The move the editor performs: take element 0 out and put it back at
        // the end. Counting the destination in the list it has already left is
        // what makes this land on 2 rather than one short of it.
        let mut elem = alloc::vec::Vec::from(arr_path());
        elem.push(Step::Index(0));
        let without = remove(ARR, &elem).expect("must remove");
        let out = insert(&without, &arr_path(), 2, r#"{"n": 0}"#).expect("must insert");
        assert_eq!(order(&out), [1, 2, 0]);
    }

    #[test]
    fn an_inserted_scene_widget_still_builds() {
        let src = r#"{"width":10,"height":10,"root":{"type":"panel","rect":[0,0,10,10],
                      "children":[{"type":"bar","rect":[0,0,5,5]}]}}"#;
        let path = [Step::Key("root"), Step::Key("children")];
        let out = insert(src, &path, 0, r#"{"type":"led","rect":[1,1,2,2]}"#).expect("must insert");
        let doc = crate::scene::parse(&out).expect("parses");
        let tree = crate::scene::build(&doc).expect("builds");
        assert_eq!(tree.len(), 4, "root, panel, led, bar");
    }

    #[test]
    fn unset_removes_a_middle_key_and_its_comma() {
        let src = r#"{ "type": "bar", "antialias": true, "rect": [0, 0, 1, 1] }"#;
        let out = unset(src, &[], "antialias").expect("removed");
        assert_eq!(out, r#"{ "type": "bar", "rect": [0, 0, 1, 1] }"#);
    }

    #[test]
    fn unset_removes_a_last_key_with_the_comma_before_it() {
        let src = "{\n  \"type\": \"bar\",\n  \"antialias\": false\n}";
        let out = unset(src, &[], "antialias").expect("removed");
        assert_eq!(out, "{\n  \"type\": \"bar\"\n}");
    }

    #[test]
    fn unset_reaches_into_a_nested_widget() {
        let src = r#"{"root":{"type":"panel","children":[{"type":"bar","antialias":true,"rect":[0,0,1,1]}]}}"#;
        let path = [Step::Key("root"), Step::Key("children"), Step::Index(0)];
        let out = unset(src, &path, "antialias").expect("removed");
        assert_eq!(
            out,
            r#"{"root":{"type":"panel","children":[{"type":"bar","rect":[0,0,1,1]}]}}"#
        );
    }

    #[test]
    fn unset_of_a_missing_key_is_none() {
        assert!(unset(r#"{ "type": "bar" }"#, &[], "antialias").is_none());
    }

    #[test]
    fn set_then_unset_is_the_identity() {
        let src = r#"{ "type": "bar", "rect": [0, 0, 1, 1] }"#;
        let with = set(src, &[], "antialias", "true").expect("set");
        assert_ne!(with, src);
        assert_eq!(unset(&with, &[], "antialias").expect("unset"), src);
    }
}
