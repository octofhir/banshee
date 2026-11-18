use std::fmt::Write;

use banshee_format::{
    FormatConfig, PgFormatterConfig, format_compact, format_pgformatter, format_sqlstyle,
    format_with_pgformatter, try_format,
};
use serde_json::Value;

type Formatter = fn(&str) -> String;

const STYLES: &[(&str, Formatter)] = &[
    ("sqlstyle", format_sqlstyle),
    ("pgformatter", format_pgformatter),
    ("compact", format_compact),
];

const REGRESSIONS: &[&str] = &[
    "select a from orders o where o.id in (select order_id from lines l)",
    "select a from t order by a nulls last",
    "select * from (select a from t order by a collate \"C\") s",
    "select ts at time zone 'UTC' from t",
    "select sum(a) over (order by b rows between unbounded preceding and current row exclude current row) from t",
    "with r as (select 1 a) select r.a from r",
    "with r as (select 1 a) select r.*, r.a from r",
    "select x from a cross join b join c on c.id = b.id",
    "select x from a cross join b cross join lateral (select 1) z where x > 0",
    "select a from t where x and exists (select 1 from u)",
    "select a from t where x or exists (select 1 from u)",
    "select sum(a) over w from t window w as (order by b)",
    "select array(select id from pay) as ids",
    "-- header comment\nselect a, -- after a comma\n       /* block */ b\nfrom t",
    "select a from t\nwhere x -- note\n  and y",
    "select 1 union all select 2",
    "(select 1 union select 2) intersect select 1",
    "select a, sum(b) from t group by grouping sets ((a), ())",
    "select a from t;",
    "select 1;; select 2; -- tail",
    "select * from t order by a asc, b desc nulls last limit 10 offset 5",
    "select Ä from t",
    "with r as (select $$first\nsecond$$ as value) select r.value from r",
    "select 'a'\n'b'",
    "select E'a'\r\n'b\\'c'",
    "select a from t where x -- note\r  and y",
    "select 1 /* ; /* nested */ */; -- ;",
    "select x from t where x != 1 and x > -2 for no key update of t skip locked;",
    "select a from t fetch first 5 rows only;",
    "delete from t where x = 1 returning t.*, x;",
    "insert into t (x, y) values (1, 2) returning x, y;",
    "select $тег$exact\n   content$тег$;",
    "select E'a\r\nb', 'c\r\nd';",
    "select a from t where x in (1, 2, 3)",
    "select a from t where x = any(array[1, 2, 3])",
    "select * from unnest(array[1, 2]) with ordinality as x(v, n)",
];

fn postgres_tree(sql: &str) -> Value {
    fn strip_locations(tree: &mut Value) {
        match tree {
            Value::Object(fields) => {
                // ParseLoc fields in PostgreSQL's raw AST for this oracle version.
                for field in [
                    "location",
                    "stmt_location",
                    "stmt_len",
                    "list_start",
                    "list_end",
                    "rexpr_list_start",
                    "rexpr_list_end",
                    "name_location",
                ] {
                    fields.remove(field);
                }
                for value in fields.values_mut() {
                    strip_locations(value);
                }
            }
            Value::Array(items) => items.iter_mut().for_each(strip_locations),
            _ => {}
        }
    }

    // Compare the raw tree so the Rust wrapper cannot discard AST fields.
    let (_, raw) =
        pg_parse::parse_debug(sql).unwrap_or_else(|e| panic!("PostgreSQL rejected {sql:?}: {e:?}"));
    let mut tree = serde_json::from_str(&raw).expect("PostgreSQL parse tree JSON");
    strip_locations(&mut tree);
    tree
}

fn comments_and_semicolons(sql: &str) -> Vec<&str> {
    use banshee_syntax::SyntaxKind::{BLOCK_COMMENT, LINE_COMMENT, SEMICOLON};

    let mut start = 0;
    banshee_lexer::tokenize(sql)
        .into_iter()
        .filter_map(|token| {
            let end = start + usize::from(token.len);
            let text = &sql[start..end];
            start = end;
            matches!(token.kind, BLOCK_COMMENT | LINE_COMMENT | SEMICOLON).then_some(text)
        })
        .collect()
}

#[test]
fn issue_one_round_trips_in_every_preset() {
    for &sql in REGRESSIONS {
        assert_round_trip(sql);
    }
}

fn assert_round_trip(sql: &str) {
    let original = postgres_tree(sql);
    let prefix = sql.lines().next().unwrap_or_default();
    for &(style, format) in STYLES {
        let output = format(sql);
        assert_ne!(sql, output, "{style} did not format {prefix:?}");
        assert!(
            original == postgres_tree(&output),
            "{style} changed the PostgreSQL AST for {prefix:?}"
        );
        assert_eq!(
            comments_and_semicolons(sql),
            comments_and_semicolons(&output),
            "{style} changed comments or semicolons for {prefix:?}"
        );
        assert!(
            output == format(&output),
            "{style} is not idempotent for {prefix:?}"
        );
    }
}

#[test]
fn long_cte_query_round_trips_in_every_preset() {
    let mut sql = "with\n".to_owned();
    for i in 0..600 {
        writeln!(
            sql,
            "q{i} as (select sum(t.x) over w from transactions t cross join users u \
             join orders o on o.user_id = u.id where t.x > 0 -- keep this predicate\n\
             and exists (select 1 from items l where l.order_id = o.id) \
             window w as (order by t.created_at nulls last)),"
        )
        .expect("append CTE to SQL");
    }
    sql.push_str(
        "result as (select array(select * from q599) as value) select result.* from result;",
    );
    assert!(sql.len() > 128 * 1024);
    assert_round_trip(&sql);
}

#[test]
fn comment_removal_is_explicit_and_preserves_the_ast() {
    let sql = "-- header\nselect x from t where x -- inline\n and y /* block */;";
    let config = PgFormatterConfig::default().with_no_comment(true);
    let output = format_with_pgformatter(sql, &config);
    assert_eq!(comments_and_semicolons(&output), [";"]);
    assert_eq!(postgres_tree(sql), postgres_tree(&output));
}

#[test]
fn unsupported_input_is_unchanged_for_library_and_editor_callers() {
    for sql in [
        "select a from (",
        "copy t from stdin;",
        "select 'unterminated",
    ] {
        assert!(try_format(sql, &FormatConfig::sqlstyle()).is_err());
        for &(_, format) in STYLES {
            assert_eq!(format(sql), sql);
        }
        assert!(banshee_format::format_edits(sql, &FormatConfig::sqlstyle()).is_empty());
    }
}
