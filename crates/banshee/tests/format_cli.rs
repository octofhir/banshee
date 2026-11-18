#![cfg(feature = "cli")]

use std::io::Write;
use std::process::{Command, Stdio};

fn cli(dir: &tempfile::TempDir) -> Command {
    std::fs::write(
        dir.path().join("banshee.toml"),
        "[format]\nstyle = \"compact\"\n",
    )
    .expect("write test config");
    let mut command = Command::new(env!("CARGO_BIN_EXE_banshee"));
    command.current_dir(dir.path()).arg("format");
    command
}

#[test]
fn write_and_check_preserve_the_filter_and_terminator() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    let query = dir.path().join("query.sql");
    std::fs::write(&query, "select a from t where x -- note\n  and y;\n").expect("write SQL");

    let before = cli(&dir)
        .args(["--check", "query.sql"])
        .output()
        .expect("run format --check");
    assert_eq!(before.status.code(), Some(1));

    let write = cli(&dir)
        .args(["--write", "query.sql"])
        .output()
        .expect("run format --write");
    assert!(write.status.success(), "{:?}", write);
    let formatted = std::fs::read_to_string(query).expect("read formatted SQL");
    assert!(formatted.ends_with(";\n"), "{formatted:?}");
    assert!(formatted.contains("-- note\n"), "{formatted:?}");
    assert!(formatted.contains("AND y;"), "{formatted:?}");

    let after = cli(&dir)
        .args(["--check", "query.sql"])
        .output()
        .expect("run format --check");
    assert_eq!(after.status.code(), Some(0));
}

#[test]
fn a_format_error_leaves_the_batch_untouched() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    let good = dir.path().join("good.sql");
    let bad = dir.path().join("bad.sql");
    let good_sql = "select a from t;\n";
    let bad_sql = "select a from (";
    std::fs::write(&good, good_sql).expect("write good SQL");
    std::fs::write(&bad, bad_sql).expect("write bad SQL");

    for args in [
        &["good.sql", "bad.sql"][..],
        &["--check", "good.sql", "bad.sql"][..],
        &["--write", "good.sql", "bad.sql"][..],
    ] {
        let output = cli(&dir).args(args).output().expect("run format");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("bad.sql"));
        assert_eq!(
            std::fs::read_to_string(&good).expect("read good SQL"),
            good_sql
        );
        assert_eq!(
            std::fs::read_to_string(&bad).expect("read bad SQL"),
            bad_sql
        );
    }
}

#[test]
fn stdin_format_error_produces_no_sql() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    let mut child = cli(&dir)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run format on stdin");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(b"select 'unterminated")
        .expect("supply SQL");
    let output = child.wait_with_output().expect("collect output");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}
