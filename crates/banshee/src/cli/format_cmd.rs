//! `banshee format` — format SQL to stdout, in place, or check formatting.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use rayon::prelude::*;

use super::io::{self, gather_inputs};
use super::{Cli, exit};

#[derive(Args, Debug)]
pub struct FormatArgs {
    /// Files or directories to format. Use `-` or omit to read stdin.
    paths: Vec<PathBuf>,

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
    let config = cli.load_config(&io::discovery_anchor(&args.paths))?;
    let inputs = gather_inputs(&args.paths)?;

    if args.write && inputs.iter().any(|input| input.path.is_none()) {
        bail!("--write requires file inputs, not stdin");
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
