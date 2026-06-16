//! Regression tests for grammar constructs that previously failed to parse.
//! Each statement here is valid PostgreSQL (accepted by libpg_query) and must
//! parse with no errors.

use banshee_parser::parse;

fn assert_parses(sql: &str) {
    let parse = parse(sql);
    assert!(
        parse.errors().is_empty(),
        "expected clean parse for {sql:?}, got errors: {:?}",
        parse.errors()
    );
}

#[test]
fn replace_used_as_function() {
    // REPLACE is a keyword in banshee (CREATE OR REPLACE) but also a builtin
    // function and must be callable as one.
    assert_parses("SELECT replace(name, ' ', '-') FROM t");
    assert_parses("UPDATE t SET slug = LOWER(REPLACE(name, ' ', '-'))");
}

#[test]
fn nested_string_functions_with_unicode() {
    // Mirrors the real migration: TRANSLATE/REGEXP_REPLACE/REPLACE nested, with
    // multi-byte (Cyrillic) string arguments.
    assert_parses(
        "UPDATE t SET slug = LOWER(\
            REPLACE(\
                REGEXP_REPLACE(\
                    TRANSLATE(name, 'абв', 'abv'),\
                    '[^a-z0-9 ]', '', 'g'\
                ),\
                ' ', '-'\
            ))",
    );
}

#[test]
fn niladic_special_functions() {
    assert_parses("SELECT CURRENT_DATE, CURRENT_TIME, CURRENT_TIMESTAMP");
    assert_parses("SELECT CURRENT_USER, SESSION_USER");
    assert_parses("SELECT LOCALTIME, LOCALTIMESTAMP");
    // Optional precision on the time-valued forms.
    assert_parses("SELECT CURRENT_TIMESTAMP(3), CURRENT_TIME(2), LOCALTIMESTAMP(6)");
}

#[test]
fn current_timestamp_as_column_default() {
    assert_parses(
        "CREATE TABLE t (\
            id int PRIMARY KEY,\
            created_at timestamptz DEFAULT CURRENT_TIMESTAMP,\
            updated_at timestamptz DEFAULT now()\
        )",
    );
}

#[test]
fn unreserved_keywords_as_identifiers() {
    // `type`, `value`, `source` etc. are unreserved keywords usable as names.
    assert_parses("CREATE TABLE t (type text NOT NULL, value int, source text)");
    assert_parses("SELECT type, value FROM t WHERE type = 'x'");
    assert_parses("CREATE INDEX ON t (type, value)");
    assert_parses("UPDATE t SET value = 1 WHERE type = 'a'");
}

#[test]
fn alter_type_forms() {
    assert_parses("ALTER TYPE my_enum ADD VALUE 'x'");
    assert_parses("ALTER TYPE my_enum ADD VALUE IF NOT EXISTS 'x' AFTER 'y'");
    assert_parses("ALTER TYPE my_enum RENAME TO other");
    assert_parses("ALTER TYPE my_enum OWNER TO postgres");
}

#[test]
fn unique_nulls_not_distinct() {
    assert_parses("CREATE TABLE t (a int, b int, UNIQUE NULLS NOT DISTINCT (a, b))");
    assert_parses("CREATE TABLE t (a int UNIQUE NULLS NOT DISTINCT)");
    assert_parses("ALTER TABLE t ADD CONSTRAINT u UNIQUE NULLS NOT DISTINCT (a, b)");
}

#[test]
fn insert_overriding_system_value() {
    assert_parses("INSERT INTO t (id, name) OVERRIDING SYSTEM VALUE VALUES (1, 'a')");
    assert_parses("INSERT INTO t (id, name) OVERRIDING USER VALUE VALUES (1, 'a')");
    // Without a column list.
    assert_parses("INSERT INTO t OVERRIDING SYSTEM VALUE VALUES (1, 'a')");
}
