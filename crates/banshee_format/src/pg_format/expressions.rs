//! Expression formatting for the pgFormatter style. Split from the module
//! root for maintainability; see `super` for the printer and dispatch.

use banshee_syntax::{SyntaxKind, SyntaxNode};

use super::*;

/// Formats expression children of a node.
pub(crate) fn format_expression_children(node: &SyntaxNode, printer: &mut PgPrinter) {
    for child in node.children() {
        format_expression(child, printer);
    }
}

/// Formats an expression.
pub(crate) fn format_expression(node: &SyntaxNode, printer: &mut PgPrinter) {
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
        SyntaxKind::JSONB_ACCESS_EXPR | SyntaxKind::JSONB_PATH_EXPR => {
            format_jsonb_access(node, printer)
        }
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
pub(crate) fn format_binary_expr(node: &SyntaxNode, printer: &mut PgPrinter) {
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_expression(child, printer);
            }
            cstree::util::NodeOrToken::Token(token) => {
                format_token(token, printer);
            }
        }
    }
}

/// Formats a unary expression.
pub(crate) fn format_unary_expr(node: &SyntaxNode, printer: &mut PgPrinter) {
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_expression(child, printer);
            }
            cstree::util::NodeOrToken::Token(token) => {
                let kind = token.kind();
                if kind == SyntaxKind::MINUS || kind == SyntaxKind::PLUS {
                    printer.write(token.text());
                } else {
                    format_token(token, printer);
                }
            }
        }
    }
}

/// Formats a parenthesized expression.
pub(crate) fn format_paren_expr(node: &SyntaxNode, printer: &mut PgPrinter) {
    printer.write("(");
    for child in node.children() {
        format_expression(child, printer);
    }
    printer.write(")");
}

/// Formats a function call.
pub(crate) fn format_func_call(node: &SyntaxNode, printer: &mut PgPrinter) {
    // The grammar emits arguments and parentheses flat inside FUNC_CALL (no
    // ARG_LIST node), so track whether we have passed the opening paren to tell
    // the function name from its arguments.
    let mut seen_open = false;
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => match child.kind() {
                SyntaxKind::ARG_LIST => format_arg_list(child, printer),
                SyntaxKind::OVER_CLAUSE => {
                    printer.space();
                    format_over_clause(child, printer);
                }
                SyntaxKind::FILTER_CLAUSE => {
                    printer.space();
                    format_filter_clause(child, printer);
                }
                SyntaxKind::QUALIFIED_NAME => format_column_ref(child, printer),
                _ => format_expression(child, printer),
            },
            cstree::util::NodeOrToken::Token(token) => match token.kind() {
                SyntaxKind::L_PAREN => {
                    printer.write("(");
                    seen_open = true;
                }
                SyntaxKind::R_PAREN => printer.write(")"),
                SyntaxKind::COMMA => {
                    printer.write(",");
                    printer.space();
                }
                SyntaxKind::STAR => printer.write("*"),
                SyntaxKind::DISTINCT_KW => {
                    printer.write_keyword("DISTINCT");
                    printer.space();
                }
                SyntaxKind::ALL_KW => {
                    printer.write_keyword("ALL");
                    printer.space();
                }
                SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => {
                    if seen_open {
                        printer.write_identifier(token.text());
                    } else {
                        printer.write_function(token.text());
                    }
                }
                // A keyword before the paren is the function name (COUNT, …).
                k if k.is_keyword() && !seen_open => printer.write_function(token.text()),
                _ => format_token(token, printer),
            },
        }
    }
}

/// Formats a function argument list.
pub(crate) fn format_arg_list(node: &SyntaxNode, printer: &mut PgPrinter) {
    let mut first = true;
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                if !first {
                    printer.write(",");
                    printer.space();
                }
                format_expression(child, printer);
                first = false;
            }
            cstree::util::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::DISTINCT_KW {
                    printer.write_keyword("DISTINCT");
                    printer.space();
                } else if token.kind() == SyntaxKind::ALL_KW {
                    printer.write_keyword("ALL");
                    printer.space();
                }
            }
        }
    }
}

/// Formats an OVER clause.
pub(crate) fn format_over_clause(node: &SyntaxNode, printer: &mut PgPrinter) {
    printer.write_keyword("OVER");
    printer.space();
    printer.write("(");

    if let Some(partition) = find_child(node, SyntaxKind::PARTITION_BY) {
        printer.write_keyword("PARTITION");
        printer.space();
        printer.write_keyword("BY");
        printer.space();
        format_partition_by(&partition, printer);
    }

    if let Some(order) = find_child(node, SyntaxKind::ORDER_BY_CLAUSE) {
        printer.space();
        printer.write_keyword("ORDER");
        printer.space();
        printer.write_keyword("BY");
        printer.space();
        format_order_by(&order, printer);
    }

    if let Some(frame) = find_child(node, SyntaxKind::FRAME_CLAUSE) {
        printer.space();
        format_children(&frame, printer);
    }

    printer.write(")");
}

/// Formats PARTITION BY items.
pub(crate) fn format_partition_by(node: &SyntaxNode, printer: &mut PgPrinter) {
    let mut first = true;
    for child in node.children() {
        if !first {
            printer.write(",");
            printer.space();
        }
        format_expression(child, printer);
        first = false;
    }
}

/// Formats a FILTER clause.
pub(crate) fn format_filter_clause(node: &SyntaxNode, printer: &mut PgPrinter) {
    printer.write_keyword("FILTER");
    printer.space();
    printer.write("(");
    printer.write_keyword("WHERE");
    printer.space();
    for child in node.children() {
        format_expression(child, printer);
    }
    printer.write(")");
}

/// Formats a CASE expression.
pub(crate) fn format_case_expr(node: &SyntaxNode, printer: &mut PgPrinter) {
    printer.write_keyword("CASE");

    let mut saw_when = false;
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                if child.kind() == SyntaxKind::CASE_WHEN {
                    printer.newline();
                    printer.indent();
                    format_case_when(child, printer);
                    printer.dedent();
                    saw_when = true;
                } else if child.kind() == SyntaxKind::CASE_ELSE {
                    printer.newline();
                    printer.indent();
                    format_case_else(child, printer);
                    printer.dedent();
                } else if !saw_when {
                    printer.space();
                    format_expression(child, printer);
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
pub(crate) fn format_case_when(node: &SyntaxNode, printer: &mut PgPrinter) {
    printer.write_keyword("WHEN");
    printer.space();

    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_expression(child, printer);
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
pub(crate) fn format_case_else(node: &SyntaxNode, printer: &mut PgPrinter) {
    printer.write_keyword("ELSE");
    printer.space();
    for child in node.children() {
        format_expression(child, printer);
    }
}

/// Formats a CAST expression.
pub(crate) fn format_cast_expr(node: &SyntaxNode, printer: &mut PgPrinter) {
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
                    format_expression(child, printer);
                    first = false;
                }
                cstree::util::NodeOrToken::Token(token) if !token.kind().is_trivia() => {
                    printer.space();
                    printer.write_type(token.text());
                }
                cstree::util::NodeOrToken::Token(_) => {}
            }
        }
        return;
    }
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
        let mut saw_colon = false;
        for element in node.children_with_tokens() {
            match element {
                cstree::util::NodeOrToken::Node(child) => {
                    format_expression(child, printer);
                }
                cstree::util::NodeOrToken::Token(token) => {
                    if token.kind() == SyntaxKind::DOUBLE_COLON {
                        printer.write("::");
                        saw_colon = true;
                    } else if saw_colon && token.kind().is_keyword() {
                        printer.write_type(token.text());
                    } else {
                        format_token(token, printer);
                    }
                }
            }
        }
    } else {
        printer.write_keyword("CAST");
        printer.write("(");

        let mut saw_as = false;
        for element in node.children_with_tokens() {
            match element {
                cstree::util::NodeOrToken::Node(child) => {
                    format_expression(child, printer);
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
                            printer.write_type(token.text());
                        } else {
                            format_token(token, printer);
                        }
                    }
                }
            }
        }

        printer.write(")");
    }
}

/// Formats a BETWEEN expression.
pub(crate) fn format_between_expr(node: &SyntaxNode, printer: &mut PgPrinter) {
    let children: Vec<_> = node.children().collect();
    if children.len() >= 3 {
        format_expression(children[0], printer);
        printer.space();
        printer.write_keyword("BETWEEN");
        printer.space();
        format_expression(children[1], printer);
        printer.space();
        printer.write_keyword("AND");
        printer.space();
        format_expression(children[2], printer);
    } else {
        format_children(node, printer);
    }
}

/// Formats an IN expression.
pub(crate) fn format_in_expr(node: &SyntaxNode, printer: &mut PgPrinter) {
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                if child.kind() == SyntaxKind::SUBQUERY {
                    format_subquery(child, printer);
                } else {
                    format_expression(child, printer);
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
                    format_token(token, printer);
                }
            }
        }
    }
}

/// Formats a LIKE expression.
pub(crate) fn format_like_expr(node: &SyntaxNode, printer: &mut PgPrinter) {
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_expression(child, printer);
            }
            cstree::util::NodeOrToken::Token(token) => {
                format_token(token, printer);
            }
        }
    }
}

/// Formats an IS expression.
pub(crate) fn format_is_expr(node: &SyntaxNode, printer: &mut PgPrinter) {
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_expression(child, printer);
            }
            cstree::util::NodeOrToken::Token(token) => {
                format_token(token, printer);
            }
        }
    }
}

/// Formats an EXISTS expression.
pub(crate) fn format_exists_expr(node: &SyntaxNode, printer: &mut PgPrinter) {
    printer.write_keyword("EXISTS");
    printer.space();
    for child in node.children() {
        format_subquery(child, printer);
    }
}

/// Formats a subquery.
pub(crate) fn format_subquery(node: &SyntaxNode, printer: &mut PgPrinter) {
    printer.write("(");
    printer.newline();
    printer.indent();

    for child in node.children() {
        if child.kind() == SyntaxKind::SELECT_STMT {
            format_select(child, printer);
        } else {
            format_expression(child, printer);
        }
    }

    printer.dedent();
    printer.newline();
    printer.write(")");
}

/// Formats an array expression.
pub(crate) fn format_array_expr(node: &SyntaxNode, printer: &mut PgPrinter) {
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
                format_expression(child, printer);
                first = false;
            }
            cstree::util::NodeOrToken::Token(_) => {}
        }
    }

    printer.write("]");
}

/// Formats a JSONB access expression.
pub(crate) fn format_jsonb_access(node: &SyntaxNode, printer: &mut PgPrinter) {
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_expression(child, printer);
            }
            cstree::util::NodeOrToken::Token(token) => {
                printer.write(token.text());
            }
        }
    }
}

/// Formats a COALESCE expression.
pub(crate) fn format_coalesce(node: &SyntaxNode, printer: &mut PgPrinter) {
    printer.write_function("COALESCE");
    printer.write("(");

    let mut first = true;
    for child in node.children() {
        if !first {
            printer.write(",");
            printer.space();
        }
        format_expression(child, printer);
        first = false;
    }

    printer.write(")");
}

/// Formats a NULLIF expression.
pub(crate) fn format_nullif(node: &SyntaxNode, printer: &mut PgPrinter) {
    printer.write_function("NULLIF");
    printer.write("(");

    let children: Vec<_> = node.children().collect();
    if children.len() >= 2 {
        format_expression(children[0], printer);
        printer.write(",");
        printer.space();
        format_expression(children[1], printer);
    }

    printer.write(")");
}

/// Formats GREATEST/LEAST expressions.
pub(crate) fn format_greatest_least(node: &SyntaxNode, printer: &mut PgPrinter) {
    let keyword = if node.kind() == SyntaxKind::GREATEST_EXPR {
        "GREATEST"
    } else {
        "LEAST"
    };

    printer.write_function(keyword);
    printer.write("(");

    let mut first = true;
    for child in node.children() {
        if !first {
            printer.write(",");
            printer.space();
        }
        format_expression(child, printer);
        first = false;
    }

    printer.write(")");
}

/// Formats a column reference.
pub(crate) fn format_column_ref(node: &SyntaxNode, printer: &mut PgPrinter) {
    let mut first = true;
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                if !first {
                    printer.write(".");
                }
                format_expression(child, printer);
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
                    format_token(token, printer);
                }
            }
        }
    }
}

/// Formats a literal value.
pub(crate) fn format_literal(node: &SyntaxNode, printer: &mut PgPrinter) {
    for element in node.children_with_tokens() {
        if let cstree::util::NodeOrToken::Token(token) = element {
            let kind = token.kind();
            if kind != SyntaxKind::WHITESPACE && kind != SyntaxKind::NEWLINE {
                printer.write(token.text());
            }
        }
    }
}

/// Formats a type name: its words separated by single spaces
/// (`timestamp with time zone`, `double precision`, `numeric(10, 2)`).
fn format_type_name(node: &SyntaxNode, printer: &mut PgPrinter) {
    let mut after_word = false;
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_expression(child, printer);
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
                    printer.write_type(token.text());
                } else {
                    format_token(token, printer);
                }
                after_word = word || kind == SyntaxKind::R_PAREN;
            }
        }
    }
}
