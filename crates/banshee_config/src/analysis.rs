//! Configure analysis and project findings back onto source documents.

use std::sync::Arc;

use banshee_hir::{
    AnalysisOptions, BuiltinLintPack, Diagnostic, MigrationColumns, NullSchemaProvider,
    SchemaProvider, Scope, Severity, analyze_query_with_options,
};
use banshee_syntax::Parse;

use crate::{LintSettings, SeverityLevel, Templated};

/// Results shared by CLI and editor analysis.
pub struct ConfiguredAnalysis {
    /// The resolution scope of the parsed input, in parser coordinates.
    pub scope: Arc<Scope>,
    /// Findings and fixes in the original document's coordinates.
    pub diagnostics: Vec<Diagnostic>,
}

impl LintSettings {
    /// Analyzes a parsed document with the configured rules and severity overrides.
    ///
    /// If the input was rendered from a template, pass its source map so
    /// diagnostics and fixes use the original coordinates.
    pub fn analyze(
        &self,
        parse: &Parse,
        provider: Option<&dyn SchemaProvider>,
        migration_columns: MigrationColumns,
        templated: Option<&Templated>,
    ) -> ConfiguredAnalysis {
        let mut packs = vec![
            BuiltinLintPack::Core,
            BuiltinLintPack::Jsonb,
            BuiltinLintPack::Convention,
            BuiltinLintPack::Migration,
        ];
        if self.is_rule_enabled("CP01") || self.is_rule_enabled("CP02") {
            packs.push(BuiltinLintPack::Capitalisation);
        }
        let mut options = AnalysisOptions::new().with_builtin_lint_packs(packs);
        options.rule_options = self.rule_options();
        options.migration_columns = migration_columns;
        let schema_aware = provider.is_some();
        let mut analysis =
            analyze_query_with_options(parse, provider.unwrap_or(&NullSchemaProvider), &options);

        analysis
            .diagnostics
            .retain_mut(|diagnostic| match diagnostic.code {
                None => schema_aware,
                Some(code) if code.is_reference() && !schema_aware => false,
                Some(code) => {
                    match self.severity_for(code.as_str()) {
                        Some(SeverityLevel::Error) => diagnostic.severity = Severity::Error,
                        Some(SeverityLevel::Warning) => diagnostic.severity = Severity::Warning,
                        Some(SeverityLevel::Info) => diagnostic.severity = Severity::Info,
                        Some(SeverityLevel::Off) | None => {}
                    }
                    self.is_rule_enabled(code.as_str())
                }
            });
        analysis.diagnostics.extend(
            parse
                .errors()
                .iter()
                .map(|err| Diagnostic::error(err.message.clone()).with_range(err.range)),
        );
        if let Some(templated) = templated {
            for diagnostic in &mut analysis.diagnostics {
                project_diagnostic(diagnostic, templated);
            }
        }
        ConfiguredAnalysis {
            scope: analysis.scope,
            diagnostics: analysis.diagnostics,
        }
    }
}

fn project_diagnostic(diagnostic: &mut Diagnostic, templated: &Templated) {
    diagnostic.range = diagnostic.range.map(|range| templated.map_range(range));
    for related in &mut diagnostic.related {
        related.range = related.range.map(|range| templated.map_range(range));
    }
    diagnostic.fixes.retain_mut(|fix| {
        fix.edits.iter_mut().all(|edit| {
            let Some(range) = templated.map_edit_range(edit.range) else {
                return false;
            };
            edit.range = range;
            true
        })
    });
}

#[cfg(test)]
mod tests {
    use banshee_hir::{Fix, TextEdit};
    use text_size::TextRange;

    use super::*;

    #[test]
    fn project_fixes_atomically_and_preserve_adjacent_placeholders() {
        let source = "select :first,:second";
        let templated = banshee_templater::render(source, crate::PlaceholderStyle::Colon);
        let comma = TextRange::new(8.into(), 9.into());
        let parameter = TextRange::new(9.into(), 10.into());
        let mut diagnostic = Diagnostic::warning("test")
            .with_range(comma)
            .with_fix(Fix::new("space", vec![TextEdit::replace(comma, ", ")]))
            .with_fix(Fix::new(
                "two edits",
                vec![
                    TextEdit::replace(comma, ", "),
                    TextEdit::replace(parameter, "2"),
                ],
            ));
        project_diagnostic(&mut diagnostic, &templated);
        assert_eq!(diagnostic.fixes.len(), 1);
        let edit = &diagnostic.fixes[0].edits[0];
        assert_eq!(diagnostic.range, Some(edit.range));
        let mut fixed = source.to_owned();
        fixed.replace_range(std::ops::Range::<usize>::from(edit.range), &edit.new_text);
        assert_eq!(fixed, "select :first, :second");
    }
}
