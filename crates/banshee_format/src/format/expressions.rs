//! Expression formatting.

#![allow(clippy::needless_borrow)]

use banshee_syntax::{SyntaxKind, SyntaxNode};

use crate::printer::Printer;

use super::{find_child, format_children, format_token};

/// Formats an expression node.
pub fn format_expression(node: &SyntaxNode, printer: &mut Printer) {
    match node.kind() {
        SyntaxKind::BINARY_EXPR => format_binary_expr(node, printer),
        SyntaxKind::UNARY_EXPR => format_unary_expr(node, printer),
        SyntaxKind::PAREN_EXPR => format_paren_expr(node, printer),
        SyntaxKind::FUNC_CALL => format_func_call(node, printer),
        SyntaxKind::CASE_EXPR => format_case_expr(node, printer),
        SyntaxKind::TYPE_NAME => format_type_name(node, printer),
        SyntaxKind::CAST_EXPR => format_cast_expr(node, printer),
        SyntaxKind::BETWEEN_EXPR => format_between_expr(node, printer),
        SyntaxKind::IN_EXPR => format_in_expr(node, printer),
        SyntaxKind::LIKE_EXPR => format_like_expr(node, printer),
        SyntaxKind::IS_EXPR => format_is_expr(node, printer),
        SyntaxKind::EXISTS_EXPR => format_exists_expr(node, printer),
        SyntaxKind::SUBQUERY_EXPR | SyntaxKind::SUBQUERY => format_subquery(node, printer),
        SyntaxKind::ARRAY_EXPR => format_array_expr(node, printer),
        SyntaxKind::JSONB_ACCESS_EXPR => format_jsonb_access(node, printer),
        SyntaxKind::JSONB_PATH_EXPR => format_jsonb_path(node, printer),
        SyntaxKind::COALESCE_EXPR => format_coalesce(node, printer),
        SyntaxKind::NULLIF_EXPR => format_nullif(node, printer),
        SyntaxKind::GREATEST_EXPR | SyntaxKind::LEAST_EXPR => format_greatest_least(node, printer),
        SyntaxKind::COLUMN_REF | SyntaxKind::QUALIFIED_NAME => format_column_ref(node, printer),
        SyntaxKind::LITERAL => format_literal(node, printer),
        SyntaxKind::STAR_EXPR => printer.write("*"),
        SyntaxKind::OVER_CLAUSE => format_over_clause(node, printer),
        _ => format_children(node, printer),
    }
}

/// Formats a binary expression.
fn format_binary_expr(node: &SyntaxNode, printer: &mut Printer) {
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_expression(&child, printer);
            }
            cstree::util::NodeOrToken::Token(token) => {
                format_token(&token, printer);
            }
        }
    }
}

/// Formats a unary expression.
fn format_unary_expr(node: &SyntaxNode, printer: &mut Printer) {
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_expression(&child, printer);
            }
            cstree::util::NodeOrToken::Token(token) => {
                let kind = token.kind();
                // No space after unary minus/plus
                if kind == SyntaxKind::MINUS || kind == SyntaxKind::PLUS {
                    printer.write(token.text());
                } else {
                    format_token(&token, printer);
                }
            }
        }
    }
}

/// Formats a parenthesized expression.
fn format_paren_expr(node: &SyntaxNode, printer: &mut Printer) {
    printer.write("(");
    if printer.config().parentheses_spacing {
        printer.space();
    }

    for child in node.children() {
        format_expression(&child, printer);
    }

    if printer.config().parentheses_spacing {
        printer.space();
    }
    printer.write(")");
}

/// Formats a function call.
fn format_func_call(node: &SyntaxNode, printer: &mut Printer) {
    // Check if there's an ARG_LIST node (if not, we need to handle parentheses manually)
    let has_arg_list = node.children().any(|c| c.kind() == SyntaxKind::ARG_LIST);

    // Track if we're inside parentheses (for COUNT(*) case)
    let mut in_parens = false;

    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                if child.kind() == SyntaxKind::ARG_LIST {
                    printer.write("(");
                    format_arg_list(&child, printer);
                    printer.write(")");
                } else if child.kind() == SyntaxKind::OVER_CLAUSE {
                    printer.space();
                    format_over_clause(&child, printer);
                } else if child.kind() == SyntaxKind::FILTER_CLAUSE {
                    printer.space();
                    format_filter_clause(&child, printer);
                } else if child.kind() == SyntaxKind::QUALIFIED_NAME {
                    format_column_ref(&child, printer);
                } else {
                    format_expression(&child, printer);
                }
            }
            cstree::util::NodeOrToken::Token(token) => {
                let kind = token.kind();
                if kind == SyntaxKind::IDENT {
                    printer.write_identifier(token.text());
                } else if kind == SyntaxKind::L_PAREN {
                    // If no ARG_LIST, we need to write the parentheses ourselves
                    if !has_arg_list {
                        printer.write("(");
                        in_parens = true;
                    }
                } else if kind == SyntaxKind::R_PAREN {
                    if !has_arg_list {
                        printer.write(")");
                        in_parens = false;
                    }
                } else if kind == SyntaxKind::STAR && in_parens {
                    // Special case: * inside function call like COUNT(*)
                    printer.write("*");
                } else if !kind.is_trivia() {
                    format_token(&token, printer);
                }
            }
        }
    }
}

/// Formats a function argument list.
fn format_arg_list(node: &SyntaxNode, printer: &mut Printer) {
    let mut first = true;
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                if !first {
                    printer.write(",");
                    printer.space();
                }
                format_expression(&child, printer);
                first = false;
            }
            cstree::util::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::DISTINCT_KW {
                    printer.write_keyword("DISTINCT");
                    printer.space();
                } else if token.kind() == SyntaxKind::ALL_KW {
                    printer.write_keyword("ALL");
                    printer.space();
                } else if token.kind() != SyntaxKind::COMMA
                    && token.kind() != SyntaxKind::WHITESPACE
                    && token.kind() != SyntaxKind::NEWLINE
                {
                    format_token(&token, printer);
                }
            }
        }
    }
}

/// Formats an OVER clause (window function).
fn format_over_clause(node: &SyntaxNode, printer: &mut Printer) {
    printer.write_keyword("OVER");
    printer.space();
    printer.write("(");

    // PARTITION BY
    if let Some(partition) = find_child(node, SyntaxKind::PARTITION_BY) {
        printer.write_keyword("PARTITION");
        printer.space();
        printer.write_keyword("BY");
        printer.space();
        format_partition_by(&partition, printer);
    }

    // ORDER BY
    if let Some(order) = find_child(node, SyntaxKind::ORDER_BY_CLAUSE) {
        printer.space();
        printer.write_keyword("ORDER");
        printer.space();
        printer.write_keyword("BY");
        printer.space();
        format_order_by_items(&order, printer);
    }

    // Frame clause
    if let Some(frame) = find_child(node, SyntaxKind::FRAME_CLAUSE) {
        printer.space();
        format_frame_clause(&frame, printer);
    }

    printer.write(")");
}

/// Formats PARTITION BY items.
fn format_partition_by(node: &SyntaxNode, printer: &mut Printer) {
    let mut first = true;
    for child in node.children() {
        if !first {
            printer.write(",");
            printer.space();
        }
        format_expression(&child, printer);
        first = false;
    }
}

/// Formats ORDER BY items within an OVER clause.
fn format_order_by_items(node: &SyntaxNode, printer: &mut Printer) {
    let mut first = true;
    for child in node.children() {
        if child.kind() == SyntaxKind::ORDER_BY_ITEM {
            if !first {
                printer.write(",");
                printer.space();
            }
            format_children(&child, printer);
            first = false;
        }
    }
}

/// Formats a frame clause.
fn format_frame_clause(node: &SyntaxNode, printer: &mut Printer) {
    format_children(node, printer);
}

/// Formats a FILTER clause.
fn format_filter_clause(node: &SyntaxNode, printer: &mut Printer) {
    printer.write_keyword("FILTER");
    printer.space();
    printer.write("(");
    printer.write_keyword("WHERE");
    printer.space();

    for child in node.children() {
        format_expression(&child, printer);
    }

    printer.write(")");
}

/// Formats a CASE expression.
fn format_case_expr(node: &SyntaxNode, printer: &mut Printer) {
    printer.write_keyword("CASE");

    // Simple CASE (CASE expr WHEN...)
    let mut saw_when = false;
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                if child.kind() == SyntaxKind::CASE_WHEN {
                    printer.newline();
                    printer.indent();
                    format_case_when(&child, printer);
                    printer.dedent();
                    saw_when = true;
                } else if child.kind() == SyntaxKind::CASE_ELSE {
                    printer.newline();
                    printer.indent();
                    format_case_else(&child, printer);
                    printer.dedent();
                } else if !saw_when {
                    // Expression after CASE (simple case)
                    printer.space();
                    format_expression(&child, printer);
                }
            }
            cstree::util::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::END_KW {
                    printer.newline();
                    printer.write_keyword("END");
                }
            }
        }
    }
}

/// Formats a CASE WHEN clause.
fn format_case_when(node: &SyntaxNode, printer: &mut Printer) {
    printer.write_keyword("WHEN");
    printer.space();

    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_expression(&child, printer);
            }
            cstree::util::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::THEN_KW {
                    printer.space();
                    printer.write_keyword("THEN");
                    printer.space();
                }
            }
        }
    }
}

/// Formats a CASE ELSE clause.
fn format_case_else(node: &SyntaxNode, printer: &mut Printer) {
    printer.write_keyword("ELSE");
    printer.space();

    for child in node.children() {
        format_expression(&child, printer);
    }
}

/// Formats a CAST expression.
fn format_cast_expr(node: &SyntaxNode, printer: &mut Printer) {
    // Typed literal: `type 'string'` (the type name comes first).
    if node
        .children()
        .next()
        .is_some_and(|first| first.kind() == SyntaxKind::TYPE_NAME)
    {
        let mut first = true;
        for element in node.children_with_tokens() {
            match element {
                cstree::util::NodeOrToken::Node(child) => {
                    if !first {
                        printer.space();
                    }
                    format_expression(&child, printer);
                    first = false;
                }
                cstree::util::NodeOrToken::Token(token) if !token.kind().is_trivia() => {
                    printer.space();
                    printer.write_keyword(token.text());
                }
                cstree::util::NodeOrToken::Token(_) => {}
            }
        }
        return;
    }
    // Check for PostgreSQL :: cast syntax
    let mut is_double_colon = false;
    for element in node.children_with_tokens() {
        if let cstree::util::NodeOrToken::Token(token) = element
            && token.kind() == SyntaxKind::DOUBLE_COLON
        {
            is_double_colon = true;
            break;
        }
    }

    if is_double_colon {
        // PostgreSQL :: syntax
        let mut saw_colon = false;
        for element in node.children_with_tokens() {
            match element {
                cstree::util::NodeOrToken::Node(child) => {
                    format_expression(&child, printer);
                }
                cstree::util::NodeOrToken::Token(token) => {
                    if token.kind() == SyntaxKind::DOUBLE_COLON {
                        printer.write("::");
                        saw_colon = true;
                    } else if saw_colon && token.kind().is_keyword() {
                        printer.write_keyword(token.text());
                    } else {
                        format_token(&token, printer);
                    }
                }
            }
        }
    } else {
        // CAST(expr AS type) syntax
        printer.write_keyword("CAST");
        printer.write("(");

        let mut saw_as = false;
        for element in node.children_with_tokens() {
            match element {
                cstree::util::NodeOrToken::Node(child) => {
                    format_expression(&child, printer);
                }
                cstree::util::NodeOrToken::Token(token) => {
                    if token.kind() == SyntaxKind::AS_KW {
                        printer.space();
                        printer.write_keyword("AS");
                        printer.space();
                        saw_as = true;
                    } else if token.kind() != SyntaxKind::CAST_KW
                        && token.kind() != SyntaxKind::L_PAREN
                        && token.kind() != SyntaxKind::R_PAREN
                    {
                        if saw_as && token.kind().is_keyword() {
                            printer.write_keyword(token.text());
                        } else {
                            format_token(&token, printer);
                        }
                    }
                }
            }
        }

        printer.write(")");
    }
}

/// Formats a BETWEEN expression.
fn format_between_expr(node: &SyntaxNode, printer: &mut Printer) {
    let children: Vec<_> = node.children().collect();
    if children.len() >= 3 {
        format_expression(&children[0], printer);
        printer.space();
        printer.write_keyword("BETWEEN");
        printer.space();
        format_expression(&children[1], printer);
        printer.space();
        printer.write_keyword("AND");
        printer.space();
        format_expression(&children[2], printer);
    } else {
        format_children(node, printer);
    }
}

/// Formats an IN expression.
fn format_in_expr(node: &SyntaxNode, printer: &mut Printer) {
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                if child.kind() == SyntaxKind::SUBQUERY {
                    format_subquery(&child, printer);
                } else {
                    format_expression(&child, printer);
                }
            }
            cstree::util::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::IN_KW {
                    printer.space();
                    printer.write_keyword("IN");
                    printer.space();
                } else if token.kind() == SyntaxKind::NOT_KW {
                    printer.space();
                    printer.write_keyword("NOT");
                } else if token.kind() == SyntaxKind::L_PAREN {
                    printer.write("(");
                } else if token.kind() == SyntaxKind::R_PAREN {
                    printer.write(")");
                } else if token.kind() == SyntaxKind::COMMA {
                    printer.write(",");
                    printer.space();
                } else {
                    format_token(&token, printer);
                }
            }
        }
    }
}

/// Formats a LIKE expression.
fn format_like_expr(node: &SyntaxNode, printer: &mut Printer) {
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_expression(&child, printer);
            }
            cstree::util::NodeOrToken::Token(token) => {
                format_token(&token, printer);
            }
        }
    }
}

/// Formats an IS expression.
fn format_is_expr(node: &SyntaxNode, printer: &mut Printer) {
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_expression(&child, printer);
            }
            cstree::util::NodeOrToken::Token(token) => {
                format_token(&token, printer);
            }
        }
    }
}

/// Formats an EXISTS expression.
fn format_exists_expr(node: &SyntaxNode, printer: &mut Printer) {
    printer.write_keyword("EXISTS");
    printer.space();

    for child in node.children() {
        format_subquery(&child, printer);
    }
}

/// Formats a subquery.
fn format_subquery(node: &SyntaxNode, printer: &mut Printer) {
    printer.write("(");
    printer.newline();
    printer.indent();

    for child in node.children() {
        if child.kind() == SyntaxKind::SELECT_STMT {
            super::statements::format_select(&child, printer);
        } else {
            format_expression(&child, printer);
        }
    }

    printer.dedent();
    printer.newline();
    printer.write(")");
}

/// Formats an array expression.
fn format_array_expr(node: &SyntaxNode, printer: &mut Printer) {
    printer.write_keyword("ARRAY");
    printer.write("[");

    let mut first = true;
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                if !first {
                    printer.write(",");
                    printer.space();
                }
                format_expression(&child, printer);
                first = false;
            }
            cstree::util::NodeOrToken::Token(token) => {
                if token.kind() != SyntaxKind::ARRAY_KW
                    && token.kind() != SyntaxKind::L_BRACKET
                    && token.kind() != SyntaxKind::R_BRACKET
                    && token.kind() != SyntaxKind::COMMA
                {
                    format_token(&token, printer);
                }
            }
        }
    }

    printer.write("]");
}

/// Formats a JSONB access expression (-> or ->>).
fn format_jsonb_access(node: &SyntaxNode, printer: &mut Printer) {
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_expression(&child, printer);
            }
            cstree::util::NodeOrToken::Token(token) => {
                // No spaces around JSONB operators
                printer.write(token.text());
            }
        }
    }
}

/// Formats a JSONB path expression (#> or #>>).
fn format_jsonb_path(node: &SyntaxNode, printer: &mut Printer) {
    format_jsonb_access(node, printer);
}

/// Formats a COALESCE expression.
fn format_coalesce(node: &SyntaxNode, printer: &mut Printer) {
    printer.write_keyword("COALESCE");
    printer.write("(");

    let mut first = true;
    for child in node.children() {
        if !first {
            printer.write(",");
            printer.space();
        }
        format_expression(&child, printer);
        first = false;
    }

    printer.write(")");
}

/// Formats a NULLIF expression.
fn format_nullif(node: &SyntaxNode, printer: &mut Printer) {
    printer.write_keyword("NULLIF");
    printer.write("(");

    let children: Vec<_> = node.children().collect();
    if children.len() >= 2 {
        format_expression(&children[0], printer);
        printer.write(",");
        printer.space();
        format_expression(&children[1], printer);
    }

    printer.write(")");
}

/// Formats GREATEST/LEAST expressions.
fn format_greatest_least(node: &SyntaxNode, printer: &mut Printer) {
    let keyword = if node.kind() == SyntaxKind::GREATEST_EXPR {
        "GREATEST"
    } else {
        "LEAST"
    };

    printer.write_keyword(keyword);
    printer.write("(");

    let mut first = true;
    for child in node.children() {
        if !first {
            printer.write(",");
            printer.space();
        }
        format_expression(&child, printer);
        first = false;
    }

    printer.write(")");
}

/// Formats a column reference.
fn format_column_ref(node: &SyntaxNode, printer: &mut Printer) {
    let mut first = true;
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                if !first {
                    printer.write(".");
                }
                format_expression(&child, printer);
                first = false;
            }
            cstree::util::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::DOT {
                    // Handled above
                } else if token.kind() == SyntaxKind::IDENT {
                    if !first {
                        printer.write(".");
                    }
                    printer.write_identifier(token.text());
                    first = false;
                } else if token.kind() == SyntaxKind::QUOTED_IDENT {
                    if !first {
                        printer.write(".");
                    }
                    printer.write(token.text());
                    first = false;
                } else if token.kind() == SyntaxKind::STAR {
                    if !first {
                        printer.write(".");
                    }
                    printer.write("*");
                    first = false;
                } else {
                    format_token(&token, printer);
                }
            }
        }
    }
}

/// Formats a literal value.
fn format_literal(node: &SyntaxNode, printer: &mut Printer) {
    for element in node.children_with_tokens() {
        if let cstree::util::NodeOrToken::Token(token) = element {
            let kind = token.kind();
            // Skip whitespace/newlines - we control spacing
            if kind == SyntaxKind::WHITESPACE || kind == SyntaxKind::NEWLINE {
                continue;
            }
            // Preserve literals exactly
            printer.write(token.text());
        }
    }
}

/// Formats a type name: its words separated by single spaces
/// (`timestamp with time zone`, `double precision`, `numeric(10, 2)`).
fn format_type_name(node: &SyntaxNode, printer: &mut Printer) {
    let mut after_word = false;
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_expression(&child, printer);
                after_word = false;
            }
            cstree::util::NodeOrToken::Token(token) if token.kind().is_trivia() => {}
            cstree::util::NodeOrToken::Token(token) => {
                let kind = token.kind();
                let word = kind.is_keyword()
                    || matches!(kind, SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT);
                if word && after_word {
                    printer.space();
                }
                if kind.is_keyword() {
                    printer.write_keyword(token.text());
                } else {
                    format_token(&token, printer);
                }
                after_word = word || kind == SyntaxKind::R_PAREN;
            }
        }
    }
}
