//! Document outline (`textDocument/documentSymbol`).
//!
//! Walks the CST and emits one symbol per top-level statement, with the tables
//! it touches (and any CTEs) as children.

use banshee_syntax::{SyntaxKind, SyntaxNode};
use lsp_types::{DocumentSymbol, Range, SymbolKind};
use text_size::TextRange;

/// Builds the document outline for a parsed tree.
pub fn document_symbols(
    root: &SyntaxNode,
    to_range: impl Fn(TextRange) -> Range,
) -> Vec<DocumentSymbol> {
    let mut symbols = Vec::new();
    for child in root.children() {
        let Some(label) = statement_label(child.kind()) else {
            continue;
        };

        let target = first_table_name(child);
        let name = match &target {
            Some(t) => format!("{label} {t}"),
            None => label.to_string(),
        };

        symbols.push(make_symbol(
            name,
            SymbolKind::FUNCTION,
            child,
            &to_range,
            statement_children(child, &to_range),
        ));
    }
    symbols
}

fn statement_label(kind: SyntaxKind) -> Option<&'static str> {
    match kind {
        SyntaxKind::SELECT_STMT => Some("SELECT"),
        SyntaxKind::INSERT_STMT => Some("INSERT"),
        SyntaxKind::UPDATE_STMT => Some("UPDATE"),
        SyntaxKind::DELETE_STMT => Some("DELETE"),
        _ => None,
    }
}

/// CTE names and table references within a statement, as child symbols.
fn statement_children(
    stmt: &SyntaxNode,
    to_range: &impl Fn(TextRange) -> Range,
) -> Vec<DocumentSymbol> {
    let mut children = Vec::new();
    for node in stmt.descendants() {
        match node.kind() {
            SyntaxKind::CTE => {
                if let Some(name) = first_ident(node) {
                    children.push(make_symbol(
                        name,
                        SymbolKind::NAMESPACE,
                        node,
                        to_range,
                        vec![],
                    ));
                }
            }
            SyntaxKind::TABLE_REF => {
                if let Some(name) = first_ident(node) {
                    children.push(make_symbol(name, SymbolKind::CLASS, node, to_range, vec![]));
                }
            }
            _ => {}
        }
    }
    children
}

fn make_symbol(
    name: String,
    kind: SymbolKind,
    node: &SyntaxNode,
    to_range: &impl Fn(TextRange) -> Range,
    children: Vec<DocumentSymbol>,
) -> DocumentSymbol {
    let range = to_range(node.text_range());
    #[allow(deprecated)]
    DocumentSymbol {
        name,
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range: range,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

fn first_ident(node: &SyntaxNode) -> Option<String> {
    node.descendants_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| matches!(t.kind(), SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT))
        .map(|t| t.text().to_string())
}

fn first_table_name(stmt: &SyntaxNode) -> Option<String> {
    stmt.descendants()
        .find(|n| n.kind() == SyntaxKind::TABLE_REF)
        .and_then(first_ident)
}
