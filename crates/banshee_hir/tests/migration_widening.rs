//! Cross-file schema-aware MG06: a column's type history across the whole
//! migration set decides whether an `ALTER COLUMN TYPE` is a safe widening
//! (not flagged) or a narrowing / unknown change (flagged).

use banshee_hir::{
    AnalysisOptions, BuiltinLintPack, NullSchemaProvider, analyze_query_with_options,
    collect_migration_columns,
};
use banshee_parser::parse;

/// Runs the migration pack over `alter_sql` with column history built from
/// `history` (the prior migrations), and returns whether MG06 fired.
fn mg06_fires(history: &[&str], alter_sql: &str) -> bool {
    let mut roots: Vec<_> = history.iter().map(|s| parse(s).syntax()).collect();
    roots.push(parse(alter_sql).syntax());
    let columns = collect_migration_columns(&roots);

    let options = AnalysisOptions::new()
        .with_builtin_lint_packs([BuiltinLintPack::Migration])
        .with_migration_columns(columns);

    let parsed = parse(alter_sql);
    let analysis = analyze_query_with_options(&parsed, &NullSchemaProvider, &options);
    analysis
        .diagnostics
        .iter()
        .any(|d| d.code.map(|c| c.as_str()) == Some("MG06"))
}

#[test]
fn safe_widenings_are_not_flagged() {
    let history = ["CREATE TABLE public.t (id int, code varchar(50), amount real);"];
    assert!(
        !mg06_fires(
            &history,
            "ALTER TABLE public.t ALTER COLUMN id TYPE bigint;"
        ),
        "int -> bigint is a safe widening"
    );
    assert!(
        !mg06_fires(
            &history,
            "ALTER TABLE public.t ALTER COLUMN code TYPE text;"
        ),
        "varchar(50) -> text is a safe widening"
    );
    assert!(
        !mg06_fires(
            &history,
            "ALTER TABLE public.t ALTER COLUMN code TYPE varchar(100);"
        ),
        "varchar(50) -> varchar(100) is a safe widening"
    );
    assert!(
        !mg06_fires(
            &history,
            "ALTER TABLE public.t ALTER COLUMN amount TYPE double precision;"
        ),
        "real -> double precision is a safe widening"
    );
}

#[test]
fn narrowings_are_flagged() {
    let history = ["CREATE TABLE public.t (id bigint, code varchar(100));"];
    assert!(
        mg06_fires(&history, "ALTER TABLE public.t ALTER COLUMN id TYPE int;"),
        "bigint -> int is a narrowing"
    );
    assert!(
        mg06_fires(
            &history,
            "ALTER TABLE public.t ALTER COLUMN code TYPE varchar(50);"
        ),
        "varchar(100) -> varchar(50) is a narrowing"
    );
}

#[test]
fn unknown_history_stays_conservative() {
    // No CREATE TABLE for the column → cannot prove widening → flag.
    assert!(
        mg06_fires(&[], "ALTER TABLE public.t ALTER COLUMN id TYPE bigint;"),
        "without type history MG06 must stay conservative"
    );
}

#[test]
fn double_retype_is_flagged() {
    // int -> bigint -> int: the column is retyped twice, so the original type is
    // not a reliable one-step comparison; both changes must be flagged.
    let history = [
        "CREATE TABLE public.t (id int);",
        "ALTER TABLE public.t ALTER COLUMN id TYPE bigint;",
    ];
    assert!(
        mg06_fires(&history, "ALTER TABLE public.t ALTER COLUMN id TYPE int;"),
        "a column retyped more than once is never treated as a safe widening"
    );
}
