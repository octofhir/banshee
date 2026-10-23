//! Completion context detection.
//!
//! Analyzes the cursor position in a parse tree to determine
//! what kind of completions are appropriate.

#![allow(clippy::needless_borrow)]

use banshee_syntax::{Parse, SyntaxKind, SyntaxNode, SyntaxToken};
use std::collections::HashMap;
use text_size::TextSize;

use crate::types::CompletionContext;

/// Detects the completion context at a given offset in the source.
pub fn detect_context(parse: &Parse, offset: TextSize) -> CompletionContext {
    let root = parse.syntax();

    // Find the token at or before the offset
    let token = find_token_at_offset(&root, offset);

    // Find the node containing the offset
    let node = find_node_at_offset(&root, offset);

    match (token.as_ref(), node.as_ref()) {
        (Some(token), Some(node)) => detect_context_from_token_and_node(token, node, offset),
        (Some(token), None) => detect_context_from_token(token, offset),
        (None, Some(node)) => detect_context_from_node(node, offset),
        (None, None) => detect_statement_start(&root, offset),
    }
}

/// Finds the token at or immediately before the given offset.
fn find_token_at_offset(root: &SyntaxNode, offset: TextSize) -> Option<SyntaxToken> {
    // Try to find a token that contains the offset
    let mut best_token: Option<SyntaxToken> = None;
    let mut best_non_trivia: Option<SyntaxToken> = None;

    for token in root
        .descendants_with_tokens()
        .filter_map(|it| it.into_token())
    {
        let range = token.text_range();
        if range.contains(offset) || range.end() == offset {
            if token.kind().is_trivia() {
                return best_non_trivia
                    .or(best_token)
                    .or_else(|| Some(token.clone()));
            }
            return Some(token.clone());
        }
        if range.end() <= offset {
            best_token = Some(token.clone());
            if !token.kind().is_trivia() {
                best_non_trivia = Some(token.clone());
            }
        }
    }

    best_non_trivia.or(best_token)
}

/// Finds the deepest node containing the offset.
///
/// The end is inclusive: a cursor at the end of the text (the common case
/// while typing) must still resolve to the clause being written.
fn find_node_at_offset(root: &SyntaxNode, offset: TextSize) -> Option<SyntaxNode> {
    root.descendants()
        .filter(|n| n.text_range().contains_inclusive(offset))
        .last()
        .cloned()
}

/// Detects context using both token and node information.
fn detect_context_from_token_and_node(
    token: &SyntaxToken,
    node: &SyntaxNode,
    offset: TextSize,
) -> CompletionContext {
    let token_kind = token.kind();
    let node_kind = node.kind();

    // Check for window function context
    if is_in_over_clause(node) {
        return detect_window_context(token, node);
    }

    // Check for JSONB access context
    if is_jsonb_context(token_kind, node) {
        return detect_jsonb_context(token, node);
    }

    if matches!(token_kind, SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT)
        && let Some(ctx) = detect_table_context_from_token(token)
    {
        return ctx;
    }

    if token_kind == SyntaxKind::DOT
        && let Some(table) = find_table_qualifier(token, node)
    {
        return CompletionContext::ColumnRef {
            table: Some(table),
            tables: find_tables_in_scope(node),
        };
    }

    // Check for function argument context
    if (node_kind == SyntaxKind::ARG_LIST || node_kind == SyntaxKind::FUNC_CALL)
        && let Some(ctx) = detect_function_arg_context(node)
    {
        return ctx;
    }

    // Check by parent node type
    match node_kind {
        SyntaxKind::SELECT_ITEM | SyntaxKind::SELECT_ITEM_LIST | SyntaxKind::SELECT_CLAUSE => {
            detect_select_column_context(node)
        }
        SyntaxKind::FROM_CLAUSE | SyntaxKind::TABLE_REF | SyntaxKind::TABLE_NAME => {
            detect_table_name_context(token)
        }
        SyntaxKind::WHERE_CLAUSE => detect_where_context(node),
        SyntaxKind::JOIN_CONDITION => detect_join_condition_context(node),
        SyntaxKind::ORDER_BY_CLAUSE | SyntaxKind::ORDER_BY_ITEM => detect_order_by_context(node),
        SyntaxKind::COLUMN_REF | SyntaxKind::QUALIFIED_NAME => {
            detect_column_ref_context(token, node)
        }
        SyntaxKind::TYPE_NAME => CompletionContext::TypeName,
        _ => detect_from_token(token, offset),
    }
}

/// Checks if we're in a JSONB access context.
fn is_jsonb_context(token_kind: SyntaxKind, node: &SyntaxNode) -> bool {
    if token_kind.is_jsonb_operator() {
        return true;
    }
    node.ancestors().any(is_jsonb_expr)
}

/// Detects JSONB completion context.
fn detect_jsonb_context(_token: &SyntaxToken, node: &SyntaxNode) -> CompletionContext {
    let (table, column, path) = extract_jsonb_path_with_table(node);
    CompletionContext::JsonbField {
        table,
        column,
        path,
    }
}

/// Extracts the table, column name and path from a JSONB access expression.
fn extract_jsonb_path_with_table(node: &SyntaxNode) -> (Option<String>, String, Vec<String>) {
    let root = node
        .ancestors()
        .filter(|ancestor| is_jsonb_expr(ancestor))
        .last()
        .unwrap_or(node);

    let mut table = None;
    let mut column = None;
    let mut path = Vec::new();
    collect_jsonb_chain(root, &mut table, &mut column, &mut path);

    (table, column.unwrap_or_default(), path)
}

/// Extracts a qualified column name from a COLUMN_REF node.
/// Returns (table, column) where table is Some if qualified (e.g., `patient.resource`).
fn extract_qualified_column(node: &SyntaxNode) -> (Option<String>, String) {
    let mut parts = Vec::new();
    for child in node.children_with_tokens() {
        if let Some(token) = child.as_token()
            && (token.kind() == SyntaxKind::IDENT || token.kind() == SyntaxKind::QUOTED_IDENT)
        {
            parts.push(token.text().to_string());
        }
    }

    match parts.as_slice() {
        [] => (None, String::new()),
        [col] => (None, col.clone()),
        [tbl, col] => (Some(tbl.clone()), col.clone()),
        // schema.table.column - return table and column
        [_schema, tbl, col] => (Some(tbl.clone()), col.clone()),
        _ => (None, parts.last().cloned().unwrap_or_default()),
    }
}

/// Extracts a column name from a COLUMN_REF node (unqualified).
#[allow(dead_code)]
fn extract_column_name(node: &SyntaxNode) -> String {
    extract_qualified_column(node).1
}

fn is_jsonb_expr(node: &SyntaxNode) -> bool {
    if matches!(
        node.kind(),
        SyntaxKind::JSONB_ACCESS_EXPR | SyntaxKind::JSONB_PATH_EXPR
    ) {
        return true;
    }

    node.kind() == SyntaxKind::BINARY_EXPR && has_jsonb_operator(node)
}

fn has_jsonb_operator(node: &SyntaxNode) -> bool {
    node.children_with_tokens()
        .filter_map(|child| child.into_token())
        .any(|token| token.kind().is_jsonb_operator())
}

/// Checks if the node contains a #> or #>> path operator.
fn has_path_operator(node: &SyntaxNode) -> bool {
    node.children_with_tokens()
        .filter_map(|child| child.into_token())
        .any(|token| {
            matches!(
                token.kind(),
                SyntaxKind::HASH_ARROW | SyntaxKind::HASH_ARROW_TEXT
            )
        })
}

fn collect_jsonb_chain(
    node: &SyntaxNode,
    table: &mut Option<String>,
    column: &mut Option<String>,
    path: &mut Vec<String>,
) {
    // Base case: column reference - extract table and column
    if node.kind() == SyntaxKind::COLUMN_REF {
        if column.is_none() {
            let (tbl, col) = extract_qualified_column(node);
            if !col.is_empty() {
                *table = tbl;
                *column = Some(col);
            }
        }
        return;
    }

    // Must be a JSONB binary expression
    if !is_jsonb_expr(node) {
        return;
    }

    // Extract left (base) and right (accessor) children
    let children: Vec<_> = node.children().collect();
    let (lhs, rhs) = match children.as_slice() {
        [left, right] => (left, right),
        _ => return,
    };

    // Recurse into left side (base expression - either column ref or nested JSONB expr)
    collect_jsonb_chain(lhs, table, column, path);

    // Check if this is a #> or #>> operator with path array syntax
    // e.g., resource #> '{name,0,family}'
    // The parser produces BINARY_EXPR for these operators, not JSONB_PATH_EXPR
    if node.kind() == SyntaxKind::JSONB_PATH_EXPR || has_path_operator(node) {
        if let Some(path_segments) = extract_path_array_literal(rhs) {
            path.extend(path_segments);
        }
        return;
    }

    // Right side could be a literal (simple accessor) or another BINARY_EXPR (nested chain)
    if let Some(key) = extract_accessor_key(rhs) {
        path.push(key);
    } else if is_jsonb_expr(rhs) {
        // Right side is a nested JSONB chain (right-associative tree)
        // e.g., for `col->'a'->'b'->'c'`, rhs might be `'a'->'b'->'c'`
        collect_right_chain_keys(rhs, path);
    }
}

/// Extracts path segments from a PostgreSQL text array literal.
/// e.g., `'{name,0,family}'` -> ["name", "0", "family"]
/// e.g., `'{name, 0, family}'` -> ["name", "0", "family"] (with spaces)
fn extract_path_array_literal(node: &SyntaxNode) -> Option<Vec<String>> {
    // Look for a STRING token in the node (the path array is a string literal)
    for child in node.descendants_with_tokens() {
        if let Some(token) = child.as_token()
            && token.kind() == SyntaxKind::STRING
        {
            let text = token.text();
            return Some(parse_path_array_string(text));
        }
    }
    None
}

/// Parses a PostgreSQL path array string.
/// e.g., `'{name,0,family}'` -> ["name", "0", "family"]
fn parse_path_array_string(s: &str) -> Vec<String> {
    // Remove surrounding quotes and braces
    let s = s.trim();
    let s = s.trim_matches('\'');
    let s = s.trim_matches('{').trim_matches('}');

    if s.is_empty() {
        return Vec::new();
    }

    // Split by comma and trim whitespace
    s.split(',')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

/// Collects all keys from a right-associative JSONB chain.
/// For `'a'->'b'->'c'`, this collects ["a", "b", "c"].
fn collect_right_chain_keys(node: &SyntaxNode, path: &mut Vec<String>) {
    if !is_jsonb_expr(node) {
        // Base case: try to extract literal key
        if let Some(key) = extract_accessor_key(node) {
            path.push(key);
        }
        return;
    }

    // Extract children
    let children: Vec<_> = node.children().collect();
    let (lhs, rhs) = match children.as_slice() {
        [left, right] => (left, right),
        _ => return,
    };

    // Extract key from left side (the current accessor)
    if let Some(key) = extract_accessor_key(lhs) {
        path.push(key);
    }

    // Recurse into right side for remaining keys
    collect_right_chain_keys(rhs, path);
}

/// Extracts the accessor key from a JSONB access expression's right-hand side.
/// Handles both string keys (`'name'`) and integer indexes (`0`).
fn extract_accessor_key(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::LITERAL => {
            // Try string literal first
            if let Some(s) = extract_literal_string(node) {
                return Some(s);
            }
            // Try integer literal for array access
            extract_literal_integer(node)
        }
        _ => None,
    }
}

/// Extracts an integer value from a LITERAL node (for array index access).
fn extract_literal_integer(node: &SyntaxNode) -> Option<String> {
    for child in node.children_with_tokens() {
        if let Some(token) = child.as_token()
            && token.kind() == SyntaxKind::INTEGER
        {
            return Some(token.text().to_string());
        }
    }
    None
}

fn extract_literal_string(node: &SyntaxNode) -> Option<String> {
    for child in node.children_with_tokens() {
        if let Some(token) = child.as_token()
            && token.kind() == SyntaxKind::STRING
        {
            let text = token.text();
            return Some(text.trim_matches('\'').to_string());
        }
    }
    None
}

/// Detects function argument context.
fn detect_function_arg_context(node: &SyntaxNode) -> Option<CompletionContext> {
    // Find the function name
    let func_call = if node.kind() == SyntaxKind::FUNC_CALL {
        node.clone()
    } else {
        node.ancestors()
            .find(|n| n.kind() == SyntaxKind::FUNC_CALL)
            .cloned()?
    };

    let mut function_name = String::new();
    for child in func_call.children_with_tokens() {
        if let Some(token) = child.as_token()
            && token.kind() == SyntaxKind::IDENT
        {
            function_name = token.text().to_string();
            break;
        }
    }

    if function_name.is_empty() {
        return None;
    }

    // Count the argument position
    let arg_list = func_call
        .children()
        .find(|c| c.kind() == SyntaxKind::ARG_LIST)
        .cloned()?;

    let mut arg_index = 0;
    for child in arg_list.children_with_tokens() {
        if let Some(token) = child.as_token()
            && token.kind() == SyntaxKind::COMMA
        {
            arg_index += 1;
        }
    }

    Some(CompletionContext::FunctionArg {
        function: function_name,
        arg_index,
    })
}

/// Detects SELECT column context.
fn detect_select_column_context(node: &SyntaxNode) -> CompletionContext {
    // Find tables from FROM clause
    let tables = find_tables_in_scope(node);
    CompletionContext::SelectColumn { tables }
}

/// Detects table name context.
fn detect_table_name_context(token: &SyntaxToken) -> CompletionContext {
    // Check if there's a schema prefix
    let schema = find_schema_prefix(token);
    CompletionContext::TableName { schema }
}

/// Finds a schema prefix before the current token.
fn find_schema_prefix(token: &SyntaxToken) -> Option<String> {
    // Look for pattern: IDENT DOT <cursor>
    let mut prev = token.prev_token();
    while let Some(t) = prev {
        if t.kind() == SyntaxKind::DOT
            && let Some(ident) = t.prev_token()
            && (ident.kind() == SyntaxKind::IDENT || ident.kind() == SyntaxKind::QUOTED_IDENT)
        {
            return Some(ident.text().to_string());
        } else if t.kind() != SyntaxKind::DOT && !t.kind().is_trivia() {
            break;
        }
        prev = t.prev_token();
    }
    None
}

/// Detects WHERE clause context.
fn detect_where_context(node: &SyntaxNode) -> CompletionContext {
    let tables = find_tables_in_scope(node);
    CompletionContext::WhereCondition { tables }
}

/// Detects JOIN condition context.
fn detect_join_condition_context(node: &SyntaxNode) -> CompletionContext {
    // Find the JOIN_EXPR and extract table names
    let (left_table, right_table) = find_join_tables(node);
    CompletionContext::JoinCondition {
        left_table,
        right_table,
    }
}

/// Finds the tables involved in a JOIN.
fn find_join_tables(node: &SyntaxNode) -> (String, String) {
    let join_expr = node.ancestors().find(|n| n.kind() == SyntaxKind::JOIN_EXPR);

    if let Some(join) = join_expr {
        let mut tables = Vec::new();
        for child in join.children() {
            if (child.kind() == SyntaxKind::TABLE_REF || child.kind() == SyntaxKind::TABLE_NAME)
                && let Some(name) = extract_table_name(&child)
            {
                tables.push(name);
            }
        }
        if tables.len() >= 2 {
            return (tables[0].clone(), tables[1].clone());
        }
    }

    (String::new(), String::new())
}

/// Extracts a table name from a TABLE_REF or TABLE_NAME node.
fn extract_table_name(node: &SyntaxNode) -> Option<String> {
    if node.kind() == SyntaxKind::TABLE_NAME {
        return extract_table_name_tokens(node);
    }

    if node.kind() == SyntaxKind::TABLE_REF {
        return extract_table_ref_parts(node).0;
    }

    // Prefer the actual table name over aliases for scope resolution.
    for token in node.children_with_tokens().filter_map(|c| c.into_token()) {
        if token.kind() == SyntaxKind::IDENT || token.kind() == SyntaxKind::QUOTED_IDENT {
            return Some(token.text().to_string());
        }
    }

    // Fall back to alias if no table name was found.
    for child in node.children() {
        if child.kind() == SyntaxKind::ALIAS {
            for token in child.children_with_tokens().filter_map(|c| c.into_token()) {
                if token.kind() == SyntaxKind::IDENT || token.kind() == SyntaxKind::QUOTED_IDENT {
                    return Some(token.text().to_string());
                }
            }
        }
    }
    None
}

fn extract_table_ref_parts(node: &SyntaxNode) -> (Option<String>, Option<String>) {
    let mut name = String::new();
    let mut alias = None;
    let mut saw_name = false;
    let mut in_alias = false;

    for element in node.children_with_tokens() {
        if let Some(token) = element.as_token() {
            match token.kind() {
                SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => {
                    if in_alias {
                        alias = Some(token.text().to_string());
                        break;
                    }
                    saw_name = true;
                    name.push_str(token.text());
                }
                SyntaxKind::DOT if !in_alias => {
                    saw_name = true;
                    name.push('.');
                }
                SyntaxKind::AS_KW if saw_name => {
                    in_alias = true;
                }
                _ if token.kind().is_trivia() && saw_name => {
                    in_alias = true;
                }
                _ => {}
            }
        }
    }

    let name = if name.is_empty() { None } else { Some(name) };
    (name, alias)
}

/// Detects ORDER BY context.
fn detect_order_by_context(node: &SyntaxNode) -> CompletionContext {
    let tables = find_tables_in_scope(node);
    CompletionContext::ColumnRef {
        table: None,
        tables,
    }
}

/// Detects column reference context.
fn detect_column_ref_context(token: &SyntaxToken, node: &SyntaxNode) -> CompletionContext {
    // Check if there's a table qualifier
    let table = find_table_qualifier(token, node);
    let tables = find_tables_in_scope(node);

    CompletionContext::ColumnRef { table, tables }
}

/// Finds a table qualifier before the current position.
fn find_table_qualifier(token: &SyntaxToken, node: &SyntaxNode) -> Option<String> {
    let qualifier = extract_qualifier_before_dot(token)?;
    Some(resolve_table_alias(node, &qualifier))
}

fn extract_qualifier_before_dot(token: &SyntaxToken) -> Option<String> {
    let dot_token = if token.kind() == SyntaxKind::DOT {
        token.clone()
    } else {
        let prev = token.prev_token()?.clone();
        if prev.kind() != SyntaxKind::DOT {
            return None;
        }
        prev
    };

    let mut tokens = Vec::new();
    let mut cursor = dot_token.prev_token().cloned();
    while let Some(t) = cursor {
        match t.kind() {
            SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT | SyntaxKind::DOT => {
                tokens.push(t.text().to_string());
            }
            _ if t.kind().is_trivia() => {}
            _ => break,
        }
        cursor = t.prev_token().cloned();
    }

    if tokens.is_empty() {
        return None;
    }

    tokens.reverse();
    Some(tokens.concat())
}

fn resolve_table_alias(node: &SyntaxNode, table: &str) -> String {
    if table.contains('.') {
        return table.to_string();
    }

    let statement = nearest_statement_scope(node);

    let Some(statement) = statement else {
        return table.to_string();
    };

    let alias_map = collect_aliases(&statement);
    alias_map
        .get(table)
        .cloned()
        .unwrap_or_else(|| table.to_string())
}

fn nearest_statement_scope(node: &SyntaxNode) -> Option<SyntaxNode> {
    node.ancestors()
        .find(|n| {
            matches!(
                n.kind(),
                SyntaxKind::SELECT_STMT
                    | SyntaxKind::UPDATE_STMT
                    | SyntaxKind::DELETE_STMT
                    | SyntaxKind::INSERT_STMT
            )
        })
        .cloned()
}

fn statement_ancestors(node: &SyntaxNode) -> Vec<SyntaxNode> {
    node.ancestors()
        .filter(|n| {
            matches!(
                n.kind(),
                SyntaxKind::SELECT_STMT
                    | SyntaxKind::UPDATE_STMT
                    | SyntaxKind::DELETE_STMT
                    | SyntaxKind::INSERT_STMT
            )
        })
        .cloned()
        .collect()
}

fn collect_aliases(statement: &SyntaxNode) -> HashMap<String, String> {
    let mut map = HashMap::new();

    if let (Some(table_name), Some(alias)) = extract_statement_target_parts(statement) {
        map.insert(alias, table_name);
    }

    for table_ref in scoped_table_refs(statement) {
        let (table_name, alias) = extract_table_ref_parts(&table_ref);
        if let (Some(alias), Some(table_name)) = (alias, table_name) {
            map.insert(alias, table_name);
        }
    }

    map
}

fn extract_table_name_tokens(node: &SyntaxNode) -> Option<String> {
    let mut name = String::new();
    for token in node
        .children_with_tokens()
        .filter_map(|child| child.into_token())
    {
        match token.kind() {
            SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => {
                name.push_str(token.text());
            }
            SyntaxKind::DOT => {
                name.push('.');
            }
            _ => {}
        }
    }

    if name.is_empty() { None } else { Some(name) }
}

/// Finds all tables in scope for the given node.
fn find_tables_in_scope(node: &SyntaxNode) -> Vec<String> {
    let mut tables = Vec::new();

    if let Some(statement) = nearest_statement_scope(node) {
        for table_ref in scoped_table_refs(&statement) {
            if let Some(name) = extract_table_name(&table_ref)
                && !tables.contains(&name)
            {
                tables.push(name);
            }
        }
    }

    for statement in statement_ancestors(node) {
        for child in statement.children() {
            if child.kind() == SyntaxKind::WITH_CLAUSE {
                collect_ctes(&child, &mut tables);
            }
        }
    }

    tables
}

fn scoped_table_refs(statement: &SyntaxNode) -> Vec<SyntaxNode> {
    match statement.kind() {
        SyntaxKind::SELECT_STMT => direct_clause_table_refs(statement, &[SyntaxKind::FROM_CLAUSE]),
        SyntaxKind::UPDATE_STMT => {
            let mut refs = statement_target_table_ref(statement);
            refs.extend(direct_clause_table_refs(
                statement,
                &[SyntaxKind::FROM_CLAUSE],
            ));
            refs
        }
        SyntaxKind::DELETE_STMT => {
            let mut refs = statement_target_table_ref(statement);
            refs.extend(direct_clause_table_refs(
                statement,
                &[SyntaxKind::USING_CLAUSE],
            ));
            refs
        }
        SyntaxKind::INSERT_STMT => statement_target_table_ref(statement),
        _ => Vec::new(),
    }
}

fn statement_target_table_ref(statement: &SyntaxNode) -> Vec<SyntaxNode> {
    statement
        .children()
        .filter(|child| child.kind() == SyntaxKind::TABLE_REF)
        .take(1)
        .cloned()
        .collect()
}

fn direct_clause_table_refs(
    statement: &SyntaxNode,
    clause_kinds: &[SyntaxKind],
) -> Vec<SyntaxNode> {
    let mut refs = Vec::new();

    for child in statement.children() {
        if clause_kinds.contains(&child.kind()) {
            refs.extend(
                child
                    .descendants()
                    .filter(|n| n.kind() == SyntaxKind::TABLE_REF)
                    .cloned(),
            );
        }
    }

    refs
}

fn extract_statement_target_parts(statement: &SyntaxNode) -> (Option<String>, Option<String>) {
    let Some(target) = statement_target_table_ref(statement).into_iter().next() else {
        return (None, None);
    };

    let table_name = extract_table_name(&target);
    let mut after_target = false;
    let mut saw_as = false;

    for element in statement.children_with_tokens() {
        if let Some(node) = element.as_node() {
            if !after_target {
                after_target =
                    node.kind() == target.kind() && node.text_range() == target.text_range();
                continue;
            }

            if matches!(
                node.kind(),
                SyntaxKind::SET_CLAUSE
                    | SyntaxKind::FROM_CLAUSE
                    | SyntaxKind::USING_CLAUSE
                    | SyntaxKind::WHERE_CLAUSE
                    | SyntaxKind::RETURNING_CLAUSE
                    | SyntaxKind::INSERT_COLUMNS
                    | SyntaxKind::VALUES_CLAUSE
                    | SyntaxKind::SELECT_STMT
                    | SyntaxKind::ON_CONFLICT_CLAUSE
            ) {
                break;
            }

            continue;
        }

        let Some(token) = element.as_token() else {
            continue;
        };

        if !after_target || token.kind().is_trivia() {
            continue;
        }

        match token.kind() {
            SyntaxKind::AS_KW => saw_as = true,
            SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => {
                if saw_as || can_have_implicit_target_alias(statement, token.kind()) {
                    return (table_name, Some(token.text().to_string()));
                }
                break;
            }
            _ if is_target_alias_stop_token(token.kind()) => break,
            _ => break,
        }
    }

    (table_name, None)
}

fn can_have_implicit_target_alias(statement: &SyntaxNode, kind: SyntaxKind) -> bool {
    matches!(
        statement.kind(),
        SyntaxKind::UPDATE_STMT | SyntaxKind::DELETE_STMT | SyntaxKind::INSERT_STMT
    ) && matches!(kind, SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT)
}

fn is_target_alias_stop_token(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::L_PAREN
            | SyntaxKind::VALUES_KW
            | SyntaxKind::SELECT_KW
            | SyntaxKind::DEFAULT_KW
            | SyntaxKind::SET_KW
            | SyntaxKind::FROM_KW
            | SyntaxKind::USING_KW
            | SyntaxKind::WHERE_KW
            | SyntaxKind::RETURNING_KW
            | SyntaxKind::ON_KW
    )
}

/// Collects CTE names from a WITH clause.
fn collect_ctes(with_clause: &SyntaxNode, tables: &mut Vec<String>) {
    for cte_info in collect_cte_info(with_clause) {
        if !tables.contains(&cte_info.name) {
            tables.push(cte_info.name);
        }
    }
}

/// Information about a Common Table Expression.
#[derive(Clone, Debug)]
pub struct CteInfo {
    /// The CTE name.
    pub name: String,
    /// The columns exposed by this CTE (explicit or inferred).
    pub columns: Vec<String>,
}

/// Collects detailed CTE information including columns from a WITH clause.
pub fn collect_cte_info(with_clause: &SyntaxNode) -> Vec<CteInfo> {
    let mut ctes = Vec::new();

    for child in with_clause.descendants() {
        if child.kind() == SyntaxKind::CTE
            && let Some(info) = extract_cte_info(&child)
        {
            ctes.push(info);
        }
    }

    ctes
}

/// Extracts CTE name and columns from a CTE node.
fn extract_cte_info(cte: &SyntaxNode) -> Option<CteInfo> {
    let mut name = None;
    let mut explicit_columns = Vec::new();
    let mut in_column_list = false;
    let mut found_as = false;

    // Parse the CTE structure: name (col1, col2, ...) AS (SELECT ...)
    for child in cte.children_with_tokens() {
        if let Some(token) = child.as_token() {
            match token.kind() {
                SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => {
                    if name.is_none() {
                        // First IDENT is the CTE name
                        name = Some(token.text().to_string());
                    } else if in_column_list && !found_as {
                        // IDENT inside column list
                        explicit_columns.push(token.text().to_string());
                    }
                }
                SyntaxKind::L_PAREN if name.is_some() && !found_as => {
                    in_column_list = true;
                }
                SyntaxKind::R_PAREN if in_column_list && !found_as => {
                    in_column_list = false;
                }
                SyntaxKind::AS_KW => {
                    found_as = true;
                    in_column_list = false;
                }
                _ => {}
            }
        } else if let Some(node) = child.as_node()
            && found_as
            && explicit_columns.is_empty()
        {
            explicit_columns = infer_cte_output_columns(&node);
        }
    }

    name.map(|n| CteInfo {
        name: n,
        columns: explicit_columns,
    })
}

fn infer_cte_output_columns(statement: &SyntaxNode) -> Vec<String> {
    match statement.kind() {
        SyntaxKind::SELECT_STMT => extract_select_output_columns(statement),
        SyntaxKind::INSERT_STMT | SyntaxKind::UPDATE_STMT | SyntaxKind::DELETE_STMT => {
            extract_returning_output_columns(statement)
        }
        _ => Vec::new(),
    }
}

/// Extracts output column names from a SELECT statement.
fn extract_select_output_columns(select: &SyntaxNode) -> Vec<String> {
    let item_list = select
        .children()
        .find(|c| c.kind() == SyntaxKind::SELECT_ITEM_LIST)
        .or_else(|| {
            select
                .children()
                .find(|c| c.kind() == SyntaxKind::SELECT_CLAUSE)
                .and_then(|clause| {
                    clause
                        .children()
                        .find(|child| child.kind() == SyntaxKind::SELECT_ITEM_LIST)
                })
        });

    let Some(item_list) = item_list else {
        return Vec::new();
    };

    item_list
        .children()
        .filter(|item| item.kind() == SyntaxKind::SELECT_ITEM)
        .filter_map(extract_select_item_name)
        .collect()
}

fn extract_returning_output_columns(statement: &SyntaxNode) -> Vec<String> {
    let Some(returning_clause) = statement
        .children()
        .find(|c| c.kind() == SyntaxKind::RETURNING_CLAUSE)
    else {
        return Vec::new();
    };

    let direct_items: Vec<_> = returning_clause
        .children()
        .filter(|child| child.kind() == SyntaxKind::SELECT_ITEM)
        .filter_map(extract_select_item_name)
        .collect();

    if !direct_items.is_empty() {
        return direct_items;
    }

    returning_clause
        .children()
        .filter_map(extract_select_item_name)
        .collect()
}

/// Extracts the output name from a SELECT item.
/// Returns the alias if present, otherwise the column name.
fn extract_select_item_name(item: &SyntaxNode) -> Option<String> {
    // Check for explicit alias: expr AS name
    for child in item.children() {
        if child.kind() == SyntaxKind::ALIAS {
            for token in child.children_with_tokens().filter_map(|c| c.into_token()) {
                if token.kind() == SyntaxKind::IDENT || token.kind() == SyntaxKind::QUOTED_IDENT {
                    return Some(token.text().to_string());
                }
            }
        }
    }

    // No alias - try to extract column name from expression
    // Look for COLUMN_REF first
    for child in item.descendants() {
        if child.kind() == SyntaxKind::COLUMN_REF {
            // Return the last IDENT (column name, not table qualifier)
            let mut last_ident = None;
            for token in child.children_with_tokens().filter_map(|c| c.into_token()) {
                if token.kind() == SyntaxKind::IDENT || token.kind() == SyntaxKind::QUOTED_IDENT {
                    last_ident = Some(token.text().to_string());
                }
            }
            if last_ident.is_some() {
                return last_ident;
            }
        }
    }

    // Fallback: look for any IDENT in the item
    for token in item
        .descendants_with_tokens()
        .filter_map(|c| c.into_token())
    {
        if matches!(token.kind(), SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT) {
            return Some(token.text().to_string());
        }
    }

    None
}

/// Finds CTE columns for a given CTE name in scope.
pub fn find_cte_columns(node: &SyntaxNode, cte_name: &str) -> Vec<String> {
    for statement in statement_ancestors(node) {
        for child in statement.children() {
            if child.kind() == SyntaxKind::WITH_CLAUSE {
                for cte_info in collect_cte_info(&child) {
                    if cte_info.name == cte_name {
                        return cte_info.columns;
                    }
                }
            }
        }
    }

    Vec::new()
}

/// Finds CTE columns for the statement scope at the given offset.
pub fn find_cte_columns_at_offset(parse: &Parse, offset: TextSize, cte_name: &str) -> Vec<String> {
    let root = parse.syntax();
    let node = find_node_at_offset(&root, offset)
        .or_else(|| find_token_at_offset(&root, offset).map(|token| token.parent().clone()))
        .unwrap_or(root);
    find_cte_columns(&node, cte_name)
}

/// Detects context from token alone.
fn detect_context_from_token(token: &SyntaxToken, offset: TextSize) -> CompletionContext {
    detect_from_token(token, offset)
}

/// Detects context from node alone.
fn detect_context_from_node(node: &SyntaxNode, _offset: TextSize) -> CompletionContext {
    match node.kind() {
        SyntaxKind::SELECT_STMT => CompletionContext::Statement,
        SyntaxKind::SELECT_CLAUSE => detect_select_column_context(node),
        SyntaxKind::FROM_CLAUSE => CompletionContext::TableName { schema: None },
        SyntaxKind::WHERE_CLAUSE => detect_where_context(node),
        _ => CompletionContext::Unknown,
    }
}

/// Detects context based on the token at cursor.
fn detect_from_token(token: &SyntaxToken, offset: TextSize) -> CompletionContext {
    let kind = token.kind();
    let range = token.text_range();

    // If we're at the end of a token, consider what comes next
    let at_token_end = range.end() == offset;

    if matches!(kind, SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT)
        && at_token_end
        && let Some(ctx) = detect_table_context_from_token(token)
    {
        return ctx;
    }

    // Check for incomplete keyword
    if matches!(kind, SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT) && at_token_end {
        // Could be a partial keyword
        return CompletionContext::Keyword {
            expected: get_expected_keywords(token),
        };
    }

    match kind {
        SyntaxKind::SELECT_KW => CompletionContext::SelectColumn {
            tables: find_tables_in_scope(token.parent()),
        },
        SyntaxKind::FROM_KW | SyntaxKind::UPDATE_KW | SyntaxKind::INTO_KW => {
            CompletionContext::TableName { schema: None }
        }
        // `UPDATE t SET |` assigns columns of the target table
        SyntaxKind::SET_KW => CompletionContext::ColumnRef {
            table: None,
            tables: find_tables_in_scope(token.parent()),
        },
        SyntaxKind::WHERE_KW | SyntaxKind::AND_KW | SyntaxKind::OR_KW => {
            CompletionContext::WhereCondition {
                tables: find_tables_in_scope(token.parent()),
            }
        }
        SyntaxKind::JOIN_KW
        | SyntaxKind::LEFT_KW
        | SyntaxKind::RIGHT_KW
        | SyntaxKind::INNER_KW
        | SyntaxKind::OUTER_KW => CompletionContext::TableName { schema: None },
        SyntaxKind::ON_KW => CompletionContext::JoinCondition {
            left_table: String::new(),
            right_table: String::new(),
        },
        SyntaxKind::ORDER_KW => CompletionContext::Keyword {
            expected: vec!["BY".to_string()],
        },
        SyntaxKind::GROUP_KW => CompletionContext::Keyword {
            expected: vec!["BY".to_string()],
        },
        SyntaxKind::DOT => {
            // After a dot, we need column completion
            let parent = token.parent();
            if let Some(table) = find_table_qualifier(token, parent) {
                CompletionContext::ColumnRef {
                    table: Some(table),
                    tables: find_tables_in_scope(parent),
                }
            } else if let Some(table) = find_table_qualifier_from_token(token) {
                CompletionContext::ColumnRef {
                    table: Some(table),
                    tables: Vec::new(),
                }
            } else {
                CompletionContext::Unknown
            }
        }
        SyntaxKind::ARROW | SyntaxKind::ARROW_TEXT => CompletionContext::JsonbField {
            table: None,
            column: String::new(),
            path: Vec::new(),
        },
        _ if kind.is_keyword() => {
            // After a keyword, suggest what typically follows
            CompletionContext::Keyword {
                expected: get_keywords_after(kind),
            }
        }
        _ => CompletionContext::Unknown,
    }
}

fn detect_table_context_from_token(token: &SyntaxToken) -> Option<CompletionContext> {
    let mut prev = token.prev_token();
    while let Some(t) = prev {
        if t.kind().is_trivia() {
            prev = t.prev_token();
            continue;
        }

        if matches!(
            t.kind(),
            SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT | SyntaxKind::DOT
        ) {
            prev = t.prev_token();
            continue;
        }

        let is_table_kw = matches!(
            t.kind(),
            SyntaxKind::FROM_KW | SyntaxKind::JOIN_KW | SyntaxKind::UPDATE_KW | SyntaxKind::INTO_KW
        );
        if is_table_kw {
            return Some(CompletionContext::TableName {
                schema: find_schema_prefix(token),
            });
        }
        break;
    }

    None
}

fn find_table_qualifier_from_token(token: &SyntaxToken) -> Option<String> {
    extract_qualifier_before_dot(token)
}

/// Detects statement start context.
fn detect_statement_start(root: &SyntaxNode, _offset: TextSize) -> CompletionContext {
    // At the start or in an empty file
    if root.children().count() == 0 {
        return CompletionContext::Statement;
    }

    CompletionContext::Statement
}

/// Gets expected keywords based on context.
fn get_expected_keywords(token: &SyntaxToken) -> Vec<String> {
    let text = token.text().to_uppercase();

    // Suggest keywords that start with the current text
    let keywords = [
        "SELECT",
        "FROM",
        "WHERE",
        "AND",
        "OR",
        "JOIN",
        "LEFT",
        "RIGHT",
        "INNER",
        "OUTER",
        "ON",
        "ORDER",
        "BY",
        "GROUP",
        "HAVING",
        "LIMIT",
        "OFFSET",
        "INSERT",
        "UPDATE",
        "DELETE",
        "INTO",
        "VALUES",
        "SET",
        "CREATE",
        "DROP",
        "ALTER",
        "TABLE",
        "INDEX",
        "VIEW",
        "WITH",
        "AS",
        "DISTINCT",
        "ALL",
        "UNION",
        "INTERSECT",
        "EXCEPT",
        "CASE",
        "WHEN",
        "THEN",
        "ELSE",
        "END",
        "NULL",
        "NOT",
        "IN",
        "EXISTS",
        "BETWEEN",
        "LIKE",
        "ILIKE",
        "IS",
        "TRUE",
        "FALSE",
        "ASC",
        "DESC",
        "NULLS",
        "FIRST",
        "LAST",
        "RETURNING",
        "CASCADE",
        "RESTRICT",
    ];

    keywords
        .iter()
        .filter(|kw| kw.starts_with(&text))
        .map(|s| s.to_string())
        .collect()
}

/// Checks if we're inside an OVER clause for window functions.
fn is_in_over_clause(node: &SyntaxNode) -> bool {
    node.ancestors()
        .any(|n| n.kind() == SyntaxKind::OVER_CLAUSE)
}

/// Detects window function context.
fn detect_window_context(token: &SyntaxToken, node: &SyntaxNode) -> CompletionContext {
    let tables = find_tables_in_scope(node);
    let token_kind = token.kind();

    // Check for frame specification keywords
    if matches!(
        token_kind,
        SyntaxKind::ROWS_KW
            | SyntaxKind::RANGE_KW
            | SyntaxKind::GROUPS_KW
            | SyntaxKind::BETWEEN_KW
            | SyntaxKind::UNBOUNDED_KW
            | SyntaxKind::PRECEDING_KW
            | SyntaxKind::FOLLOWING_KW
            | SyntaxKind::CURRENT_KW
    ) {
        return CompletionContext::WindowFrame;
    }

    // Check for sort direction context
    if is_after_order_by_column(token, node) {
        return CompletionContext::WindowOrderDirection;
    }

    // Check if we're after PARTITION BY or ORDER BY
    if is_after_partition_or_order_by(token) {
        return CompletionContext::WindowPartition { tables };
    }

    // Default: inside OVER clause
    CompletionContext::WindowClause { tables }
}

/// Checks if we're after an ORDER BY column (expecting ASC/DESC).
fn is_after_order_by_column(token: &SyntaxToken, node: &SyntaxNode) -> bool {
    // Look for ORDER_BY_ITEM ancestor
    let has_order_item = node
        .ancestors()
        .any(|n| n.kind() == SyntaxKind::ORDER_BY_ITEM);

    if !has_order_item {
        return false;
    }

    // Check if we're after an identifier (column name)
    let mut prev = token.prev_token();
    while let Some(t) = prev {
        if t.kind().is_trivia() {
            prev = t.prev_token();
            continue;
        }
        // After an identifier, we're likely expecting sort direction
        if matches!(t.kind(), SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT) {
            return true;
        }
        break;
    }
    false
}

/// Checks if we're after PARTITION BY or ORDER BY keywords.
fn is_after_partition_or_order_by(token: &SyntaxToken) -> bool {
    let mut prev = token.prev_token();
    while let Some(t) = prev {
        if t.kind().is_trivia() {
            prev = t.prev_token();
            continue;
        }
        if t.kind() == SyntaxKind::BY_KW {
            // Check what's before BY
            let mut before_by = t.prev_token();
            while let Some(bt) = before_by {
                if bt.kind().is_trivia() {
                    before_by = bt.prev_token();
                    continue;
                }
                return matches!(bt.kind(), SyntaxKind::PARTITION_KW | SyntaxKind::ORDER_KW);
            }
        }
        break;
    }
    false
}

/// Gets keywords that typically follow a given keyword.
fn get_keywords_after(kind: SyntaxKind) -> Vec<String> {
    match kind {
        SyntaxKind::SELECT_KW => vec!["DISTINCT".to_string(), "ALL".to_string(), "*".to_string()],
        SyntaxKind::FROM_KW => vec![],
        SyntaxKind::WHERE_KW => vec!["NOT".to_string(), "EXISTS".to_string()],
        SyntaxKind::ORDER_KW => vec!["BY".to_string()],
        SyntaxKind::GROUP_KW => vec!["BY".to_string()],
        SyntaxKind::INSERT_KW => vec!["INTO".to_string()],
        SyntaxKind::UPDATE_KW => vec![],
        SyntaxKind::DELETE_KW => vec!["FROM".to_string()],
        SyntaxKind::CREATE_KW => vec![
            "TABLE".to_string(),
            "INDEX".to_string(),
            "VIEW".to_string(),
            "MATERIALIZED".to_string(),
        ],
        SyntaxKind::DROP_KW => vec!["TABLE".to_string(), "INDEX".to_string(), "VIEW".to_string()],
        SyntaxKind::LEFT_KW | SyntaxKind::RIGHT_KW | SyntaxKind::FULL_KW => {
            vec!["OUTER".to_string(), "JOIN".to_string()]
        }
        SyntaxKind::INNER_KW | SyntaxKind::CROSS_KW | SyntaxKind::NATURAL_KW => {
            vec!["JOIN".to_string()]
        }
        SyntaxKind::OUTER_KW => vec!["JOIN".to_string()],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_types() {
        // Basic type checks
        let ctx = CompletionContext::Statement;
        assert!(matches!(ctx, CompletionContext::Statement));

        let ctx = CompletionContext::SelectColumn {
            tables: vec!["users".to_string()],
        };
        if let CompletionContext::SelectColumn { tables } = ctx {
            assert_eq!(tables.len(), 1);
        }
    }

    #[test]
    fn test_extract_column_name() {
        // Would need a real syntax node to test
    }

    #[test]
    fn test_get_expected_keywords() {
        // Create a mock token scenario
        // For now, just test the keyword list function
        let keywords = get_keywords_after(SyntaxKind::SELECT_KW);
        assert!(keywords.contains(&"DISTINCT".to_string()));
    }

    #[test]
    fn test_context_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CompletionContext>();
    }

    #[test]
    fn test_parse_path_array_string() {
        // Standard path array
        assert_eq!(
            parse_path_array_string("'{name,0,family}'"),
            vec!["name", "0", "family"]
        );

        // With spaces
        assert_eq!(
            parse_path_array_string("'{name, 0, family}'"),
            vec!["name", "0", "family"]
        );

        // Single element
        assert_eq!(parse_path_array_string("'{name}'"), vec!["name"]);

        // Empty array
        assert_eq!(parse_path_array_string("'{}'"), Vec::<String>::new());

        // Without quotes (edge case)
        assert_eq!(
            parse_path_array_string("{name,given}"),
            vec!["name", "given"]
        );
    }

    #[test]
    fn test_detect_context_resolves_join_alias() {
        let source = "SELECT u. FROM users u JOIN orders o ON u.id = o.user_id";
        let offset = source.find(" FROM").unwrap() as u32;
        let parse = banshee_parser::parse(source);
        let context = detect_context(&parse, TextSize::new(offset));

        assert!(matches!(
            context,
            CompletionContext::ColumnRef {
                table: Some(ref table),
                ref tables,
            } if table == "users" && tables == &vec!["users".to_string(), "orders".to_string()]
        ));
    }

    #[test]
    fn test_detect_context_schema_qualified_table_scope() {
        let source = "SELECT public.users. FROM public.users";
        let offset = source.find(" FROM").unwrap() as u32;
        let parse = banshee_parser::parse(source);
        let context = detect_context(&parse, TextSize::new(offset));

        assert!(matches!(
            context,
            CompletionContext::ColumnRef {
                table: Some(ref table),
                ref tables,
            } if table == "public.users" && tables == &vec!["public.users".to_string()]
        ));
    }

    #[test]
    fn test_detect_context_update_from_alias_scope() {
        let source = "UPDATE users u SET name = o. FROM orders o WHERE u.id = o.user_id";
        let offset = source.find(" FROM").unwrap() as u32;
        let parse = banshee_parser::parse(source);
        let context = detect_context(&parse, TextSize::new(offset));

        assert!(matches!(
            context,
            CompletionContext::ColumnRef {
                table: Some(ref table),
                ref tables,
            } if table == "orders" && tables == &vec!["users".to_string(), "orders".to_string()]
        ));
    }

    #[test]
    fn test_detect_context_delete_using_alias_scope() {
        let source = "DELETE FROM users u USING orders o WHERE o.";
        let offset = source.len() as u32;
        let parse = banshee_parser::parse(source);
        let context = detect_context(&parse, TextSize::new(offset));

        assert!(matches!(
            context,
            CompletionContext::ColumnRef {
                table: Some(ref table),
                ref tables,
            } if table == "orders" && tables == &vec!["users".to_string(), "orders".to_string()]
        ));
    }

    #[test]
    fn test_detect_context_insert_returning_alias_scope() {
        let source = "INSERT INTO users u (name) VALUES ('alice') RETURNING u.";
        let offset = source.len() as u32;
        let parse = banshee_parser::parse(source);
        let context = detect_context(&parse, TextSize::new(offset));

        assert!(matches!(
            context,
            CompletionContext::ColumnRef {
                table: Some(ref table),
                ref tables,
            } if table == "users" && tables == &vec!["users".to_string()]
        ));
    }
}
