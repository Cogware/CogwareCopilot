// SPDX-License-Identifier: GPL-3.0-only
//! Update pin fields while retaining comments, including those beside obsolete fields.

use copilot::scene::Step;
use copilot::scene::lex::{Lexer, Token};

pub(super) fn mapping(mut text: String, path: &[Step<'_>], body: &str) -> Result<String, String> {
    for key in [
        "pin",
        "label",
        "dir",
        "active",
        "pull",
        "indicator",
        "gauge",
        "above",
        "at_least",
        "below",
        "at_most",
        "action",
        "pager",
        "to",
        "menu",
        "button",
        "mode",
        "name",
    ] {
        if let Some(span) = copilot::scene::locate(body, &[Step::Key(key)]) {
            let value = body
                .get(span.start..span.end)
                .ok_or("could not read the pin draft")?;
            text = copilot::scene::set(&text, path, key, value)
                .ok_or("could not write that pin onto the rig")?;
        } else {
            text = unset(text, path, key)?;
        }
    }
    Ok(text)
}

fn unset(mut text: String, path: &[Step<'_>], key: &str) -> Result<String, String> {
    let mut field = path.to_vec();
    field.push(Step::Key(key));
    let Some(value) = copilot::scene::locate(&text, &field) else {
        return Ok(text);
    };
    let failure = "could not remove an obsolete GPIO field";
    let mut lexer = Lexer::new(&text);
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token().map_err(|_| failure)? {
        tokens.push(token);
    }
    let first = tokens
        .iter()
        .position(|token| token.start == value.start)
        .ok_or(failure)?;
    let key_index = first.checked_sub(2).ok_or(failure)?;
    if !matches!(
        tokens.get(first - 1).map(|token| &token.value),
        Some(Token::Colon)
    ) {
        return Err(failure.into());
    }
    let last = tokens
        .iter()
        .rposition(|token| token.end <= value.end)
        .ok_or(failure)?;
    // Remove syntax tokens, not the gaps between them: those gaps hold wiring comments.
    let mut ranges: Vec<_> = tokens
        .get(key_index..=last)
        .ok_or(failure)?
        .iter()
        .map(|token| token.start..token.end)
        .collect();
    let comma = tokens
        .get(last + 1)
        .filter(|token| token.value == Token::Comma)
        .or_else(|| {
            key_index
                .checked_sub(1)
                .and_then(|i| tokens.get(i))
                .filter(|token| token.value == Token::Comma)
        });
    if let Some(comma) = comma {
        ranges.push(comma.start..comma.end);
    }
    ranges.sort_unstable_by_key(|range| range.start);
    for range in ranges.into_iter().rev() {
        text.get(range.clone()).ok_or(failure)?;
        text.replace_range(range, "");
    }
    Ok(text)
}
