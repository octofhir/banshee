use criterion::{Criterion, criterion_group, criterion_main};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;
use std::hint::black_box;

const SIMPLE_QUERY: &str = "SELECT id FROM users WHERE active = true;";
const COMPLEX_QUERY: &str =
    "WITH active AS (SELECT id FROM users WHERE active = true) SELECT * FROM active;";
const LARGE_SQL: &str = include_str!("data/large.sql");

fn bench_parse(c: &mut Criterion) {
    let dialect = PostgreSqlDialect {};
    c.bench_function("parse_simple", |b| {
        b.iter(|| banshee_parser::parse(black_box(SIMPLE_QUERY)))
    });

    c.bench_function("sqlparser_parse_simple", |b| {
        b.iter(|| Parser::parse_sql(&dialect, black_box(SIMPLE_QUERY)))
    });

    c.bench_function("parse_complex_cte", |b| {
        b.iter(|| banshee_parser::parse(black_box(COMPLEX_QUERY)))
    });

    c.bench_function("sqlparser_parse_complex_cte", |b| {
        b.iter(|| Parser::parse_sql(&dialect, black_box(COMPLEX_QUERY)))
    });

    c.bench_function("parse_large_file", |b| {
        b.iter(|| banshee_parser::parse(black_box(LARGE_SQL)))
    });

    c.bench_function("sqlparser_parse_large_file", |b| {
        b.iter(|| Parser::parse_sql(&dialect, black_box(LARGE_SQL)))
    });
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
