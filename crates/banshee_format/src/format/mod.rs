//! SQL formatting implementation.
//!
//! This module contains the core formatting logic for SQL statements.

#![allow(clippy::needless_borrow)]

pub mod expressions;
pub mod statements;

use banshee_syntax::{Parse, SyntaxKind, SyntaxNode, SyntaxToken};

use crate::config::FormatConfig;
use crate::printer::Printer;
use crate::rules;
use crate::safe::{self, Style};
use crate::validate::FormatError;

/// Formats SQL source code.
#[must_use]
pub fn format(source: &str, config: &FormatConfig) -> String {
    try_format(source, config).unwrap_or_else(|_| source.to_owned())
}

/// Formats SQL or returns an error when it cannot be safely formatted.
pub fn try_format(source: &str, config: &FormatConfig) -> Result<String, FormatError> {
    let parse = banshee_parser::parse(source);
    safe::format(source, &parse, Style::Sql(config), |root| {
        print(root, config)
    })
}

/// Formats a parsed SQL tree.
pub fn format_parse(parse: &Parse, config: &FormatConfig) -> String {
    let source = safe::source_text(&parse.syntax());
    safe::format(&source, parse, Style::Sql(config), |root| {
        print(root, config)
    })
    .unwrap_or(source)
}

fn print(root: &SyntaxNode, config: &FormatConfig) -> String {
    let mut printer = Printer::new(config.clone());
    printer.set_river_width(rules::standard_river_width());
    format_node(root, &mut printer);
    printer.finish()
}

/// Formats a syntax node.
fn format_node(node: &SyntaxNode, printer: &mut Printer) {
    match node.kind() {
        SyntaxKind::SOURCE_FILE => {
            format_source_file(node, printer);
        }
        SyntaxKind::SELECT_STMT => {
            statements::format_select(node, printer);
        }
        SyntaxKind::INSERT_STMT => {
            statements::format_insert(node, printer);
        }
        SyntaxKind::UPDATE_STMT => {
            statements::format_update(node, printer);
        }
        SyntaxKind::DELETE_STMT => {
            statements::format_delete(node, printer);
        }
        SyntaxKind::CREATE_TABLE_STMT => {
            statements::format_create_table(node, printer);
        }
        SyntaxKind::CREATE_INDEX_STMT => {
            statements::format_create_index(node, printer);
        }
        SyntaxKind::ALTER_STMT => {
            statements::format_alter(node, printer);
        }
        SyntaxKind::ERROR => {
            // Preserve original text for error nodes
            format_error_node(node, printer);
        }
        _ => {
            // For other nodes, recursively format children
            format_children(node, printer);
        }
    }
}

/// Formats the source file (root node).
fn format_source_file(node: &SyntaxNode, printer: &mut Printer) {
    let mut first_stmt = true;

    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                if !first_stmt {
                    printer.ensure_blank_line();
                }
                format_node(child, printer);
                first_stmt = false;
            }
            cstree::util::NodeOrToken::Token(token) => format_token(token, printer),
        }
    }

    // Ensure file ends with newline
    printer.ensure_newline();
}

/// Formats children of a node.
pub fn format_children(node: &SyntaxNode, printer: &mut Printer) {
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_node(&child, printer);
            }
            cstree::util::NodeOrToken::Token(token) => {
                format_token(&token, printer);
            }
        }
    }
}

/// Formats a token.
pub fn format_token(token: &SyntaxToken, printer: &mut Printer) {
    let kind = token.kind();
    let text = token.text();

    match kind {
        // Trivia - handle specially
        SyntaxKind::WHITESPACE => {
            // Don't output raw whitespace - we control spacing
        }
        SyntaxKind::NEWLINE => {
            // Don't output raw newlines - we control line breaks
        }
        SyntaxKind::LINE_COMMENT => {
            if printer.column() > 0 {
                printer.space();
            }
            printer.write_verbatim(text);
            printer.newline();
        }
        SyntaxKind::BLOCK_COMMENT => printer.write_verbatim(text),

        // Keywords - transform case
        k if k.is_keyword() => {
            if rules::keywords::needs_space_before(k) {
                printer.space();
            }
            printer.write_keyword(text);
            if rules::keywords::needs_space_after(k) {
                printer.space();
            }
        }

        // Identifiers
        SyntaxKind::IDENT => {
            printer.write_identifier(text);
        }
        SyntaxKind::QUOTED_IDENT => {
            // Preserve quoted identifiers exactly
            printer.write_verbatim(text);
        }

        // Literals - preserve exactly
        SyntaxKind::INTEGER
        | SyntaxKind::FLOAT
        | SyntaxKind::STRING
        | SyntaxKind::DOLLAR_STRING
        | SyntaxKind::BIT_STRING
        | SyntaxKind::HEX_STRING
        | SyntaxKind::PARAM => {
            printer.write_verbatim(text);
        }

        // Punctuation
        SyntaxKind::SEMICOLON => {
            printer.write(";");
        }
        SyntaxKind::COMMA => {
            printer.write_comma();
        }
        SyntaxKind::DOT => {
            printer.write(".");
        }
        SyntaxKind::COLON => {
            printer.write(":");
        }
        SyntaxKind::DOUBLE_COLON => {
            printer.write("::");
        }
        SyntaxKind::L_PAREN => {
            printer.write("(");
            if printer.config().parentheses_spacing {
                printer.space();
            }
        }
        SyntaxKind::R_PAREN => {
            if printer.config().parentheses_spacing {
                printer.space();
            }
            printer.write(")");
        }
        SyntaxKind::L_BRACKET => {
            printer.write("[");
        }
        SyntaxKind::R_BRACKET => {
            printer.write("]");
        }
        SyntaxKind::L_BRACE => {
            printer.write("{");
        }
        SyntaxKind::R_BRACE => {
            printer.write("}");
        }

        // Operators
        SyntaxKind::EQ
        | SyntaxKind::NE
        | SyntaxKind::LT
        | SyntaxKind::LE
        | SyntaxKind::GT
        | SyntaxKind::GE
        | SyntaxKind::PLUS
        | SyntaxKind::MINUS
        | SyntaxKind::STAR
        | SyntaxKind::SLASH
        | SyntaxKind::PERCENT
        | SyntaxKind::CARET
        | SyntaxKind::PIPE_PIPE => {
            printer.write_operator(text);
        }

        // JSONB operators - no space around arrows
        SyntaxKind::ARROW | SyntaxKind::ARROW_TEXT => {
            printer.write_operator_compact(text);
        }
        SyntaxKind::HASH_ARROW | SyntaxKind::HASH_ARROW_TEXT => {
            printer.write_operator_compact(text);
        }
        SyntaxKind::AT_GT
        | SyntaxKind::LT_AT
        | SyntaxKind::QUESTION
        | SyntaxKind::QUESTION_PIPE
        | SyntaxKind::QUESTION_AMP
        | SyntaxKind::HASH_MINUS
        | SyntaxKind::AT_QUESTION
        | SyntaxKind::AT_AT => {
            printer.write_operator(text);
        }

        // Default - preserve text
        _ => {
            printer.write(text);
        }
    }
}

/// Formats an error node by preserving its original text.
fn format_error_node(node: &SyntaxNode, printer: &mut Printer) {
    // Collect all text from the error node
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_error_node(&child, printer);
            }
            cstree::util::NodeOrToken::Token(token) => {
                printer.write_raw(token.text());
            }
        }
    }
}

/// Finds a child node of a specific kind.
pub fn find_child(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxNode> {
    node.children().find(|c| c.kind() == kind).cloned()
}

/// Finds a child token of a specific kind.
pub fn find_token(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    for element in node.children_with_tokens() {
        if let cstree::util::NodeOrToken::Token(token) = element
            && token.kind() == kind
        {
            return Some(token.clone());
        }
    }
    None
}

/// Returns all child nodes of a specific kind.
pub fn children_of_kind(node: &SyntaxNode, kind: SyntaxKind) -> Vec<SyntaxNode> {
    node.children()
        .filter(|c| c.kind() == kind)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_simple_select() {
        let sql = "select id from users";
        let formatted = format(sql, &FormatConfig::compact());
        assert!(formatted.contains("SELECT"));
        assert!(formatted.contains("FROM"));
    }

    #[test]
    fn test_format_preserves_error_nodes() {
        // If we have invalid SQL, we should preserve it
        let sql = "SELECT ??? FROM";
        let formatted = format(sql, &FormatConfig::compact());
        // The error content should be preserved somehow
        assert!(formatted.contains("???") || formatted.contains("SELECT"));
    }
}
