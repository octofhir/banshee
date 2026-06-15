//! Shared analysis pipeline used by `lint` and `fix`.
//!
//! Translates [`BansheeConfig`] lint settings into [`AnalysisOptions`], runs the
//! analyzer with a schema-less provider, and filters diagnostics by the
//! configured rule selection.

use banshee_config::{BansheeConfig, SeverityLevel};
use banshee_hir::{
    AnalysisOptions, BuiltinLintPack, Diagnostic, MigrationColumns, NullSchemaProvider,
    SchemaProvider, Severity, analyze_query_with_options, collect_migration_columns,
};

use super::io::InputFile;

/// The result of analyzing one source file.
pub struct Analyzed {
    /// Lint findings and syntax errors surviving rule selection.
    pub diagnostics: Vec<Diagnostic>,
}

/// Builds cumulative column knowledge across every input file, so migration
/// rules can reason about a column's type history (e.g. MG06 safe widenings).
pub fn migration_columns(inputs: &[InputFile]) -> MigrationColumns {
    let roots: Vec<_> = inputs
        .iter()
        .map(|input| banshee_parser::parse(&input.text).syntax())
        .collect();
    collect_migration_columns(&roots)
}

/// Parses and analyzes `text` under the given configuration.
///
/// Without a live schema (no `[database]` connection), reference-resolution
/// diagnostics (unknown table/column) are false positives, so only keyed lint
/// findings are kept. Once a schema provider is wired in, semantic diagnostics
/// can be surfaced too.
pub fn analyze(
    text: &str,
    config: &BansheeConfig,
    provider: Option<&dyn SchemaProvider>,
    migration_columns: &MigrationColumns,
) -> Analyzed {
    // Substitute placeholders first, when a templater style is configured, so
    // parameterised SQL parses. Diagnostics are mapped back to `text` below.
    let templated = config
        .templater
        .style
        .map(|style| banshee_templater::render(text, style));
    let input: &str = templated.as_ref().map_or(text, |t| t.text());

    let parse = banshee_parser::parse(input);

    let mut options = build_options(config);
    options.migration_columns = migration_columns.clone();
    let null = NullSchemaProvider;
    let active: &dyn SchemaProvider = provider.unwrap_or(&null);
    let analysis = analyze_query_with_options(&parse, active, &options);

    let schema_aware = provider.is_some();

    let mut diagnostics: Vec<Diagnostic> = analysis
        .diagnostics
        .into_iter()
        .filter(|d| match d.code {
            // Unkeyed semantic diagnostics are only trustworthy with a schema.
            None => schema_aware,
            // Reference checks (RF*) need a schema; suppress them without one.
            Some(code) if code.is_reference() => {
                schema_aware && config.lint.is_rule_enabled(code.as_str())
            }
            Some(code) => config.lint.is_rule_enabled(code.as_str()),
        })
        .map(|mut d| {
            if let Some(code) = d.code
                && let Some(level) = config.lint.severity_for(code.as_str())
            {
                d.severity = map_severity(level).unwrap_or(d.severity);
            }
            d
        })
        .collect();

    // Surface syntax errors alongside lint findings so they render too.
    for err in parse.errors() {
        diagnostics.push(Diagnostic::error(err.message.clone()).with_range(err.range));
    }

    // Translate ranges from the rendered text back to the original source, and
    // drop any fix that would edit across a substituted placeholder.
    if let Some(t) = &templated {
        for d in &mut diagnostics {
            if let Some(range) = d.range {
                d.range = Some(t.map_range(range));
            }
            for related in &mut d.related {
                related.range = related.range.map(|r| t.map_range(r));
            }
            d.fixes
                .retain(|fix| fix.edits.iter().all(|e| !t.touches_placeholder(e.range)));
            for fix in &mut d.fixes {
                for edit in &mut fix.edits {
                    edit.range = t.map_range(edit.range);
                }
            }
        }
    }

    Analyzed { diagnostics }
}

/// Builds analyzer options, enabling the capitalisation pack only when at least
/// one of its rules is active (it is off by default).
fn build_options(config: &BansheeConfig) -> AnalysisOptions {
    let mut packs = vec![
        BuiltinLintPack::Core,
        BuiltinLintPack::Jsonb,
        BuiltinLintPack::Convention,
        BuiltinLintPack::Migration,
    ];
    if config.lint.is_rule_enabled("CP01") || config.lint.is_rule_enabled("CP02") {
        packs.push(BuiltinLintPack::Capitalisation);
    }
    let mut options = AnalysisOptions::new().with_builtin_lint_packs(packs);
    options.rule_options = config.lint.rule_options();
    options
}

fn map_severity(level: SeverityLevel) -> Option<Severity> {
    match level {
        SeverityLevel::Error => Some(Severity::Error),
        SeverityLevel::Warning => Some(Severity::Warning),
        SeverityLevel::Info => Some(Severity::Info),
        SeverityLevel::Off => None,
    }
}
