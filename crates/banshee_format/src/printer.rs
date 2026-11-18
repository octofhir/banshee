//! Core printer for SQL formatting output.
//!
//! The printer manages the output buffer and tracks position for proper formatting.

use crate::config::FormatConfig;

/// A printer that manages formatted output.
#[derive(Debug)]
pub struct Printer {
    /// The configuration for formatting
    config: FormatConfig,
    /// The output buffer
    buffer: String,
    /// Current line number (0-indexed)
    line: usize,
    /// Current column position (0-indexed)
    column: usize,
    /// Current indentation level
    indent_level: usize,
    /// Whether we're at the start of a line (after newline, before content)
    at_line_start: bool,
    /// River width for current statement (for alignment)
    current_river_width: usize,
}

impl Printer {
    /// Creates a new printer with the given configuration.
    pub fn new(config: FormatConfig) -> Self {
        let river_width = config.river_width;
        Self {
            config,
            buffer: String::new(),
            line: 0,
            column: 0,
            indent_level: 0,
            at_line_start: true,
            current_river_width: river_width,
        }
    }

    /// Creates a new printer with sqlstyle.guide defaults.
    pub fn sqlstyle() -> Self {
        Self::new(FormatConfig::sqlstyle())
    }

    /// Creates a new printer with compact defaults.
    pub fn compact() -> Self {
        Self::new(FormatConfig::compact())
    }

    /// Returns the configuration.
    pub fn config(&self) -> &FormatConfig {
        &self.config
    }

    /// Returns the current output.
    pub fn output(&self) -> &str {
        &self.buffer
    }

    /// Consumes the printer and returns the output.
    pub fn finish(self) -> String {
        self.buffer
    }

    /// Returns the current line number.
    pub fn line(&self) -> usize {
        self.line
    }

    /// Returns the current column position.
    pub fn column(&self) -> usize {
        self.column
    }

    /// Returns the current indentation level.
    pub fn indent_level(&self) -> usize {
        self.indent_level
    }

    /// Increases the indentation level.
    pub fn indent(&mut self) {
        self.indent_level += 1;
    }

    /// Decreases the indentation level.
    pub fn dedent(&mut self) {
        self.indent_level = self.indent_level.saturating_sub(1);
    }

    /// Sets the indentation level.
    pub fn set_indent_level(&mut self, level: usize) {
        self.indent_level = level;
    }

    /// Sets the river width for the current statement.
    pub fn set_river_width(&mut self, width: usize) {
        self.current_river_width = width;
    }

    /// Returns the current river width.
    pub fn river_width(&self) -> usize {
        self.current_river_width
    }

    /// Writes text to the output buffer.
    pub fn write(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        // Handle line start indentation
        if self.at_line_start && !text.starts_with('\n') {
            self.write_indent();
            self.at_line_start = false;
        }

        for ch in text.chars() {
            if ch == '\n' {
                self.buffer.push(ch);
                self.line += 1;
                self.column = 0;
                self.at_line_start = true;
            } else {
                if self.at_line_start {
                    self.write_indent();
                    self.at_line_start = false;
                }
                self.buffer.push(ch);
                self.column += 1;
            }
        }
    }

    /// Writes text without any transformation.
    pub fn write_raw(&mut self, text: &str) {
        for ch in text.chars() {
            if ch == '\n' {
                self.buffer.push(ch);
                self.line += 1;
                self.column = 0;
                self.at_line_start = true;
            } else {
                self.buffer.push(ch);
                self.column += 1;
                self.at_line_start = false;
            }
        }
    }

    /// Indents the start of a token and copies its contents byte for byte.
    pub(crate) fn write_verbatim(&mut self, text: &str) {
        if self.at_line_start {
            self.write_indent();
        }
        self.write_raw(text);
    }

    /// Writes a newline.
    pub fn newline(&mut self) {
        self.buffer.push('\n');
        self.line += 1;
        self.column = 0;
        self.at_line_start = true;
    }

    /// Writes a space.
    pub fn space(&mut self) {
        if self.at_line_start {
            self.write_indent();
            self.at_line_start = false;
        }
        self.buffer.push(' ');
        self.column += 1;
    }

    /// Writes multiple spaces.
    pub fn spaces(&mut self, count: usize) {
        for _ in 0..count {
            self.space();
        }
    }

    /// Writes indentation based on current level.
    fn write_indent(&mut self) {
        let indent = self.config.indent_str().repeat(self.indent_level);
        self.buffer.push_str(&indent);
        self.column += indent.len();
    }

    /// Writes a keyword, transforming case according to config.
    pub fn write_keyword(&mut self, keyword: &str) {
        let transformed = match self.config.keyword_case {
            crate::config::KeywordCase::Upper => keyword.to_uppercase(),
            crate::config::KeywordCase::Lower => keyword.to_lowercase(),
            crate::config::KeywordCase::Preserve => keyword.to_string(),
        };
        self.write(&transformed);
    }

    /// Writes an identifier, transforming case according to config.
    pub fn write_identifier(&mut self, ident: &str) {
        let transformed = match self.config.identifier_case {
            crate::config::IdentifierCase::Lower => ident.to_ascii_lowercase(),
            crate::config::IdentifierCase::Preserve => ident.to_string(),
        };
        self.write(&transformed);
    }

    /// Writes a keyword right-aligned to the river width.
    ///
    /// On the first line (line == 0), sets the river width to this keyword's length
    /// and outputs without padding. Subsequent lines align to this width.
    pub fn write_keyword_river(&mut self, keyword: &str) {
        if !self.config.river_alignment {
            self.write_keyword(keyword);
            return;
        }

        let keyword_upper = keyword.to_uppercase();
        let keyword_len = keyword_upper.len();

        // On first line, set river width to this keyword's length and skip padding
        if self.line == 0 && self.at_line_start {
            self.current_river_width = keyword_len;
            self.at_line_start = false;
            self.write_keyword(&keyword_upper);
            return;
        }

        let base_indent = self.config.indent_str().repeat(self.indent_level);
        let base_width = base_indent.len();

        // Calculate padding to right-align keyword to the river width
        let total_width = base_width + self.current_river_width;
        let padding = total_width.saturating_sub(keyword_len);

        // Write spaces before keyword (skip normal indent)
        if self.at_line_start {
            self.at_line_start = false;
            for _ in 0..padding {
                self.buffer.push(' ');
                self.column += 1;
            }
        }

        self.write_keyword(&keyword_upper);
    }

    /// Writes a comma according to the configured style.
    pub fn write_comma(&mut self) {
        match self.config.comma_style {
            crate::config::CommaStyle::Trailing => {
                self.write(",");
            }
            crate::config::CommaStyle::Leading => {
                // For leading commas, we write comma at the start of the next line
                self.write(",");
            }
        }
    }

    /// Writes an operator with optional surrounding spaces.
    pub fn write_operator(&mut self, op: &str) {
        if self.config.spaces_around_operators && !is_unary_context_operator(op) {
            self.space();
            self.write(op);
            self.space();
        } else {
            self.write(op);
        }
    }

    /// Writes an operator that should not have surrounding spaces.
    pub fn write_operator_compact(&mut self, op: &str) {
        self.write(op);
    }

    /// Returns true if we would exceed max_width by adding this text.
    pub fn would_overflow(&self, text: &str) -> bool {
        self.column + text.len() > self.config.max_width
    }

    /// Returns the remaining width on the current line.
    pub fn remaining_width(&self) -> usize {
        self.config.max_width.saturating_sub(self.column)
    }

    /// Ensures a blank line exists (writes newline if not at line start).
    pub fn ensure_blank_line(&mut self) {
        if !self.at_line_start {
            self.newline();
        }
        self.newline();
    }

    /// Ensures we're at the start of a line.
    pub fn ensure_newline(&mut self) {
        if !self.at_line_start {
            self.newline();
        }
    }

    /// Soft break: add newline if we would overflow.
    pub fn soft_break(&mut self, next_text: &str) {
        if self.would_overflow(next_text) {
            self.newline();
        }
    }
}

/// Returns true if the operator is typically used in unary context (no space before).
fn is_unary_context_operator(op: &str) -> bool {
    matches!(op, "-" | "+" | "~" | "!")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_write() {
        let mut printer = Printer::sqlstyle();
        printer.write("SELECT");
        assert_eq!(printer.output(), "SELECT");
        assert_eq!(printer.column(), 6);
    }

    #[test]
    fn test_newline() {
        let mut printer = Printer::sqlstyle();
        printer.write("SELECT");
        printer.newline();
        printer.write("FROM");
        assert_eq!(printer.output(), "SELECT\nFROM");
        assert_eq!(printer.line(), 1);
    }

    #[test]
    fn test_indentation() {
        let mut printer = Printer::sqlstyle();
        printer.indent();
        printer.write("test");
        assert_eq!(printer.output(), "    test");
    }

    #[test]
    fn test_keyword_case() {
        let mut printer = Printer::new(FormatConfig::sqlstyle());
        printer.write_keyword("select");
        assert_eq!(printer.output(), "SELECT");

        let mut printer = Printer::new(
            FormatConfig::sqlstyle().with_keyword_case(crate::config::KeywordCase::Lower),
        );
        printer.write_keyword("SELECT");
        assert_eq!(printer.output(), "select");
    }

    #[test]
    fn test_operator_spacing() {
        let mut printer = Printer::sqlstyle();
        printer.write("a");
        printer.write_operator("=");
        printer.write("1");
        assert_eq!(printer.output(), "a = 1");
    }

    #[test]
    fn test_river_alignment() {
        let mut printer = Printer::sqlstyle();

        // First line sets river width to keyword length and outputs without padding
        printer.write_keyword_river("SELECT");
        assert_eq!(printer.output(), "SELECT");
        assert_eq!(printer.river_width(), 6); // River width set to SELECT length

        // Subsequent lines should be right-aligned to river width
        printer.newline();
        printer.write_keyword_river("FROM");
        // FROM (4 chars) + 2 spaces = 6 (matches SELECT length)
        assert!(printer.output().ends_with("  FROM"));
    }
}
