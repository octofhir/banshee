//! `banshee fix` — apply autofixes from fixable lint rules.

use std::path::PathBuf;

use anyhow::Result;
use banshee_hir::TextEdit;
use clap::Args;
use rayon::prelude::*;

use super::analysis;
use super::io::{self, apply_edits, gather_inputs};
use super::{Cli, exit};

#[derive(Args, Debug)]
pub struct FixArgs {
    /// Files or directories to fix. Use `-` or omit to read stdin.
    paths: Vec<PathBuf>,

    /// Print a unified-ish diff instead of writing changes.
    #[arg(long)]
    diff: bool,

    /// Write changes to files. Implied unless `--diff` or stdin is used.
    #[arg(long)]
    write: bool,

    /// Do not print the per-file "fixed:" lines.
    #[arg(long)]
    quiet: bool,
}

pub fn run(args: &FixArgs, cli: &Cli) -> Result<u8> {
    let config = cli.load_config(&io::discovery_anchor(&args.paths))?;
    let inputs = gather_inputs(&args.paths)?;
    let provider = super::schema::resolve(&config)?;
    let provider_ref = provider
        .as_ref()
        .map(|p| p as &dyn banshee_hir::SchemaProvider);

    let columns = analysis::migration_columns(&inputs);

    // Compute the fixed text per file in parallel; `None` means unchanged.
    let fixed: Vec<Option<String>> = inputs
        .par_iter()
        .map(|input| {
            let analyzed = analysis::analyze(&input.text, &config, provider_ref, &columns);
            let edits: Vec<TextEdit> = analyzed
                .diagnostics
                .iter()
                .flat_map(|d| d.fixes.iter())
                .flat_map(|f| f.edits.iter().cloned())
                .collect();
            if edits.is_empty() {
                return None;
            }
            let out = apply_edits(&input.text, edits);
            (out != input.text).then_some(out)
        })
        .collect();

    let mut fixed_files = 0usize;
    for (input, fixed) in inputs.iter().zip(&fixed) {
        let Some(fixed) = fixed else {
            // Nothing to fix: pass stdin through unchanged.
            if input.path.is_none() && !args.diff {
                print!("{}", input.text);
            }
            continue;
        };
        fixed_files += 1;

        if args.diff {
            print_diff(&input.label, &input.text, fixed);
        } else if let Some(path) = &input.path {
            std::fs::write(path, fixed)?;
            if !args.quiet {
                eprintln!("fixed: {}", input.label);
            }
        } else {
            // stdin: emit fixed source to stdout.
            print!("{fixed}");
        }
    }

    if args.diff && fixed_files > 0 {
        return Ok(exit::FINDINGS);
    }
    Ok(exit::OK)
}

/// Prints a minimal line-oriented diff. Not a full unified diff, but enough to
/// review changes at a glance without pulling in a diff dependency.
fn print_diff(label: &str, before: &str, after: &str) {
    println!("--- {label}");
    println!("+++ {label} (fixed)");
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    let max = before_lines.len().max(after_lines.len());
    for i in 0..max {
        match (before_lines.get(i), after_lines.get(i)) {
            (Some(b), Some(a)) if b == a => {}
            (Some(b), Some(a)) => {
                println!("-{b}");
                println!("+{a}");
            }
            (Some(b), None) => println!("-{b}"),
            (None, Some(a)) => println!("+{a}"),
            (None, None) => {}
        }
    }
}
