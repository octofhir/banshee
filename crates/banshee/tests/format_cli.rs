#![cfg(feature = "cli")]

use std::io::Write;
use std::process::{Command, Output, Stdio};

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

fn format_stdin(command: &mut Command, sql: &str) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run format on stdin");
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(sql.as_bytes())
        .expect("supply SQL");
    child.wait_with_output().expect("collect output")
}

#[test]
fn all_presets_reject_missing_select_items_and_statement_separators() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    for style in ["sqlstyle", "pgformatter", "compact"] {
        std::fs::write(
            dir.path().join("banshee.toml"),
            format!("[format]\nstyle = \"{style}\"\n"),
        )
        .expect("write preset");
        let format = |sql: &str| {
            format_stdin(
                Command::new(env!("CARGO_BIN_EXE_banshee"))
                    .current_dir(dir.path())
                    .args(["format", "-"]),
                sql,
            )
        };
        for source in [
            "SELECT ,;",
            "SELECT a, FROM t;",
            "SELECT a +, b;",
            "SELECT 1 SELECT 2;",
            "SELECT 1e+;",
            "SELECT 123abc;",
            "SELECT 0b102;",
        ] {
            let result = format(source);
            assert_eq!(result.status.code(), Some(2), "{style}: {source}");
            assert!(result.stdout.is_empty());
        }
        assert!(format("SELECT FROM t;").status.success());
    }
}

#[test]
fn format_preserves_the_effect_of_noqa_in_all_presets() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    let protected = "SELECT * FROM t; -- noqa: AM04\r\n";
    let source = format!("{protected}select  1;");
    for style in ["sqlstyle", "pgformatter", "compact"] {
        std::fs::write(
            dir.path().join("banshee.toml"),
            format!("[format]\nstyle = \"{style}\"\n[format.pgformatter]\nno-comment = true\n"),
        )
        .expect("write preset");
        let run = |args: &[&str], sql: &str| {
            format_stdin(
                Command::new(env!("CARGO_BIN_EXE_banshee"))
                    .current_dir(dir.path())
                    .args(args),
                sql,
            )
        };
        let lint = ["lint", "--format", "json", "-"];
        let before = run(&lint, &source);
        assert!(!String::from_utf8_lossy(&before.stdout).contains("\"AM04\""));
        let formatted = run(&["format", "-"], &source);
        assert!(formatted.status.success(), "{formatted:?}");
        let output = String::from_utf8(formatted.stdout).expect("formatted UTF-8");
        assert!(output.starts_with(protected), "{style}: {output:?}");
        assert_ne!(source, output);
        let after = run(&lint, &output);
        assert!(after.status.success(), "{style}: {after:?}");
        assert!(!String::from_utf8_lossy(&after.stdout).contains("\"AM04\""));
    }
}

#[test]
fn format_lint_and_fix_agree_on_native_operators_and_parameters() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    for (style, native, template, expression) in [
        (
            "question-mark",
            "SELECT data ? 'key' AS key_exists FROM t;",
            "select data ? ? as key_exists from t;",
            "data ? ?",
        ),
        (
            "percent",
            "SELECT 20%s AS remainder FROM t;",
            "select n, %s as parameter from t;",
            "%s",
        ),
    ] {
        std::fs::write(
            dir.path().join("banshee.toml"),
            format!("[format]\nstyle = \"compact\"\n[templater]\nstyle = \"{style}\"\n"),
        )
        .expect("write preset");
        let run = |args: &[&str], sql: &str| {
            format_stdin(
                Command::new(env!("CARGO_BIN_EXE_banshee"))
                    .current_dir(dir.path())
                    .args(args),
                sql,
            )
        };
        let lint = ["lint", "--format", "json", "-"];
        let checked = run(&lint, native);
        assert!(checked.status.success(), "{style}: {checked:?}");
        for command in ["format", "fix"] {
            let result = run(&[command, "-"], template);
            assert!(result.status.success(), "{style}: {result:?}");
            let sql = String::from_utf8(result.stdout).expect("output UTF-8");
            assert!(sql.contains(expression), "{command}: {sql:?}");
            assert!(sql.starts_with("SELECT"), "{command}: {sql:?}");
            let checked = run(&lint, &sql);
            assert!(checked.status.success(), "{style}: {checked:?}");
        }
    }
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
        &["bad.sql"][..],
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
fn stdout_formats_one_input_and_batch_modes_handle_multiple_files() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    let first = dir.path().join("first.sql");
    let second = dir.path().join("second.sql");
    std::fs::write(&first, "select 1").expect("write first SQL");
    std::fs::write(&second, "select 2 -- trailing comment").expect("write second SQL");

    let one = cli(&dir)
        .arg("first.sql")
        .output()
        .expect("format one file");
    assert!(one.status.success(), "{one:?}");
    assert_eq!(one.stdout, b"SELECT 1\n");

    for args in [&["first.sql", "second.sql"][..], &["."][..]] {
        let output = cli(&dir).args(args).output().expect("run format");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("--write or --check"), "{error}");
    }

    let before = cli(&dir)
        .args(["--check", "."])
        .output()
        .expect("run check");
    assert_eq!(before.status.code(), Some(1));
    let write = cli(&dir)
        .args(["--write", "."])
        .output()
        .expect("run write");
    assert!(write.status.success(), "{write:?}");
    assert_eq!(
        std::fs::read_to_string(first).expect("read SQL"),
        "SELECT 1\n"
    );
    assert_eq!(
        std::fs::read_to_string(second).expect("read SQL"),
        "SELECT 2 -- trailing comment\n"
    );
    let after = cli(&dir)
        .args(["--check", "."])
        .output()
        .expect("run check");
    assert_eq!(after.status.code(), Some(0));
}

#[test]
fn stdin_format_error_produces_no_sql() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    let output = format_stdin(cli(&dir).arg("-"), "select 'unterminated");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn stdin_filename_uses_the_files_config_and_diagnostic_location() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    let project = dir.path().join("project");
    std::fs::create_dir(&project).expect("create project");
    std::fs::write(
        project.join("banshee.toml"),
        "[format]\nstyle = \"pgformatter\"\n",
    )
    .expect("write project config");
    let args = ["--stdin-filename", "project/unsaved.sql", "-"];

    let output = format_stdin(cli(&dir).args(args), "select 1");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"SELECT\n    1\n");
    assert!(!project.join("unsaved.sql").exists());

    let output = format_stdin(
        cli(&dir).args(args).args(["--config", "banshee.toml"]),
        "select 1",
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"SELECT 1\n");

    let output = format_stdin(cli(&dir).args(args), "select 'unterminated");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsaved.sql"));

    let output = cli(&dir)
        .args(["--stdin-filename", "unsaved.sql", "other.sql"])
        .output()
        .expect("reject a path with stdin filename");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires stdin input"));
}
