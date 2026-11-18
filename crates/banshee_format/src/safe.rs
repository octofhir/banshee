//! Check the printers against the source token stream and use conservative
//! whitespace formatting when a specialized printer cannot emit every token.

use std::borrow::Cow;

use banshee_syntax::{Parse, ParseError, SyntaxKind, SyntaxNode};
use text_size::TextSize;

use crate::config::{FormatConfig, IdentifierCase, KeywordCase};
use crate::pg_formatter::PgFormatterConfig;
use crate::validate::FormatError;

use SyntaxKind::*;

#[derive(Clone, Copy)]
pub(crate) enum Style<'a> {
    Sql(&'a FormatConfig),
    Pg(&'a PgFormatterConfig),
}

impl Style<'_> {
    fn removes_comments(self) -> bool {
        matches!(self, Self::Pg(config) if config.no_comment)
    }

    fn trailing_newline(self) -> bool {
        !matches!(self, Self::Pg(config) if config.no_extra_line)
    }

    fn token_text<'a>(self, token: Token<'a>) -> Cow<'a, str> {
        match self {
            Self::Sql(config) if token.kind.is_keyword() => match config.keyword_case {
                KeywordCase::Upper => token.text.to_ascii_uppercase().into(),
                KeywordCase::Lower => token.text.to_ascii_lowercase().into(),
                KeywordCase::Preserve => token.text.into(),
            },
            Self::Sql(config)
                if token.kind == IDENT && config.identifier_case == IdentifierCase::Lower =>
            {
                token.text.to_ascii_lowercase().into()
            }
            Self::Pg(config) if token.kind.is_keyword() => {
                config.keyword_case.apply(token.text).into()
            }
            _ => token.text.into(),
        }
    }
}

#[derive(Clone, Copy)]
struct Token<'a> {
    kind: SyntaxKind,
    text: &'a str,
    start: usize,
    gap: &'a str,
}

fn is_comment(kind: SyntaxKind) -> bool {
    matches!(kind, LINE_COMMENT | BLOCK_COMMENT)
}

fn has_newline(text: &str) -> bool {
    text.contains(['\r', '\n'])
}

fn tokens(source: &str, remove_comments: bool) -> Vec<Token<'_>> {
    let mut offset = 0;
    let mut previous_end = 0;
    banshee_lexer::tokenize(source)
        .into_iter()
        .filter_map(|token| {
            let start = offset;
            offset += usize::from(token.len);
            let kind = token.kind;
            if matches!(kind, WHITESPACE | NEWLINE) || (remove_comments && is_comment(kind)) {
                return None;
            }
            let gap = &source[previous_end..start];
            previous_end = offset;
            Some(Token {
                kind,
                text: &source[start..offset],
                start,
                gap,
            })
        })
        .collect()
}

// PostgreSQL's scanner can combine operators, numeric components, and string
// prefixes that our CST lexer represents as separate tokens.
fn needs_same_adjacency(left: Token<'_>, right: Token<'_>) -> bool {
    fn operator(ch: u8) -> bool {
        b"+-*/<>=~!@#%^&|`?:.$".contains(&ch)
    }
    fn word(ch: u8) -> bool {
        ch.is_ascii_alphanumeric() || matches!(ch, b'_' | b'$') || !ch.is_ascii()
    }

    let Some((&last, &first)) = left
        .text
        .as_bytes()
        .last()
        .zip(right.text.as_bytes().first())
    else {
        return false;
    };
    (operator(last) && operator(first))
        || (word(last) && word(first))
        || (last == b'.' && first.is_ascii_digit())
        || (last.is_ascii_digit() && first == b'.')
        || (first == b'\'' && (last.is_ascii_alphanumeric() || last == b'\''))
        || (left.kind == QUOTED_IDENT && right.kind == QUOTED_IDENT)
}

fn same_gap(left: Token<'_>, before: Token<'_>, after: Token<'_>) -> bool {
    if is_comment(left.kind) || is_comment(before.kind) {
        return has_newline(before.gap) == has_newline(after.gap);
    }
    !needs_same_adjacency(left, before) || before.gap.is_empty() == after.gap.is_empty()
}

fn preserves(original: &[Token<'_>], output: &str) -> bool {
    let output = tokens(output, false);
    original.len() == output.len()
        && original.iter().zip(&output).enumerate().all(|(i, (a, b))| {
            a.kind == b.kind
                && (a.text == b.text
                    || ((a.kind.is_keyword() || a.kind == IDENT)
                        && a.text.eq_ignore_ascii_case(b.text)))
                && (i == 0 || same_gap(original[i - 1], *a, *b))
        })
}

pub(crate) fn preserves_sql(original: &str, output: &str) -> bool {
    preserves(&tokens(original, false), output)
}

pub(crate) fn source_text(root: &SyntaxNode) -> String {
    let mut source = String::with_capacity(usize::from(root.text_range().len()));
    for token in root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
    {
        source.push_str(token.text());
    }
    source
}

pub(crate) fn format(
    source: &str,
    parse: &Parse,
    style: Style<'_>,
    candidate: impl FnOnce(&SyntaxNode) -> String,
) -> Result<String, FormatError> {
    if !parse.errors().is_empty() {
        return Err(FormatError::InvalidInput(parse.errors().to_vec()));
    }
    let root = parse.syntax();
    if let Some(offset) = copy_from_stdin(&root) {
        return Err(FormatError::InvalidInput(vec![ParseError::at_offset(
            "COPY FROM STDIN scripts may contain raw data",
            offset,
        )]));
    }
    let original = tokens(source, style.removes_comments());
    if let Some(token) = original.iter().find(|token| token.kind == ERROR) {
        return Err(FormatError::InvalidInput(vec![ParseError::at_offset(
            "unrecognized SQL token",
            (token.start as u32).into(),
        )]));
    }

    let output = candidate(&root);
    if preserves(&original, &output) {
        return Ok(output);
    }
    let output = format_tokens(source, &root, &original, style);
    if preserves(&original, &output) {
        Ok(output)
    } else {
        Err(FormatError::SemanticMismatch {
            expected: "the original SQL token stream".to_owned(),
            actual: "formatting changed a token or a significant boundary".to_owned(),
        })
    }
}

fn copy_from_stdin(root: &SyntaxNode) -> Option<TextSize> {
    for copy in root.children().filter(|node| node.kind() == COPY_STMT) {
        let mut depth: usize = 0;
        let mut after_from = false;
        for token in copy.children_with_tokens().filter_map(|e| e.into_token()) {
            let kind = token.kind();
            if kind.is_trivia() {
                continue;
            }
            if after_from && token.text().eq_ignore_ascii_case("stdin") {
                return Some(token.text_range().start());
            }
            match kind {
                L_PAREN => depth += 1,
                R_PAREN => depth = depth.saturating_sub(1),
                _ => {}
            }
            after_from = depth == 0 && kind == FROM_KW;
        }
    }
    None
}

fn format_tokens(
    source: &str,
    root: &SyntaxNode,
    tokens: &[Token<'_>],
    style: Style<'_>,
) -> String {
    let mut output = String::with_capacity(source.len());
    let mut clause_starts = root
        .descendants()
        .filter(|node| {
            matches!(
                node.kind(),
                FROM_CLAUSE
                    | WHERE_CLAUSE
                    | GROUP_BY_CLAUSE
                    | HAVING_CLAUSE
                    | ORDER_BY_CLAUSE
                    | LIMIT_CLAUSE
                    | OFFSET_CLAUSE
                    | FETCH_CLAUSE
                    | FOR_CLAUSE
                    | RETURNING_CLAUSE
                    | WINDOW_SPEC
            )
        })
        .map(|node| node.text_range().start())
        .peekable();
    let mut depth: usize = 0;

    for (i, &token) in tokens.iter().enumerate() {
        let start = TextSize::from(token.start as u32);
        while clause_starts.next_if(|&offset| offset < start).is_some() {}
        let starts_clause = clause_starts.next_if_eq(&start).is_some();
        if matches!(token.kind, R_PAREN | R_BRACKET) {
            depth = depth.saturating_sub(1);
        }

        if let Some(&left) = i.checked_sub(1).and_then(|i| tokens.get(i)) {
            if is_comment(left.kind) || is_comment(token.kind) {
                if has_newline(token.gap) {
                    output.push('\n');
                } else if !token.gap.is_empty() || !needs_same_adjacency(left, token) {
                    output.push(' ');
                }
            } else if !token.gap.is_empty() || !needs_same_adjacency(left, token) {
                if !style.removes_comments() && has_newline(token.gap) {
                    output.push_str(token.gap);
                } else if depth == 0
                    && (left.kind == SEMICOLON
                        || starts_clause
                        || matches!(token.kind, UNION_KW | INTERSECT_KW | EXCEPT_KW))
                {
                    output.push('\n');
                } else if !token.gap.is_empty() || left.kind == COMMA {
                    output.push(' ');
                }
            }
        }

        output.push_str(&style.token_text(token));
        if matches!(token.kind, L_PAREN | L_BRACKET) {
            depth += 1;
        }
    }

    if !output.is_empty() && style.trailing_newline() {
        output.push('\n');
    }
    output
}
