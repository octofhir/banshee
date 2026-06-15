//! `banshee lint` — analyze SQL and report findings.

use std::path::PathBuf;

use anyhow::Result;
use banshee_config::SeverityLevel;
use banshee_hir::{BREAKING, Diagnostic, Severity};
use clap::Args;
use rayon::prelude::*;

use super::io::{self, InputFile, gather_inputs, line_col};
use super::{Cli, ReportFormat, exit};

#[derive(Args, Debug)]
pub struct LintArgs {
    /// Files or directories to lint. Use `-` or omit to read stdin.
    paths: Vec<PathBuf>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = ReportFormat::Human)]
    format: ReportFormat,

    /// Suppress the summary line (findings are still printed).
    #[arg(long)]
    quiet: bool,

    /// Print a per-rule count breakdown after the findings.
    #[arg(long)]
    statistics: bool,

    /// Treat backward-incompatible (breaking) changes as errors that fail the
    /// run, while every other rule stays advisory (reported, but does not affect
    /// the exit code). One run lints the whole pack and gates on breaking
    /// changes — no second invocation needed. Combine with `exclude-paths` to
    /// skip legacy files.
    #[arg(long)]
    breaking: bool,
}

pub fn run(args: &LintArgs, cli: &Cli) -> Result<u8> {
    let mut config = cli.load_config(&io::discovery_anchor(&args.paths))?;
    if args.breaking {
        escalate_breaking(&mut config);
    }
    let inputs = gather_inputs(&args.paths)?;
    let inputs = io::apply_exclude_paths(inputs, &config.lint.exclude_paths)?;
    let provider = super::schema::resolve(&config)?;
    let provider_ref = provider
        .as_ref()
        .map(|p| p as &dyn banshee_hir::SchemaProvider);

    // Analyze in parallel (the provider is Sync), then report in input order so
    // output is deterministic and never interleaved.
    let per_file: Vec<Vec<Diagnostic>> = inputs
        .par_iter()
        .map(|input| super::analysis::analyze(&input.text, &config, provider_ref).diagnostics)
        .collect();
    let total: usize = per_file.iter().map(Vec::len).sum();
    let errors = per_file
        .iter()
        .flatten()
        .filter(|d| d.severity == Severity::Error)
        .count();

    let color = cli.use_color();
    if matches!(args.format, ReportFormat::Sarif) {
        let analyzed_all: Vec<(&InputFile, Vec<Diagnostic>)> =
            inputs.iter().zip(per_file).collect();
        print!("{}", super::render::render_sarif(&analyzed_all));
    } else {
        for (input, diags) in inputs.iter().zip(&per_file) {
            report(args.format, input, diags, color);
        }
        if args.statistics && matches!(args.format, ReportFormat::Human) {
            print_statistics(per_file.iter().flatten());
        }
        if !args.quiet && matches!(args.format, ReportFormat::Human) {
            if total == 0 {
                eprintln!("No issues found.");
            } else {
                eprintln!("{}", summary_line(per_file.iter().flatten()));
            }
        }
    }

    // `--breaking` is a gate: only error-severity findings (the breaking changes)
    // fail the run, so the rest of the lint pack stays advisory and a single
    // invocation both reports everything and blocks on breaking changes. The
    // default contract is unchanged: any finding fails.
    let fail = if args.breaking { errors > 0 } else { total > 0 };
    Ok(if fail { exit::FINDINGS } else { exit::OK })
}

/// Raises the breaking-change rules to error severity (and force-enables them so
/// a project `exclude`/per-rule toggle cannot hide them). Every other rule keeps
/// its configured selection and severity, so one `lint --breaking` run reports
/// the full pack while only breaking changes fail the run.
fn escalate_breaking(config: &mut banshee_config::BansheeConfig) {
    config.lint.enabled = true;
    for code in BREAKING {
        let setting = config
            .lint
            .rules
            .entry(code.as_str().to_string())
            .or_default();
        setting.enabled = Some(true);
        setting.severity = Some(SeverityLevel::Error);
    }
}

fn report(format: ReportFormat, input: &InputFile, diags: &[Diagnostic], color: bool) {
    match format {
        ReportFormat::Human => report_human(input, diags, color),
        ReportFormat::Json => report_json(input, diags),
        ReportFormat::Github => report_github(input, diags),
        // Aggregated separately in `run`.
        ReportFormat::Sarif => {}
    }
}

/// Prints a per-rule count breakdown, most frequent first (ties by code).
fn print_statistics<'a>(diags: impl Iterator<Item = &'a Diagnostic>) {
    use std::collections::BTreeMap;
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for d in diags {
        let code = d.code.map(|c| c.as_str()).unwrap_or("(other)");
        *counts.entry(code).or_default() += 1;
    }
    if counts.is_empty() {
        return;
    }
    let mut rows: Vec<(&str, usize)> = counts.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    eprintln!("\nfindings by rule:");
    for (code, n) in rows {
        eprintln!("  {code:<6} {n}");
    }
}

/// A rustc-style summary, e.g. `found 1 error, 2 warnings`, counting findings
/// by severity instead of a flat `N issue(s)` total.
fn summary_line<'a>(diags: impl Iterator<Item = &'a Diagnostic>) -> String {
    let (mut errors, mut warnings, mut notes) = (0usize, 0usize, 0usize);
    for d in diags {
        match d.severity {
            Severity::Error => errors += 1,
            Severity::Warning => warnings += 1,
            _ => notes += 1,
        }
    }
    let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    let mut parts = Vec::new();
    if errors > 0 {
        parts.push(plural(errors, "error"));
    }
    if warnings > 0 {
        parts.push(plural(warnings, "warning"));
    }
    if notes > 0 {
        parts.push(plural(notes, "note"));
    }
    format!("found {}", parts.join(", "))
}

fn severity_label(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
        Severity::Hint => "hint",
        _ => "warning",
    }
}

fn report_human(input: &InputFile, diags: &[Diagnostic], color: bool) {
    if diags.is_empty() {
        return;
    }
    print!(
        "{}",
        super::render::render(&input.label, &input.text, diags, color)
    );
}

fn report_github(input: &InputFile, diags: &[Diagnostic]) {
    for d in diags {
        let (line, col) = d
            .range
            .map(|r| line_col(&input.text, u32::from(r.start())))
            .unwrap_or((1, 1));
        let level = match d.severity {
            Severity::Error => "error",
            _ => "warning",
        };
        let code = d.code.map(|c| c.as_str()).unwrap_or("");
        println!(
            "::{level} file={},line={},col={}::{} {}",
            input.label, line, col, code, d.message
        );
    }
}

fn report_json(input: &InputFile, diags: &[Diagnostic]) {
    // Hand-rolled JSON to avoid imposing serde on the diagnostic types.
    let items: Vec<String> = diags
        .iter()
        .map(|d| {
            let (line, col) = d
                .range
                .map(|r| line_col(&input.text, u32::from(r.start())))
                .unwrap_or((1, 1));
            format!(
                r#"{{"code":{},"severity":"{}","line":{},"col":{},"message":{},"fixable":{}}}"#,
                json_str(d.code.map(|c| c.as_str())),
                severity_label(d.severity),
                line,
                col,
                io::json_escape(&d.message),
                !d.fixes.is_empty()
            )
        })
        .collect();
    println!(
        r#"{{"file":{},"diagnostics":[{}]}}"#,
        io::json_escape(&input.label),
        items.join(",")
    );
}

fn json_str(s: Option<&str>) -> String {
    match s {
        Some(s) => io::json_escape(s),
        None => "null".to_string(),
    }
}
