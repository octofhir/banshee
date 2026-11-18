use banshee_format::{FormatError, TextEdit, diff_edits, edits::apply_edits, pg_format};
use banshee_syntax::Parse;
use text_size::{TextRange, TextSize};

use crate::{BansheeConfig, PlaceholderStyle, StylePreset, Templated};

impl BansheeConfig {
    /// Formats SQL using the configured style and placeholders.
    #[must_use]
    pub fn format(&self, source: &str) -> String {
        self.try_format(source)
            .unwrap_or_else(|_| source.to_owned())
    }

    /// Formats SQL and reports failures in the original file's byte offsets.
    pub fn try_format(&self, source: &str) -> Result<String, FormatError> {
        let (parse, template) = self.templater.parse(source);
        let sql = template.as_ref().map_or(source, Templated::text);
        let result = match self.format.style {
            StylePreset::Pgformatter => {
                pg_format::try_format_parsed(sql, &parse, &self.format.to_pg_config())
            }
            _ => banshee_format::format::try_format_parsed(
                sql,
                &parse,
                &self.format.to_format_config(),
            ),
        };
        let Some((rendered, style)) = template.zip(self.templater.style) else {
            return result;
        };
        let formatted = result.map_err(|error| source_error(error, &rendered))?;
        rendered
            .restore_formatting(source, &formatted, style)
            .map(|restored| restored.text)
            .ok_or_else(placeholder_mismatch)
    }

    /// Returns formatting edits in the original SQL's byte offsets.
    #[must_use]
    pub fn format_edits(&self, source: &str) -> Vec<TextEdit> {
        self.format_selected(source, None)
    }

    /// Formats whole top-level statements intersecting the byte range `start..end`.
    #[must_use]
    pub fn format_range(&self, source: &str, start: u32, end: u32) -> Vec<TextEdit> {
        if start >= end || end as usize > source.len() {
            return Vec::new();
        }
        self.format_selected(source, Some(TextRange::new(start.into(), end.into())))
    }

    fn format_selected(&self, source: &str, selection: Option<TextRange>) -> Vec<TextEdit> {
        let native = banshee_parser::parse(source);
        let rendered = self.templater.render_on_error(source, &native);
        let template = rendered
            .as_ref()
            .map(|text| (text, banshee_parser::parse(text.text())));
        self.format_parsed_edits(
            source,
            &native,
            template.as_ref().map(|(text, parse)| (*text, parse)),
            selection,
        )
    }

    /// Formats an existing document snapshot. `native` must be the parse of
    /// `source`, and any template and its parse must come from the same source
    /// and configuration. Selection and edit offsets refer to `source`.
    #[must_use]
    pub fn format_parsed_edits(
        &self,
        source: &str,
        native: &Parse,
        template: Option<(&Templated, &Parse)>,
        selection: Option<TextRange>,
    ) -> Vec<TextEdit> {
        if selection
            .is_some_and(|range| range.is_empty() || usize::from(range.end()) > source.len())
        {
            return Vec::new();
        }
        // A native expression such as a JSONB question mark takes precedence
        // over a substitution when the original document parses.
        let template = template
            .filter(|_| !native.errors().is_empty())
            .and_then(|(text, parse)| Some((text, parse, self.templater.style?)));
        let (sql, parse) =
            template.map_or((source, native), |(text, parse, _)| (text.text(), parse));
        let selection = selection
            .map(|range| template.map_or(range, |(text, _, _)| text.rendered_range(range)));
        let edits = match self.format.style {
            StylePreset::Pgformatter => {
                pg_format::format_parsed_edits(sql, parse, &self.format.to_pg_config(), selection)
            }
            _ => banshee_format::edits::format_parsed_edits(
                sql,
                parse,
                &self.format.to_format_config(),
                selection,
            ),
        };
        match template {
            Some((rendered, _, style)) if !edits.is_empty() => {
                project_edits(source, rendered, style, &edits).unwrap_or_default()
            }
            _ => edits,
        }
    }
}

fn project_edits(
    source: &str,
    rendered: &Templated,
    style: PlaceholderStyle,
    edits: &[TextEdit],
) -> Option<Vec<TextEdit>> {
    let output = apply_edits(rendered.text(), edits);
    let restored = rendered.restore_formatting(source, &output, style)?;
    let mut before_end: TextSize = 0.into();
    let mut after_end: TextSize = 0.into();
    let mut projected = Vec::with_capacity(edits.len());
    for edit in edits {
        let start = after_end + (edit.range.start() - before_end);
        after_end = start + TextSize::of(edit.new_text.as_str());
        before_end = edit.range.end();
        let before = rendered.map_range(edit.range);
        let after = restored.map_range(TextRange::new(start, after_end));
        projected.extend(
            diff_edits(&source[before], &restored.text[after])
                .into_iter()
                .map(|mut edit| {
                    edit.range += before.start();
                    edit
                }),
        );
    }
    (apply_edits(source, &projected) == restored.text).then_some(projected)
}

fn source_error(mut error: FormatError, rendered: &Templated) -> FormatError {
    if let FormatError::InvalidInput(errors) = &mut error {
        for error in errors {
            error.range = rendered.map_range(error.range);
        }
    }
    error
}

fn placeholder_mismatch() -> FormatError {
    FormatError::SemanticMismatch {
        expected: "the original template placeholders".to_owned(),
        actual: "a placeholder could not be restored safely".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn config(preset: &str, style: &str) -> BansheeConfig {
        BansheeConfig::from_toml(&format!(
            "[format]\nstyle = \"{preset}\"\n[templater]\nstyle = \"{style}\"\n"
        ))
        .expect("formatting configuration")
    }

    fn postgres_tree(sql: &str) -> Value {
        fn strip_offsets(value: &mut Value) {
            match value {
                Value::Object(fields) => {
                    for name in [
                        "location",
                        "stmt_location",
                        "stmt_len",
                        "list_start",
                        "list_end",
                        "rexpr_list_start",
                        "rexpr_list_end",
                        "name_location",
                    ] {
                        fields.remove(name);
                    }
                    fields.values_mut().for_each(strip_offsets);
                }
                Value::Array(items) => items.iter_mut().for_each(strip_offsets),
                _ => {}
            }
        }

        let (_, raw) = pg_parse::parse_debug(sql).expect("PostgreSQL accepts rendered SQL");
        let mut tree = serde_json::from_str(&raw).expect("PostgreSQL AST as JSON");
        strip_offsets(&mut tree);
        tree
    }

    #[test]
    fn formats_templates_with_native_sql_in_all_presets() {
        for (style, source, markers) in [
            (
                "colon",
                "select  1, :Account::int, 1, array(select :Account), arr[1:2], arr[:Lower: :Upper], ':Quoted', $2 from t;",
                &[
                    "array(",
                    ":Account::",
                    "arr[1:2]",
                    ":Lower",
                    ":Upper",
                    "':Quoted'",
                    "$2",
                ][..],
            ),
            (
                "question-mark",
                "select  data ? 'x', data ?| array['a'], data ?& array['b'], data @? '$.c', data ? ?, ?::int, ? from t;",
                &["? 'x'", "?|", "?&", "@?", "? ?", "?::"][..],
            ),
            (
                "percent",
                "select  '🦀', 1, %(Account)s::int, %s, 1, %d from t  -- comment %s\r\n;",
                &["%(Account)s::", "%s", "%d", "-- comment %s"][..],
            ),
        ] {
            for preset in ["sqlstyle", "compact", "pgformatter"] {
                let config = config(preset, style);
                let output = config.try_format(source).expect("format with placeholders");
                assert_ne!(source, output);
                for marker in markers {
                    assert!(output.contains(marker), "{preset}: {output:?}");
                }
                let original = config.templater.render(source).expect("input parameters");
                let formatted = config.templater.render(&output).expect("output parameters");
                assert_eq!(
                    postgres_tree(original.text()),
                    postgres_tree(formatted.text())
                );
                assert_eq!(config.try_format(&output).expect("second pass"), output);
                assert_eq!(apply_edits(source, &config.format_edits(source)), output);
            }
        }
    }

    #[test]
    fn preserves_native_sql_that_also_matches_template_syntax() {
        for (style, source) in [
            ("colon", "select  arr[1:2], arr[:upper], $2 from t;"),
            ("question-mark", "select  data ? 'key' from t;"),
            ("percent", "select  20%s from t;"),
        ] {
            let output = config("compact", style)
                .try_format(source)
                .expect("native SQL");
            assert_ne!(source, output);
            assert_eq!(postgres_tree(source), postgres_tree(&output));
        }
    }

    #[test]
    fn editor_edits_respect_placeholder_selections_and_suppressed_ranges() {
        let before = "select  '🦀', :LongName::int;\r\n";
        let protected = "-- fmt: off\r\nselect  :Keep, /* keep */ 2;\r\n-- fmt: on\r\n";
        let after = "select  :Other;\r\n";
        let source = format!("{before}{protected}{after}");
        let start = source.find(":LongName").expect("placeholder") as u32 + 2;
        for preset in ["sqlstyle", "compact", "pgformatter"] {
            let mut config = config(preset, "colon");
            config.format.pgformatter.no_comment = true;
            let selected = config.format_range(&source, start, start + 2);
            let output = apply_edits(&source, &selected);
            assert_ne!(output, source);
            assert!(
                output.ends_with(&format!("{protected}{after}")),
                "{output:?}"
            );
            assert!(output.contains(":LongName::"), "{output:?}");
            for edits in [
                config.format_edits(&source),
                config.format_range(&source, 0, source.len() as u32),
            ] {
                assert_eq!(edits.len(), 2, "{edits:?}");
                assert!(usize::from(edits[0].range.end()) <= before.len());
                assert!(usize::from(edits[1].range.start()) >= before.len() + protected.len());
                assert!(apply_edits(&source, &edits).contains(protected));
            }
        }
    }

    #[test]
    fn formatting_keeps_the_space_between_a_slice_separator_and_a_placeholder() {
        let cfg = config("compact", "colon");
        let source = "SELECT arr[1: :Upper];\n";
        assert_eq!(cfg.try_format(source).expect("format array slice"), source);
        assert!(cfg.format_edits(source).is_empty());
        assert!(cfg.format_range(source, 0, source.len() as u32).is_empty());
    }

    #[test]
    fn maps_parse_errors_past_replaced_placeholders() {
        let cfg = config("compact", "colon");
        let source = "select :LongName, )";
        let error = cfg.try_format(source).expect_err("invalid SQL");
        let FormatError::InvalidInput(errors) = error else {
            panic!("unexpected format error {error:?}");
        };
        assert!(
            errors
                .iter()
                .any(|error| source.get(std::ops::Range::<usize>::from(error.range)) == Some(")"))
        );
        assert!(cfg.format_edits(source).is_empty());
    }

    #[test]
    fn format_selection_projects_edits_with_an_unfinished_neighbour() {
        let cfg = config("compact", "colon");
        let source = "select  '🦀', :LongName;\r\nselect (";
        let position = source.find(":LongName").expect("placeholder") as u32 + 2;
        let edits = cfg.format_range(source, position, position + 2);
        assert_eq!(
            apply_edits(source, &edits),
            "SELECT '🦀', :LongName;\r\nselect ("
        );
        assert!(cfg.format_edits(source).is_empty());
    }
}
