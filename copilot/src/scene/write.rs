// SPDX-License-Identifier: GPL-3.0-only
//! Writing a scene back out as text.
//!
//! The editor's half of the round trip. Output is indented one entry per line
//! so a scene file stays reviewable in a diff -- an editor that emits
//! everything on one line produces files nobody can merge.
//!
//! Comments are lost. They live in the source text, not in the [`Value`] tree,
//! and inventing somewhere to keep them would mean the parser carrying trivia
//! through every node for the sake of one consumer. An editor that rewrites a
//! hand-annotated file is expected to say so.

use alloc::string::String;

use super::value::Value;
use core::fmt::Write;

/// Render `value` as text an editor can write to a scene file.
///
/// Output is indented two spaces per level, one entry per line, so a scene
/// file stays reviewable in a diff.
pub fn to_string(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0);
    out
}

/// Recursively write a value at the given indentation depth.
fn write_value(out: &mut String, value: &Value, indent: usize) {
    match value {
        Value::Null => {
            let _ = out.write_str("null");
        }
        Value::Bool(b) => {
            let _ = out.write_str(if *b { "true" } else { "false" });
        }
        Value::Int(i) => {
            let _ = write!(out, "{}", i);
        }
        Value::Float(f) => {
            // `f64::trunc` lives in `std`, not `core`, so whole-numberedness is
            // tested by round-tripping through i64 -- the same trick
            // `Value::as_i64` uses, and for the same reason.
            let whole = f.is_finite() && (*f as i64) as f64 == *f;
            if whole {
                // The ".0" is not cosmetic: writing `4` for a Float would make
                // it reload as an Int and silently change the value's type.
                let _ = write!(out, "{}.0", *f as i64);
            } else if f.is_finite() {
                let _ = write!(out, "{f}");
            } else {
                // JSON cannot spell NaN or infinity; emitting one produces a
                // file the parser rejects on reload.
                let _ = out.write_str("null");
            }
        }
        Value::Str(s) => {
            write_string(out, s);
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                let _ = out.write_str("[]");
            } else if arr
                .iter()
                .all(|v| matches!(v, Value::Int(_) | Value::Float(_)))
            {
                // Single-line for numeric arrays keeps rects like [0, 0, 320, 160] readable.
                let _ = out.write_str("[");
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        let _ = out.write_str(", ");
                    }
                    write_value(out, v, 0);
                }
                let _ = out.write_str("]");
            } else {
                let _ = out.write_str("[\n");
                for (i, v) in arr.iter().enumerate() {
                    write_indent(out, indent + 1);
                    write_value(out, v, indent + 1);
                    if i + 1 < arr.len() {
                        let _ = out.write_str(",");
                    }
                    let _ = out.write_str("\n");
                }
                write_indent(out, indent);
                let _ = out.write_str("]");
            }
        }
        Value::Object(obj) => {
            if obj.is_empty() {
                let _ = out.write_str("{}");
            } else {
                let _ = out.write_str("{\n");
                for (i, (key, v)) in obj.iter().enumerate() {
                    write_indent(out, indent + 1);
                    write_string(out, key);
                    let _ = out.write_str(": ");
                    write_value(out, v, indent + 1);
                    if i + 1 < obj.len() {
                        let _ = out.write_str(",");
                    }
                    let _ = out.write_str("\n");
                }
                write_indent(out, indent);
                let _ = out.write_str("}");
            }
        }
    }
}

/// Write `indent` levels of two-space indentation.
fn write_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        let _ = out.write_str("  ");
    }
}

/// Write a string with JSON-style escaping.
fn write_string(out: &mut String, s: &str) {
    let _ = out.write_str("\"");
    for c in s.chars() {
        match c {
            '"' => {
                let _ = out.write_str("\\\"");
            }
            '\\' => {
                let _ = out.write_str("\\\\");
            }
            '\n' => {
                let _ = out.write_str("\\n");
            }
            '\r' => {
                let _ = out.write_str("\\r");
            }
            '\t' => {
                let _ = out.write_str("\\t");
            }
            _ if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            _ => {
                let _ = out.write_char(c);
            }
        }
    }
    let _ = out.write_str("\"");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::parse;
    use alloc::vec;

    fn round(src: &str) -> alloc::string::String {
        to_string(&parse(src).expect("must parse"))
    }

    #[test]
    fn scalars() {
        assert_eq!(round("null"), "null");
        assert_eq!(round("true"), "true");
        assert_eq!(round("false"), "false");
        assert_eq!(round("-7"), "-7");
    }

    #[test]
    fn empty_containers_stay_on_one_line() {
        assert_eq!(round("{}"), "{}");
        assert_eq!(round("[]"), "[]");
    }

    #[test]
    fn a_numeric_array_stays_on_one_line() {
        // A rect spread over six lines makes a scene file unreadable, and a
        // rect is the most common array in the format by a wide margin.
        assert_eq!(round("[0,0,320,160]"), "[0, 0, 320, 160]");
    }

    #[test]
    fn a_non_numeric_array_is_broken_up() {
        let got = round(r#"["a","b"]"#);
        assert!(got.contains('\n'), "expected multiple lines, got {got:?}");
    }

    #[test]
    fn an_object_is_one_entry_per_line() {
        let got = round(r#"{"a":1,"b":2}"#);
        assert_eq!(got, "{\n  \"a\": 1,\n  \"b\": 2\n}");
    }

    #[test]
    fn nesting_indents_relative_to_the_parent() {
        let got = round(r#"{"a":{"b":1}}"#);
        assert_eq!(got, "{\n  \"a\": {\n    \"b\": 1\n  }\n}");
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(round(r#""a\"b""#), r#""a\"b""#);
        assert_eq!(round(r#""a\\b""#), r#""a\\b""#);
        assert_eq!(round(r#""a\nb""#), r#""a\nb""#);
        assert_eq!(round(r#""a\tb""#), r#""a\tb""#);
    }

    #[test]
    fn a_control_character_becomes_a_unicode_escape() {
        // Emitting it raw produces a file that parses differently than it
        // looks, which is the worst kind of round-trip failure.
        let src = alloc::format!("\"a{}b\"", '\u{1}');
        assert_eq!(round(&src), r#""a\u0001b""#);
    }

    #[test]
    fn a_whole_float_keeps_one_decimal_place() {
        // Writing 4 for a float would silently change its type on reload.
        assert_eq!(to_string(&Value::Float(4.0)), "4.0");
        assert_eq!(to_string(&Value::Float(-2.0)), "-2.0");
    }

    #[test]
    fn a_fractional_float_is_written_as_it_is() {
        assert_eq!(to_string(&Value::Float(0.25)), "0.25");
    }

    #[test]
    fn a_non_finite_float_becomes_null() {
        // JSON has no way to spell NaN, and emitting one produces a file the
        // parser will reject on reload.
        assert_eq!(to_string(&Value::Float(f64::NAN)), "null");
        assert_eq!(to_string(&Value::Float(f64::INFINITY)), "null");
    }

    #[test]
    fn there_is_no_trailing_newline() {
        let got = round(r#"{"a":1}"#);
        assert!(!got.ends_with('\n'), "got {got:?}");
    }

    #[test]
    fn output_parses_back_to_the_same_tree() {
        // The property that actually matters: what the editor writes must load
        // as what it had.
        for src in [
            r#"{"width":320,"height":160,"root":{"type":"panel","rect":[0,0,320,160]}}"#,
            r#"{"a":[1,2,3],"b":{"c":true,"d":null},"e":"text"}"#,
            r#"[[1,2],[3,4]]"#,
            r#"{"empty_obj":{},"empty_arr":[]}"#,
        ] {
            let once = parse(src).expect("source parses");
            let text = to_string(&once);
            let twice = parse(&text).unwrap_or_else(|e| panic!("re-parse of {text:?}: {e:?}"));
            assert_eq!(once, twice, "round trip changed the tree for {src}");
        }
    }

    #[test]
    fn a_deep_tree_round_trips() {
        let mut v = Value::Int(1);
        for _ in 0..20 {
            v = Value::Array(vec![v]);
        }
        let text = to_string(&v);
        assert_eq!(parse(&text).expect("re-parse"), v);
    }
}
