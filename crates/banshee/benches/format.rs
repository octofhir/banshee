use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

const FORMAT_QUERY: &str = "SELECT id, name FROM users WHERE active = true;";

fn bench_format(c: &mut Criterion) {
    c.bench_function("format_sqlstyle", |b| {
        b.iter(|| banshee_format::format_sqlstyle(black_box(FORMAT_QUERY)))
    });
}

criterion_group!(benches, bench_format);
criterion_main!(benches);
