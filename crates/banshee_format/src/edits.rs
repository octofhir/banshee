//! Edit-producing formatting.
//!
//! Instead of returning a fully rebuilt string, these helpers diff the source
//! against the formatted output and return the minimal replacement(s). Minimal
//! edits keep the editor cursor stable, compose with other edits (e.g. lint
//! fixes), and enable range formatting.

use banshee_syntax::{Parse, ParseError, SyntaxKind, SyntaxNode};
use text_size::{TextRange, TextSize};

use crate::config::FormatConfig;
use crate::safe::{self, Style};
use crate::suppression::protected_ranges;
use crate::validate::FormatError;

/// A single text replacement against the original source offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub range: TextRange,
    pub new_text: String,
}

/// Formats `source` and returns the minimal edit(s) to apply.
///
/// Returns an empty vector when the source is already formatted.
#[must_use]
pub fn format_edits(source: &str, config: &FormatConfig) -> Vec<TextEdit> {
    format_parsed_edits(source, &banshee_parser::parse(source), config, None)
}

/// Computes the minimal edit(s) turning `source` into `formatted`.
///
/// Engine-agnostic: callers may format with any engine (sqlstyle or
/// pgFormatter) and diff the result here.
#[must_use]
pub fn diff_edits(source: &str, formatted: &str) -> Vec<TextEdit> {
    minimal_diff(source, formatted)
}

/// Formats whole top-level statements intersecting the selection. Surrounding
/// statements and the whitespace outside the selected statements stay intact.
/// Empty selections and inputs that cannot safely be edited yield no edits.
#[must_use]
pub fn format_range(source: &str, config: &FormatConfig, range: TextRange) -> Vec<TextEdit> {
    format_parsed_edits(source, &banshee_parser::parse(source), config, Some(range))
}

/// Returns edits for the parsed document or its selected statements.
/// Supply the same source text that produced `parse`. Offsets are in bytes.
#[must_use]
pub fn format_parsed_edits(
    source: &str,
    parse: &Parse,
    config: &FormatConfig,
    selection: Option<TextRange>,
) -> Vec<TextEdit> {
    format_parsed_with(source, parse, selection, Style::Sql(config), |root| {
        crate::format::print(root, config)
    })
}

pub(crate) fn format_parsed_with(
    source: &str,
    parse: &Parse,
    selection: Option<TextRange>,
    style: Style<'_>,
    print: impl Fn(&SyntaxNode) -> String,
) -> Vec<TextEdit> {
    match selection {
        Some(selection) => format_selected(source, parse, selection, style, print),
        None => safe::format_edits(source, parse, style, print),
    }
}

fn format_selected(
    source: &str,
    parse: &Parse,
    selection: TextRange,
    style: Style<'_>,
    print: impl Fn(&SyntaxNode) -> String,
) -> Vec<TextEdit> {
    if selection.is_empty() || usize::from(selection.end()) > source.len() {
        return Vec::new();
    }
    let root = parse.syntax();
    if safe::copy_from_stdin(&root).is_some() {
        return Vec::new();
    }
    let protected = protected_ranges(source, &root);
    let format = |sql: &str, offset| safe::format_section(sql, offset, style, &print);
    let edits = if parse.errors().is_empty() {
        let mut statements = root
            .children()
            .filter_map(statement_body)
            .filter(|range| overlaps(*range, selection));
        let Some(first) = statements.next() else {
            return Vec::new();
        };
        let span = first.cover(statements.last().unwrap_or(first));
        format_sections(source, span, &protected, true, format).unwrap_or_default()
    } else {
        recoverable_statements(&root, parse.errors(), selection)
            .into_iter()
            .filter_map(|span| format_sections(source, span, &protected, true, format).ok())
            .flatten()
            .collect()
    };
    if edits.is_empty() || safe::preserves_range_edit(source, &apply_edits(source, &edits), style) {
        edits
    } else {
        Vec::new()
    }
}

fn overlaps(left: TextRange, right: TextRange) -> bool {
    left.intersect(right).is_some_and(|range| !range.is_empty())
}

fn recoverable_statements(
    root: &SyntaxNode,
    errors: &[ParseError],
    selection: TextRange,
) -> Vec<TextRange> {
    use SyntaxKind::*;

    // Error recovery can expose an inner semicolon at the root. Accept a
    // separator only when the CST and lexical delimiter nesting agree.
    let mut terminators = Vec::new();
    let mut brackets = Vec::new();
    for token in root
        .descendants_with_tokens()
        .filter_map(|e| e.into_token())
    {
        let kind = token.kind();
        if kind.is_trivia() {
            continue;
        }
        match kind {
            L_PAREN => brackets.push(R_PAREN),
            L_BRACKET => brackets.push(R_BRACKET),
            R_PAREN | R_BRACKET if brackets.last() == Some(&kind) => {
                brackets.pop();
            }
            SEMICOLON if brackets.is_empty() && token.parent().kind() == SOURCE_FILE => {
                terminators.push(token.text_range().end());
            }
            _ => {}
        }
    }

    let statements: Vec<_> = root.children().filter_map(statement_body).collect();
    let mut error_starts: Vec<_> = errors.iter().map(|error| error.range.start()).collect();
    error_starts.sort_unstable();
    let eof = root.text_range().end();
    let mut start = root.text_range().start();
    let mut recovered = Vec::new();
    for end in terminators
        .into_iter()
        .chain(brackets.is_empty().then_some(eof))
    {
        let section = TextRange::new(start, end);
        start = end;
        if !overlaps(section, selection) {
            continue;
        }
        let first = statements.partition_point(|stmt| stmt.end() <= section.start());
        let after_last = statements.partition_point(|stmt| stmt.start() < end);
        let [statement] = &statements[first..after_last] else {
            continue;
        };
        let error = error_starts.partition_point(|&offset| offset < section.start());
        if error_starts
            .get(error)
            .is_some_and(|&offset| offset < end || offset == eof && end == eof)
            || !overlaps(*statement, selection)
        {
            continue;
        }
        recovered.push(*statement);
    }
    recovered
}

/// Format the gaps around suppressed ranges while keeping whitespace at each
/// splice. A selection also keeps whitespace at its outer edges.
pub(crate) fn format_sections(
    source: &str,
    span: TextRange,
    protected: &[TextRange],
    selection: bool,
    format: impl Fn(&str, TextSize) -> Result<String, FormatError>,
) -> Result<Vec<TextEdit>, FormatError> {
    let mut edits = Vec::new();
    let mut start = span.start();
    let mut keep_start = selection;
    let skipped = protected
        .iter()
        .filter_map(|range| range.intersect(span))
        .filter(|range| !range.is_empty())
        .chain(std::iter::once(TextRange::empty(span.end())));
    for range in skipped {
        let gap = TextRange::new(start, range.start());
        let keep_end = selection || range.start() < span.end();
        let bounds = safe::layout_bounds(&source[gap], keep_start, keep_end);
        if !bounds.is_empty() {
            let offset = start + TextSize::from(bounds.start as u32);
            let original = &source[gap][bounds];
            let formatted = format(original, offset)?;
            let bounds = safe::layout_bounds(&formatted, keep_start, keep_end);
            edits.extend(
                diff_edits(original, &formatted[bounds])
                    .into_iter()
                    .map(|mut edit| {
                        edit.range += offset;
                        edit
                    }),
            );
        }
        start = range.end();
        keep_start = true;
    }
    Ok(edits)
}

/// Applies edits ordered by byte offset, with nonoverlapping UTF-8 ranges.
///
/// # Panics
///
/// Panics if an edit range is out of order, overlaps another edit, or falls
/// outside the source's UTF-8 character boundaries.
#[must_use]
pub fn apply_edits(source: &str, edits: &[TextEdit]) -> String {
    let mut output = String::with_capacity(source.len());
    let mut start = 0.into();
    for edit in edits {
        output.push_str(&source[TextRange::new(start, edit.range.start())]);
        output.push_str(&edit.new_text);
        start = edit.range.end();
    }
    output.push_str(&source[usize::from(start)..]);
    output
}

fn statement_body(node: &SyntaxNode) -> Option<TextRange> {
    let mut tokens = node
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| !matches!(token.kind(), SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE));
    // The parser can attach a file header to the first statement.
    let first = tokens.find(|token| !token.kind().is_trivia())?.text_range();
    Some(first.cover(tokens.last().map_or(first, |token| token.text_range())))
}

/// Computes a single minimal replacement by trimming the common prefix and
/// suffix. Offsets are snapped to UTF-8 char boundaries.
fn minimal_diff(before: &str, after: &str) -> Vec<TextEdit> {
    if before == after {
        return Vec::new();
    }

    let prefix = common_prefix(before, after);
    let suffix = common_suffix(&before[prefix..], &after[prefix..]);

    let before_end = before.len() - suffix;
    let after_end = after.len() - suffix;

    vec![TextEdit {
        range: TextRange::new(
            TextSize::from(prefix as u32),
            TextSize::from(before_end as u32),
        ),
        new_text: after[prefix..after_end].to_string(),
    }]
}

/// Length of the common byte prefix, snapped down to a char boundary in `a`.
fn common_prefix(a: &str, b: &str) -> usize {
    let max = a.len().min(b.len());
    let mut i = 0;
    while i < max && a.as_bytes()[i] == b.as_bytes()[i] {
        i += 1;
    }
    while i > 0 && !a.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Length of the common byte suffix, snapped to a char boundary in `a`.
fn common_suffix(a: &str, b: &str) -> usize {
    let max = a.len().min(b.len());
    let mut i = 0;
    while i < max && a.as_bytes()[a.len() - 1 - i] == b.as_bytes()[b.len() - 1 - i] {
        i += 1;
    }
    while i > 0 && !a.is_char_boundary(a.len() - i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn already_formatted_yields_no_edits() {
        let cfg = FormatConfig::sqlstyle();
        let formatted = crate::format::format("select 1", &cfg);
        assert!(format_edits(&formatted, &cfg).is_empty());
    }

    #[test]
    fn edit_applies_to_produce_formatted_output() {
        let cfg = FormatConfig::sqlstyle();
        let src = "select id,name from users";
        let edits = format_edits(src, &cfg);
        assert_eq!(edits.len(), 1);

        // Applying the edit reproduces the full formatted string.
        let edit = &edits[0];
        let start = u32::from(edit.range.start()) as usize;
        let end = u32::from(edit.range.end()) as usize;
        let mut out = src.to_string();
        out.replace_range(start..end, &edit.new_text);
        assert_eq!(out, crate::format::format(src, &cfg));
    }

    #[test]
    fn range_outside_change_yields_nothing() {
        let cfg = FormatConfig::sqlstyle();
        let src = "select id,name from users";
        let full = format_edits(src, &cfg);
        let change = full[0].range;
        // A zero-length range far past the edit should be filtered out.
        let far = TextRange::empty(TextSize::from(u32::from(change.end()) + 5));
        assert!(format_range(src, &cfg, far).is_empty());
    }

    #[test]
    fn selections_inside_copy_from_stdin_yield_no_edits() {
        let source = "copy t from stdin;\nselect 1;";
        let range = TextRange::new(19.into(), 28.into());
        assert!(format_range(source, &FormatConfig::compact(), range).is_empty());
    }
}
