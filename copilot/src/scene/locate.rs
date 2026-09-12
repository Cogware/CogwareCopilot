// SPDX-License-Identifier: GPL-3.0-only
//! Finding where a value lives in the source text.
//!
//! The editor edits *text*, not a tree, so changing one property means
//! rewriting exactly the bytes that property occupies and leaving the rest of
//! the file alone. That is what preserves comments, formatting and key order
//! through an edit -- a round trip through [`super::Value`] would discard all
//! three.
//!
//! Re-lexing rather than recording spans during the parse, because a path is
//! asked for at most a few times per keystroke and a span on every node would
//! cost memory on the bare-metal side, which never needs one.

use alloc::vec::Vec;

use super::lex::{Lexer, Spanned, Token};
use super::value::Span;

/// One step along a path into a document.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step<'a> {
    /// A key in an object.
    Key(&'a str),
    /// An index in an array.
    Index(usize),
}

/// The source span of the value at `path`, if the document has one there.
///
/// Returns `None` for a path that does not exist, and for input the lexer
/// rejects.
pub fn locate(src: &str, path: &[Step<'_>]) -> Option<Span> {
    // Lex everything up front so we can index freely without re-lexing.
    let mut lexer = Lexer::new(src);
    let mut tokens: Vec<Spanned<Token>> = Vec::new();
    loop {
        match lexer.next_token() {
            Ok(Some(tok)) => tokens.push(tok),
            Ok(None) => break,
            Err(_) => return None,
        }
    }

    let mut i: usize = 0;

    for step in path {
        match step {
            Step::Key(k) => {
                if tokens.get(i).map(|t| matches!(t.value, Token::LBrace)) != Some(true) {
                    return None;
                }
                i += 1;

                let found;
                loop {
                    let key_tok = tokens.get(i)?;
                    let key_str: &str = match key_tok.value {
                        Token::Str(s) => s,
                        Token::OwnedStr(ref s) => s.as_str(),
                        _ => return None,
                    };
                    i += 1;

                    if tokens.get(i).map(|t| matches!(t.value, Token::Colon)) != Some(true) {
                        return None;
                    }
                    i += 1;

                    if key_str == *k {
                        // Leave `i` at the start of the value.
                        found = true;
                        break;
                    }

                    i = skip_value(&tokens, i)?;

                    // After the value, expect Comma (more keys) or RBrace (end).
                    match tokens.get(i) {
                        Some(t) if matches!(t.value, Token::Comma) => {
                            i += 1;
                            // Trailing comma before RBrace: key not found.
                            if tokens.get(i).map(|t| matches!(t.value, Token::RBrace)) == Some(true)
                            {
                                return None;
                            }
                        }
                        Some(t) if matches!(t.value, Token::RBrace) => {
                            // Object ended without finding the key.
                            return None;
                        }
                        _ => return None,
                    }
                }
                let _ = found;
            }
            Step::Index(n) => {
                if tokens.get(i).map(|t| matches!(t.value, Token::LBracket)) != Some(true) {
                    return None;
                }
                i += 1;

                // Skip `n` values, each followed by a Comma.
                for _ in 0..*n {
                    // If we hit RBracket before skipping enough, the index is out of range.
                    if tokens.get(i).map(|t| matches!(t.value, Token::RBracket)) == Some(true) {
                        return None;
                    }
                    i = skip_value(&tokens, i)?;
                    // Expect a Comma after each skipped value.
                    if tokens.get(i).map(|t| matches!(t.value, Token::Comma)) != Some(true) {
                        return None;
                    }
                    i += 1;
                    // Trailing comma before RBracket: the index is out of range.
                    if tokens.get(i).map(|t| matches!(t.value, Token::RBracket)) == Some(true) {
                        return None;
                    }
                }

                // `i` is now at the start of value `n`.
            }
        }
    }

    // `i` is at the first token of the target value. Skip it to find the last token.
    let start = tokens.get(i)?.start;
    let after = skip_value(&tokens, i)?;
    // The last token of the value is the one just before `after`.
    let last_idx = after.checked_sub(1)?;
    let end = tokens.get(last_idx)?.end;

    Some(Span { start, end })
}

/// Skip a value starting at index `i`, returning the index of the token after it.
///
/// Returns `None` if the input runs out before the value is fully consumed.
fn skip_value(tokens: &[Spanned<Token>], i: usize) -> Option<usize> {
    let tok = tokens.get(i)?;
    match tok.value {
        Token::LBrace | Token::LBracket => {
            let mut depth: usize = 1;
            let mut j = i + 1;
            loop {
                let t = tokens.get(j)?;
                match t.value {
                    Token::LBrace | Token::LBracket => depth += 1,
                    Token::RBrace | Token::RBracket => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        _ => Some(i + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"{
  // a comment, which must survive being spliced around
  "width": 320,
  "height": 160,
  "root": {
    "type": "panel",
    "rect": [0, 0, 320, 160],
    "children": [
      { "type": "bar", "rect": [1, 2, 3, 4], "name": "first" },
      { "type": "label", "rect": [5, 6, 7, 8], "text": "hi",
        "children": [ { "type": "frame", "rect": [9, 9, 9, 9] } ] },
    ],
  },
}"#;

    fn text(path: &[Step<'_>]) -> Option<&'static str> {
        locate(DOC, path).map(|s| &DOC[s.start..s.end])
    }

    #[test]
    fn an_empty_path_is_the_whole_document() {
        let s = locate(DOC, &[]).unwrap();
        assert_eq!(s.start, 0);
        assert_eq!(s.end, DOC.len());
    }

    #[test]
    fn a_top_level_scalar() {
        assert_eq!(text(&[Step::Key("width")]), Some("320"));
        assert_eq!(text(&[Step::Key("height")]), Some("160"));
    }

    #[test]
    fn a_nested_object() {
        let got = text(&[Step::Key("root"), Step::Key("type")]).unwrap();
        assert_eq!(got, "\"panel\"");
    }

    #[test]
    fn an_array_is_returned_whole() {
        let got = text(&[Step::Key("root"), Step::Key("rect")]).unwrap();
        assert_eq!(got, "[0, 0, 320, 160]");
    }

    #[test]
    fn an_array_element() {
        let p = [Step::Key("root"), Step::Key("rect"), Step::Index(2)];
        assert_eq!(text(&p), Some("320"));
    }

    #[test]
    fn a_child_by_index() {
        let p = [
            Step::Key("root"),
            Step::Key("children"),
            Step::Index(0),
            Step::Key("name"),
        ];
        assert_eq!(text(&p).unwrap(), "\"first\"");
    }

    #[test]
    fn the_second_child_is_not_the_first() {
        // Skipping a value has to skip the whole nested structure, not just
        // its opening token, or every later index is off.
        let p = [
            Step::Key("root"),
            Step::Key("children"),
            Step::Index(1),
            Step::Key("text"),
        ];
        assert_eq!(text(&p).unwrap(), "\"hi\"");
    }

    #[test]
    fn a_deeply_nested_child() {
        let p = [
            Step::Key("root"),
            Step::Key("children"),
            Step::Index(1),
            Step::Key("children"),
            Step::Index(0),
            Step::Key("rect"),
        ];
        assert_eq!(text(&p).unwrap(), "[9, 9, 9, 9]");
    }

    #[test]
    fn comments_do_not_shift_the_span() {
        // The comment sits between the brace and "width"; a locator that
        // counted raw bytes rather than tokens would land inside it.
        let s = locate(DOC, &[Step::Key("width")]).unwrap();
        assert_eq!(&DOC[s.start..s.end], "320");
    }

    #[test]
    fn a_trailing_comma_does_not_hide_the_last_entry() {
        let p = [Step::Key("root"), Step::Key("children"), Step::Index(1)];
        assert!(text(&p).unwrap().starts_with('{'));
    }

    #[test]
    fn a_missing_key_is_none() {
        assert_eq!(locate(DOC, &[Step::Key("nonexistent")]), None);
        assert_eq!(locate(DOC, &[Step::Key("root"), Step::Key("nope")]), None);
    }

    #[test]
    fn an_out_of_range_index_is_none() {
        let p = [Step::Key("root"), Step::Key("children"), Step::Index(9)];
        assert_eq!(locate(DOC, &p), None);
    }

    #[test]
    fn indexing_an_object_or_keying_an_array_is_none() {
        assert_eq!(locate(DOC, &[Step::Index(0)]), None);
        assert_eq!(
            locate(DOC, &[Step::Key("root"), Step::Key("rect"), Step::Key("x")]),
            None
        );
    }

    #[test]
    fn unparseable_input_is_none_rather_than_a_panic() {
        for bad in ["", "{", "{\"a\"", "@@@", "{\"a\": }"] {
            let _ = locate(bad, &[Step::Key("a")]);
        }
    }

    #[test]
    fn every_prefix_of_the_document_is_handled() {
        for i in 0..=DOC.len() {
            if DOC.is_char_boundary(i) {
                let _ = locate(&DOC[..i], &[Step::Key("root"), Step::Key("rect")]);
            }
        }
    }

    #[test]
    fn a_located_span_can_be_spliced_without_disturbing_the_rest() {
        // The property that matters: replace the bytes, and everything else --
        // including the comment -- is byte-identical.
        let s = locate(DOC, &[Step::Key("root"), Step::Key("rect")]).unwrap();
        let mut out = alloc::string::String::new();
        out.push_str(&DOC[..s.start]);
        out.push_str("[1, 2, 3, 4]");
        out.push_str(&DOC[s.end..]);
        assert!(out.contains("// a comment, which must survive"));
        assert!(out.contains("[1, 2, 3, 4]"));
        assert!(!out.contains("[0, 0, 320, 160]"));
        // And it still parses.
        crate::scene::parse(&out).expect("spliced document must parse");
    }
}
