//! Statement formatting.

#![allow(clippy::needless_borrow)]

use banshee_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use crate::config::CommaStyle;
use crate::printer::Printer;

use super::{children_of_kind, find_child, find_token, format_children, format_token};

/// Formats a SELECT statement.
pub fn format_select(node: &SyntaxNode, printer: &mut Printer) {
    // Handle WITH clause if present
    if let Some(with_clause) = find_child(node, SyntaxKind::WITH_CLAUSE) {
        format_with_clause(&with_clause, printer);
    }

    // Track state for handling UNION/INTERSECT/EXCEPT
    // We process children in order to handle multiple SELECT clauses
    let mut after_set_op = false;
    let mut in_first_select = true;

    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Token(token) => {
                let kind = token.kind();
                match kind {
                    SyntaxKind::SELECT_KW => {
                        if !in_first_select {
                            // This is a SELECT after UNION/INTERSECT/EXCEPT
                            printer.newline();
                        }
                        if printer.config().river_alignment {
                            printer.write_keyword_river("SELECT");
                        } else {
                            printer.write_keyword("SELECT");
                        }
                        printer.space();
                        after_set_op = false;
                    }
                    SyntaxKind::DISTINCT_KW => {
                        printer.write_keyword("DISTINCT");
                        printer.space();
                    }
                    SyntaxKind::ALL_KW if !after_set_op => {
                        // ALL after SELECT, not after UNION
                        printer.write_keyword("ALL");
                        printer.space();
                    }
                    SyntaxKind::ALL_KW if after_set_op => {
                        // ALL after UNION/INTERSECT/EXCEPT
                        printer.write_keyword("ALL");
                        printer.space();
                    }
                    SyntaxKind::UNION_KW | SyntaxKind::INTERSECT_KW | SyntaxKind::EXCEPT_KW => {
                        printer.newline();
                        printer.newline();
                        printer.write_keyword(token.text());
                        printer.space();
                        after_set_op = true;
                        in_first_select = false;
                    }
                    _ => {}
                }
            }
            cstree::util::NodeOrToken::Node(child) => {
                let kind = child.kind();
                match kind {
                    SyntaxKind::WITH_CLAUSE => {
                        // Already handled above
                    }
                    SyntaxKind::SELECT_CLAUSE => {
                        format_select_clause(&child, printer);
                    }
                    SyntaxKind::SELECT_ITEM_LIST => {
                        let items = children_of_kind(&child, SyntaxKind::SELECT_ITEM);
                        format_select_items(&items, printer);
                    }
                    SyntaxKind::FROM_CLAUSE => {
                        format_from_clause(&child, printer);
                    }
                    SyntaxKind::WHERE_CLAUSE => {
                        format_where_clause(&child, printer);
                    }
                    SyntaxKind::GROUP_BY_CLAUSE => {
                        format_group_by_clause(&child, printer);
                    }
                    SyntaxKind::HAVING_CLAUSE => {
                        format_having_clause(&child, printer);
                    }
                    SyntaxKind::ORDER_BY_CLAUSE => {
                        format_order_by_clause(&child, printer);
                    }
                    SyntaxKind::LIMIT_CLAUSE => {
                        format_limit_clause(&child, printer);
                    }
                    SyntaxKind::OFFSET_CLAUSE => {
                        format_offset_clause(&child, printer);
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Formats a WITH clause (CTEs).
fn format_with_clause(node: &SyntaxNode, printer: &mut Printer) {
    if printer.config().river_alignment {
        printer.write_keyword_river("WITH");
    } else {
        printer.write_keyword("WITH");
    }
    printer.space();

    // Check for RECURSIVE
    for element in node.children_with_tokens() {
        if let cstree::util::NodeOrToken::Token(token) = element
            && token.kind() == SyntaxKind::RECURSIVE_KW
        {
            printer.write_keyword("RECURSIVE");
            printer.space();
        }
    }

    // Format CTEs
    let ctes = children_of_kind(node, SyntaxKind::CTE);
    for (i, cte) in ctes.iter().enumerate() {
        if i > 0 {
            printer.write(",");
            printer.newline();
            if printer.config().river_alignment {
                printer.spaces(printer.river_width() - 4); // Align with first CTE
            }
        }
        format_cte(cte, printer);
    }

    printer.newline();
}

/// Formats a single CTE.
fn format_cte(node: &SyntaxNode, printer: &mut Printer) {
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                if child.kind() == SyntaxKind::SUBQUERY || child.kind() == SyntaxKind::SELECT_STMT {
                    // The parentheses are separate tokens (L_PAREN, R_PAREN) at the CTE level,
                    // so we just format the SELECT_STMT without adding extra parentheses.
                    printer.newline();
                    printer.indent();
                    format_select(&child, printer);
                    printer.dedent();
                    printer.newline();
                } else {
                    format_children(&child, printer);
                }
            }
            cstree::util::NodeOrToken::Token(token) => {
                let kind = token.kind();
                if kind == SyntaxKind::L_PAREN {
                    printer.write("(");
                } else if kind == SyntaxKind::R_PAREN {
                    printer.write(")");
                } else {
                    format_token(&token, printer);
                }
            }
        }
    }
}

/// Formats a SELECT clause.
fn format_select_clause(node: &SyntaxNode, printer: &mut Printer) {
    if printer.config().river_alignment {
        printer.write_keyword_river("SELECT");
    } else {
        printer.write_keyword("SELECT");
    }
    printer.space();

    // Check for DISTINCT/ALL
    for element in node.children_with_tokens() {
        if let cstree::util::NodeOrToken::Token(token) = element {
            if token.kind() == SyntaxKind::DISTINCT_KW {
                printer.write_keyword("DISTINCT");
                printer.space();
            } else if token.kind() == SyntaxKind::ALL_KW {
                printer.write_keyword("ALL");
                printer.space();
            }
        }
    }

    // Format select items
    let items = children_of_kind(node, SyntaxKind::SELECT_ITEM);
    format_select_items(&items, printer);
}

/// Formats a list of select items.
fn format_select_items(items: &[SyntaxNode], printer: &mut Printer) {
    let comma_style = printer.config().comma_style;
    let river_alignment = printer.config().river_alignment;
    let river_width = printer.river_width();

    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            match comma_style {
                CommaStyle::Trailing => {
                    // sqlstyle.guide: comma followed by space, keep on same line
                    printer.write(",");
                    printer.space();
                }
                CommaStyle::Leading => {
                    // Leading comma style: newline then comma
                    printer.newline();
                    if river_alignment {
                        // Right-align comma
                        printer.spaces(river_width - 1);
                        printer.write(",");
                        printer.space();
                    } else {
                        printer.write(",");
                        printer.space();
                    }
                }
            }
        }

        format_select_item(item, printer);
    }
}

/// Formats a single select item.
fn format_select_item(node: &SyntaxNode, printer: &mut Printer) {
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

/// Formats an expression node.
fn format_expression(node: &SyntaxNode, printer: &mut Printer) {
    super::expressions::format_expression(node, printer);
}

/// Formats a FROM clause.
fn format_from_clause(node: &SyntaxNode, printer: &mut Printer) {
    printer.newline();
    if printer.config().river_alignment {
        printer.write_keyword_river("FROM");
    } else {
        printer.write_keyword("FROM");
    }
    printer.space();

    // Format table references - JOIN_EXPR contains the left table as first child
    let mut first = true;
    for child in node.children() {
        match child.kind() {
            SyntaxKind::TABLE_REF => {
                if !first {
                    printer.write(",");
                    printer.space();
                }
                format_table_ref(&child, printer);
                first = false;
            }
            SyntaxKind::JOIN_EXPR => {
                format_join_expr(&child, printer, first);
                first = false;
            }
            _ => {}
        }
    }
}

/// Formats a table reference.
fn format_table_ref(node: &SyntaxNode, printer: &mut Printer) {
    let mut first = true;
    let mut after_as_or_dot = false;
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                if child.kind() == SyntaxKind::SUBQUERY {
                    printer.write("(");
                    printer.newline();
                    printer.indent();
                    if let Some(select) = find_child(&child, SyntaxKind::SELECT_STMT) {
                        format_select(&select, printer);
                    }
                    printer.dedent();
                    printer.newline();
                    printer.write(")");
                } else if child.kind() == SyntaxKind::ALIAS {
                    format_alias(&child, printer);
                } else if child.kind() == SyntaxKind::TABLE_NAME
                    || child.kind() == SyntaxKind::QUALIFIED_NAME
                {
                    format_table_name(&child, printer);
                } else {
                    format_children(&child, printer);
                }
                first = false;
            }
            cstree::util::NodeOrToken::Token(token) => {
                let kind = token.kind();
                // Skip whitespace and handle identifiers
                if kind == SyntaxKind::WHITESPACE || kind == SyntaxKind::NEWLINE {
                    continue;
                }
                if kind == SyntaxKind::AS_KW {
                    printer.space();
                    printer.write_keyword("AS");
                    printer.space();
                    after_as_or_dot = true;
                    first = false;
                } else if kind == SyntaxKind::DOT {
                    printer.write(".");
                    after_as_or_dot = true;
                } else if kind == SyntaxKind::IDENT || kind == SyntaxKind::QUOTED_IDENT {
                    if !first && !after_as_or_dot {
                        printer.space();
                    }
                    printer.write_identifier(token.text());
                    first = false;
                    after_as_or_dot = false;
                } else {
                    format_token(&token, printer);
                }
            }
        }
    }
}

/// Formats a table name.
fn format_table_name(node: &SyntaxNode, printer: &mut Printer) {
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                format_children(&child, printer);
            }
            cstree::util::NodeOrToken::Token(token) => {
                let kind = token.kind();
                if kind == SyntaxKind::WHITESPACE || kind == SyntaxKind::NEWLINE {
                    continue;
                }
                if kind == SyntaxKind::DOT {
                    printer.write(".");
                } else if kind == SyntaxKind::IDENT || kind == SyntaxKind::QUOTED_IDENT {
                    printer.write_identifier(token.text());
                } else {
                    format_token(&token, printer);
                }
            }
        }
    }
}

/// Formats an alias.
fn format_alias(node: &SyntaxNode, printer: &mut Printer) {
    // Check if there's an AS keyword
    let has_as = node
        .children_with_tokens()
        .any(|e| matches!(e, cstree::util::NodeOrToken::Token(t) if t.kind() == SyntaxKind::AS_KW));

    printer.space();

    for element in node.children_with_tokens() {
        if let cstree::util::NodeOrToken::Token(token) = element {
            let kind = token.kind();
            if kind == SyntaxKind::WHITESPACE || kind == SyntaxKind::NEWLINE {
                continue;
            }
            if kind == SyntaxKind::AS_KW {
                printer.write_keyword("AS");
                printer.space();
            } else if kind == SyntaxKind::IDENT || kind == SyntaxKind::QUOTED_IDENT {
                // If no AS keyword, this is a shorthand alias
                if !has_as {
                    // Just write the alias directly (already added space above)
                }
                printer.write_identifier(token.text());
            }
        }
    }
}

/// Formats a JOIN expression.
/// The JOIN_EXPR structure is left-associative:
/// - For multiple JOINs: JOIN_EXPR(JOIN_EXPR(table1, join, table2), join, table3)
/// - The inner JOIN_EXPR is the left side, the outer adds another join
fn format_join_expr(node: &SyntaxNode, printer: &mut Printer, _is_first: bool) {
    // Check if the first child is a nested JOIN_EXPR
    let first_child = node.children().next();
    let has_nested_join = first_child
        .as_ref()
        .is_some_and(|c| c.kind() == SyntaxKind::JOIN_EXPR);

    if has_nested_join {
        // Recursively format the nested join first
        if let Some(nested_join) = first_child {
            format_join_expr(&nested_join, printer, _is_first);
        }
    } else {
        // No nested join - format the left table (first TABLE_REF)
        let left_table = node.children().find(|c| c.kind() == SyntaxKind::TABLE_REF);
        if let Some(table) = left_table {
            format_table_ref(&table, printer);
        }
    }

    // Determine join type from tokens at THIS level (not nested)
    let mut join_parts: Vec<&str> = Vec::new();
    for element in node.children_with_tokens() {
        if let cstree::util::NodeOrToken::Token(token) = element {
            match token.kind() {
                SyntaxKind::LEFT_KW => join_parts.push("LEFT"),
                SyntaxKind::RIGHT_KW => join_parts.push("RIGHT"),
                SyntaxKind::INNER_KW => join_parts.push("INNER"),
                SyntaxKind::FULL_KW => join_parts.push("FULL"),
                SyntaxKind::CROSS_KW => join_parts.push("CROSS"),
                SyntaxKind::NATURAL_KW => join_parts.push("NATURAL"),
                SyntaxKind::OUTER_KW => join_parts.push("OUTER"),
                SyntaxKind::JOIN_KW => join_parts.push("JOIN"),
                _ => {}
            }
        }
    }

    let join_keyword = join_parts.join(" ");
    let join_keyword = if join_keyword.is_empty() {
        "JOIN".to_string()
    } else {
        join_keyword
    };

    // Add blank line between JOINs (not before the first one)
    if has_nested_join {
        printer.newline();
    }

    printer.newline();
    if printer.config().river_alignment {
        // Indent JOIN to align with table name after FROM
        printer.spaces(printer.river_width() + 1);
    }
    printer.write_keyword(&join_keyword);
    printer.space();

    // Format the right table (TABLE_REF that's not the first/nested one)
    let table_refs: Vec<_> = node
        .children()
        .filter(|c| c.kind() == SyntaxKind::TABLE_REF)
        .collect();

    // If we have nested join, the first TABLE_REF is the right table
    // If no nested join, the second TABLE_REF is the right table
    let right_table = if has_nested_join {
        table_refs.first()
    } else {
        table_refs.get(1)
    };

    if let Some(table) = right_table {
        format_table_ref(table, printer);
    }

    // Format ON condition
    if let Some(condition) = find_child(node, SyntaxKind::JOIN_CONDITION) {
        printer.newline();
        if printer.config().river_alignment {
            // Align ON with JOIN
            printer.spaces(printer.river_width() + 1);
        }
        printer.write_keyword("ON");
        printer.space();

        // Format condition with special handling for AND
        format_join_condition(&condition, printer);
    }
}

/// Formats a JOIN condition, handling AND with proper indentation.
fn format_join_condition(node: &SyntaxNode, printer: &mut Printer) {
    for child in node.children() {
        format_condition_expr(&child, printer, true);
    }
}

/// Formats a condition expression in JOIN context.
fn format_condition_expr(node: &SyntaxNode, printer: &mut Printer, is_top_level: bool) {
    // Check if this is a binary expression with AND/OR
    let has_logical_op = node.children_with_tokens().any(|e| {
        matches!(e, cstree::util::NodeOrToken::Token(t)
            if t.kind() == SyntaxKind::AND_KW || t.kind() == SyntaxKind::OR_KW)
    });

    if has_logical_op && is_top_level {
        // Split on AND/OR for proper formatting
        let mut first = true;
        for element in node.children_with_tokens() {
            match element {
                cstree::util::NodeOrToken::Node(child) => {
                    if child.kind() == SyntaxKind::BINARY_EXPR {
                        format_condition_expr(&child, printer, false);
                    } else {
                        format_expression(&child, printer);
                    }
                    first = false;
                }
                cstree::util::NodeOrToken::Token(token) => {
                    if token.kind() == SyntaxKind::AND_KW || token.kind() == SyntaxKind::OR_KW {
                        printer.newline();
                        if printer.config().river_alignment {
                            // Indent AND/OR more than ON
                            printer.spaces(printer.river_width() + 4);
                        }
                        printer.write_keyword(token.text());
                        printer.space();
                    } else if !first {
                        format_token(&token, printer);
                    }
                }
            }
        }
    } else {
        format_expression(node, printer);
    }
}

/// Formats a WHERE clause.
fn format_where_clause(node: &SyntaxNode, printer: &mut Printer) {
    printer.newline();
    if printer.config().river_alignment {
        printer.write_keyword_river("WHERE");
    } else {
        printer.write_keyword("WHERE");
    }
    printer.space();

    // Format the condition
    for child in node.children() {
        format_where_condition(&child, printer);
    }
}

/// Formats a WHERE condition, handling AND/OR with newlines.
fn format_where_condition(node: &SyntaxNode, printer: &mut Printer) {
    let newline_before_logical = printer.config().newline_before_logical;
    let river_alignment = printer.config().river_alignment;
    let river_width = printer.river_width();

    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => {
                if child.kind() == SyntaxKind::BINARY_EXPR {
                    format_where_condition(&child, printer);
                } else {
                    format_expression(&child, printer);
                }
            }
            cstree::util::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::AND_KW || token.kind() == SyntaxKind::OR_KW {
                    if newline_before_logical {
                        printer.newline();
                        if river_alignment {
                            // Right-align AND/OR
                            let kw = if token.kind() == SyntaxKind::AND_KW {
                                "AND"
                            } else {
                                "OR"
                            };
                            let padding = river_width.saturating_sub(kw.len());
                            printer.spaces(padding);
                            printer.write_keyword(kw);
                        } else {
                            printer.write_keyword(token.text());
                        }
                        printer.space();
                    } else {
                        printer.space();
                        printer.write_keyword(token.text());
                        printer.space();
                    }
                } else {
                    format_token(&token, printer);
                }
            }
        }
    }
}

/// Formats a GROUP BY clause.
fn format_group_by_clause(node: &SyntaxNode, printer: &mut Printer) {
    printer.newline();
    if printer.config().river_alignment {
        printer.write_keyword_river("GROUP BY");
    } else {
        printer.write_keyword("GROUP");
        printer.space();
        printer.write_keyword("BY");
    }
    printer.space();

    let mut first = true;
    for child in node.children() {
        if child.kind() != SyntaxKind::GROUP_KW && child.kind() != SyntaxKind::BY_KW {
            if !first {
                printer.write(",");
                printer.space();
            }
            format_expression(&child, printer);
            first = false;
        }
    }
}

/// Formats a HAVING clause.
fn format_having_clause(node: &SyntaxNode, printer: &mut Printer) {
    printer.newline();
    if printer.config().river_alignment {
        printer.write_keyword_river("HAVING");
    } else {
        printer.write_keyword("HAVING");
    }
    printer.space();

    for child in node.children() {
        format_expression(&child, printer);
    }
}

/// Formats an ORDER BY clause.
fn format_order_by_clause(node: &SyntaxNode, printer: &mut Printer) {
    printer.newline();
    if printer.config().river_alignment {
        printer.write_keyword_river("ORDER BY");
    } else {
        printer.write_keyword("ORDER");
        printer.space();
        printer.write_keyword("BY");
    }
    printer.space();

    let items = children_of_kind(node, SyntaxKind::ORDER_BY_ITEM);
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            printer.write(",");
            printer.space();
        }
        format_order_by_item(item, printer);
    }
}

/// Formats an ORDER BY item.
fn format_order_by_item(node: &SyntaxNode, printer: &mut Printer) {
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

/// Formats a LIMIT clause.
fn format_limit_clause(node: &SyntaxNode, printer: &mut Printer) {
    printer.newline();
    if printer.config().river_alignment {
        printer.write_keyword_river("LIMIT");
    } else {
        printer.write_keyword("LIMIT");
    }
    printer.space();

    for child in node.children() {
        format_expression(&child, printer);
    }
}

/// Formats an OFFSET clause.
fn format_offset_clause(node: &SyntaxNode, printer: &mut Printer) {
    printer.newline();
    if printer.config().river_alignment {
        printer.write_keyword_river("OFFSET");
    } else {
        printer.write_keyword("OFFSET");
    }
    printer.space();

    for child in node.children() {
        format_expression(&child, printer);
    }
}

/// Formats an INSERT statement.
pub fn format_insert(node: &SyntaxNode, printer: &mut Printer) {
    // Handle WITH clause if present
    if let Some(with_clause) = find_child(node, SyntaxKind::WITH_CLAUSE) {
        format_with_clause(&with_clause, printer);
    }

    if printer.config().river_alignment {
        printer.write_keyword_river("INSERT INTO");
    } else {
        printer.write_keyword("INSERT");
        printer.space();
        printer.write_keyword("INTO");
    }
    printer.space();

    // DML targets are parsed as TABLE_REF, not TABLE_NAME.
    if let Some(table_ref) = find_child(node, SyntaxKind::TABLE_REF) {
        format_table_ref(&table_ref, printer);
    }

    // Format column list
    if let Some(columns) = find_child(node, SyntaxKind::INSERT_COLUMNS) {
        printer.space();
        format_children(&columns, printer);
    }

    // Format VALUES or SELECT
    if let Some(values) = find_child(node, SyntaxKind::VALUES_CLAUSE) {
        printer.newline();
        if printer.config().river_alignment {
            printer.write_keyword_river("VALUES");
        } else {
            printer.write_keyword("VALUES");
        }
        printer.space();
        format_values_clause(&values, printer);
    } else if let Some(select) = find_child(node, SyntaxKind::SELECT_STMT) {
        printer.newline();
        format_select(&select, printer);
    }

    // Format ON CONFLICT
    if let Some(conflict) = find_child(node, SyntaxKind::ON_CONFLICT_CLAUSE) {
        format_on_conflict(&conflict, printer);
    }

    // Format RETURNING
    if let Some(returning) = find_child(node, SyntaxKind::RETURNING_CLAUSE) {
        format_returning(&returning, printer);
    }
}

/// Formats a VALUES clause.
fn format_values_clause(node: &SyntaxNode, printer: &mut Printer) {
    let rows = children_of_kind(node, SyntaxKind::VALUES_ROW);
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            printer.write(",");
            printer.newline();
            if printer.config().river_alignment {
                printer.spaces(printer.river_width() + 1);
            }
        }
        format_children(row, printer);
    }
}

/// Formats an ON CONFLICT clause.
fn format_on_conflict(node: &SyntaxNode, printer: &mut Printer) {
    printer.newline();
    printer.write_keyword("ON");
    printer.space();
    printer.write_keyword("CONFLICT");

    // Format conflict target
    if let Some(target) = find_child(node, SyntaxKind::CONFLICT_TARGET) {
        printer.space();
        format_children(&target, printer);
    }

    // Format DO action
    if let Some(action) = find_child(node, SyntaxKind::CONFLICT_ACTION) {
        printer.space();
        format_children(&action, printer);
    }
}

/// Formats a RETURNING clause.
fn format_returning(node: &SyntaxNode, printer: &mut Printer) {
    printer.newline();
    if printer.config().river_alignment {
        printer.write_keyword_river("RETURNING");
    } else {
        printer.write_keyword("RETURNING");
    }
    printer.space();

    for (i, item) in node.children().enumerate() {
        if i > 0 {
            printer.write(",");
            printer.space();
        }
        format_expression(item, printer);
    }
}

/// Formats an UPDATE statement.
pub fn format_update(node: &SyntaxNode, printer: &mut Printer) {
    // Handle WITH clause if present
    if let Some(with_clause) = find_child(node, SyntaxKind::WITH_CLAUSE) {
        format_with_clause(&with_clause, printer);
    }

    if printer.config().river_alignment {
        printer.write_keyword_river("UPDATE");
    } else {
        printer.write_keyword("UPDATE");
    }
    printer.space();

    // DML targets are parsed as TABLE_REF, not TABLE_NAME.
    if let Some(table_ref) = find_child(node, SyntaxKind::TABLE_REF) {
        format_table_ref(&table_ref, printer);
    }

    // Format SET clause
    if let Some(set_clause) = find_child(node, SyntaxKind::SET_CLAUSE) {
        printer.newline();
        if printer.config().river_alignment {
            printer.write_keyword_river("SET");
        } else {
            printer.write_keyword("SET");
        }
        printer.space();
        format_set_clause(&set_clause, printer);
    }

    // Format FROM clause (PostgreSQL extension)
    if let Some(from_clause) = find_child(node, SyntaxKind::FROM_CLAUSE) {
        format_from_clause(&from_clause, printer);
    }

    // Format WHERE clause
    if let Some(where_clause) = find_child(node, SyntaxKind::WHERE_CLAUSE) {
        format_where_clause(&where_clause, printer);
    }

    // Format RETURNING
    if let Some(returning) = find_child(node, SyntaxKind::RETURNING_CLAUSE) {
        format_returning(&returning, printer);
    }
}

/// Formats a SET clause.
fn format_set_clause(node: &SyntaxNode, printer: &mut Printer) {
    let items = children_of_kind(node, SyntaxKind::SET_ITEM);
    let comma_style = printer.config().comma_style;
    let river_alignment = printer.config().river_alignment;
    let river_width = printer.river_width();

    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            match comma_style {
                CommaStyle::Trailing => {
                    printer.write(",");
                    printer.newline();
                    if river_alignment {
                        printer.spaces(river_width + 1);
                    }
                }
                CommaStyle::Leading => {
                    printer.newline();
                    if river_alignment {
                        printer.spaces(river_width - 1);
                        printer.write(",");
                        printer.space();
                    } else {
                        printer.write(",");
                        printer.space();
                    }
                }
            }
        }
        format_children(item, printer);
    }
}

/// Formats a DELETE statement.
pub fn format_delete(node: &SyntaxNode, printer: &mut Printer) {
    // Handle WITH clause if present
    if let Some(with_clause) = find_child(node, SyntaxKind::WITH_CLAUSE) {
        format_with_clause(&with_clause, printer);
    }

    if printer.config().river_alignment {
        printer.write_keyword_river("DELETE FROM");
    } else {
        printer.write_keyword("DELETE");
        printer.space();
        printer.write_keyword("FROM");
    }
    printer.space();

    // DML targets are parsed as TABLE_REF, not TABLE_NAME.
    if let Some(table_ref) = find_child(node, SyntaxKind::TABLE_REF) {
        format_table_ref(&table_ref, printer);
    }

    // Format USING clause (PostgreSQL extension)
    if let Some(using_clause) = find_child(node, SyntaxKind::USING_CLAUSE) {
        printer.newline();
        if printer.config().river_alignment {
            printer.write_keyword_river("USING");
        } else {
            printer.write_keyword("USING");
        }
        printer.space();
        format_children(&using_clause, printer);
    }

    // Format WHERE clause
    if let Some(where_clause) = find_child(node, SyntaxKind::WHERE_CLAUSE) {
        format_where_clause(&where_clause, printer);
    }

    // Format RETURNING
    if let Some(returning) = find_child(node, SyntaxKind::RETURNING_CLAUSE) {
        format_returning(&returning, printer);
    }
}

/// Formats a CREATE TABLE statement.
/// Constraint node kinds that may appear in a column definition list.
const CONSTRAINT_KINDS: &[SyntaxKind] = &[
    SyntaxKind::PRIMARY_KEY_CONSTRAINT,
    SyntaxKind::FOREIGN_KEY_CONSTRAINT,
    SyntaxKind::UNIQUE_CONSTRAINT,
    SyntaxKind::CHECK_CONSTRAINT,
    SyntaxKind::NOT_NULL_CONSTRAINT,
    SyntaxKind::DEFAULT_EXPR,
    SyntaxKind::CONSTRAINT,
];

fn is_constraint_kind(kind: SyntaxKind) -> bool {
    CONSTRAINT_KINDS.contains(&kind)
}

pub fn format_create_table(node: &SyntaxNode, printer: &mut Printer) {
    printer.write_keyword("CREATE");
    printer.space();

    for element in node.children_with_tokens() {
        if let cstree::util::NodeOrToken::Token(token) = element
            && matches!(
                token.kind(),
                SyntaxKind::TEMP_KW | SyntaxKind::TEMPORARY_KW | SyntaxKind::UNLOGGED_KW
            )
        {
            printer.write_keyword(token.text());
            printer.space();
        }
    }

    printer.write_keyword("TABLE");
    printer.space();

    if find_token(node, SyntaxKind::IF_KW).is_some() {
        printer.write_keyword("IF");
        printer.space();
        printer.write_keyword("NOT");
        printer.space();
        printer.write_keyword("EXISTS");
        printer.space();
    }

    // The relation name is a TABLE_REF in DDL.
    if let Some(table_name) = find_child(node, SyntaxKind::TABLE_REF) {
        format_inline(&table_name, printer, &mut Inline::default());
    }

    // CREATE TABLE ... AS SELECT ...
    if let Some(query) = find_child(node, SyntaxKind::SELECT_STMT) {
        printer.space();
        printer.write_keyword("AS");
        printer.newline();
        format_select(&query, printer);
        return;
    }

    // Column/constraint list, one element per line.
    if let Some(columns) = find_child(node, SyntaxKind::COLUMN_DEF_LIST) {
        printer.space();
        printer.write("(");
        printer.newline();
        printer.indent();

        let elements: Vec<SyntaxNode> = columns
            .children()
            .filter(|c| c.kind() == SyntaxKind::COLUMN_DEF || is_constraint_kind(c.kind()))
            .cloned()
            .collect();
        for (i, element) in elements.iter().enumerate() {
            if i > 0 {
                printer.write(",");
                printer.newline();
            }
            format_inline(element, printer, &mut Inline::default());
        }

        printer.dedent();
        printer.newline();
        printer.write(")");
    }
}

/// A `CREATE INDEX` statement: header inline, partial `WHERE` on its own line.
pub fn format_create_index(node: &SyntaxNode, printer: &mut Printer) {
    let mut state = Inline::default();
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Token(token) => {
                // The WHERE keyword is a child token; the predicate is in the
                // WHERE_CLAUSE node. Emit both together when the node is reached.
                if matches!(token.kind(), SyntaxKind::SEMICOLON | SyntaxKind::WHERE_KW) {
                    continue;
                }
                emit_inline_token(&token, printer, &mut state);
            }
            cstree::util::NodeOrToken::Node(child) => {
                if child.kind() == SyntaxKind::WHERE_CLAUSE {
                    printer.newline();
                    printer.write_keyword("WHERE");
                    let mut where_state = Inline {
                        prev_word: true,
                        prev_kind: Some(SyntaxKind::WHERE_KW),
                    };
                    format_inline(&child, printer, &mut where_state);
                } else {
                    format_inline(&child, printer, &mut state);
                }
            }
        }
    }
}

/// An `ALTER TABLE` statement: header inline, one action per line.
pub fn format_alter(node: &SyntaxNode, printer: &mut Printer) {
    printer.write_keyword("ALTER");
    printer.space();
    printer.write_keyword("TABLE");
    printer.space();

    if find_token(node, SyntaxKind::IF_KW).is_some() {
        printer.write_keyword("IF");
        printer.space();
        printer.write_keyword("EXISTS");
        printer.space();
    }
    if find_token(node, SyntaxKind::ONLY_KW).is_some() {
        printer.write_keyword("ONLY");
        printer.space();
    }

    if let Some(table_name) = find_child(node, SyntaxKind::TABLE_REF) {
        format_inline(&table_name, printer, &mut Inline::default());
    }

    let actions = children_of_kind(node, SyntaxKind::ALTER_TABLE_ACTION);
    if actions.is_empty() {
        // RENAME (and other action-less tails): keep on the header line.
        let mut state = Inline {
            prev_word: true,
            prev_kind: Some(SyntaxKind::TABLE_KW),
        };
        for element in node.children_with_tokens() {
            if let cstree::util::NodeOrToken::Token(token) = element
                && !matches!(
                    token.kind(),
                    SyntaxKind::ALTER_KW
                        | SyntaxKind::TABLE_KW
                        | SyntaxKind::ONLY_KW
                        | SyntaxKind::SEMICOLON
                )
                && !token.kind().is_trivia()
            {
                emit_inline_token(&token, printer, &mut state);
            }
        }
        return;
    }

    printer.indent();
    for (i, action) in actions.iter().enumerate() {
        if i > 0 {
            printer.write(",");
        }
        printer.newline();
        format_inline(action, printer, &mut Inline::default());
    }
    printer.dedent();
}

/// Spacing state for [`format_inline`]: whether the previous emitted token ended
/// on a word character, and its kind (to decide spacing before `(`).
#[derive(Default)]
struct Inline {
    prev_word: bool,
    prev_kind: Option<SyntaxKind>,
}

/// Keywords after which a `(` opens a column/expression list and reads better
/// with a leading space (`PRIMARY KEY (a)`, `UNIQUE (x)`, `CHECK (…)`), as
/// opposed to a type modifier or function call (`varchar(255)`, `now()`).
fn space_before_paren(prev_kind: Option<SyntaxKind>) -> bool {
    matches!(
        prev_kind,
        Some(SyntaxKind::KEY_KW | SyntaxKind::UNIQUE_KW | SyntaxKind::CHECK_KW | SyntaxKind::IN_KW)
    )
}

/// Walks a node's tokens (recursing into child nodes) and prints them with a
/// single space between adjacent word tokens and none around punctuation —
/// the spacing DDL fragments (types, constraints, actions) want.
fn format_inline(node: &SyntaxNode, printer: &mut Printer, state: &mut Inline) {
    for element in node.children_with_tokens() {
        match element {
            cstree::util::NodeOrToken::Node(child) => format_inline(&child, printer, state),
            cstree::util::NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::SEMICOLON {
                    continue;
                }
                emit_inline_token(&token, printer, state);
            }
        }
    }
}

/// Emits one token for [`format_inline`], updating the spacing state.
fn emit_inline_token(token: &SyntaxToken, printer: &mut Printer, state: &mut Inline) {
    let kind = token.kind();
    if kind.is_trivia() {
        return;
    }
    match kind {
        SyntaxKind::COMMA => {
            printer.write(",");
            printer.space();
            state.prev_word = false;
        }
        SyntaxKind::L_PAREN => {
            if space_before_paren(state.prev_kind) {
                printer.space();
            }
            printer.write("(");
            state.prev_word = false;
        }
        SyntaxKind::R_PAREN => {
            printer.write(")");
            state.prev_word = true;
        }
        SyntaxKind::L_BRACKET => {
            printer.write("[");
            state.prev_word = false;
        }
        SyntaxKind::R_BRACKET => {
            printer.write("]");
            state.prev_word = true;
        }
        SyntaxKind::DOT => {
            printer.write(".");
            state.prev_word = false;
        }
        k if k.is_keyword() => {
            if state.prev_word {
                printer.space();
            }
            printer.write_keyword(token.text());
            state.prev_word = true;
        }
        SyntaxKind::IDENT => {
            if state.prev_word {
                printer.space();
            }
            printer.write_identifier(token.text());
            state.prev_word = true;
        }
        _ => {
            if state.prev_word {
                printer.space();
            }
            printer.write(token.text());
            state.prev_word = true;
        }
    }
    state.prev_kind = Some(kind);
}
