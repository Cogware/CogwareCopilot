//! Tokeniser for a JSON-superset scene format.
//!
//! Supports line comments (`//`), block comments (`/* */`), and trailing
//! commas in addition to standard JSON tokens.

use alloc::string::String;
use alloc::vec::Vec;

/// A lexical token produced by the [`Lexer`].
#[derive(Debug, Clone, PartialEq)]
pub enum Token<'a> {
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `:`
    Colon,
    /// `,`
    Comma,
    /// A string literal that required no escape decoding (borrowed from source).
    Str(&'a str),
    /// A string literal that contained escapes and was decoded into an owned `String`.
    OwnedStr(String),
    /// An integer value that fits in `i64`.
    Int(i64),
    /// A floating-point value, or an integer too large for `i64`.
    Float(f64),
    /// The literal `true`.
    True,
    /// The literal `false`.
    False,
    /// The literal `null`.
    Null,
}

/// A token (or any value) annotated with its source position.
#[derive(Debug, Clone)]
pub struct Spanned<T> {
    /// Byte offset of the token's first character in the source.
    ///
    /// Line and column are for a person reading an error; this is for a tool
    /// splicing the text. An editor that edits by line and column has to
    /// re-count the file on every change.
    pub start: usize,
    /// Byte offset just past the token's last character.
    pub end: usize,
    /// The inner value.
    pub value: T,
    /// 1-based line number where the token starts.
    pub line: u32,
    /// 1-based column number where the token starts.
    pub col: u32,
}

/// Errors that can occur during lexing.
#[derive(Debug, Clone, PartialEq)]
pub enum LexError {
    /// A string literal was not terminated before end of input.
    UnterminatedString {
        /// Line where the string started.
        line: u32,
        /// Column where the string started.
        col: u32,
    },
    /// A block comment was not terminated before end of input.
    UnterminatedComment {
        /// Line where the comment started.
        line: u32,
        /// Column where the comment started.
        col: u32,
    },
    /// An invalid escape sequence was encountered inside a string.
    BadEscape {
        /// Line of the backslash.
        line: u32,
        /// Column of the backslash.
        col: u32,
    },
    /// A number literal does not conform to the JSON grammar.
    BadNumber {
        /// Line where the number started.
        line: u32,
        /// Column where the number started.
        col: u32,
    },
    /// A character that cannot begin any token was encountered.
    UnexpectedChar {
        /// The offending character.
        ch: char,
        /// Line of the character.
        line: u32,
        /// Column of the character.
        col: u32,
    },
}

/// A streaming lexer over a JSON-superset source string.
pub struct Lexer<'a> {
    src: &'a str,
    /// Byte offset of the next character to read.
    pos: usize,
    /// Current 1-based line.
    line: u32,
    /// Current 1-based column.
    col: u32,
}

impl<'a> Lexer<'a> {
    /// Create a new lexer over `src`.
    pub fn new(src: &'a str) -> Self {
        Self {
            src,
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    /// Advance by one character, updating line/col. Returns the character.
    fn advance(&mut self) -> Option<char> {
        if self.pos >= self.src.len() {
            return None;
        }
        let ch = self.src[self.pos..].chars().next()?;
        let len = ch.len_utf8();
        self.pos += len;
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    /// Peek at the current character without consuming it.
    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    /// Skip whitespace and both comment forms, leaving the cursor on the next
    /// real character. Errors only on an unterminated block comment.
    fn skip_whitespace_and_comments(&mut self) -> Result<(), LexError> {
        loop {
            match self.peek() {
                None => return Ok(()),
                Some(' ') | Some('\t') | Some('\r') | Some('\n') => {
                    self.advance();
                }
                Some('/') => {
                    let next = self.src[self.pos + 1..].chars().next();
                    match next {
                        Some('/') => {
                            // Line comment: skip to end of line (don't consume the \n).
                            while let Some(c) = self.peek() {
                                if c == '\n' {
                                    break;
                                }
                                self.advance();
                            }
                        }
                        Some('*') => {
                            let start_line = self.line;
                            let start_col = self.col;
                            self.advance(); // consume '/'
                            self.advance(); // consume '*'
                            loop {
                                match self.peek() {
                                    None => {
                                        return Err(LexError::UnterminatedComment {
                                            line: start_line,
                                            col: start_col,
                                        });
                                    }
                                    Some('*') => {
                                        let after = self.src[self.pos + 1..].chars().next();
                                        if after == Some('/') {
                                            self.advance(); // consume '*'
                                            self.advance(); // consume '/'
                                            break;
                                        } else {
                                            self.advance();
                                        }
                                    }
                                    Some(_) => {
                                        self.advance();
                                    }
                                }
                            }
                        }
                        _ => {
                            // A bare '/' is not valid; treat as unexpected char.
                            let ch = self.advance().unwrap();
                            return Err(LexError::UnexpectedChar {
                                ch,
                                line: self.line - if ch == '\n' { 1 } else { 0 },
                                col: self.col - 1,
                            });
                        }
                    }
                }
                Some(_) => return Ok(()),
            }
        }
    }

    /// Lex the next token, or return `Ok(None)` at end of input.
    pub fn next_token(&mut self) -> Result<Option<Spanned<Token<'a>>>, LexError> {
        self.skip_whitespace_and_comments()?;

        let start_line = self.line;
        let start_col = self.col;
        let start = self.pos;

        let ch = match self.peek() {
            None => return Ok(None),
            Some(c) => c,
        };

        let token = match ch {
            '{' => {
                self.advance();
                Token::LBrace
            }
            '}' => {
                self.advance();
                Token::RBrace
            }
            '[' => {
                self.advance();
                Token::LBracket
            }
            ']' => {
                self.advance();
                Token::RBracket
            }
            ':' => {
                self.advance();
                Token::Colon
            }
            ',' => {
                self.advance();
                Token::Comma
            }
            't' => {
                self.expect_word("true")?;
                Token::True
            }
            'f' => {
                self.expect_word("false")?;
                Token::False
            }
            'n' => {
                self.expect_word("null")?;
                Token::Null
            }
            '"' => self.lex_string()?,
            '-' | '0'..='9' => self.lex_number()?,
            other => {
                self.advance();
                return Err(LexError::UnexpectedChar {
                    ch: other,
                    line: start_line,
                    col: start_col,
                });
            }
        };

        Ok(Some(Spanned {
            value: token,
            start,
            end: self.pos,
            line: start_line,
            col: start_col,
        }))
    }

    /// Consume the remainder of a keyword starting at the current position.
    fn expect_word(&mut self, word: &str) -> Result<(), LexError> {
        for expected in word.chars() {
            match self.advance() {
                Some(c) if c == expected => {}
                _ => {
                    return Err(LexError::UnexpectedChar {
                        ch: self.peek().unwrap_or('\0'),
                        line: self.line,
                        col: self.col,
                    });
                }
            }
        }
        Ok(())
    }

    /// Lex a string literal starting at the current position (which is at `"`).
    fn lex_string(&mut self) -> Result<Token<'a>, LexError> {
        let start_line = self.line;
        let start_col = self.col;

        self.advance();

        // Byte position right after the opening quote.
        let content_start = self.pos;

        let mut has_escapes = false;
        let mut decoded: Vec<char> = Vec::new();

        loop {
            match self.peek() {
                None => {
                    return Err(LexError::UnterminatedString {
                        line: start_line,
                        col: start_col,
                    });
                }
                Some('"') => {
                    // Closing quote found.
                    let content_end = self.pos;
                    self.advance(); // consume closing quote

                    if !has_escapes {
                        // Safe to borrow: no escapes means the raw slice is the value.
                        let raw = &self.src[content_start..content_end];
                        return Ok(Token::Str(raw));
                    } else {
                        let s: String = decoded.into_iter().collect();
                        return Ok(Token::OwnedStr(s));
                    }
                }
                Some('\\') => {
                    has_escapes = true;
                    let esc_line = self.line;
                    let esc_col = self.col;
                    self.advance(); // consume backslash

                    let esc_char = match self.advance() {
                        Some(c) => c,
                        None => {
                            return Err(LexError::UnterminatedString {
                                line: start_line,
                                col: start_col,
                            });
                        }
                    };

                    match esc_char {
                        '"' => decoded.push('"'),
                        '\\' => decoded.push('\\'),
                        '/' => decoded.push('/'),
                        'b' => decoded.push('\u{0008}'),
                        'f' => decoded.push('\u{000C}'),
                        'n' => decoded.push('\n'),
                        'r' => decoded.push('\r'),
                        't' => decoded.push('\t'),
                        'u' => {
                            let cp = self.lex_unicode_escape(esc_line, esc_col)?;
                            // Check for surrogate pair.
                            if (0xD800..=0xDBFF).contains(&cp) {
                                // Expect \uXXXX for low surrogate.
                                if self.peek() == Some('\\') {
                                    self.advance();
                                    if self.peek() == Some('u') {
                                        self.advance();
                                        let low = self.lex_unicode_escape(esc_line, esc_col)?;
                                        if (0xDC00..=0xDFFF).contains(&low) {
                                            let combined =
                                                0x10000 + ((cp - 0xD800) << 10) + (low - 0xDC00);
                                            decoded.push(char::from_u32(combined).unwrap());
                                        } else {
                                            return Err(LexError::BadEscape {
                                                line: esc_line,
                                                col: esc_col,
                                            });
                                        }
                                    } else {
                                        return Err(LexError::BadEscape {
                                            line: esc_line,
                                            col: esc_col,
                                        });
                                    }
                                } else {
                                    return Err(LexError::BadEscape {
                                        line: esc_line,
                                        col: esc_col,
                                    });
                                }
                            } else if (0xDC00..=0xDFFF).contains(&cp) {
                                // Lone low surrogate is invalid.
                                return Err(LexError::BadEscape {
                                    line: esc_line,
                                    col: esc_col,
                                });
                            } else {
                                decoded.push(char::from_u32(cp).unwrap());
                            }
                        }
                        _ => {
                            return Err(LexError::BadEscape {
                                line: esc_line,
                                col: esc_col,
                            });
                        }
                    }
                }
                Some('\n') => {
                    // Raw newline in string is not valid JSON.
                    return Err(LexError::UnterminatedString {
                        line: start_line,
                        col: start_col,
                    });
                }
                Some(c) => {
                    decoded.push(c);
                    self.advance();
                }
            }
        }
    }

    /// Parse a `\uXXXX` escape (the `u` has already been consumed).
    fn lex_unicode_escape(&mut self, line: u32, col: u32) -> Result<u32, LexError> {
        let mut val: u32 = 0;
        for _ in 0..4 {
            match self.advance() {
                Some(c) if c.is_ascii_hexdigit() => {
                    val = val * 16 + c.to_digit(16).unwrap();
                }
                _ => {
                    return Err(LexError::BadEscape { line, col });
                }
            }
        }
        Ok(val)
    }

    /// Lex a number literal starting at the current position.
    fn lex_number(&mut self) -> Result<Token<'a>, LexError> {
        let start_line = self.line;
        let start_col = self.col;
        let num_start = self.pos;

        if self.peek() == Some('-') {
            self.advance();
        }

        match self.peek() {
            Some('0') => {
                self.advance();
                // Next char must not be a digit (no leading zeros).
                if let Some(c) = self.peek()
                    && c.is_ascii_digit()
                {
                    return Err(LexError::BadNumber {
                        line: start_line,
                        col: start_col,
                    });
                }
            }
            Some(c) if c.is_ascii_digit() => {
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            _ => {
                return Err(LexError::BadNumber {
                    line: start_line,
                    col: start_col,
                });
            }
        }

        let mut is_float = false;

        if self.peek() == Some('.') {
            is_float = true;
            self.advance();
            match self.peek() {
                Some(c) if c.is_ascii_digit() => {
                    while let Some(c) = self.peek() {
                        if c.is_ascii_digit() {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                _ => {
                    return Err(LexError::BadNumber {
                        line: start_line,
                        col: start_col,
                    });
                }
            }
        }

        if let Some(c) = self.peek()
            && (c == 'e' || c == 'E')
        {
            is_float = true;
            self.advance();
            if self.peek() == Some('+') || self.peek() == Some('-') {
                self.advance();
            }
            match self.peek() {
                Some(c) if c.is_ascii_digit() => {
                    while let Some(c) = self.peek() {
                        if c.is_ascii_digit() {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                _ => {
                    return Err(LexError::BadNumber {
                        line: start_line,
                        col: start_col,
                    });
                }
            }
        }

        let num_str = &self.src[num_start..self.pos];

        if is_float {
            match num_str.parse::<f64>() {
                Ok(f) => Ok(Token::Float(f)),
                Err(_) => Err(LexError::BadNumber {
                    line: start_line,
                    col: start_col,
                }),
            }
        } else {
            match num_str.parse::<i64>() {
                Ok(i) => Ok(Token::Int(i)),
                Err(_) => {
                    // Overflow: fall back to float.
                    match num_str.parse::<f64>() {
                        Ok(f) => Ok(Token::Float(f)),
                        Err(_) => Err(LexError::BadNumber {
                            line: start_line,
                            col: start_col,
                        }),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// Drain the lexer, discarding spans. Most tests care only about the
    /// token sequence; the ones that care about position ask for it directly.
    fn lex(src: &str) -> Result<Vec<Token<'_>>, LexError> {
        let mut lx = Lexer::new(src);
        let mut out = Vec::new();
        while let Some(t) = lx.next_token()? {
            out.push(t.value);
        }
        Ok(out)
    }

    fn spans(src: &str) -> Result<Vec<(u32, u32)>, LexError> {
        let mut lx = Lexer::new(src);
        let mut out = Vec::new();
        while let Some(t) = lx.next_token()? {
            out.push((t.line, t.col));
        }
        Ok(out)
    }

    #[test]
    fn empty_input_yields_nothing() {
        assert_eq!(lex("").unwrap(), &[]);
        assert_eq!(lex("   \n\t ").unwrap(), &[]);
    }

    #[test]
    fn structural_tokens() {
        assert_eq!(
            lex("{}[],:").unwrap(),
            &[
                Token::LBrace,
                Token::RBrace,
                Token::LBracket,
                Token::RBracket,
                Token::Comma,
                Token::Colon
            ]
        );
    }

    #[test]
    fn line_comments_are_skipped() {
        assert_eq!(lex("// gone\n1 // also gone").unwrap(), &[Token::Int(1)]);
    }

    #[test]
    fn block_comments_are_skipped() {
        assert_eq!(
            lex("/* gone */ 1 /* also\ngone */").unwrap(),
            &[Token::Int(1)]
        );
    }

    #[test]
    fn trailing_commas_are_just_commas() {
        // The lexer does not enforce grammar; it only has to emit the comma so
        // the parser can choose to forgive it.
        assert_eq!(
            lex("[1,]").unwrap(),
            &[
                Token::LBracket,
                Token::Int(1),
                Token::Comma,
                Token::RBracket
            ]
        );
    }

    #[test]
    fn integers_and_floats_are_distinguished() {
        assert_eq!(lex("0").unwrap(), &[Token::Int(0)]);
        assert_eq!(lex("-7").unwrap(), &[Token::Int(-7)]);
        assert!(matches!(lex("1.5").unwrap()[0], Token::Float(_)));
        assert!(matches!(lex("1e3").unwrap()[0], Token::Float(_)));
        assert!(matches!(lex("-2.5E-3").unwrap()[0], Token::Float(_)));
    }

    #[test]
    fn an_integer_too_big_for_i64_becomes_a_float() {
        // Silently wrapping here would turn a coordinate into garbage.
        assert!(matches!(
            lex("99999999999999999999").unwrap()[0],
            Token::Float(_)
        ));
    }

    #[test]
    fn keywords() {
        assert_eq!(
            lex("true false null").unwrap(),
            &[Token::True, Token::False, Token::Null]
        );
    }

    #[test]
    fn a_string_without_escapes_is_borrowed() {
        // The whole point of the borrow: a scene full of plain strings must
        // not allocate once per string.
        assert_eq!(lex(r#""plain""#).unwrap(), &[Token::Str("plain")]);
    }

    #[test]
    fn escapes_decode() {
        let t = lex(r#""a\nb\t\"\\\/""#).unwrap();
        match &t[0] {
            Token::OwnedStr(s) => assert_eq!(s, "a\nb\t\"\\/"),
            other => panic!("escaped string should be owned, got {other:?}"),
        }
    }

    #[test]
    fn unicode_escape_decodes() {
        let t = lex(r#""A\u00e9""#).unwrap();
        match &t[0] {
            Token::OwnedStr(s) => assert_eq!(s, "Aé"),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn surrogate_pair_decodes_to_one_char() {
        let t = lex(r#""\ud83d\ude00""#).unwrap();
        match &t[0] {
            Token::OwnedStr(s) => assert_eq!(s, "😀"),
            other => panic!("got {other:?}"),
        }
    }

    // --- malformed input: none of these may panic ---

    #[test]
    fn unterminated_string_is_an_error() {
        assert!(matches!(
            lex(r#""oops"#),
            Err(LexError::UnterminatedString { .. })
        ));
    }

    #[test]
    fn unterminated_block_comment_is_an_error() {
        assert!(matches!(
            lex("/* forever"),
            Err(LexError::UnterminatedComment { .. })
        ));
    }

    #[test]
    fn bad_escape_is_an_error() {
        assert!(matches!(lex(r#""\q""#), Err(LexError::BadEscape { .. })));
    }

    #[test]
    fn truncated_unicode_escape_is_an_error() {
        assert!(lex(r#""\u12""#).is_err());
    }

    #[test]
    fn lone_high_surrogate_is_an_error() {
        // A lone surrogate is not a scalar value; producing one would be an
        // invalid `char` and is exactly the case that tempts an unwrap.
        assert!(lex(r#""\ud83d""#).is_err());
    }

    #[test]
    fn stray_character_is_an_error() {
        assert!(matches!(lex("@"), Err(LexError::UnexpectedChar { .. })));
    }

    #[test]
    fn truncated_input_never_panics() {
        // Every prefix of a valid document must produce Ok or Err, never a
        // panic. This is the cheapest fuzzing there is and it catches the
        // "just index one more byte" class of bug.
        let doc = r#"{"a":[1,-2.5,true,null,"x\n"],/*c*/"b":{}}"#;
        for i in 0..=doc.len() {
            if doc.is_char_boundary(i) {
                let _ = lex(&doc[..i]);
            }
        }
    }

    // --- positions ---

    #[test]
    fn positions_are_one_based() {
        assert_eq!(spans("1").unwrap(), &[(1, 1)]);
    }

    #[test]
    fn line_and_column_survive_a_multiline_comment() {
        // The reason positions exist at all: an error in a hand-edited scene
        // has to point at the right line, and a block comment is where naive
        // counting goes wrong.
        let got = spans("/* one\ntwo\nthree */ 42").unwrap();
        assert_eq!(got, &[(3, 10)]);
    }

    #[test]
    fn column_resets_after_a_newline() {
        assert_eq!(spans("1\n  2").unwrap(), &[(1, 1), (2, 3)]);
    }
}
