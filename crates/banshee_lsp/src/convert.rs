//! Conversions between banshee's byte offsets and LSP's UTF-16 positions,
//! including diagnostics, edits and completion items.

use std::borrow::Cow;
use std::ops::Range as ByteRange;

use anyhow::Context;
use banshee_completion::{CompletionItem as SqlCompletionItem, CompletionItemKind as SqlKind};
use banshee_hir::{Diagnostic as HirDiagnostic, Severity, TextEdit as HirEdit};
use lsp_types::{
    CompletionItem, CompletionItemKind, Diagnostic as LspDiagnostic, DiagnosticSeverity,
    Documentation, NumberOrString, Position, Range, TextDocumentContentChangeEvent,
};
use text_size::TextRange;

pub fn completion_item(item: SqlCompletionItem) -> CompletionItem {
    let kind = match item.kind {
        SqlKind::Table | SqlKind::View => CompletionItemKind::STRUCT,
        SqlKind::Column => CompletionItemKind::FIELD,
        SqlKind::Function => CompletionItemKind::FUNCTION,
        SqlKind::Keyword => CompletionItemKind::KEYWORD,
        SqlKind::Alias => CompletionItemKind::VARIABLE,
        SqlKind::Schema => CompletionItemKind::MODULE,
        SqlKind::JsonbPath => CompletionItemKind::PROPERTY,
        SqlKind::Snippet => CompletionItemKind::SNIPPET,
        SqlKind::Type => CompletionItemKind::CLASS,
        SqlKind::Operator => CompletionItemKind::OPERATOR,
        _ => CompletionItemKind::TEXT,
    };
    CompletionItem {
        label: item.label,
        kind: Some(kind),
        detail: item.detail,
        documentation: item.documentation.map(Documentation::String),
        insert_text: item.insert_text,
        sort_text: item.sort_key,
        filter_text: item.filter_text,
        ..Default::default()
    }
}

/// Document text and the byte bounds of each line, excluding its terminator.
#[derive(Clone)]
pub struct LineIndex {
    text: String,
    lines: Vec<Line>,
}

#[derive(Clone)]
struct Line {
    start: usize,
    end: usize,
}

impl LineIndex {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let lines = Self::index_lines(&text);
        Self { text, lines }
    }

    fn index_lines(text: &str) -> Vec<Line> {
        let mut lines = Vec::new();
        let mut start = 0;
        let mut bytes = text.bytes().enumerate().peekable();
        while let Some((end, byte)) = bytes.next() {
            let terminator_len = match byte {
                b'\n' => 1,
                b'\r' => 1 + usize::from(bytes.next_if(|&(_, b)| b == b'\n').is_some()),
                _ => continue,
            };
            lines.push(Line { start, end });
            start = end + terminator_len;
        }
        lines.push(Line {
            start,
            end: text.len(),
        });
        lines
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    fn line_at(&self, offset: usize) -> usize {
        self.lines.partition_point(|line| line.start <= offset) - 1
    }

    /// Maps a byte offset to an LSP position (UTF-16 column).
    pub fn position(&self, offset: u32) -> Position {
        let line = self.line_at(offset as usize);
        let bounds = &self.lines[line];
        let mut end = (offset as usize).min(bounds.end);
        while !self.text.is_char_boundary(end) {
            end -= 1;
        }
        Position::new(line as u32, self.utf16_len(bounds.start, end))
    }

    /// Maps an LSP position back to a byte offset.
    pub fn offset(&self, pos: Position) -> u32 {
        self.byte_offset(pos)
            .map_or(self.text.len(), |(offset, _)| offset) as u32
    }

    fn byte_offset(&self, pos: Position) -> Option<(usize, bool)> {
        let line = self.lines.get(pos.line as usize)?;
        let mut remaining = pos.character;
        for (offset, ch) in self.text[line.start..line.end].char_indices() {
            let width = ch.len_utf16() as u32;
            if remaining < width {
                return Some((line.start + offset, remaining == 0));
            }
            remaining -= width;
        }
        Some((line.end, true))
    }

    fn edit_range(&self, range: Range) -> Option<ByteRange<usize>> {
        let (start, start_on_boundary) = self.byte_offset(range.start)?;
        let (end, end_on_boundary) = self.byte_offset(range.end)?;
        (range.start <= range.end && start_on_boundary && end_on_boundary).then_some(start..end)
    }

    /// Applies each range to the text produced by the preceding changes.
    pub fn apply_changes(
        previous: Option<&Self>,
        changes: Vec<TextDocumentContentChangeEvent>,
    ) -> anyhow::Result<Self> {
        let mut document = previous.map(Cow::Borrowed);
        for change in changes {
            if let Some(range) = change.range {
                let index = document.as_mut().context("a full document is required")?;
                let bytes = index
                    .edit_range(range)
                    .context("invalid UTF-16 change range")?;
                let index = index.to_mut();
                index.text.replace_range(bytes, &change.text);
                index.lines = Self::index_lines(&index.text);
            } else {
                document = Some(Cow::Owned(Self::new(change.text)));
            }
        }
        document
            .map(Cow::into_owned)
            .context("a full document is required")
    }

    /// Maps a byte range to an LSP range.
    pub fn range(&self, range: TextRange) -> Range {
        Range {
            start: self.position(u32::from(range.start())),
            end: self.position(u32::from(range.end())),
        }
    }

    /// Returns the nonempty, single-line parts of a token's byte range.
    pub fn line_segments(&self, range: TextRange) -> impl Iterator<Item = (Position, u32)> + '_ {
        let start = usize::from(range.start());
        let end = usize::from(range.end());
        (self.line_at(start)..=self.line_at(end)).filter_map(move |line| {
            let bounds = &self.lines[line];
            let start = start.max(bounds.start);
            let end = end.min(bounds.end);
            (start < end).then(|| {
                let pos = Position::new(line as u32, self.utf16_len(bounds.start, start));
                (pos, self.utf16_len(start, end))
            })
        })
    }

    fn utf16_len(&self, start: usize, end: usize) -> u32 {
        self.text[start..end].encode_utf16().count() as u32
    }

    pub fn replacement(&self, range: TextRange, new_text: &str) -> lsp_types::TextEdit {
        let mut start = u32::from(range.start());
        let mut end = u32::from(range.end());
        let mut replacement = String::new();
        // LSP cannot address the byte between CR and LF. Include the unchanged
        // part of the terminator when a minimal diff ends inside it.
        if self.inside_crlf(start as usize) {
            start -= 1;
            replacement.push('\r');
        }
        replacement.push_str(new_text);
        if self.inside_crlf(end as usize) {
            end += 1;
            replacement.push('\n');
        }
        lsp_types::TextEdit {
            range: Range::new(self.position(start), self.position(end)),
            new_text: replacement,
        }
    }

    fn inside_crlf(&self, offset: usize) -> bool {
        let bytes = self.text.as_bytes();
        offset > 0 && bytes.get(offset - 1..=offset) == Some(b"\r\n")
    }
}

fn severity(s: Severity) -> DiagnosticSeverity {
    match s {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Info => DiagnosticSeverity::INFORMATION,
        Severity::Hint => DiagnosticSeverity::HINT,
        _ => DiagnosticSeverity::WARNING,
    }
}

/// Converts a banshee diagnostic to its LSP form. Diagnostics without a range are
/// anchored at the document start.
pub fn diagnostic(index: &LineIndex, d: &HirDiagnostic) -> LspDiagnostic {
    let range = d.range.map(|r| index.range(r)).unwrap_or_default();
    let message = match &d.help {
        Some(help) => format!("{}\nhelp: {help}", d.message),
        None => d.message.clone(),
    };
    LspDiagnostic {
        range,
        severity: Some(severity(d.severity)),
        code: d
            .code
            .map(|c| NumberOrString::String(c.as_str().to_string())),
        source: Some("banshee".to_string()),
        message,
        ..Default::default()
    }
}

/// Converts a banshee text edit to an LSP text edit.
pub fn text_edit(index: &LineIndex, edit: &HirEdit) -> lsp_types::TextEdit {
    index.replacement(edit.range, &edit.new_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_roundtrip_ascii() {
        let idx = LineIndex::new("select 1\nfrom t");
        let pos = idx.position(9); // start of "from"
        assert_eq!(
            pos,
            Position {
                line: 1,
                character: 0
            }
        );
        assert_eq!(idx.offset(pos), 9);
    }

    #[test]
    fn handles_multibyte() {
        // 'é' is 2 bytes UTF-8, 1 UTF-16 unit.
        let idx = LineIndex::new("-- é\nx");
        let pos = idx.position(idx.text.find('x').unwrap() as u32);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);
    }

    #[test]
    fn utf16_positions_handle_astral_characters_and_line_endings() {
        let index = LineIndex::new("é😀\r\nx\ry\n");
        for (byte, pos) in [
            (0, Position::new(0, 0)),
            (2, Position::new(0, 1)),
            (6, Position::new(0, 3)),
            (8, Position::new(1, 0)),
            (9, Position::new(1, 1)),
            (10, Position::new(2, 0)),
            (12, Position::new(3, 0)),
        ] {
            assert_eq!(index.position(byte), pos);
            assert_eq!(index.offset(pos), byte);
        }
        assert_eq!(index.position(3), Position::new(0, 1));
        assert_eq!(index.position(7), Position::new(0, 3));
        assert_eq!(index.offset(Position::new(0, 2)), 2);
        assert_eq!(index.offset(Position::new(0, 20)), 6);
    }

    #[test]
    fn replacements_can_normalize_crlf() {
        let index = LineIndex::new("a\r\nb");
        let edit = index.replacement(TextRange::new(1.into(), 2.into()), "");
        assert_eq!(
            edit.range,
            Range::new(Position::new(0, 1), Position::new(1, 0))
        );
        assert_eq!(edit.new_text, "\n");
        let edit = index.replacement(TextRange::new(2.into(), 3.into()), "");
        assert_eq!(
            edit.range,
            Range::new(Position::new(0, 1), Position::new(1, 0))
        );
        assert_eq!(edit.new_text, "\r");
    }
}
