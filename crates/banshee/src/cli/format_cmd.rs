//! `banshee format` — format SQL to stdout, in place, or check formatting.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use rayon::prelude::*;

use super::io::{self, gather_inputs};
use super::{Cli, exit};

#[derive(Args, Debug)]
pub struct FormatArgs {
    /// One file for stdout, or files and directories with --write or --check.
    /// Use `-` or omit to read stdin.
    paths: Vec<PathBuf>,

    /// Use this file's location for config discovery and diagnostics with stdin.
    #[arg(long, value_name = "PATH", conflicts_with = "write")]
    stdin_filename: Option<PathBuf>,

    /// Rewrite files in place instead of printing to stdout.
    #[arg(long, conflicts_with = "check")]
    write: bool,

    /// Exit non-zero if any input is not already formatted (no output).
    #[arg(long)]
    check: bool,

    /// In --check mode, do not print the per-file "would reformat" lines.
    #[arg(long)]
    quiet: bool,
}

pub fn run(args: &FormatArgs, cli: &Cli) -> Result<u8> {
    let reads_stdin = io::reads_stdin(&args.paths);
    if args.write && reads_stdin {
        bail!("--write requires file inputs, not stdin");
    }
    let anchor = if let Some(filename) = &args.stdin_filename {
        if !reads_stdin {
            bail!("--stdin-filename requires stdin input");
        }
        let absolute = std::path::absolute(filename)?;
        absolute.parent().unwrap_or(&absolute).to_path_buf()
    } else {
        io::discovery_anchor(&args.paths)
    };

    let config = cli.load_config(&anchor)?;
    let mut inputs = gather_inputs(&args.paths)?;
    if let (Some(filename), [input]) = (&args.stdin_filename, inputs.as_mut_slice()) {
        input.label = filename.display().to_string();
    }
    if !args.write && !args.check && inputs.len() > 1 {
        bail!("stdout formatting requires one input. Use --write or --check for multiple files");
    }

    // Check every input before printing output or changing any file.
    let candidates: Vec<_> = inputs
        .par_iter()
        .map(|i| config.try_format(&i.text))
        .collect();
    let formatted = inputs
        .iter()
        .zip(candidates)
        .map(|(input, result)| result.with_context(|| format!("cannot format {}", input.label)))
        .collect::<Result<Vec<_>>>()?;

    let mut unformatted = 0usize;
    for (input, formatted) in inputs.iter().zip(&formatted) {
        if args.check {
            if *formatted != input.text {
                unformatted += 1;
                if !args.quiet {
                    eprintln!("would reformat: {}", input.label);
                }
            }
            continue;
        }

        if let Some(path) = input.path.as_ref().filter(|_| args.write) {
            if *formatted != input.text {
                std::fs::write(path, formatted)?;
            }
        } else {
            print!("{formatted}");
        }
    }

    if args.check && unformatted > 0 {
        if !args.quiet {
            eprintln!("{unformatted} file(s) need formatting");
        }
        return Ok(exit::FINDINGS);
    }
    Ok(exit::OK)
}
