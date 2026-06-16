use crate::event::Event;
use crate::grammar::jsonpath_lexer::tokenize_jsonpath;
use banshee_syntax::{ParseError, SyntaxKind, Token};
use cstree::build::GreenNodeBuilder;
use cstree::green::GreenNode;
use cstree::interning::TokenInterner;
use std::sync::Arc;
use text_size::{TextRange, TextSize};

pub struct Sink<'t> {
    builder: GreenNodeBuilder<'static, 'static, SyntaxKind>,
    tokens: &'t [Token],
    source: &'t str,
    cursor: usize,
    text_pos: TextSize,
    events: Vec<Event>,
    errors: Vec<ParseError>,
    /// Track when we're inside a JSONPATH_LITERAL node
    in_jsonpath_literal: bool,
}

impl<'t> Sink<'t> {
    pub fn new(tokens: &'t [Token], source: &'t str, events: Vec<Event>) -> Self {
        Self {
            builder: GreenNodeBuilder::new(),
            tokens,
            source,
            cursor: 0,
            text_pos: TextSize::new(0),
            events,
            errors: Vec::new(),
            in_jsonpath_literal: false,
        }
    }

    pub fn finish(mut self) -> (GreenNode, Vec<ParseError>, Arc<TokenInterner>) {
        // Track node depth for JSONPATH_LITERAL handling
        let mut jsonpath_depth = 0u32;

        // Process events, following forward_parent chains at processing time
        for i in 0..self.events.len() {
            match std::mem::replace(&mut self.events[i], Event::Placeholder) {
                Event::Start {
                    kind,
                    forward_parent,
                } => {
                    // Collect the parent chain: [self, parent, grandparent, ...]
                    let mut kinds = vec![kind];
                    let mut fp = forward_parent;

                    while let Some(parent_idx) = fp {
                        fp = match std::mem::replace(
                            &mut self.events[parent_idx as usize],
                            Event::Placeholder,
                        ) {
                            Event::Start {
                                kind: parent_kind,
                                forward_parent: parent_fp,
                            } => {
                                kinds.push(parent_kind);
                                parent_fp
                            }
                            _ => unreachable!(),
                        };
                    }

                    // Start nodes in reverse order (outermost parent first)
                    for kind in kinds.into_iter().rev() {
                        if kind == SyntaxKind::JSONPATH_LITERAL {
                            self.in_jsonpath_literal = true;
                            jsonpath_depth = 1;
                        } else if self.in_jsonpath_literal {
                            jsonpath_depth += 1;
                        }
                        self.builder.start_node(kind);
                    }
                }
                Event::Finish => {
                    if self.in_jsonpath_literal {
                        jsonpath_depth = jsonpath_depth.saturating_sub(1);
                        if jsonpath_depth == 0 {
                            self.in_jsonpath_literal = false;
                        }
                    }
                    self.builder.finish_node();
                }
                Event::Token { kind, n_raw_tokens } => {
                    // Check if this is a STRING token inside JSONPATH_LITERAL
                    if self.in_jsonpath_literal && kind == SyntaxKind::STRING {
                        self.jsonpath_token(n_raw_tokens);
                    } else {
                        self.token(kind, n_raw_tokens);
                    }
                }
                Event::Error { msg } => {
                    let context = self.error_context();
                    let message = self.format_error_message(msg, &context);
                    self.errors.push(ParseError::new(message, context.range));
                }
                Event::Placeholder => {}
            }
        }

        let (node, cache) = self.builder.finish();
        let interner = Arc::new(
            cache
                .expect("builder should have cache")
                .into_interner()
                .expect("cache should have interner"),
        );
        (node, self.errors, interner)
    }

    fn token(&mut self, kind: SyntaxKind, n_raw_tokens: u8) {
        let mut len = TextSize::new(0);
        for _ in 0..n_raw_tokens {
            if self.cursor < self.tokens.len() {
                len += self.tokens[self.cursor].len;
                self.cursor += 1;
            }
        }
        let text = &self.source[self.text_pos.into()..][..len.into()];
        self.text_pos += len;
        self.builder.token(kind, text);
    }

    /// Handle a STRING token inside JSONPATH_LITERAL by parsing its content as JSONPath
    fn jsonpath_token(&mut self, n_raw_tokens: u8) {
        // Calculate the string's length and text
        let mut len = TextSize::new(0);
        for _ in 0..n_raw_tokens {
            if self.cursor < self.tokens.len() {
                len += self.tokens[self.cursor].len;
                self.cursor += 1;
            }
        }
        let full_text = &self.source[self.text_pos.into()..][..len.into()];
        self.text_pos += len;

        // Extract the content without quotes
        let content = strip_string_quotes(full_text);

        // Start a JSONPATH_CONTENT wrapper node
        self.builder.start_node(SyntaxKind::JSONPATH_CONTENT);

        // Emit the opening quote
        let quote_char = full_text.chars().next().unwrap_or('\'');
        let quote_str = &full_text[..quote_char.len_utf8()];
        self.builder.token(SyntaxKind::JP_QUOTE, quote_str);

        // Tokenize and emit the JSONPath content
        let jp_tokens = tokenize_jsonpath(content);
        let mut content_pos = 0usize;
        for jpt in &jp_tokens {
            let token_len: usize = jpt.len.into();
            let token_text = &content[content_pos..content_pos + token_len];
            self.builder.token(jpt.kind, token_text);
            content_pos += token_len;
        }

        // Emit the closing quote
        let close_quote = &full_text[full_text.len() - quote_char.len_utf8()..];
        self.builder.token(SyntaxKind::JP_QUOTE, close_quote);

        self.builder.finish_node();
    }

    fn error_context(&self) -> ErrorContext<'_> {
        let mut idx = self.cursor;
        let mut offset = self.text_pos;
        while idx < self.tokens.len() && self.tokens[idx].kind.is_trivia() {
            offset += self.tokens[idx].len;
            idx += 1;
        }

        if idx < self.tokens.len() {
            let token = &self.tokens[idx];
            let range = TextRange::new(offset, offset + token.len);
            let start: usize = offset.into();
            let end: usize = (offset + token.len).into();
            let text = self.source.get(start..end);
            ErrorContext {
                range,
                kind: Some(token.kind),
                text,
            }
        } else {
            ErrorContext {
                range: TextRange::empty(offset),
                kind: None,
                text: None,
            }
        }
    }

    fn format_error_message(&self, msg: impl Into<String>, context: &ErrorContext) -> String {
        let mut message = msg.into();
        match context.kind {
            Some(kind) => {
                let mut found = format!("{:?}", kind);
                if let Some(text) = context.text {
                    let snippet = sanitize_snippet(text);
                    found.push_str(&format!(" `{}`", snippet));
                }
                message.push_str(&format!("; found {found}"));
            }
            None => {
                message.push_str("; found end of input");
            }
        }

        if let Some(hint) = suggestion_for(&message, context.kind) {
            message.push_str(&format!(" ({hint})"));
        }

        message
    }
}

struct ErrorContext<'a> {
    range: TextRange,
    kind: Option<SyntaxKind>,
    text: Option<&'a str>,
}

fn sanitize_snippet(text: &str) -> String {
    let mut snippet = text.replace('\n', "\\n").replace('\t', "\\t");
    if snippet.len() > 32 {
        // Truncate to the largest char boundary at or below 32 bytes so we
        // never split a multi-byte UTF-8 sequence (e.g. Cyrillic text).
        let mut end = 32;
        while end > 0 && !snippet.is_char_boundary(end) {
            end -= 1;
        }
        snippet.truncate(end);
        snippet.push_str("...");
    }
    snippet
}

fn suggestion_for(msg: &str, found_kind: Option<SyntaxKind>) -> Option<&'static str> {
    if msg.contains("expected type name") {
        return Some("Hint: use a valid type name like `text`, `int`, or `uuid`.");
    }
    if msg.contains("expected JOIN") {
        return Some("Hint: add a JOIN type like `JOIN`, `LEFT JOIN`, or `CROSS JOIN`.");
    }
    if msg.contains("expected SELECT, INSERT, UPDATE, or DELETE in CTE body") {
        return Some("Hint: CTE bodies must be a full SELECT/INSERT/UPDATE/DELETE statement.");
    }
    if msg.contains("expected DO_KW") {
        return Some("Hint: add `DO NOTHING` or `DO UPDATE` after ON CONFLICT.");
    }
    if msg.contains("expected $ or @") {
        return Some("Hint: JSONPath roots start with `$` or filter contexts with `@`.");
    }
    if msg.contains("expected") && found_kind == Some(SyntaxKind::COMMA) {
        return Some("Hint: remove the trailing comma.");
    }
    None
}

/// Strip quotes from a SQL string literal, returning the inner content
fn strip_string_quotes(s: &str) -> &str {
    if s.len() < 2 {
        return s;
    }
    let first = s.chars().next().unwrap();
    match first {
        '\'' | '"' => {
            // Simple quoted string
            &s[1..s.len() - 1]
        }
        '$' => {
            // Dollar-quoted string: $tag$content$tag$
            // Find the first $ after the opening
            if let Some(end_of_tag) = s[1..].find('$') {
                let tag_len = end_of_tag + 2; // includes both $
                let tag = &s[..tag_len];
                if s.ends_with(tag) {
                    return &s[tag_len..s.len() - tag_len];
                }
            }
            s
        }
        _ => s,
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_snippet;

    #[test]
    fn truncates_at_char_boundary_for_multibyte_text() {
        // Cyrillic chars are 2 bytes each in UTF-8; byte 32 lands mid-character.
        let text = "проверка границы символов в длинной строке";
        let snippet = sanitize_snippet(text);
        // Must not panic and must remain valid UTF-8 cut on a char boundary.
        assert!(snippet.ends_with("..."));
        assert!(snippet.len() <= 32 + "...".len());
    }

    #[test]
    fn leaves_short_text_untouched() {
        assert_eq!(sanitize_snippet("привет"), "привет");
    }
}
