//! Differential test: PostgreSQL's own parser (libpg_query via `pg_parse`) is
//! the oracle. For every statement PostgreSQL accepts, banshee must produce a tree
//! with no parse errors. Mismatches (PG-valid but banshee-errors) are bugs or
//! unimplemented grammar.
//!
//! The corpus is intentionally limited to the statement classes banshee claims to
//! support (DML + expressions). DDL is added here as it lands in the grammar.

/// Statements known to be valid PostgreSQL that banshee must parse cleanly.
const CORPUS: &[&str] = &[
    // --- SELECT basics ---
    "SELECT 1",
    "SELECT id, name FROM users",
    "SELECT * FROM users WHERE active",
    "SELECT u.id, p.title FROM users u JOIN posts p ON u.id = p.user_id",
    "SELECT * FROM a LEFT JOIN b ON a.id = b.a_id",
    "SELECT * FROM a NATURAL JOIN b",
    "SELECT * FROM a CROSS JOIN b",
    "SELECT DISTINCT city FROM users",
    "SELECT DISTINCT ON (city) city, name FROM users ORDER BY city, name",
    "SELECT count(*), sum(amount) FROM orders GROUP BY status HAVING count(*) > 1",
    "SELECT * FROM t ORDER BY a ASC, b DESC NULLS LAST LIMIT 10 OFFSET 5",
    "SELECT * FROM t FETCH FIRST 5 ROWS ONLY",
    "SELECT * FROM t FOR UPDATE",
    "SELECT * FROM t FOR NO KEY UPDATE OF t SKIP LOCKED",
    // --- expressions ---
    "SELECT a + b * c - d / e % f FROM t",
    "SELECT 2 ^ 3 ^ 2",
    "SELECT a ~ 'x', a ~* 'x', a !~ 'x', a !~* 'x' FROM t",
    "SELECT a FROM t WHERE NOT a = b",
    "SELECT a FROM t WHERE NOT a = b AND c < d OR e",
    "SELECT a FROM t WHERE x BETWEEN 1 AND 10",
    "SELECT a FROM t WHERE x NOT BETWEEN 1 AND 10",
    "SELECT a FROM t WHERE name LIKE 'A%' ESCAPE '\\'",
    "SELECT a FROM t WHERE name SIMILAR TO 'A%'",
    "SELECT a FROM t WHERE name NOT SIMILAR TO 'A%'",
    "SELECT a FROM t WHERE x IS NULL AND y IS NOT NULL",
    "SELECT a FROM t WHERE x IS DISTINCT FROM y",
    "SELECT a FROM t WHERE x ISNULL",
    "SELECT a FROM t WHERE x IN (1, 2, 3)",
    "SELECT a FROM t WHERE x IN (SELECT id FROM u)",
    "SELECT a FROM t WHERE EXISTS (SELECT 1 FROM u WHERE u.id = t.id)",
    "SELECT a FROM t WHERE x = ANY(ARRAY[1, 2, 3])",
    "SELECT a FROM t WHERE x > ALL(SELECT v FROM u)",
    "SELECT CASE WHEN x > 0 THEN 'p' WHEN x < 0 THEN 'n' ELSE 'z' END FROM t",
    "SELECT CASE x WHEN 1 THEN 'a' ELSE 'b' END FROM t",
    "SELECT COALESCE(a, b, c), NULLIF(x, y), GREATEST(a, b), LEAST(a, b) FROM t",
    // --- casts / types ---
    "SELECT id::text, CAST(amount AS numeric(10, 2)) FROM t",
    "SELECT x::double precision, y::character varying, z::bit varying FROM t",
    "SELECT a::timestamp with time zone, b::time without time zone FROM t",
    "SELECT a::timestamp(3) with time zone FROM t",
    "SELECT ARRAY[1, 2, 3]::int[] FROM t",
    // --- AT TIME ZONE / COLLATE ---
    "SELECT ts AT TIME ZONE 'UTC' FROM t",
    "SELECT name COLLATE \"C\" FROM t ORDER BY name COLLATE \"C\"",
    // --- arrays / subscript / slice ---
    "SELECT arr[1], arr[1:3], arr[:2], arr[2:] FROM t",
    // --- JSONB ---
    "SELECT data->'a'->>'b', data #> '{a,b}', data #>> '{a,b}' FROM t",
    "SELECT * FROM t WHERE data @> '{\"k\": 1}' AND data ? 'k'",
    "SELECT * FROM t WHERE data @? '$.a' AND data @@ '$.a == 1'",
    // --- functions / windows ---
    "SELECT row_number() OVER (PARTITION BY dept ORDER BY salary DESC) FROM emp",
    "SELECT sum(x) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM t",
    "SELECT count(*) FILTER (WHERE active) FROM users",
    "SELECT array_agg(name ORDER BY id) FROM users",
    "SELECT string_agg(name, ',') WITHIN GROUP (ORDER BY name) FROM users",
    "SELECT * FROM generate_series(1, 10) AS s(n)",
    "SELECT extract(year FROM now()), substring('abc' FROM 1 FOR 2), trim(both ' ' FROM x) FROM t",
    "SELECT replace(name, ' ', '-'), translate(name, 'abc', 'xyz') FROM t",
    "SELECT lower(replace(regexp_replace(translate(name, 'abc', 'xyz'), '[^a-z]', '', 'g'), ' ', '-')) FROM t",
    // --- niladic special functions ---
    "SELECT CURRENT_DATE, CURRENT_TIME, CURRENT_TIMESTAMP, CURRENT_USER, SESSION_USER",
    "SELECT LOCALTIME, LOCALTIMESTAMP, CURRENT_TIMESTAMP(3), CURRENT_TIME(2), LOCALTIMESTAMP(6)",
    // --- CTE / set ops ---
    "WITH cte AS (SELECT id FROM users) SELECT * FROM cte",
    "WITH RECURSIVE t(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM t WHERE n < 10) SELECT * FROM t",
    "SELECT a FROM t1 UNION SELECT a FROM t2 EXCEPT SELECT a FROM t3",
    "SELECT a FROM t1 UNION ALL SELECT a FROM t2 ORDER BY a",
    // --- DML ---
    "INSERT INTO users (id, name) VALUES (1, 'Alice'), (2, 'Bob')",
    "INSERT INTO users (name) VALUES ('x') ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING id",
    "INSERT INTO t DEFAULT VALUES",
    "INSERT INTO t (id, name) OVERRIDING SYSTEM VALUE VALUES (1, 'a')",
    "INSERT INTO t (id, name) OVERRIDING USER VALUE VALUES (1, 'a')",
    "INSERT INTO t (created_at) VALUES (CURRENT_TIMESTAMP)",
    "UPDATE users SET active = true, name = 'x' WHERE id = 1 RETURNING *",
    "UPDATE u SET v = c.v FROM contacts c WHERE u.id = c.uid",
    "DELETE FROM users USING blocklist b WHERE users.id = b.uid RETURNING id",
    // --- bitwise / shift operators ---
    "SELECT flags & 1, flags | 2, flags # 4 FROM t",
    "SELECT a << 2, b >> 1 FROM t",
    "SELECT * FROM t WHERE flags & 1 = 0 AND a | b > c",
    // --- escape / unicode string literals ---
    "SELECT E'line\\n\\ttab' FROM t",
    "SELECT U&'d\\0061t' FROM t",
    // --- named / variadic function arguments ---
    "SELECT make_interval(days => 5, hours => 3)",
    "SELECT concat_ws(',', VARIADIC ARRAY['a', 'b'])",
    // --- unicode identifiers ---
    "SELECT имя FROM таблица WHERE имя = 1",
    "SELECT * FROM \"Имя Таблицы\" t WHERE t.поле = 1",
    // --- query primaries: VALUES / TABLE / parenthesized / set ops ---
    "VALUES (1, 'a'), (2, 'b')",
    "TABLE users",
    "(SELECT 1) UNION (SELECT 2)",
    "SELECT 1 UNION VALUES (2)",
    "(SELECT 1) UNION (SELECT 2) ORDER BY 1 LIMIT 5",
    "WITH x AS (VALUES (1), (2)) SELECT * FROM x",
    "SELECT * FROM (VALUES (1, 'a'), (2, 'b')) AS t (id, name)",
    "SELECT * FROM t WHERE id IN (VALUES (1), (2))",
    // --- grouping sets ---
    "SELECT a, count(*) FROM t GROUP BY ROLLUP (a, b)",
    "SELECT a FROM t GROUP BY CUBE (a, b)",
    "SELECT a, b FROM t GROUP BY GROUPING SETS ((a), (b), ())",
    "SELECT a FROM t GROUP BY a, ROLLUP (b, c)",
    // --- TABLESAMPLE / WITH ORDINALITY ---
    "SELECT * FROM t TABLESAMPLE BERNOULLI (10)",
    "SELECT * FROM t TABLESAMPLE SYSTEM (10) REPEATABLE (42)",
    "SELECT * FROM unnest(ARRAY[1, 2]) WITH ORDINALITY AS x(v, n)",
];

/// DDL statements. Same contract: PG-valid ⇒ banshee parses cleanly.
const DDL_CORPUS: &[&str] = &[
    // CREATE TABLE
    "CREATE TABLE users (id bigint PRIMARY KEY, name text NOT NULL, email varchar(255) UNIQUE)",
    "CREATE TABLE IF NOT EXISTS t (id serial PRIMARY KEY, created_at timestamptz DEFAULT now())",
    "CREATE TABLE orders (id bigint, user_id bigint REFERENCES users(id) ON DELETE CASCADE, total numeric(10, 2) CHECK (total >= 0))",
    "CREATE TABLE t (a int, b int, PRIMARY KEY (a, b), CONSTRAINT fk FOREIGN KEY (a) REFERENCES other(id))",
    "CREATE UNLOGGED TABLE t (id int)",
    "CREATE TEMP TABLE t (id int)",
    "CREATE TABLE t (id int GENERATED ALWAYS AS IDENTITY)",
    "CREATE TABLE t (id int GENERATED BY DEFAULT AS IDENTITY)",
    "CREATE TABLE t (h int, w int GENERATED ALWAYS AS (h * 2) STORED)",
    "CREATE TABLE archive AS SELECT * FROM users WHERE active",
    "CREATE TABLE t (id int, name text COLLATE \"C\")",
    "CREATE TABLE t (id int, CHECK (id > 0))",
    // unreserved keywords used as identifiers (ColId)
    "CREATE TABLE t (type text NOT NULL, value int, source text, name text)",
    "SELECT type, value FROM t WHERE type = 'x' AND source IS NOT NULL",
    "CREATE TABLE t (a int, b int, UNIQUE NULLS NOT DISTINCT (a, b))",
    "CREATE TABLE t (a int UNIQUE NULLS NOT DISTINCT)",
    // ALTER TYPE
    "ALTER TYPE my_enum ADD VALUE 'new_value'",
    "ALTER TYPE my_enum ADD VALUE IF NOT EXISTS 'v' BEFORE 'w'",
    "ALTER TYPE my_enum RENAME TO other_enum",
    "ALTER TYPE my_enum OWNER TO postgres",
    // ALTER TABLE
    "ALTER TABLE users ADD COLUMN age int NOT NULL DEFAULT 0",
    "ALTER TABLE users DROP COLUMN age",
    "ALTER TABLE users ALTER COLUMN name SET NOT NULL",
    "ALTER TABLE users ALTER COLUMN name DROP NOT NULL",
    "ALTER TABLE users ALTER COLUMN id TYPE bigint",
    "ALTER TABLE users ALTER COLUMN id SET DEFAULT 1",
    "ALTER TABLE t ADD CONSTRAINT ck CHECK (x > 0) NOT VALID",
    "ALTER TABLE t ADD CONSTRAINT fk FOREIGN KEY (uid) REFERENCES u(id) NOT VALID",
    "ALTER TABLE t RENAME COLUMN a TO b",
    "ALTER TABLE t RENAME TO t2",
    "ALTER TABLE t VALIDATE CONSTRAINT ck",
    "ALTER TABLE t ADD COLUMN a int, DROP COLUMN b",
    "ALTER TABLE IF EXISTS only_t ADD COLUMN a int",
    // CREATE INDEX
    "CREATE INDEX CONCURRENTLY idx ON users (lower(email))",
    "CREATE UNIQUE INDEX idx ON t USING btree (a, b DESC NULLS LAST) WHERE active",
    "CREATE INDEX ON t (a)",
    "CREATE INDEX idx ON t (a, b)",
    // DROP / TRUNCATE
    "DROP TABLE IF EXISTS a, b CASCADE",
    "DROP INDEX CONCURRENTLY idx",
    "DROP TABLE t",
    "TRUNCATE t",
    "TRUNCATE TABLE a, b RESTART IDENTITY CASCADE",
];

/// Session, transaction, and utility statements.
const UTIL_CORPUS: &[&str] = &[
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "START TRANSACTION ISOLATION LEVEL SERIALIZABLE",
    "BEGIN TRANSACTION READ ONLY",
    "SAVEPOINT sp1",
    "RELEASE SAVEPOINT sp1",
    "ROLLBACK TO SAVEPOINT sp1",
    "COMMIT AND CHAIN",
    "SET search_path TO public",
    "SET LOCAL statement_timeout = '5s'",
    "SET TIME ZONE 'UTC'",
    "RESET ALL",
    "SHOW search_path",
    "SHOW ALL",
    "EXPLAIN SELECT 1",
    "EXPLAIN ANALYZE SELECT * FROM t",
    "EXPLAIN (ANALYZE, BUFFERS, FORMAT json) SELECT 1",
    "EXPLAIN VERBOSE SELECT 1",
    "COMMENT ON TABLE t IS 'hi'",
    "COMMENT ON COLUMN t.c IS NULL",
    "CREATE VIEW v AS SELECT 1",
    "CREATE OR REPLACE VIEW v (a, b) AS SELECT 1, 2",
    "CREATE MATERIALIZED VIEW mv AS SELECT * FROM t WITH NO DATA",
    "CREATE SEQUENCE s START 1 INCREMENT 2",
    "CREATE SCHEMA app",
    "CREATE SCHEMA AUTHORIZATION bob",
    "CREATE EXTENSION IF NOT EXISTS pg_trgm",
];

/// CALL, DO, VACUUM, ANALYZE, COPY, GRANT/REVOKE, MERGE, and the remaining
/// CREATE object statements.
const COMMANDS_CORPUS: &[&str] = &[
    "CALL my_proc(1, 2)",
    "DO $$ BEGIN RAISE NOTICE 'hi'; END $$",
    "VACUUM ANALYZE t",
    "ANALYZE t",
    "COPY t FROM '/tmp/f.csv' WITH (FORMAT csv)",
    "COPY (SELECT 1) TO STDOUT",
    "GRANT SELECT, INSERT ON t TO alice",
    "REVOKE ALL ON t FROM alice",
    "CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy')",
    "CREATE TYPE pair AS (a int, b text)",
    "CREATE FUNCTION f(x int) RETURNS int AS $$ SELECT x + 1 $$ LANGUAGE sql",
    "CREATE OR REPLACE PROCEDURE p() LANGUAGE plpgsql AS $$ BEGIN END $$",
    "CREATE TRIGGER trg BEFORE INSERT ON t FOR EACH ROW EXECUTE FUNCTION f()",
    "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN UPDATE SET v = s.v WHEN NOT MATCHED THEN INSERT (id, v) VALUES (s.id, s.v)",
    "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED AND s.x > 0 THEN UPDATE SET v = CASE WHEN s.v > 0 THEN s.v ELSE 0 END WHEN NOT MATCHED THEN DO NOTHING",
];

fn pg_accepts(sql: &str) -> bool {
    pg_parse::parse(sql).is_ok()
}

#[test]
fn oracle_corpus_matches_postgres() {
    check_corpus(CORPUS);
}

#[test]
fn oracle_ddl_corpus_matches_postgres() {
    check_corpus(DDL_CORPUS);
}

#[test]
fn oracle_util_corpus_matches_postgres() {
    check_corpus(UTIL_CORPUS);
}

#[test]
fn oracle_commands_corpus_matches_postgres() {
    check_corpus(COMMANDS_CORPUS);
}

fn check_corpus(corpus: &[&str]) {
    let mut mismatches = Vec::new();
    let mut checked = 0;

    for &sql in corpus {
        if !pg_accepts(sql) {
            // Our own corpus entry is not valid PG (or version skew) — skip but warn.
            eprintln!("note: PG rejected corpus entry, skipping: {sql}");
            continue;
        }
        checked += 1;
        let parse = banshee_parser::parse(sql);
        if !parse.errors().is_empty() {
            mismatches.push(format!("  {sql}\n    -> {:?}", parse.errors()));
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} of {checked} PG-valid statements produced banshee errors:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

#[test]
fn oracle_valid_fixtures_match_postgres() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("fixtures")
        .join("valid");

    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("fixtures/valid")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("sql"))
        .collect();
    entries.sort_by_key(|e| e.path());

    let mut mismatches = Vec::new();
    for entry in entries {
        let path = entry.path();
        let sql = std::fs::read_to_string(&path).unwrap();
        if !pg_accepts(&sql) {
            eprintln!("note: PG rejected fixture {}, skipping", path.display());
            continue;
        }
        let parse = banshee_parser::parse(&sql);
        if !parse.errors().is_empty() {
            mismatches.push(format!("  {}\n    -> {:?}", path.display(), parse.errors()));
        }
    }

    assert!(
        mismatches.is_empty(),
        "PG-valid fixtures produced banshee errors:\n{}",
        mismatches.join("\n")
    );
}
