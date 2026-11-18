#![cfg(feature = "cli")]

use std::process::Command;

#[test]
fn completion_uses_document_settings_and_supports_a_limit_override() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    std::fs::write(
        directory.path().join("banshee.toml"),
        "[completion]\nkeywords = false\nmax-items = 1\n",
    )?;
    std::fs::write(directory.path().join("query.sql"), "SELECT ")?;

    let complete = |args: &[&str]| -> anyhow::Result<String> {
        let output = Command::new(env!("CARGO_BIN_EXE_banshee"))
            .current_dir(directory.path())
            .args(["complete", "--offset", "7"])
            .args(args)
            .arg("query.sql")
            .output()?;
        assert!(output.status.success(), "{output:?}");
        Ok(String::from_utf8(output.stdout)?)
    };

    let limited = complete(&[])?;
    assert_eq!(limited.lines().count(), 1);
    assert!(limited.contains("Function"), "{limited}");

    let unlimited = complete(&["--limit", "0"])?;
    assert!(unlimited.lines().count() > 1);
    assert!(unlimited.lines().all(|line| line.contains("Function")));
    Ok(())
}
