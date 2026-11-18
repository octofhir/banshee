#![cfg(feature = "cli")]

use std::process::{Command, Output};

fn fix(dir: &tempfile::TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_banshee"))
        .current_dir(dir.path())
        .arg("fix")
        .args(args)
        .output()
        .expect("run banshee fix")
}

#[test]
fn batch_fix_preserves_call_spelling_and_requires_opt_in_for_sql_rewrites() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    let path = dir.path().join("query.sql");
    std::fs::write(
        &path,
        "select array[1], count(*) from Items;\ncreate index idx on Items(id);\n",
    )
    .expect("write SQL");

    let safe = fix(&dir, &["query.sql"]);
    assert!(safe.status.success(), "{safe:?}");
    assert_eq!(
        std::fs::read_to_string(&path).expect("read fixed SQL"),
        "SELECT array[1], count(*) FROM items;\nCREATE INDEX idx ON items(id);\n"
    );

    let review = fix(&dir, &["--unsafe-fixes", "query.sql"]);
    assert!(review.status.success(), "{review:?}");
    let output = std::fs::read_to_string(&path).expect("read fixed SQL");
    assert!(
        output.contains("CREATE INDEX CONCURRENTLY idx ON items(id)"),
        "{output}"
    );
    assert!(output.contains("array[1], count(*)"), "{output}");
}

#[test]
fn fix_errors_leave_the_batch_untouched() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    let good = dir.path().join("good.sql");
    let invalid = dir.path().join("invalid.sql");
    let copy = dir.path().join("copy.sql");
    let source = "select 1;\n";
    let broken = "select a from (";
    let copy_source = "copy t from stdin;\nselect 1;\n";
    std::fs::write(&good, source).expect("write good SQL");
    std::fs::write(&invalid, broken).expect("write invalid SQL");
    std::fs::write(&copy, copy_source).expect("write copy script");

    for bad in ["invalid.sql", "copy.sql"] {
        let output = fix(&dir, &["good.sql", bad]);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(bad));
        assert_eq!(
            std::fs::read_to_string(&good).expect("read good SQL"),
            source
        );
    }
    assert_eq!(
        std::fs::read_to_string(invalid).expect("read invalid SQL"),
        broken
    );
    assert_eq!(
        std::fs::read_to_string(copy).expect("read copy script"),
        copy_source
    );
}
