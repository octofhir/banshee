use banshee_syntax::{SyntaxKind, SyntaxNode};
use cstree::util::NodeOrToken;
use text_size::{TextRange, TextSize};

/// Find formatter directives in SQL line comments, then align their protected
/// spans to statement boundaries so the formatter always sees complete SQL.
pub(crate) fn protected_ranges(source: &str, root: &SyntaxNode) -> Vec<TextRange> {
    if !source.contains("fmt:") && !source.contains("noqa") {
        return Vec::new();
    }
    let mut statements = None;
    let mut protected = Vec::new();
    let mut disabled = None;
    for token in root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
    {
        if !matches!(
            token.kind(),
            SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
        ) {
            continue;
        }
        let comment = token.text_range();
        let start = usize::from(comment.start());
        let end = usize::from(comment.end());
        let line_start = source[..start].rfind(['\r', '\n']).map_or(0, |i| i + 1);
        if banshee_syntax::noqa_payload(token.text()).is_some() {
            // Keep the SQL sharing the directive's line attached to its
            // suppression. Statement expansion below handles multiline SQL.
            let line_end = end
                + source[end..]
                    .find(['\r', '\n'])
                    .unwrap_or(source.len() - end);
            let end = line_end + newline_len(source, line_end);
            protected.push(TextRange::new(
                (line_start as u32).into(),
                (end as u32).into(),
            ));
            continue;
        }
        let Some(command) = token
            .text()
            .strip_prefix("--")
            .and_then(|text| text.trim().strip_prefix("fmt:"))
        else {
            continue;
        };
        let standalone = source[line_start..start]
            .bytes()
            .all(|ch| matches!(ch, b' ' | b'\t' | b'\x0b' | b'\x0c'));
        let marker = TextRange::new(
            if standalone {
                comment.start() - TextSize::from((start - line_start) as u32)
            } else {
                comment.start()
            },
            comment.end() + TextSize::from(newline_len(source, end) as u32),
        );
        match command.trim() {
            "off" if standalone => {
                disabled.get_or_insert(marker.start());
            }
            "on" if standalone => {
                protected.push(TextRange::new(
                    disabled.take().unwrap_or(marker.start()),
                    marker.end(),
                ));
            }
            "skip" if disabled.is_none() => {
                let statements = statements.get_or_insert_with(|| statement_ranges(root));
                let next = statements.partition_point(|range| range.end() <= comment.start());
                let target = match statements.get(next) {
                    Some(range) if standalone || range.contains(comment.start()) => Some(range),
                    _ => next
                        .checked_sub(1)
                        .and_then(|i| statements.get(i))
                        .filter(|range| usize::from(range.end()) >= line_start),
                };
                protected.push(target.map_or(marker, |range| marker.cover(*range)));
            }
            _ => {}
        }
    }
    if let Some(start) = disabled {
        protected.push(TextRange::new(start, root.text_range().end()));
    }
    if protected.is_empty() {
        return protected;
    }

    let statements = statements.get_or_insert_with(|| statement_ranges(root));
    for range in &mut protected {
        let first = statements.partition_point(|stmt| stmt.end() <= range.start());
        let after_last = statements.partition_point(|stmt| stmt.start() < range.end());
        if first < after_last {
            *range = range
                .cover(statements[first])
                .cover(statements[after_last - 1]);
        }
    }
    protected.sort_unstable_by_key(|range| range.start());
    let mut merged: Vec<TextRange> = Vec::with_capacity(protected.len());
    for range in protected {
        if let Some(last) = merged.last_mut().filter(|last| range.start() <= last.end()) {
            *last = last.cover(range);
        } else {
            merged.push(range);
        }
    }
    merged
}

fn newline_len(source: &str, offset: usize) -> usize {
    match &source.as_bytes()[offset..] {
        [b'\r', b'\n', ..] => 2,
        [b'\r' | b'\n', ..] => 1,
        _ => 0,
    }
}

fn statement_ranges(root: &SyntaxNode) -> Vec<TextRange> {
    let mut statements: Vec<TextRange> = Vec::new();
    let mut needs_terminator = false;
    for element in root.children_with_tokens() {
        match element {
            NodeOrToken::Node(node) => {
                let mut tokens = node
                    .descendants_with_tokens()
                    .filter_map(|e| e.into_token())
                    .filter(|token| !token.kind().is_trivia());
                if let Some(first) = tokens.next() {
                    let last = tokens.last().unwrap_or(first);
                    statements.push(first.text_range().cover(last.text_range()));
                    needs_terminator = true;
                }
            }
            NodeOrToken::Token(token)
                if token.kind() == SyntaxKind::SEMICOLON && needs_terminator =>
            {
                if let Some(last) = statements.last_mut() {
                    *last = last.cover(token.text_range());
                }
                needs_terminator = false;
            }
            _ => {}
        }
    }
    statements
}
