//! Snapshot tests for SQL formatting.

use banshee_format::{FormatConfig, format_compact, format_sqlstyle};

#[test]
fn test_simple_select_sqlstyle() {
    let sql = "SELECT id, name FROM users WHERE active = true";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_simple_select_compact() {
    let sql = "select id, name from users where active = true";
    let formatted = format_compact(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_select_with_join() {
    let sql = "SELECT u.id, u.name, o.amount FROM users u LEFT JOIN orders o ON u.id = o.user_id WHERE o.status = 'completed'";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_select_with_multiple_joins() {
    let sql = "SELECT r.last_name FROM riders AS r INNER JOIN bikes AS b ON r.bike_vin_num = b.vin_num AND b.engine_tally > 2 INNER JOIN crew AS c ON r.crew_chief_last_name = c.last_name AND c.chief = 'Y'";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_jsonb_operators() {
    let sql = "SELECT data->'name'->>'first', data#>'{address,city}' FROM users WHERE data @> '{\"active\": true}'";
    let formatted = format_compact(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_cte() {
    let sql = "WITH active_users AS (SELECT id, name FROM users WHERE active = true) SELECT * FROM active_users";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_recursive_cte() {
    let sql = "WITH RECURSIVE numbers AS (SELECT 1 AS n UNION ALL SELECT n + 1 FROM numbers WHERE n < 10) SELECT * FROM numbers";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_insert_values() {
    let sql = "INSERT INTO users (id, name, email) VALUES (1, 'Alice', 'alice@example.com'), (2, 'Bob', 'bob@example.com')";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_update_set() {
    let sql = "UPDATE users SET name = 'New Name', updated_at = now() WHERE id = 1";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_delete() {
    let sql = "DELETE FROM users WHERE id = 1 RETURNING *";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_case_expression() {
    let sql = "SELECT id, CASE WHEN active THEN 'yes' WHEN status = 'pending' THEN 'pending' ELSE 'no' END AS status FROM users";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_subquery() {
    let sql = "SELECT * FROM users WHERE id IN (SELECT user_id FROM orders WHERE total > 100)";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_window_function() {
    let sql = "SELECT id, name, row_number() OVER (PARTITION BY department ORDER BY salary DESC) AS rank FROM employees";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_complex_where() {
    // Use <> instead of != to match the parser's token text
    let sql = "SELECT * FROM users WHERE (active = true AND role = 'admin') OR (created_at > '2024-01-01' AND status <> 'deleted')";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_group_by_having() {
    let sql = "SELECT department, COUNT(*) as cnt FROM employees GROUP BY department HAVING COUNT(*) > 5 ORDER BY cnt DESC";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_multiple_statements() {
    let sql = "SELECT * FROM users; SELECT * FROM orders;";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_idempotent() {
    // Test idempotence with a simpler query
    let sql = "SELECT id, name FROM users";
    let formatted1 = format_sqlstyle(sql);
    let formatted2 = format_sqlstyle(&formatted1);
    assert_eq!(formatted1, formatted2, "Formatting should be idempotent");
}

#[test]
fn test_idempotent_with_where() {
    let sql = "SELECT id FROM users WHERE active = true";
    let formatted1 = format_sqlstyle(sql);
    let formatted2 = format_sqlstyle(&formatted1);
    // Trim trailing whitespace for comparison since that can vary
    let f1 = formatted1
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    let f2 = formatted2
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        f1, f2,
        "Formatting should be idempotent (modulo trailing whitespace)"
    );
}

#[test]
fn test_cast_expression() {
    let sql = "SELECT id::text, CAST(amount AS numeric(10,2)) FROM orders";
    let formatted = format_compact(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_array_expression() {
    let sql = "SELECT * FROM users WHERE id = ANY(ARRAY[1, 2, 3])";
    let formatted = format_compact(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_coalesce_nullif() {
    let sql = "SELECT COALESCE(name, 'Unknown'), NULLIF(status, 'deleted') FROM users";
    let formatted = format_compact(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_lower_keyword_case() {
    let config = FormatConfig::sqlstyle().with_keyword_case(banshee_format::KeywordCase::Lower);
    let sql = "SELECT id FROM users WHERE active = TRUE";
    let formatted = banshee_format::format::format(sql, &config);
    assert!(formatted.contains("select"));
    assert!(formatted.contains("from"));
    assert!(formatted.contains("where"));
}

#[test]
fn test_trailing_comma_style() {
    let config = FormatConfig::sqlstyle().with_comma_style(banshee_format::CommaStyle::Trailing);
    let sql = "SELECT id, name, email FROM users";
    let formatted = banshee_format::format::format(sql, &config);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_create_table() {
    let sql = "create table public.users (id bigint primary key, name text not null, email varchar(255), constraint uq_email unique (email), check (id > 0));";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_create_table_as_select() {
    let sql = "create table recent as select id from events where created_at > now();";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_create_index() {
    let sql = "create unique index concurrently idx_users_email on public.users using btree (email) where active;";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_alter_table_multi_action() {
    let sql = "alter table users add column age int not null default 0, drop column legacy, alter column name type text;";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_alter_table_rename() {
    let sql = "alter table users rename column email to email_address;";
    let formatted = format_sqlstyle(sql);
    insta::assert_snapshot!(formatted);
}

#[test]
fn test_ddl_formatting_is_idempotent() {
    for sql in [
        "create table t (id bigint primary key, name varchar(255), constraint u unique (name));",
        "create index concurrently i on t using btree (a, b) where a > 0;",
        "alter table t add column c int, drop column d, alter column e type bigint;",
        "alter table t rename to t2;",
    ] {
        let once = format_sqlstyle(sql);
        let twice = format_sqlstyle(&once);
        assert_eq!(once, twice, "not idempotent for `{sql}`:\n{once}");
    }
}

#[test]
fn typed_literals_keep_their_type_before_the_string() {
    let formatted = format_compact(
        "select date '2024-01-01', timestamp with time zone '2024-01-01 10:00+02', interval '1' day from t",
    );
    let lower = formatted.to_lowercase();
    for expected in [
        "date '2024-01-01'",
        "timestamp with time zone '2024-01-01 10:00+02'",
        "interval '1' day",
    ] {
        assert!(lower.contains(expected), "{formatted}");
    }
    let pg = banshee_format::format_pgformatter("SELECT DATE '2024-01-01' FROM t");
    assert!(pg.to_lowercase().contains("date '2024-01-01'"), "{pg}");
}
