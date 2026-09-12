// SPDX-License-Identifier: GPL-3.0-only
//! Turning a token stream into a [`Value`] tree.
//!
//! A recursive-descent parser over [`super::lex::Lexer`]. It is deliberately strict
//! about structure and deliberately lax about two things JSON forbids —
//! trailing commas, and comments anywhere whitespace is allowed — because the
//! format exists to be hand-edited.
//!
//! # Why the recursion is bounded
//!
//! A scene file is untrusted input as far as this crate is concerned: it can
//! arrive from an SD card someone else wrote. `[[[[[...` nested a few thousand
//! deep would overflow the stack, and on bare metal that is not a panic, it is
//! a silent walk into whatever is below the stack guard. [`MAX_DEPTH`] caps it
//! at a level no honest scene reaches.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::lex::{LexError, Lexer, Spanned, Token};
use super::value::Value;

/// How deeply arrays and objects may nest before the document is rejected.
///
/// A widget tree that is 64 deep is already pathological; the limit exists to
/// bound stack use, not to express a design opinion.
pub const MAX_DEPTH: u32 = 64;

/// Why a document could not be parsed.
#[derive(Clone, Debug, PartialEq)]
pub enum ParseError {
    /// The tokeniser rejected the input before the parser saw it.
    Lex(LexError),
    /// A token appeared where the grammar did not allow it.
    Unexpected {
        /// What the parser was looking for, for the message.
        expected: &'static str,
        /// Line the offending token started on, 1-based.
        line: u32,
        /// Column the offending token started at, 1-based.
        col: u32,
    },
    /// Input ended in the middle of a value.
    UnexpectedEnd {
        /// What was still outstanding.
        expected: &'static str,
    },
    /// Nesting exceeded [`MAX_DEPTH`].
    TooDeep {
        /// Line at which the limit was hit.
        line: u32,
        /// Column at which the limit was hit.
        col: u32,
    },
    /// A second value followed a complete document.
    TrailingContent {
        /// Line the extra token started on.
        line: u32,
        /// Column the extra token started at.
        col: u32,
    },
}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        Self::Lex(e)
    }
}

/// Parse a document into a [`Value`] tree.
///
/// # Errors
///
/// Returns [`ParseError`] for malformed input, and does not panic on any
/// input a caller can supply.
pub fn parse(src: &str) -> Result<Value, ParseError> {
    let mut p = Parser {
        lexer: Lexer::new(src),
        peeked: None,
    };
    let value = p.value(0)?;
    // A document is exactly one value. Anything after it is far more likely to
    // be a missing comma than a second document, and silently ignoring it
    // would drop half a scene without a word.
    match p.next()? {
        None => Ok(value),
        Some(t) => Err(ParseError::TrailingContent {
            line: t.line,
            col: t.col,
        }),
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    peeked: Option<Option<Spanned<Token<'a>>>>,
}

impl<'a> Parser<'a> {
    fn next(&mut self) -> Result<Option<Spanned<Token<'a>>>, ParseError> {
        match self.peeked.take() {
            Some(t) => Ok(t),
            None => Ok(self.lexer.next_token()?),
        }
    }

    fn peek(&mut self) -> Result<Option<&Spanned<Token<'a>>>, ParseError> {
        if self.peeked.is_none() {
            self.peeked = Some(self.lexer.next_token()?);
        }
        Ok(self.peeked.as_ref().and_then(Option::as_ref))
    }

    fn value(&mut self, depth: u32) -> Result<Value, ParseError> {
        let t = self.next()?.ok_or(ParseError::UnexpectedEnd {
            expected: "a value",
        })?;
        match t.value {
            Token::Null => Ok(Value::Null),
            Token::True => Ok(Value::Bool(true)),
            Token::False => Ok(Value::Bool(false)),
            Token::Int(i) => Ok(Value::Int(i)),
            Token::Float(f) => Ok(Value::Float(f)),
            Token::Str(s) => Ok(Value::Str(s.to_string())),
            Token::OwnedStr(s) => Ok(Value::Str(s)),
            Token::LBracket => self.array(depth, t.line, t.col),
            Token::LBrace => self.object(depth, t.line, t.col),
            _ => Err(ParseError::Unexpected {
                expected: "a value",
                line: t.line,
                col: t.col,
            }),
        }
    }

    fn array(&mut self, depth: u32, line: u32, col: u32) -> Result<Value, ParseError> {
        if depth >= MAX_DEPTH {
            return Err(ParseError::TooDeep { line, col });
        }
        let mut items = Vec::new();
        loop {
            match self.peek()? {
                None => return Err(ParseError::UnexpectedEnd { expected: "']'" }),
                // Covers both the empty array and a trailing comma, which is
                // the whole reason the check is here and not after the comma.
                Some(t) if t.value == Token::RBracket => {
                    self.next()?;
                    return Ok(Value::Array(items));
                }
                _ => {}
            }
            items.push(self.value(depth + 1)?);
            match self.next()? {
                Some(t) if t.value == Token::Comma => {}
                Some(t) if t.value == Token::RBracket => return Ok(Value::Array(items)),
                Some(t) => {
                    return Err(ParseError::Unexpected {
                        expected: "',' or ']'",
                        line: t.line,
                        col: t.col,
                    });
                }
                None => {
                    return Err(ParseError::UnexpectedEnd {
                        expected: "',' or ']'",
                    });
                }
            }
        }
    }

    fn object(&mut self, depth: u32, line: u32, col: u32) -> Result<Value, ParseError> {
        if depth >= MAX_DEPTH {
            return Err(ParseError::TooDeep { line, col });
        }
        let mut fields: Vec<(String, Value)> = Vec::new();
        loop {
            match self.peek()? {
                None => return Err(ParseError::UnexpectedEnd { expected: "'}'" }),
                Some(t) if t.value == Token::RBrace => {
                    self.next()?;
                    return Ok(Value::Object(fields));
                }
                _ => {}
            }

            let key = match self.next()? {
                Some(Spanned {
                    value: Token::Str(s),
                    ..
                }) => s.to_string(),
                Some(Spanned {
                    value: Token::OwnedStr(s),
                    ..
                }) => s,
                Some(t) => {
                    return Err(ParseError::Unexpected {
                        expected: "a quoted key",
                        line: t.line,
                        col: t.col,
                    });
                }
                None => {
                    return Err(ParseError::UnexpectedEnd {
                        expected: "a quoted key",
                    });
                }
            };

            match self.next()? {
                Some(t) if t.value == Token::Colon => {}
                Some(t) => {
                    return Err(ParseError::Unexpected {
                        expected: "':'",
                        line: t.line,
                        col: t.col,
                    });
                }
                None => return Err(ParseError::UnexpectedEnd { expected: "':'" }),
            }

            fields.push((key, self.value(depth + 1)?));

            match self.next()? {
                Some(t) if t.value == Token::Comma => {}
                Some(t) if t.value == Token::RBrace => return Ok(Value::Object(fields)),
                Some(t) => {
                    return Err(ParseError::Unexpected {
                        expected: "',' or '}'",
                        line: t.line,
                        col: t.col,
                    });
                }
                None => {
                    return Err(ParseError::UnexpectedEnd {
                        expected: "',' or '}'",
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn scalars() {
        assert_eq!(parse("null").unwrap(), Value::Null);
        assert_eq!(parse("true").unwrap(), Value::Bool(true));
        assert_eq!(parse("-3").unwrap(), Value::Int(-3));
        assert_eq!(parse(r#""hi""#).unwrap(), Value::Str("hi".into()));
    }

    #[test]
    fn empty_containers() {
        assert_eq!(parse("[]").unwrap(), Value::Array(vec![]));
        assert_eq!(parse("{}").unwrap(), Value::Object(vec![]));
    }

    #[test]
    fn trailing_commas_are_accepted() {
        // The reason the format is a superset: a hand-edited file gains and
        // loses lines, and a trailing comma is the most common casualty.
        assert_eq!(parse("[1,]").unwrap(), Value::Array(vec![Value::Int(1)]));
        assert_eq!(
            parse(r#"{"a":1,}"#).unwrap(),
            Value::Object(vec![("a".into(), Value::Int(1))])
        );
    }

    #[test]
    fn comments_are_accepted_between_anything() {
        let got = parse("{ // leading\n \"a\" /*mid*/ : /*mid*/ 1 // trailing\n }").unwrap();
        assert_eq!(got, Value::Object(vec![("a".into(), Value::Int(1))]));
    }

    #[test]
    fn object_order_is_preserved() {
        let got = parse(r#"{"z":1,"a":2}"#).unwrap();
        match got {
            Value::Object(f) => assert_eq!(f[0].0, "z"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn nesting_works() {
        let got = parse(r#"{"kids":[{"n":1},{"n":2}]}"#).unwrap();
        let kids = got.get("kids").and_then(Value::as_array).unwrap();
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[1].get("n").and_then(Value::as_i64), Some(2));
    }

    #[test]
    fn a_whole_float_reads_as_an_integer() {
        // "4" and "4.0" mean the same thing to whoever typed the coordinate.
        assert_eq!(parse("4.0").unwrap().as_i64(), Some(4));
        assert_eq!(parse("4.5").unwrap().as_i64(), None);
    }

    #[test]
    fn a_float_beyond_i64_is_not_an_integer() {
        assert_eq!(parse("1e300").unwrap().as_i64(), None);
    }

    // --- rejection ---

    #[test]
    fn trailing_content_is_rejected() {
        // Two values in a row is almost always a missing comma, and silently
        // dropping the second would lose half a scene without a word.
        assert!(matches!(
            parse("1 2"),
            Err(ParseError::TrailingContent { .. })
        ));
    }

    #[test]
    fn unclosed_containers_are_rejected() {
        assert!(matches!(parse("["), Err(ParseError::UnexpectedEnd { .. })));
        assert!(matches!(parse("{"), Err(ParseError::UnexpectedEnd { .. })));
        assert!(parse(r#"{"a":1"#).is_err());
    }

    #[test]
    fn an_unquoted_key_is_rejected() {
        // A bare identifier never reaches the parser: `a` is not a token, so
        // the tokeniser rejects it first. Both layers refusing it is the point
        // -- what must not happen is the key being accepted.
        assert!(matches!(parse("{a:1}"), Err(ParseError::Lex(_))));

        // A keyword *does* lex, so this is the input that exercises the
        // parser's own "a quoted key" branch.
        assert!(matches!(
            parse("{true:1}"),
            Err(ParseError::Unexpected {
                expected: "a quoted key",
                ..
            })
        ));
    }

    #[test]
    fn a_missing_colon_is_rejected() {
        assert!(matches!(
            parse(r#"{"a" 1}"#),
            Err(ParseError::Unexpected {
                expected: "':'",
                ..
            })
        ));
    }

    #[test]
    fn errors_carry_a_useful_position() {
        // An error in a hand-edited scene is worthless if it cannot say where.
        match parse("{\n  \"a\": 1\n  \"b\": 2\n}") {
            Err(ParseError::Unexpected { line, .. }) => assert_eq!(line, 3),
            other => panic!("expected a positioned error, got {other:?}"),
        }
    }

    #[test]
    fn deep_nesting_is_rejected_rather_than_overflowing_the_stack() {
        // On bare metal a stack overflow is not a panic, it is a silent walk
        // past the guard page. This must be an Err at any input size.
        let deep = "[".repeat(10_000);
        assert!(matches!(parse(&deep), Err(ParseError::TooDeep { .. })));
    }

    #[test]
    fn every_prefix_of_a_valid_document_is_handled() {
        let doc = r#"{"a":[1,-2.5,true,null,"x\n"],/*c*/"b":{"c":[]},}"#;
        for i in 0..=doc.len() {
            if doc.is_char_boundary(i) {
                let _ = parse(&doc[..i]);
            }
        }
    }
}
