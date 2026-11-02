//! SQL parser producing a lossless CST.
//!
//! The parser builds a concrete syntax tree with error recovery. Consumers
//! can inspect `Parse::errors()` while still traversing the tree.
//!
//! # Stability
//!
//! Pre-1.0. The intended public surface is the `parse` entry point; the
//! grammar internals are not a stable API.
//!
//! # Usage
//!
//! ```ignore
//! let parse = banshee_parser::parse("SELECT * FROM users");
//! assert!(parse.errors().is_empty());
//! let root = parse.syntax();
//! ```

mod event;
mod grammar;
mod parser;
mod sink;
mod token_set;

pub use banshee_syntax::SyntaxKind;
pub use token_set::TokenSet;

// Re-export JSONPath parser for standalone JSONPath analysis
pub use grammar::jsonpath::{JpParse, JpParseError, parse_jsonpath};
pub use grammar::jsonpath_lexer::{JpToken, tokenize_jsonpath};

use banshee_lexer::tokenize;
use banshee_syntax::Parse;

pub fn parse(source: &str) -> Parse {
    let tokens = tokenize(source);
    let mut parser = parser::Parser::new(&tokens, source);
    grammar::root(&mut parser);
    let events = parser.finish();
    let (green, errors, interner) = sink::Sink::new(&tokens, source, events).finish();
    Parse::new(green, errors, interner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_fixture(path: &std::path::Path) -> String {
        std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("failed to read fixture {}: {err}", path.display()))
    }

    fn fixture_dir(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("fixtures")
            .join(name)
    }

    fn format_tree(source: &str) -> String {
        let parse = parse(source);
        let node = parse.syntax();
        format_node(&node, 0)
    }

    fn format_node(node: &banshee_syntax::SyntaxNode, indent: usize) -> String {
        use cstree::util::NodeOrToken;
        let mut result = String::new();
        let prefix = "  ".repeat(indent);

        result.push_str(&format!(
            "{}{:?}@{:?}\n",
            prefix,
            node.kind(),
            node.text_range()
        ));

        for child in node.children_with_tokens() {
            match child {
                NodeOrToken::Node(n) => {
                    result.push_str(&format_node(n, indent + 1));
                }
                NodeOrToken::Token(t) => {
                    result.push_str(&format!(
                        "{}  {:?}@{:?} {:?}\n",
                        prefix,
                        t.kind(),
                        t.text_range(),
                        t.text()
                    ));
                }
            }
        }

        result
    }

    #[test]
    fn test_simple_select() {
        let sql = "SELECT id, name FROM users";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_select_where() {
        let sql = "SELECT * FROM users WHERE active = true";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_select_join() {
        let sql = "SELECT u.id, p.title FROM users u JOIN posts p ON u.id = p.user_id";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_jsonb_access() {
        let sql = "SELECT data->'name'->>'first' FROM users";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_jsonb_access_array_index() {
        let sql = "SELECT patient.resource->'name'->0->'family' FROM patient";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_complex_expression() {
        let sql = "SELECT (a + b) * c, CASE WHEN x THEN y ELSE z END FROM t";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_aggregate_functions() {
        let sql = "SELECT count(*), sum(amount) FROM orders";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_aggregate_order_by() {
        let sql = "SELECT array_agg(name ORDER BY id) FROM users";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_window_function() {
        let sql = "SELECT row_number() OVER (PARTITION BY dept ORDER BY salary DESC) FROM emp";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_window_frame() {
        let sql = "SELECT SUM(x) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM t";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_window_frame_exclude_current_row() {
        let sql = "SELECT SUM(x) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW EXCLUDE CURRENT ROW) FROM t";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_window_frame_exclude_group() {
        let sql = "SELECT SUM(x) OVER (ORDER BY id ROWS BETWEEN 1 PRECEDING AND 1 FOLLOWING EXCLUDE GROUP) FROM t";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_window_frame_exclude_ties() {
        let sql = "SELECT SUM(x) OVER (ORDER BY id RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW EXCLUDE TIES) FROM t";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_window_frame_exclude_no_others() {
        let sql = "SELECT SUM(x) OVER (ORDER BY id GROUPS BETWEEN 1 PRECEDING AND 1 FOLLOWING EXCLUDE NO OTHERS) FROM t";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_table_function() {
        let sql = "SELECT * FROM generate_series(1, 10) AS s(n)";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_cte() {
        let sql = "WITH cte AS (SELECT id FROM users) SELECT * FROM cte";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_subquery() {
        let sql = "SELECT * FROM (SELECT id FROM users) sub WHERE id > 10";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_jsonb_contains() {
        let sql = "SELECT * FROM users WHERE data @> '{\"active\": true}'";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_cast_expression() {
        let sql = "SELECT id::text, CAST(amount AS numeric(10, 2)) FROM t";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn typed_literals_are_casts_of_the_string() {
        for sql in [
            "SELECT DATE '2024-01-01'",
            "SELECT timestamp '2026-01-02 03:04:05' AS at FROM t",
            "SELECT TIMESTAMP WITH TIME ZONE '2026-01-02 03:04:05+02'",
            "SELECT time without time zone '10:00'",
            "SELECT INTERVAL '1' DAY, interval '1:30' hour to minute, interval '2 days'",
            "SELECT double precision '1.5', character varying 'x', int4 '7', jsonb '{}'",
            "SELECT * FROM t WHERE d >= date '2024-01-01' AND ts < now() - interval '7 days'",
        ] {
            let parse = parse(sql);
            assert!(parse.errors().is_empty(), "{sql}: {:?}", parse.errors());
            let tree = format_tree(sql);
            assert!(
                tree.contains("CAST_EXPR") && tree.contains("TYPE_NAME"),
                "{sql}\n{tree}"
            );
            assert!(!tree.contains("ERROR"), "{sql}\n{tree}");
        }
        // `name 'string'` is PostgreSQL's `func_name Sconst`: syntactically a
        // typed literal whatever the name; the type is checked when it runs.
        assert!(parse("SELECT a 'x' FROM t").errors().is_empty());
    }

    #[test]
    fn test_coalesce_nullif() {
        let sql = "SELECT COALESCE(a, b, c), NULLIF(x, y) FROM t";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_filter_clause() {
        let sql = "SELECT count(*) FILTER (WHERE active) FROM users";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_jsonpath_exists() {
        let sql = "SELECT * FROM users WHERE data @? '$.name'";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_jsonpath_match() {
        let sql = "SELECT * FROM users WHERE data @@ '$.active == true'";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    // =========================================================================
    // INSERT statement tests
    // =========================================================================

    #[test]
    fn test_insert_values() {
        let sql = "INSERT INTO users (id, name) VALUES (1, 'Alice')";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_insert_multiple_rows() {
        let sql = "INSERT INTO users (id, name) VALUES (1, 'Alice'), (2, 'Bob')";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_insert_select() {
        let sql = "INSERT INTO archive SELECT * FROM users WHERE deleted = true";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_insert_default_values() {
        let sql = "INSERT INTO counters DEFAULT VALUES";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_insert_on_conflict_nothing() {
        let sql = "INSERT INTO users (id, name) VALUES (1, 'Alice') ON CONFLICT DO NOTHING";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_insert_on_conflict_update() {
        let sql = "INSERT INTO users (id, name) VALUES (1, 'Alice') ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_insert_on_conflict_constraint() {
        let sql = "INSERT INTO users (id, name) VALUES (1, 'Alice') ON CONFLICT ON CONSTRAINT users_pkey DO UPDATE SET name = EXCLUDED.name WHERE users.active = true";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_insert_on_conflict_target_where() {
        let sql = "INSERT INTO users (id, name) VALUES (1, 'Alice') ON CONFLICT (id) WHERE id > 0 DO NOTHING";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_insert_returning() {
        let sql = "INSERT INTO users (name) VALUES ('Alice') RETURNING id";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_insert_returning_aliases() {
        let sql = "INSERT INTO users (name) VALUES ('Alice') RETURNING id AS user_id, name";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    // =========================================================================
    // UPDATE statement tests
    // =========================================================================

    #[test]
    fn test_update_simple() {
        let sql = "UPDATE users SET active = true WHERE id = 1";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_update_multiple_columns() {
        let sql = "UPDATE users SET name = 'Bob', email = 'bob@example.com' WHERE id = 1";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_update_from() {
        let sql = "UPDATE users SET email = c.email FROM contacts c WHERE users.id = c.user_id";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_update_returning() {
        let sql = "UPDATE users SET active = true WHERE id = 1 RETURNING *";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_update_from_subquery() {
        let sql = "UPDATE users u SET status = 'active' FROM (SELECT user_id FROM orders WHERE total > 100) o WHERE u.id = o.user_id";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_update_from_join() {
        let sql = "UPDATE users u SET last_order = o.created_at FROM orders o INNER JOIN order_items oi ON o.id = oi.order_id WHERE u.id = o.user_id";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_update_from_multiple_tables() {
        let sql = "UPDATE inventory i SET quantity = i.quantity - o.qty FROM orders o, order_items oi WHERE i.product_id = oi.product_id AND o.id = oi.order_id";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_update_from_lateral() {
        let sql = "UPDATE products p SET price = sub.new_price FROM LATERAL (SELECT price * 1.1 AS new_price FROM pricing WHERE product_id = p.id) sub";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    // =========================================================================
    // DELETE statement tests
    // =========================================================================

    #[test]
    fn test_delete_simple() {
        let sql = "DELETE FROM users WHERE id = 1";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_delete_using() {
        let sql = "DELETE FROM users USING inactive_list WHERE users.id = inactive_list.user_id";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_delete_returning() {
        let sql = "DELETE FROM users WHERE id = 1 RETURNING id, name";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_parse_valid_fixtures() {
        let dir = fixture_dir("valid");
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("failed to read fixtures {}: {err}", dir.display()))
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().and_then(|s| s.to_str()) == Some("sql"))
            .collect();

        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let path = entry.path();
            let sql = read_fixture(&path);
            let parse = parse(&sql);
            assert!(
                parse.errors().is_empty(),
                "fixture {} errors: {:?}",
                path.display(),
                parse.errors()
            );
        }
    }

    // =========================================================================
    // WITH/LATERAL and dollar-quoted strings
    // =========================================================================

    #[test]
    fn test_with_recursive() {
        let sql = "WITH RECURSIVE t AS (SELECT 1) SELECT * FROM t";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_select_lateral_subquery() {
        let sql = "SELECT * FROM LATERAL (SELECT id FROM users) AS u";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_join_lateral() {
        let sql = "SELECT * FROM users u LEFT JOIN LATERAL (SELECT * FROM orders o WHERE o.user_id = u.id) ord ON true";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_dollar_quoted_string() {
        let sql = "SELECT $$line1\nline2$$";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    // =========================================================================
    // CTE advanced features
    // =========================================================================

    #[test]
    fn test_cte_materialized() {
        let sql = "WITH cached AS MATERIALIZED (SELECT * FROM big_table) SELECT * FROM cached";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_cte_not_materialized() {
        let sql = "WITH inline AS NOT MATERIALIZED (SELECT id FROM t) SELECT * FROM inline";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_cte_data_modifying_insert() {
        let sql = "WITH inserted AS (INSERT INTO logs (msg) VALUES ('test') RETURNING id) SELECT * FROM inserted";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_cte_data_modifying_delete() {
        let sql = "WITH deleted AS (DELETE FROM old_data WHERE expired = true RETURNING id) SELECT count(*) FROM deleted";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    // =========================================================================
    // ANY/ALL/SOME operators
    // =========================================================================

    #[test]
    fn test_any_array() {
        let sql = "SELECT * FROM users WHERE id = ANY(ARRAY[1, 2, 3])";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_any_subquery() {
        let sql = "SELECT * FROM products WHERE price > ANY(SELECT avg_price FROM categories)";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_all_subquery() {
        let sql = "SELECT * FROM products WHERE price > ALL(SELECT min_price FROM categories)";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_some_array() {
        let sql = "SELECT * FROM users WHERE status = SOME(ARRAY['active', 'pending'])";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    // =========================================================================
    // Error recovery tests
    // =========================================================================

    #[test]
    fn test_case_recovery_missing_then() {
        // Missing THEN keyword - should recover and still parse
        let sql = "SELECT CASE WHEN x > 0 y ELSE z END FROM t";
        let parse = parse(sql);
        // Should have errors but still produce a tree
        assert!(!parse.errors().is_empty());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_case_recovery_missing_end() {
        // Missing END keyword - should recover
        let sql = "SELECT CASE WHEN x THEN y FROM t";
        let parse = parse(sql);
        assert!(!parse.errors().is_empty());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_nested_paren_recovery() {
        // Unclosed parenthesis - should recover
        let sql = "SELECT (a + b FROM t";
        let parse = parse(sql);
        assert!(!parse.errors().is_empty());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_subquery_isolated_recovery() {
        // Error in subquery should not affect outer query parsing
        let sql = "SELECT * FROM users WHERE id IN (SELECT id FROM BROKEN) AND status = 'active'";
        let _parse = parse(sql);
        // Should parse successfully even with potentially ambiguous subquery
        // The outer query structure should be intact
        let tree = format_tree(sql);
        assert!(tree.contains("WHERE_CLAUSE"));
        assert!(tree.contains("IN_EXPR"));
        assert!(tree.contains("SUBQUERY_EXPR") || tree.contains("SELECT_STMT"));
        insta::assert_snapshot!(tree);
    }

    #[test]
    fn test_exists_subquery_recovery() {
        // EXISTS subquery with error should recover gracefully
        let sql = "SELECT * FROM users WHERE EXISTS (SELECT 1 FROM orders) AND active";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        let tree = format_tree(sql);
        assert!(tree.contains("EXISTS_EXPR"));
        insta::assert_snapshot!(tree);
    }

    #[test]
    fn test_nested_subquery_context() {
        // Deeply nested subqueries should track context
        let sql = "SELECT * FROM (SELECT id FROM (SELECT id FROM inner_t) AS sub1) AS sub2";
        let parse = parse(sql);
        assert!(parse.errors().is_empty(), "errors: {:?}", parse.errors());
        insta::assert_snapshot!(format_tree(sql));
    }

    #[test]
    fn test_case_in_subquery_error_context() {
        // Error in CASE within subquery should include nesting context
        let sql = "SELECT * FROM t WHERE x IN (SELECT CASE WHEN y THEN z FROM inner_t)";
        let parse = parse(sql);
        // Should have error about missing END but should still parse outer structure
        assert!(!parse.errors().is_empty());
        // Check that error message includes context
        let errors: Vec<String> = parse.errors().iter().map(|e| e.message.clone()).collect();
        let has_context = errors
            .iter()
            .any(|e| e.contains("CASE") || e.contains("subquery"));
        assert!(
            has_context || !errors.is_empty(),
            "errors should include context: {:?}",
            errors
        );
        insta::assert_snapshot!(format_tree(sql));
    }

    // =========================================================================
    // Expression correctness
    // =========================================================================

    /// Asserts the SQL parses without errors and the tree contains `needle`.
    fn assert_parses_with(sql: &str, needle: &str) {
        let parse = parse(sql);
        assert!(
            parse.errors().is_empty(),
            "{sql}: errors: {:?}",
            parse.errors()
        );
        let tree = format_tree(sql);
        assert!(
            tree.contains(needle),
            "{sql}: tree missing {needle}:\n{tree}"
        );
    }

    #[test]
    fn test_regex_match_operators() {
        assert_parses_with("SELECT a ~ 'x' FROM t", "BINARY_EXPR");
        assert_parses_with("SELECT a ~* 'x' FROM t", "BINARY_EXPR");
        assert_parses_with("SELECT a !~ 'x' FROM t", "BINARY_EXPR");
        assert_parses_with("SELECT a !~* 'x' FROM t", "BINARY_EXPR");
    }

    #[test]
    fn test_exponent_operator() {
        assert_parses_with("SELECT 2 ^ 3", "BINARY_EXPR");
    }

    #[test]
    fn test_not_precedence() {
        // `NOT a = b` must parse as `NOT (a = b)`: a UNARY_EXPR wrapping a BINARY_EXPR.
        let sql = "SELECT 1 WHERE NOT a = b";
        let tree = format_tree(sql);
        let unary = tree.find("UNARY_EXPR").expect("UNARY_EXPR");
        let binary = tree.find("BINARY_EXPR").expect("BINARY_EXPR");
        assert!(unary < binary, "NOT should wrap the comparison:\n{tree}");
    }

    #[test]
    fn test_multiword_types() {
        assert_parses_with("SELECT x::double precision FROM t", "TYPE_NAME");
        assert_parses_with("SELECT x::character varying FROM t", "TYPE_NAME");
        assert_parses_with("SELECT x::char varying FROM t", "TYPE_NAME");
        assert_parses_with("SELECT x::bit varying FROM t", "TYPE_NAME");
        assert_parses_with("SELECT a::timestamp with time zone FROM t", "TYPE_NAME");
        assert_parses_with("SELECT a::time without time zone FROM t", "TYPE_NAME");
        assert_parses_with("SELECT a::timestamp(3) with time zone FROM t", "TYPE_NAME");
    }

    #[test]
    fn test_at_time_zone() {
        assert_parses_with("SELECT a AT TIME ZONE 'UTC' FROM t", "AT_TIME_ZONE_EXPR");
    }

    #[test]
    fn test_collate() {
        assert_parses_with("SELECT n COLLATE \"C\" FROM t", "COLLATE_EXPR");
        assert_parses_with(
            "SELECT n COLLATE \"en_US\" FROM t ORDER BY n",
            "COLLATE_EXPR",
        );
    }

    #[test]
    fn test_similar_to() {
        assert_parses_with("SELECT a SIMILAR TO 'x%' FROM t", "LIKE_EXPR");
        assert_parses_with("SELECT a NOT SIMILAR TO 'x%' FROM t", "LIKE_EXPR");
    }

    #[test]
    fn test_isnull_notnull_postfix() {
        assert_parses_with("SELECT x ISNULL FROM t", "IS_EXPR");
        assert_parses_with("SELECT x NOTNULL FROM t", "IS_EXPR");
    }

    #[test]
    fn test_array_slice() {
        assert_parses_with("SELECT arr[1:2] FROM t", "ARRAY_SLICE_EXPR");
        assert_parses_with("SELECT arr[:2] FROM t", "ARRAY_SLICE_EXPR");
        assert_parses_with("SELECT arr[1:] FROM t", "ARRAY_SLICE_EXPR");
        // Plain subscript stays a JSONB_ACCESS_EXPR.
        assert_parses_with("SELECT arr[1] FROM t", "JSONB_ACCESS_EXPR");
    }

    // =========================================================================
    // DDL: CREATE TABLE / ALTER TABLE / CREATE INDEX / DROP / TRUNCATE
    // =========================================================================

    #[test]
    fn test_create_table() {
        assert_parses_with(
            "CREATE TABLE users (id bigint PRIMARY KEY, name text NOT NULL)",
            "CREATE_TABLE_STMT",
        );
        assert_parses_with(
            "CREATE TABLE users (id bigint PRIMARY KEY, name text NOT NULL)",
            "COLUMN_DEF",
        );
        assert_parses_with(
            "CREATE TABLE users (id bigint PRIMARY KEY)",
            "PRIMARY_KEY_CONSTRAINT",
        );
        assert_parses_with(
            "CREATE TABLE t (id int, name text NOT NULL)",
            "NOT_NULL_CONSTRAINT",
        );
    }

    #[test]
    fn test_create_table_constraints() {
        assert_parses_with(
            "CREATE TABLE t (a int REFERENCES u(id) ON DELETE CASCADE)",
            "FOREIGN_KEY_CONSTRAINT",
        );
        assert_parses_with("CREATE TABLE t (a int CHECK (a > 0))", "CHECK_CONSTRAINT");
        assert_parses_with(
            "CREATE TABLE t (a int, b int, PRIMARY KEY (a, b))",
            "PRIMARY_KEY_CONSTRAINT",
        );
        assert_parses_with("CREATE TABLE t (a int, UNIQUE (a))", "UNIQUE_CONSTRAINT");
    }

    #[test]
    fn test_create_table_as_select() {
        assert_parses_with(
            "CREATE TABLE archive AS SELECT * FROM users",
            "CREATE_TABLE_STMT",
        );
        assert_parses_with("CREATE TABLE archive AS SELECT * FROM users", "SELECT_STMT");
    }

    #[test]
    fn test_alter_table() {
        assert_parses_with("ALTER TABLE t ADD COLUMN a int NOT NULL", "ALTER_STMT");
        assert_parses_with("ALTER TABLE t ADD COLUMN a int", "ALTER_TABLE_ACTION");
        assert_parses_with("ALTER TABLE t ADD COLUMN a int", "COLUMN_DEF");
        assert_parses_with(
            "ALTER TABLE t ADD CONSTRAINT ck CHECK (x > 0) NOT VALID",
            "CHECK_CONSTRAINT",
        );
        assert_parses_with("ALTER TABLE t ALTER COLUMN id TYPE bigint", "TYPE_NAME");
        assert_parses_with("ALTER TABLE t RENAME COLUMN a TO b", "ALTER_STMT");
        assert_parses_with(
            "ALTER TABLE t ADD COLUMN a int, DROP COLUMN b",
            "ALTER_TABLE_ACTION",
        );
    }

    #[test]
    fn test_create_index() {
        assert_parses_with(
            "CREATE INDEX CONCURRENTLY idx ON users (lower(email))",
            "CREATE_INDEX_STMT",
        );
        assert_parses_with(
            "CREATE UNIQUE INDEX idx ON t (a, b DESC NULLS LAST) WHERE active",
            "WHERE_CLAUSE",
        );
        assert_parses_with("CREATE INDEX ON t (a)", "CREATE_INDEX_STMT");
    }

    #[test]
    fn test_values_and_table_query() {
        assert_parses_with("VALUES (1, 'a'), (2, 'b')", "VALUES_CLAUSE");
        assert_parses_with("VALUES (1, 'a'), (2, 'b')", "VALUES_ROW");
        assert_parses_with("TABLE users", "SELECT_STMT");
        assert_parses_with("SELECT * FROM (VALUES (1), (2)) AS t (x)", "VALUES_CLAUSE");
        assert_parses_with("SELECT * FROM t WHERE id IN (VALUES (1), (2))", "IN_EXPR");
    }

    #[test]
    fn test_parenthesized_set_ops() {
        assert_parses_with("(SELECT 1) UNION (SELECT 2)", "SELECT_STMT");
        assert_parses_with("SELECT 1 UNION VALUES (2)", "VALUES_CLAUSE");
        assert_parses_with(
            "(SELECT 1) UNION (SELECT 2) ORDER BY 1 LIMIT 5",
            "ORDER_BY_CLAUSE",
        );
    }

    #[test]
    fn test_grouping_sets() {
        assert_parses_with("SELECT a FROM t GROUP BY ROLLUP (a, b)", "GROUP_BY_CLAUSE");
        assert_parses_with("SELECT a FROM t GROUP BY CUBE (a, b)", "GROUP_BY_CLAUSE");
        assert_parses_with(
            "SELECT a FROM t GROUP BY GROUPING SETS ((a), (b), ())",
            "GROUP_BY_CLAUSE",
        );
        assert_parses_with("SELECT a FROM t GROUP BY a, ROLLUP (b)", "GROUP_BY_CLAUSE");
    }

    #[test]
    fn test_tablesample_and_ordinality() {
        assert_parses_with("SELECT * FROM t TABLESAMPLE BERNOULLI (10)", "FROM_CLAUSE");
        assert_parses_with(
            "SELECT * FROM t TABLESAMPLE SYSTEM (10) REPEATABLE (42)",
            "FROM_CLAUSE",
        );
        assert_parses_with(
            "SELECT * FROM unnest(ARRAY[1, 2]) WITH ORDINALITY AS x(v, n)",
            "FROM_CLAUSE",
        );
    }

    #[test]
    fn test_transaction_control() {
        assert_parses_with("BEGIN", "TRANSACTION_STMT");
        assert_parses_with("COMMIT", "TRANSACTION_STMT");
        assert_parses_with("ROLLBACK", "TRANSACTION_STMT");
        assert_parses_with(
            "START TRANSACTION ISOLATION LEVEL SERIALIZABLE",
            "TRANSACTION_STMT",
        );
        assert_parses_with("SAVEPOINT sp1", "TRANSACTION_STMT");
        assert_parses_with("ROLLBACK TO SAVEPOINT sp1", "TRANSACTION_STMT");
    }

    #[test]
    fn test_set_show_reset() {
        assert_parses_with("SET search_path TO public", "SET_STMT");
        assert_parses_with("SET TIME ZONE 'UTC'", "SET_STMT");
        assert_parses_with("SHOW search_path", "SHOW_STMT");
        assert_parses_with("RESET ALL", "RESET_STMT");
    }

    #[test]
    fn test_explain() {
        assert_parses_with("EXPLAIN SELECT 1", "EXPLAIN_STMT");
        assert_parses_with("EXPLAIN SELECT 1", "SELECT_STMT");
        assert_parses_with("EXPLAIN ANALYZE SELECT * FROM t", "SELECT_STMT");
        assert_parses_with("EXPLAIN (ANALYZE, BUFFERS) SELECT 1", "SELECT_STMT");
    }

    #[test]
    fn test_comment_on() {
        assert_parses_with("COMMENT ON TABLE t IS 'hi'", "COMMENT_STMT");
        assert_parses_with("COMMENT ON COLUMN t.c IS NULL", "COMMENT_STMT");
    }

    #[test]
    fn test_create_view_sequence_schema_extension() {
        assert_parses_with("CREATE VIEW v AS SELECT 1", "CREATE_VIEW_STMT");
        assert_parses_with("CREATE VIEW v AS SELECT 1", "SELECT_STMT");
        assert_parses_with(
            "CREATE MATERIALIZED VIEW mv AS SELECT * FROM t WITH NO DATA",
            "CREATE_VIEW_STMT",
        );
        assert_parses_with("CREATE SEQUENCE s START 1", "CREATE_SEQUENCE_STMT");
        assert_parses_with("CREATE SCHEMA app", "CREATE_SCHEMA_STMT");
        assert_parses_with(
            "CREATE EXTENSION IF NOT EXISTS pg_trgm",
            "CREATE_EXTENSION_STMT",
        );
    }

    #[test]
    fn test_call_do_vacuum_analyze_copy() {
        assert_parses_with("CALL my_proc(1, 2)", "CALL_STMT");
        assert_parses_with("DO $$ BEGIN END $$", "DO_STMT");
        assert_parses_with("VACUUM ANALYZE t", "VACUUM_STMT");
        assert_parses_with("ANALYZE t", "ANALYZE_STMT");
        assert_parses_with("COPY t FROM '/tmp/f.csv'", "COPY_STMT");
    }

    #[test]
    fn test_grant_revoke() {
        assert_parses_with("GRANT SELECT ON t TO alice", "GRANT_STMT");
        assert_parses_with("REVOKE ALL ON t FROM alice", "REVOKE_STMT");
    }

    #[test]
    fn test_merge() {
        assert_parses_with(
            "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN UPDATE SET v = s.v",
            "MERGE_STMT",
        );
        assert_parses_with(
            "MERGE INTO t USING s ON t.id = s.id \
             WHEN MATCHED AND s.x > 0 THEN UPDATE SET v = CASE WHEN s.v > 0 THEN s.v ELSE 0 END \
             WHEN NOT MATCHED THEN INSERT (id) VALUES (s.id)",
            "MERGE_STMT",
        );
    }

    #[test]
    fn test_create_type_function_trigger() {
        assert_parses_with("CREATE TYPE mood AS ENUM ('a', 'b')", "CREATE_TYPE_STMT");
        assert_parses_with("CREATE TYPE pair AS (a int, b text)", "CREATE_TYPE_STMT");
        assert_parses_with(
            "CREATE FUNCTION f(x int) RETURNS int AS $$ SELECT x + 1 $$ LANGUAGE sql",
            "CREATE_FUNCTION_STMT",
        );
        assert_parses_with(
            "CREATE TRIGGER trg BEFORE INSERT ON t FOR EACH ROW EXECUTE FUNCTION f()",
            "CREATE_TRIGGER_STMT",
        );
    }

    #[test]
    fn test_unsupported_ddl_recovers_without_hang() {
        // Unsupported CREATE/ALTER objects must error and make progress, never
        // stall the top-level loop, and must not swallow the following statement.
        for sql in [
            "CREATE DATABASE db",
            "CREATE ROLE r",
            "ALTER VIEW v RENAME TO w",
        ] {
            let parse = parse(sql);
            assert!(!parse.errors().is_empty(), "{sql}: expected an error");
        }

        let parse = parse("CREATE DATABASE db; SELECT 2");
        let tree = format_tree("CREATE DATABASE db; SELECT 2");
        assert!(
            tree.contains("SELECT_STMT"),
            "recovery should still parse the trailing statement:\n{tree}"
        );
        assert!(!parse.errors().is_empty());
    }

    #[test]
    fn test_drop_truncate() {
        assert_parses_with("DROP TABLE IF EXISTS a, b CASCADE", "DROP_STMT");
        assert_parses_with("DROP INDEX CONCURRENTLY idx", "DROP_STMT");
        assert_parses_with(
            "TRUNCATE TABLE a, b RESTART IDENTITY CASCADE",
            "TRUNCATE_STMT",
        );
        assert_parses_with("TRUNCATE t", "TRUNCATE_STMT");
    }

    #[test]
    fn test_with_clause_subquery_recovery() {
        // CTE parsing with missing closing paren - should still produce valid structure
        let sql = "WITH cte AS (SELECT id FROM t SELECT * FROM cte";
        let parse = parse(sql);
        // Should have error but still produce valid outer structure
        assert!(!parse.errors().is_empty(), "expected errors, got none");
        let tree = format_tree(sql);
        assert!(tree.contains("WITH_CLAUSE"));
        assert!(tree.contains("CTE"));
        insta::assert_snapshot!(tree);
    }
}
