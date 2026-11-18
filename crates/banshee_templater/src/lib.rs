//! Placeholder templating.
//!
//! App SQL often carries placeholders that are not valid PostgreSQL on their
//! own — `:name`, `?`, `%(name)s`. This crate rewrites them into a harmless
//! literal so the rest of the statement parses and lints, while keeping a slice
//! map that translates rendered offsets back to the original source. Diagnostic
//! and fix ranges can then be reported against the user's file.
//!
//! PostgreSQL positional parameters such as `$1` are left untouched. Quoted
//! identifiers, strings and comments are scanned by the PostgreSQL lexer.
//!
//! ```
//! use banshee_templater::{PlaceholderStyle, render};
//! let t = render("SELECT id FROM t WHERE name = :name", PlaceholderStyle::Colon);
//! assert_eq!(t.text(), "SELECT id FROM t WHERE name = 1");
//! ```

use banshee_syntax::SyntaxKind;
use serde::{Deserialize, Serialize};
use text_size::{TextRange, TextSize};

/// The placeholder syntax used by the source. Selected via configuration; when
/// unset, no templating runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlaceholderStyle {
    /// `:name` or `:1` (psql / many ORMs). `::` casts are left alone.
    Colon,
    /// `?` positional (JDBC/ODBC), accepted in value positions.
    QuestionMark,
    /// `%s` and `%(name)s` (Python DB-API).
    Percent,
}

/// The text substituted for each placeholder. A bare integer literal parses
/// wherever a value is expected (`WHERE`, `VALUES`, `LIMIT`, …).
const REPLACEMENT: &str = "1";

/// One contiguous span of the source, mapped to its rendered position.
#[derive(Debug, Clone)]
struct Span {
    src: std::ops::Range<usize>,
    out: std::ops::Range<usize>,
    /// True when this span is a substituted placeholder (source and rendered
    /// text differ and offsets inside it cannot be mapped one-to-one).
    placeholder: bool,
}

/// A rendered SQL string plus the map back to the original source.
#[derive(Debug, Clone)]
pub struct Templated {
    text: String,
    spans: Vec<Span>,
}

/// Formatted source with its placeholders restored and a map from the
/// formatter's output coordinates back to that source.
pub struct RestoredFormat {
    pub text: String,
    rendered: Templated,
    inserted_spaces: Vec<TextSize>,
}

impl RestoredFormat {
    /// Maps a formatter output range, including spaces restored at its edges.
    pub fn map_range(&self, range: TextRange) -> TextRange {
        let before = self
            .inserted_spaces
            .partition_point(|&pos| pos < range.start());
        let through = self
            .inserted_spaces
            .partition_point(|&pos| pos <= range.end());
        self.rendered.map_range(TextRange::new(
            range.start() + TextSize::new(before as u32),
            range.end() + TextSize::new(through as u32),
        ))
    }
}

impl Templated {
    /// The rendered, parseable SQL.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the original source ranges of the substituted placeholders.
    pub fn placeholder_ranges(&self) -> impl Iterator<Item = TextRange> + '_ {
        self.placeholders().map(|span| {
            TextRange::new(
                TextSize::new(span.src.start as u32),
                TextSize::new(span.src.end as u32),
            )
        })
    }

    fn placeholders(&self) -> impl Iterator<Item = &Span> {
        self.spans.iter().filter(|span| span.placeholder)
    }

    /// Translates a range in the rendered text back to the original source.
    pub fn map_range(&self, rendered: TextRange) -> TextRange {
        TextRange::new(
            self.source_offset(rendered.start()),
            self.source_offset(rendered.end()),
        )
    }

    fn source_offset(&self, rendered: TextSize) -> TextSize {
        let offset = usize::from(rendered);
        let index = self.spans.partition_point(|span| span.out.end <= offset);
        let source = if let Some(span) = self.spans.get(index) {
            // A replacement occupies one byte, so only its boundaries are
            // representable as integer byte offsets.
            span.src.start + (offset - span.out.start)
        } else {
            self.spans.last().map_or(0, |span| span.src.end)
        };
        TextSize::new(source as u32)
    }

    /// Translates a source cursor to rendered SQL. Cursors inside a placeholder
    /// snap to the start of its replacement.
    pub fn rendered_offset(&self, source: TextSize) -> TextSize {
        let offset = usize::from(source);
        let index = self.spans.partition_point(|span| span.src.end <= offset);
        let rendered = self.spans.get(index).map_or(self.text.len(), |span| {
            if span.placeholder {
                span.out.start
            } else {
                span.out.start + (offset - span.src.start)
            }
        });
        TextSize::new(rendered as u32)
    }

    /// Maps a selection to rendered SQL, including any partly selected placeholder.
    pub fn rendered_range(&self, source: TextRange) -> TextRange {
        let start = self.rendered_offset(source.start());
        if source.is_empty() {
            return TextRange::empty(start);
        }
        let end = usize::from(source.end());
        let index = self.spans.partition_point(|span| span.src.end <= end);
        let end = match self.spans.get(index) {
            Some(span) if span.placeholder && span.src.start < end => {
                TextSize::new(span.out.end as u32)
            }
            _ => self.rendered_offset(source.end()),
        };
        TextRange::new(start, end)
    }

    /// Restores the original spelling of each placeholder after a formatter
    /// has verified that it preserved the rendered SQL's tokens. Returns the
    /// restored SQL and its map, after verifying it renders to the same result.
    pub fn restore_formatting(
        &self,
        source: &str,
        formatted: &str,
        style: PlaceholderStyle,
    ) -> Option<RestoredFormat> {
        let mut placeholders = self.placeholders().peekable();
        let mut restored = String::with_capacity(source.len());
        let mut inserted_spaces = Vec::new();
        let mut start = 0;
        for (before, after) in sql_token_ranges(self.text()).zip(sql_token_ranges(formatted)) {
            let Some(span) = placeholders.peek() else {
                break;
            };
            if before.end <= span.out.start {
                continue;
            }
            if before != span.out || &formatted[after.clone()] != REPLACEMENT {
                return None;
            }
            restored.push_str(&formatted[start..after.start]);
            // An array slice may lose the space before its upper bound.
            // Keep the delimiter and a restored :parameter from forming ::.
            if style == PlaceholderStyle::Colon && restored.ends_with(':') {
                restored.push(' ');
                inserted_spaces.push(TextSize::new(after.start as u32));
            }
            restored.push_str(source.get(span.src.clone())?);
            start = after.end;
            placeholders.next();
        }
        if placeholders.next().is_some() {
            return None;
        }
        restored.push_str(&formatted[start..]);
        let rendered = render(&restored, style);
        if !same_after_spacing(formatted, rendered.text(), &inserted_spaces)
            || !self
                .placeholders()
                .map(|span| source.get(span.src.clone()))
                .eq(rendered
                    .placeholders()
                    .map(|span| restored.get(span.src.clone())))
        {
            return None;
        }
        Some(RestoredFormat {
            text: restored,
            rendered,
            inserted_spaces,
        })
    }

    /// Whether a rendered range overlaps a substituted placeholder. Fixes that
    /// touch a placeholder cannot be applied to the source safely.
    pub fn touches_placeholder(&self, rendered: TextRange) -> bool {
        let s: usize = rendered.start().into();
        let e: usize = rendered.end().into();
        let index = self.spans.partition_point(|span| span.out.end <= s);
        self.spans[index..]
            .iter()
            .take_while(|span| span.out.start < e.max(s + 1))
            .any(|span| span.placeholder)
    }

    /// Maps an edit to the source when it leaves all placeholders intact.
    pub fn map_edit_range(&self, rendered: TextRange) -> Option<TextRange> {
        if rendered.is_empty() {
            let offset = usize::from(rendered.start());
            let index = self.spans.partition_point(|span| span.out.end < offset);
            if self.spans[index..]
                .iter()
                .take_while(|span| span.out.start <= offset)
                .any(|span| span.placeholder)
            {
                return None;
            }
        }
        (!self.touches_placeholder(rendered)).then(|| self.map_range(rendered))
    }
}

fn same_after_spacing(mut formatted: &str, mut rendered: &str, spaces: &[TextSize]) -> bool {
    let mut offset = 0;
    for &space in spaces {
        let position = usize::from(space);
        let (prefix, rest) = formatted.split_at(position - offset);
        let Some(suffix) = rendered
            .strip_prefix(prefix)
            .and_then(|text| text.strip_prefix(' '))
        else {
            return false;
        };
        formatted = rest;
        rendered = suffix;
        offset = position;
    }
    formatted == rendered
}

fn sql_token_ranges(source: &str) -> impl Iterator<Item = std::ops::Range<usize>> {
    let mut offset = 0;
    banshee_lexer::tokenize(source)
        .into_iter()
        .filter_map(move |token| {
            let start = offset;
            offset += usize::from(token.len);
            (!token.kind.is_trivia()).then_some(start..offset)
        })
}

/// Renders `source`, substituting placeholders of `style`.
pub fn render(source: &str, style: PlaceholderStyle) -> Templated {
    let ranges = match style {
        PlaceholderStyle::QuestionMark if source.contains('?') => {
            let parse = banshee_parser::parse_with_question_mark_parameters(source);
            parse
                .syntax()
                .descendants()
                .filter(|node| node.kind() == SyntaxKind::PARAM_REF)
                .filter_map(|node| {
                    node.children_with_tokens()
                        .filter_map(|element| element.into_token())
                        .find(|token| token.kind() == SyntaxKind::QUESTION)
                        .map(|token| token.text_range().into())
                })
                .collect()
        }
        PlaceholderStyle::QuestionMark => Vec::new(),
        _ => named_placeholder_ranges(source, style),
    };
    let mut spans = Vec::new();
    let mut text = String::with_capacity(source.len());
    let mut start = 0;
    for range in ranges {
        append_literal(source, start..range.start, &mut text, &mut spans);
        let out_start = text.len();
        text.push_str(REPLACEMENT);
        start = range.end;
        spans.push(Span {
            src: range,
            out: out_start..text.len(),
            placeholder: true,
        });
    }
    append_literal(source, start..source.len(), &mut text, &mut spans);
    Templated { text, spans }
}

fn append_literal(
    source: &str,
    range: std::ops::Range<usize>,
    text: &mut String,
    spans: &mut Vec<Span>,
) {
    if !range.is_empty() {
        let start = text.len();
        text.push_str(&source[range.clone()]);
        spans.push(Span {
            src: range,
            out: start..text.len(),
            placeholder: false,
        });
    }
}

fn named_placeholder_ranges(source: &str, style: PlaceholderStyle) -> Vec<std::ops::Range<usize>> {
    let mut ranges = Vec::new();
    let mut i = 0;
    let mut protected = protected_ranges(source, style).peekable();
    while i < source.len() {
        if let Some(range) = protected.next_if(|range| range.start == i) {
            i = range.end;
            continue;
        }
        if let Some(len) = match_placeholder(source, i, style)
            && protected.peek().is_none_or(|range| i + len <= range.start)
        {
            ranges.push(i..i + len);
            i += len;
        } else {
            i += utf8_len(source.as_bytes()[i]);
        }
    }
    ranges
}

fn protected_ranges(
    source: &str,
    style: PlaceholderStyle,
) -> impl Iterator<Item = std::ops::Range<usize>> {
    use SyntaxKind::*;
    let mut offset = 0;
    let mut brackets = 0usize;
    let mut previous = None;
    banshee_lexer::tokenize(source)
        .into_iter()
        .filter_map(move |token| {
            let start = offset;
            offset += usize::from(token.len);
            // After a lower bound, the colon inside a subscript separates the
            // bounds. Parameter markers can still begin either bound.
            let slice_separator = style == PlaceholderStyle::Colon
                && brackets > 0
                && token.kind == COLON
                && matches!(
                    previous,
                    Some(
                        IDENT
                            | QUOTED_IDENT
                            | PARAM
                            | INTEGER
                            | FLOAT
                            | STRING
                            | DOLLAR_STRING
                            | BIT_STRING
                            | HEX_STRING
                            | TRUE_KW
                            | FALSE_KW
                            | NULL_KW
                            | END_KW
                            | R_PAREN
                            | R_BRACKET
                    )
                );
            match token.kind {
                L_BRACKET => brackets += 1,
                R_BRACKET => brackets = brackets.saturating_sub(1),
                _ => {}
            }
            if !token.kind.is_trivia() {
                previous = Some(token.kind);
            }
            (slice_separator
                || matches!(
                    token.kind,
                    LINE_COMMENT
                        | BLOCK_COMMENT
                        | STRING
                        | DOLLAR_STRING
                        | BIT_STRING
                        | HEX_STRING
                        | QUOTED_IDENT
                        | QUESTION_PIPE
                        | QUESTION_AMP
                        | AT_QUESTION
                        | ERROR
                ))
            .then_some(start..offset)
        })
}

/// If a placeholder of `style` starts at `i`, returns its byte length.
fn match_placeholder(source: &str, i: usize, style: PlaceholderStyle) -> Option<usize> {
    let b = source.as_bytes();
    match style {
        PlaceholderStyle::QuestionMark => None,
        PlaceholderStyle::Colon => {
            if b[i] != b':' {
                return None;
            }
            // Leave `::` casts alone (either side being `:`).
            if b.get(i + 1) == Some(&b':') || (i > 0 && b[i - 1] == b':') {
                return None;
            }
            let mut j = i + 1;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            (j > i + 1).then_some(j - i)
        }
        PlaceholderStyle::Percent => {
            if b[i] != b'%' {
                return None;
            }
            // `%(name)s`
            if b.get(i + 1) == Some(&b'(') {
                let mut j = i + 2;
                while j < b.len() && b[j] != b')' {
                    j += 1;
                }
                if b.get(j) == Some(&b')') && b.get(j + 1) == Some(&b's') {
                    return Some(j + 2 - i);
                }
                return None;
            }
            // `%s` / `%d`
            matches!(b.get(i + 1), Some(&b's') | Some(&b'd')).then_some(2)
        }
    }
}

/// Byte length of a UTF-8 sequence given its lead byte.
fn utf8_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src_of(rendered: &str, style: PlaceholderStyle, needle: &str) -> String {
        // Helper: render, find `needle` in rendered, map back, slice source.
        let t = render(rendered, style);
        let pos = t.text().find(needle).unwrap();
        let r = TextRange::new(
            TextSize::new(pos as u32),
            TextSize::new((pos + needle.len()) as u32),
        );
        let mapped = t.map_range(r);
        rendered[usize::from(mapped.start())..usize::from(mapped.end())].to_string()
    }

    #[test]
    fn colon_named() {
        let t = render(
            "SELECT * FROM t WHERE a = :id AND b = :name",
            PlaceholderStyle::Colon,
        );
        assert_eq!(t.text(), "SELECT * FROM t WHERE a = 1 AND b = 1");
    }

    #[test]
    fn colon_skips_cast() {
        let t = render("SELECT a::int FROM t", PlaceholderStyle::Colon);
        assert_eq!(t.text(), "SELECT a::int FROM t");
    }

    #[test]
    fn colon_keeps_array_slice_separators() {
        let t = render(
            "select a[2:3], a[:lower: :upper], array[:item], a[t.i /* note */ :j]",
            PlaceholderStyle::Colon,
        );
        assert_eq!(
            t.text(),
            "select a[2:3], a[1: 1], array[1], a[t.i /* note */ :j]"
        );
    }

    #[test]
    fn question_mark() {
        let t = render("SELECT * FROM t LIMIT ?", PlaceholderStyle::QuestionMark);
        assert_eq!(t.text(), "SELECT * FROM t LIMIT 1");
    }

    #[test]
    fn question_mark_parameters_and_operators_can_share_an_expression() {
        let source = "select data ? ?, ?::int, '?', $$?$$, data ?| array['x']; -- ?";
        let rendered = render(source, PlaceholderStyle::QuestionMark);
        assert_eq!(
            rendered.text(),
            "select data ? 1, 1::int, '?', $$?$$, data ?| array['x']; -- ?"
        );
        let ranges: Vec<_> = rendered
            .placeholder_ranges()
            .map(|range| &source[range])
            .collect();
        assert_eq!(ranges, ["?", "?"]);
    }

    #[test]
    fn question_mark_parameters_can_follow_an_operator_without_spaces() {
        let source = "SELECT id FROM t WHERE id=? AND data ? ?";
        let rendered = render(source, PlaceholderStyle::QuestionMark);
        assert_eq!(rendered.text(), "SELECT id FROM t WHERE id=1 AND data ? 1");
    }

    #[test]
    fn percent_styles() {
        let t = render("VALUES (%s, %(name)s)", PlaceholderStyle::Percent);
        assert_eq!(t.text(), "VALUES (1, 1)");
    }

    #[test]
    fn ignores_placeholder_in_string() {
        let t = render("SELECT ':id' FROM t WHERE a = :id", PlaceholderStyle::Colon);
        assert_eq!(t.text(), "SELECT ':id' FROM t WHERE a = 1");
    }

    #[test]
    fn ignores_placeholder_in_comment() {
        let t = render("SELECT 1 -- :id\nFROM t", PlaceholderStyle::Colon);
        assert_eq!(t.text(), "SELECT 1 -- :id\nFROM t");
    }

    #[test]
    fn ignores_placeholder_in_dollar_quote() {
        let t = render("SELECT $$ :id $$ FROM t", PlaceholderStyle::Colon);
        assert_eq!(t.text(), "SELECT $$ :id $$ FROM t");
    }

    #[test]
    fn uses_sql_lexical_boundaries_for_quotes_and_comments() {
        let source =
            "SELECT \"x:id\", E'a\\':id', U&\"u:id\", /* :a /* :b */ */ :value -- :id\r, :next";
        let expected = "SELECT \"x:id\", E'a\\':id', U&\"u:id\", /* :a /* :b */ */ 1 -- :id\r, 1";
        assert_eq!(render(source, PlaceholderStyle::Colon).text(), expected);
        for unfinished in ["SELECT \"x:id", "SELECT E'\\':id", "SELECT $tag$:id"] {
            assert_eq!(
                render(unfinished, PlaceholderStyle::Colon).text(),
                unfinished
            );
        }
    }

    #[test]
    fn native_dollar_params_untouched() {
        let t = render("SELECT * FROM t WHERE a = $1", PlaceholderStyle::Colon);
        assert_eq!(t.text(), "SELECT * FROM t WHERE a = $1");
    }

    #[test]
    fn maps_literal_range_back() {
        // A token after a placeholder must map back to its true source offset.
        assert_eq!(
            src_of("WHERE a = :id AND foo = 2", PlaceholderStyle::Colon, "foo"),
            "foo"
        );
    }

    #[test]
    fn maps_placeholder_range_to_source() {
        let t = render("WHERE a = :id", PlaceholderStyle::Colon);
        let pos = t.text().find('1').unwrap();
        let r = TextRange::new(TextSize::new(pos as u32), TextSize::new(pos as u32 + 1));
        let mapped = t.map_range(r);
        // Maps back onto the `:id` placeholder span.
        assert_eq!(
            &"WHERE a = :id"[mapped.start().into()..mapped.end().into()],
            ":id"
        );
        assert!(t.touches_placeholder(r));
    }

    #[test]
    fn maps_boundaries_and_cursors_between_placeholders() {
        let t = render("é :first,:second", PlaceholderStyle::Colon);
        let comma = TextRange::new(4.into(), 5.into());
        assert_eq!(t.map_range(comma), TextRange::new(9.into(), 10.into()));
        assert_eq!(t.map_edit_range(comma), Some(t.map_range(comma)));
        assert_eq!(
            t.map_range(TextRange::empty(5.into())),
            TextRange::empty(10.into())
        );
        assert_eq!(t.map_edit_range(TextRange::empty(4.into())), None);
        assert_eq!(t.map_edit_range(TextRange::empty(5.into())), None);
        for (source, rendered) in [(2, 2), (4, 3), (9, 4), (12, 5), (17, 6)] {
            assert_eq!(t.rendered_offset(source.into()), TextSize::from(rendered));
        }
    }
}
