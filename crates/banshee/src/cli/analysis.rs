//! CLI analysis for `lint` and `fix` using the shared configuration pipeline.

use banshee_config::BansheeConfig;
use banshee_hir::{Diagnostic, MigrationColumns, SchemaProvider, collect_migration_columns};

use super::io::InputFile;

/// The result of analyzing one source file.
pub struct Analyzed {
    /// Lint findings and syntax errors surviving rule selection.
    pub diagnostics: Vec<Diagnostic>,
    pub has_parse_errors: bool,
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
    let (parse, templated) = config.templater.parse(text);
    let analysis = config.lint.analyze(
        &parse,
        provider,
        migration_columns.clone(),
        templated.as_ref(),
    );

    Analyzed {
        diagnostics: analysis.diagnostics,
        has_parse_errors: !parse.errors().is_empty(),
    }
}
