use std::fmt::Write;

use banshee_format::{
    CaseOption, CommaStyle, FormatConfig, PgFormatterConfig, format_compact, format_pgformatter,
    format_sqlstyle, format_with_pgformatter, try_format,
};
use serde_json::Value;
use text_size::TextRange;

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
    for &(style, format) in STYLES {
        assert_round_trip_with(sql, &original, style, format);
    }
}

#[test]
fn postgres_literals_and_user_defined_operators_round_trip() {
    for sql in [
        "select 1.,.5,1_000,1.2_3e+4_5,0x_FF,0o17,0b10_10;",
        "select embedding<->$1,embedding<=>$1,embedding<#>$1 from docs;",
        "select a operator(pg_catalog.+) b from t order by a using operator(pg_catalog.<);",
        "select @ -1, 2*-1, 3 + /* note */ 4;",
        "select U&'d!0061t' escape_alias, U&'d!0061t' uescape '!';",
        "select U&\"d!0061t\" uescape '!' from t;",
    ] {
        assert_round_trip(sql);
    }
}

fn assert_round_trip_with(
    sql: &str,
    original: &Value,
    style: &str,
    format: impl Fn(&str) -> String,
) {
    let prefix = sql.lines().next().unwrap_or_default();
    let output = format(sql);
    assert_ne!(sql, output, "{style} did not format {prefix:?}");
    assert!(
        *original == postgres_tree(&output),
        "{style} changed the PostgreSQL AST for {prefix:?}"
    );
    assert_eq!(
        comments_and_semicolons(sql),
        comments_and_semicolons(&output),
        "{style} changed comments or semicolons for {prefix:?}"
    );
    assert_eq!(
        output,
        format(&output),
        "{style} is not idempotent for {sql:?}"
    );
}

#[test]
fn leading_commas_round_trip_with_subscripts_and_json_paths() {
    let sqlstyle = FormatConfig::sqlstyle().with_comma_style(CommaStyle::Leading);
    let compact = FormatConfig::compact().with_comma_style(CommaStyle::Leading);
    let pgformatter = PgFormatterConfig::default()
        .with_comma_start(true)
        .with_keyword_case(CaseOption::Lower);

    for expression in [
        "arr[1]",
        "arr[1:3]",
        "arr[1][2]",
        "data['field']",
        "data -> 'field'",
        "data #>> '{field,value}'",
        "aRrAy[1, 2]",
    ] {
        for separator in [",", "\n  , ", "\r\n\t, ", " /* keep */ ,", " -- keep\n,"] {
            let sql = format!("select {expression}{separator} 2 from t;");
            let original = postgres_tree(&sql);
            assert_round_trip_with(&sql, &original, "sqlstyle with leading commas", |sql| {
                try_format(sql, &sqlstyle).expect("format select list")
            });
            assert_round_trip_with(&sql, &original, "compact with leading commas", |sql| {
                try_format(sql, &compact).expect("format select list")
            });
            assert_round_trip_with(&sql, &original, "pgformatter with leading commas", |sql| {
                banshee_format::pg_format::try_format(sql, &pgformatter)
                    .expect("format select list")
            });
        }
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

#[test]
fn range_formatting_preserves_neighbouring_sql_and_comments() {
    let before = "select 'untouched;';\r\n-- between statements\r\n  ";
    let selected = "select '🦀' as crab from t -- tail  ";
    let after = "\r\n;\r\nselect 3;\r\n";
    let source = format!("{before}{selected}{after}");
    let start = (source.find("from t").expect("selected clause") as u32).into();
    let selection = TextRange::at(start, 6.into());
    let cases = [
        banshee_format::format_range(&source, &FormatConfig::sqlstyle(), selection),
        banshee_format::format_range(&source, &FormatConfig::compact(), selection),
        banshee_format::pg_format::format_range(&source, &PgFormatterConfig::default(), selection),
    ];
    for edits in cases {
        let [edit] = edits.as_slice() else {
            panic!("expected a single edit, got {edits:?}");
        };
        let mut formatted = source.clone();
        formatted.replace_range(std::ops::Range::<usize>::from(edit.range), &edit.new_text);
        assert!(formatted.starts_with(before), "{formatted:?}");
        assert!(formatted.ends_with(after), "{formatted:?}");
        assert!(formatted.contains("-- tail  \r\n"), "{formatted:?}");
        assert_eq!(postgres_tree(&source), postgres_tree(&formatted));
        assert_eq!(
            comments_and_semicolons(&source),
            comments_and_semicolons(&formatted)
        );
    }
}

#[test]
fn range_formatting_validates_selected_sql_with_errors_on_both_sides() {
    let before = "select from;\r\n  ";
    let statement = "select  '🦀; still a literal', array(select id from t);";
    let after = "\r\nselect $unfinished$";
    let source = format!("{before}{statement}{after}");
    let at = (source.find("array").expect("selected expression") as u32).into();
    let selection = TextRange::at(at, 5.into());
    for edits in [
        banshee_format::format_range(&source, &FormatConfig::sqlstyle(), selection),
        banshee_format::format_range(&source, &FormatConfig::compact(), selection),
        banshee_format::pg_format::format_range(&source, &PgFormatterConfig::default(), selection),
    ] {
        assert_eq!(edits.len(), 1, "{edits:?}");
        let formatted = banshee_format::edits::apply_edits(&source, &edits);
        let changed = formatted
            .strip_prefix(before)
            .and_then(|body| body.strip_suffix(after))
            .expect("all neighbouring bytes are intact");
        assert_ne!(statement, changed);
        assert!(changed.contains("array("));
        assert_eq!(postgres_tree(statement), postgres_tree(changed));
    }
    assert!(try_format(&source, &FormatConfig::compact()).is_err());
    assert!(banshee_format::format_edits(&source, &FormatConfig::compact()).is_empty());
}

#[test]
fn range_formatting_skips_broken_and_protected_statements_in_a_selection() {
    let before = "select  1;";
    let broken = "\nselect from;\n";
    let protected = "-- fmt: skip\nselect  2;\n";
    let last = "select  3";
    let source = format!("{before}{broken}{protected}{last}");
    let cfg = FormatConfig::compact();
    let selection = TextRange::at(0.into(), (source.len() as u32).into());
    let edits = banshee_format::format_range(&source, &cfg, selection);
    assert_eq!(edits.len(), 2, "{edits:?}");
    let middle = format!("{broken}{protected}");
    let output = banshee_format::edits::apply_edits(&source, &edits);
    assert_eq!(output, format!("SELECT 1;{middle}SELECT 3"));
    let selection = TextRange::at((before.len() as u32).into(), (middle.len() as u32).into());
    assert!(banshee_format::format_range(&source, &cfg, selection).is_empty());
    let selection = TextRange::at(0.into(), (output.len() as u32).into());
    assert!(banshee_format::format_range(&output, &cfg, selection).is_empty());
}

#[test]
fn recovery_requires_unambiguous_statement_boundaries() {
    let cfg = FormatConfig::compact();
    for source in [
        "select  1 select from;",
        "select (; select  1;",
        "select from; select  1, array[1; select  2;",
        "select from; copy t from stdin;\nselect  1;",
    ] {
        assert!(
            !banshee_parser::parse(source).errors().is_empty(),
            "{source:?}"
        );
        let selection = TextRange::at(0.into(), (source.len() as u32).into());
        assert!(
            banshee_format::format_range(source, &cfg, selection).is_empty(),
            "{source:?}"
        );
    }
}

#[test]
fn routine_bodies_round_trip_and_range_recovery_uses_the_outer_terminator() {
    let routine = "create function f(i int) returns int language sql begin atomic select case when i > 0 then i else 0 end; return  2; end;";
    assert_round_trip(routine);
    assert_round_trip(
        "create procedure p() language sql begin atomic insert into t values (1); end;",
    );

    let tail = "\nselect (";
    let source = format!("{routine}{tail}");
    let offset = (source.find("return").expect("routine body") as u32).into();
    let edits = banshee_format::format_range(
        &source,
        &FormatConfig::compact(),
        TextRange::at(offset, 6.into()),
    );
    let output = banshee_format::edits::apply_edits(&source, &edits);
    let formatted = output
        .strip_suffix(tail)
        .expect("unfinished neighbour preserved");
    assert_ne!(formatted, routine);
    assert_eq!(postgres_tree(formatted), postgres_tree(routine));
}

#[test]
fn pgformatter_range_only_removes_selected_statement_comments() {
    let source = "-- outside\nselect /* inside */ 1;\nselect 2; -- outside too\n";
    let start = (source.find("select").expect("first statement") as u32).into();
    let config = PgFormatterConfig::default().with_no_comment(true);
    let edits =
        banshee_format::pg_format::format_range(source, &config, TextRange::at(start, 6.into()));
    let [edit] = edits.as_slice() else {
        panic!("expected a single edit, got {edits:?}");
    };
    let mut output = source.to_owned();
    output.replace_range(std::ops::Range::<usize>::from(edit.range), &edit.new_text);
    assert_eq!(
        comments_and_semicolons(&output),
        ["-- outside", ";", ";", "-- outside too"]
    );
    assert_eq!(postgres_tree(source), postgres_tree(&output));
}

#[test]
fn formatter_directives_preserve_complete_statements_in_every_preset() {
    let block = concat!(
        "  -- fmt: off  \r\n",
        " select  $$\n-- fmt: on\n$$, aRrAy(select  1);\r\n",
        "  select  2;\r\n",
        "\t-- fmt: on\r\n",
    );
    let skip_next = " -- fmt: skip\r\n\tselect  3;";
    let skip_previous = "select  4;\t-- fmt: skip  \r\n";
    let skip_inside = "select  5,\r\n  -- fmt: skip\r\n  6;";
    let inner_block = "select\r\n  -- fmt: off\r\n  7\r\n  -- fmt: on\r\n  ,   8;";
    let until_eof = "-- fmt: off\r\n  select  9;  ";
    let source = format!(
        "select  'before';\r\n{block}{skip_next}\r\n{skip_previous}{skip_inside}\r\n\
         {inner_block}\r\nselect  'after';\r\n{until_eof}"
    );

    assert_round_trip(&source);
    for &(style, format) in STYLES {
        let output = format(&source);
        for protected in [block, skip_next, skip_previous, skip_inside, inner_block] {
            assert!(output.contains(protected), "{style}: {output:?}");
        }
        assert!(output.ends_with(until_eof), "{style}: {output:?}");
        for editable in ["select  'before';", "select  'after';"] {
            assert!(!output.contains(editable), "{style}: {output:?}");
        }
    }
}

#[test]
fn only_line_comments_on_their_own_line_switch_formatting_off() {
    let source = "select  '-- fmt: off'; -- fmt: off\n\
                  select  $$-- fmt: off$$; /* -- fmt: off */\n\
                  select  3; -- fmt: skip with extra text\nselect  4;";
    assert_round_trip(source);
    for &(style, format) in STYLES {
        let output = format(source);
        for editable in ["select  '--", "select  $$", "select  3", "select  4"] {
            assert!(!output.contains(editable), "{style}: {output:?}");
        }
    }
}

#[test]
fn comment_removal_keeps_directives_and_suppressed_comments() {
    let protected = "-- fmt: off\nselect  /* keep */ 2; -- keep\n-- fmt: on\n";
    let skip = "select  /* also keep */ 3; -- fmt: skip\n";
    let source =
        format!("-- remove\nselect  1;\n{protected}{skip}select  /* remove */ 4; -- remove\n");
    let config = PgFormatterConfig::default().with_no_comment(true);
    let output = format_with_pgformatter(&source, &config);
    assert!(output.contains(protected), "{output:?}");
    assert!(output.contains(skip), "{output:?}");
    assert!(!output.contains("remove"), "{output:?}");
    assert_eq!(postgres_tree(&source), postgres_tree(&output));
    assert_eq!(output, format_with_pgformatter(&output, &config));
}

#[test]
fn range_formatting_respects_directives_outside_the_selection() {
    let before = "select  '🦀';\r\n";
    let block = "-- fmt: off\r\nselect  2;\r\n-- fmt: on\r\n";
    let after = "select  3;\r\n";
    let source = format!("{before}{block}{after}");
    let start = source.find("select  2").expect("suppressed statement") as u32;
    let inside = TextRange::at(start.into(), 6.into());
    let all = TextRange::new(0.into(), (source.len() as u32).into());
    let protected = TextRange::at((before.len() as u32).into(), (block.len() as u32).into());
    let config = FormatConfig::compact();
    let pg_config = PgFormatterConfig::default().with_no_comment(true);

    assert!(banshee_format::format_range(&source, &config, inside).is_empty());
    assert!(banshee_format::pg_format::format_range(&source, &pg_config, inside).is_empty());
    for (edits, expected) in [
        (banshee_format::format_range(&source, &config, all), None),
        (
            banshee_format::pg_format::format_range(&source, &pg_config, all),
            None,
        ),
        (
            banshee_format::format_edits(&source, &config),
            Some(format_compact(&source)),
        ),
        (
            banshee_format::pg_format::format_edits(&source, &pg_config),
            Some(format_with_pgformatter(&source, &pg_config)),
        ),
    ] {
        assert_eq!(edits.len(), 2, "{edits:?}");
        let mut output = source.clone();
        for edit in edits.iter().rev() {
            assert!(
                !edit
                    .range
                    .intersect(protected)
                    .is_some_and(|r| !r.is_empty())
            );
            output.replace_range(std::ops::Range::<usize>::from(edit.range), &edit.new_text);
        }
        assert!(output.contains(block), "{output:?}");
        assert!(!output.starts_with(before), "{output:?}");
        assert!(!output.ends_with(after), "{output:?}");
        assert_eq!(postgres_tree(&source), postgres_tree(&output));
        if let Some(expected) = expected {
            assert_eq!(output, expected);
        }
    }
}

#[test]
fn format_selection_does_not_reinterpret_inline_off_after_a_skipped_statement() {
    let source = "-- fmt: skip\nselect  1; -- fmt: off\nselect  2;\n";
    let all = TextRange::new(0.into(), (source.len() as u32).into());
    let edits = banshee_format::format_range(source, &FormatConfig::compact(), all);
    let [edit] = edits.as_slice() else {
        panic!("expected only the second statement to change, got {edits:?}");
    };
    let mut output = source.to_owned();
    output.replace_range(std::ops::Range::<usize>::from(edit.range), &edit.new_text);
    assert!(output.starts_with("-- fmt: skip\nselect  1; -- fmt: off\n"));
    assert!(output.ends_with("SELECT 2;\n"), "{output:?}");
    assert_eq!(postgres_tree(source), postgres_tree(&output));
}

#[test]
fn directives_do_not_bypass_input_validation() {
    for sql in ["-- fmt: off\ncopy t from stdin;", "-- fmt: off\nselect ("] {
        assert!(try_format(sql, &FormatConfig::compact()).is_err());
    }
}

#[test]
fn calls_keep_their_spelling_and_delimiters_in_every_preset() {
    let source = "select array(select id from pay), aRrAy[1], count(*), \
                  Pg_Catalog.LoWeR('x'), coalesce(g.n, 0), NULLif(g.n, 1), \
                  greatest(g.n, 2), least(g.n, 3), cast(g.n as int) \
                  from Pg_Catalog.GeNeRaTe_SeRiEs(1, 3) as g(n);";
    for case in [
        source,
        "select coalesce(sum(x) over w, 0), array[1] from t window w as (order by y);",
    ] {
        assert_round_trip(case);
    }
    let spellings = [
        "array(",
        "aRrAy[",
        "count(",
        "Pg_Catalog.LoWeR(",
        "coalesce(",
        "NULLif(",
        "greatest(",
        "least(",
        "cast(",
        "Pg_Catalog.GeNeRaTe_SeRiEs(",
    ];
    for (style, format) in STYLES {
        let output = format(source);
        assert!(output.contains("SELECT"), "{style}: {output}");
        for spelling in spellings {
            assert!(
                output.contains(spelling),
                "{style} lost {spelling:?}: {output}"
            );
        }
        let fallback =
            format("select coalesce(sum(x) over w, 0), array[1] from t window w as (order by y);");
        assert!(fallback.contains("coalesce(sum("), "{style}: {fallback}");
        assert!(fallback.contains("array["), "{style}: {fallback}");
        for invalid in ["select array()", "select array"] {
            assert_eq!(format(invalid), invalid);
        }
    }
}

#[test]
fn explicit_function_case_applies_to_unquoted_calls_in_both_layout_paths() {
    let config = PgFormatterConfig::default().with_function_case(banshee_format::CaseOption::Upper);
    let source = "select coalesce(sum(x) over w, 0), array[1], S.\"KeepMe\"(x) from t window w as (order by y)";
    let output = format_with_pgformatter(source, &config);
    assert_eq!(postgres_tree(source), postgres_tree(&output));
    assert!(output.contains("coalesce(SUM("), "{output}");
    assert!(output.contains("array["), "{output}");
    assert!(output.contains("S.\"KeepMe\"("), "{output}");
}
