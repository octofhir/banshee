//! Shared CLI input handling and source editing.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use banshee_hir::TextEdit;

/// A unit of SQL to operate on: either a file or stdin.
pub struct InputFile {
    /// Display label (the path, or `<stdin>`).
    pub label: String,
    /// Backing path, if read from disk. `None` for stdin.
    pub path: Option<PathBuf>,
    /// The SQL source text.
    pub text: String,
}

/// Collects inputs from the given paths.
///
/// With no paths, or a single `-`, reads stdin. Directories are walked
/// recursively for `*.sql` files.
pub fn gather_inputs(paths: &[PathBuf]) -> Result<Vec<InputFile>> {
    let use_stdin = paths.is_empty() || (paths.len() == 1 && paths[0].as_os_str() == "-");
    if use_stdin {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .context("failed to read stdin")?;
        return Ok(vec![InputFile {
            label: "<stdin>".to_string(),
            path: None,
            text,
        }]);
    }

    let mut inputs = Vec::new();
    for path in paths {
        if path.is_dir() {
            collect_sql_files(path, &mut inputs)?;
        } else {
            inputs.push(read_file(path)?);
        }
    }
    Ok(inputs)
}

/// Drops inputs whose path matches any `exclude-paths` glob (e.g. legacy
/// migrations). stdin and other path-less inputs are always kept. Returns an
/// error if a pattern is not a valid glob.
pub fn apply_exclude_paths(inputs: Vec<InputFile>, patterns: &[String]) -> Result<Vec<InputFile>> {
    if patterns.is_empty() {
        return Ok(inputs);
    }
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        let glob = globset::Glob::new(pattern)
            .with_context(|| format!("invalid exclude-paths glob: {pattern}"))?;
        builder.add(glob);
    }
    let set = builder
        .build()
        .context("failed to build the exclude-paths matcher")?;
    Ok(inputs
        .into_iter()
        .filter(|input| match &input.path {
            Some(path) => !set.is_match(path),
            None => true,
        })
        .collect())
}

/// The directory a path lives in, for config discovery; falls back to cwd.
pub fn discovery_anchor(paths: &[PathBuf]) -> PathBuf {
    paths
        .iter()
        .find(|p| p.as_os_str() != "-")
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."))
}

fn read_file(path: &Path) -> Result<InputFile> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(InputFile {
        label: path.display().to_string(),
        path: Some(path.to_path_buf()),
        text,
    })
}

fn collect_sql_files(dir: &Path, out: &mut Vec<InputFile>) -> Result<()> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("failed to read dir {}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            // Skip hidden and dependency directories.
            let skip = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.') || n == "target" || n == "node_modules");
            if !skip {
                collect_sql_files(&path, out)?;
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("sql") {
            out.push(read_file(&path)?);
        }
    }
    Ok(())
}

/// Applies a set of text edits to `source`.
///
/// On overlap the longer edit wins, so a structural rewrite (e.g. removing
/// `ELSE NULL`) takes precedence over a token tweak it contains (e.g.
/// upper-casing that `NULL`). Selected edits are applied right-to-left so
/// earlier offsets stay valid.
pub fn apply_edits(source: &str, mut edits: Vec<TextEdit>) -> String {
    // Longest edit first at each start position.
    edits.sort_by_key(|e| {
        let start = u32::from(e.range.start());
        let len = u32::from(e.range.end()) - start;
        (start, std::cmp::Reverse(len))
    });

    // Greedily keep non-overlapping edits, preferring the longer one.
    let mut selected: Vec<&TextEdit> = Vec::new();
    let mut covered_to = 0u32;
    for edit in &edits {
        let start = u32::from(edit.range.start());
        let end = u32::from(edit.range.end());
        if start < covered_to {
            continue; // overlaps an already-selected (longer/earlier) edit
        }
        selected.push(edit);
        covered_to = end;
    }

    let mut result = source.to_string();
    for edit in selected.into_iter().rev() {
        let start = u32::from(edit.range.start()) as usize;
        let end = u32::from(edit.range.end()) as usize;
        result.replace_range(start..end, &edit.new_text);
    }
    result
}

/// Escapes a string as a JSON string literal (including surrounding quotes).
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Converts a byte offset into 1-based line and column numbers.
pub fn line_col(source: &str, offset: u32) -> (usize, usize) {
    let offset = offset as usize;
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}
