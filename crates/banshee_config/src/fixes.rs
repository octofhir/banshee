//! Shared validation and application of lint fixes.

use std::{cmp::Reverse, ops::Range};

use banshee_format::{FormatError, validate::validate_input, validate_format};
use banshee_hir::{Fix, RuleCode};
use text_size::TextRange;

use crate::{Templated, TemplaterSettings};

/// Whether the rule's fixes only change unquoted SQL token casing.
pub fn is_safe_fix(code: Option<RuleCode>) -> bool {
    matches!(code, Some(RuleCode::Cp01 | RuleCode::Cp02))
}

/// Checks that a fix leaves parseable SQL and preserves all template parameters.
///
/// Capitalization fixes must also preserve the SQL token stream. Structural
/// rewrites are allowed only when the caller explicitly requests them.
pub fn validate_fix_output(
    source: &str,
    output: &str,
    templater: &TemplaterSettings,
    allow_rewrites: bool,
) -> Result<(), FormatError> {
    let (original_parse, original) = templater.parse(source);
    let (output_parse, rewritten) = templater.parse(output);
    if !placeholders(source, original.as_ref()).eq(placeholders(output, rewritten.as_ref())) {
        return Err(FormatError::SemanticMismatch {
            expected: "the original template placeholders".to_owned(),
            actual: "fix changed a template placeholder".to_owned(),
        });
    }
    let original_sql = original.as_ref().map_or(source, Templated::text);
    let output_sql = rewritten.as_ref().map_or(output, Templated::text);
    if allow_rewrites {
        validate_input(&original_parse)?;
        validate_input(&output_parse)
    } else {
        validate_format(original_sql, output_sql)
    }
}

fn placeholders<'a>(
    source: &'a str,
    templated: Option<&'a Templated>,
) -> impl Iterator<Item = &'a str> {
    templated
        .into_iter()
        .flat_map(Templated::placeholder_ranges)
        .map(|range| &source[range])
}

/// Applies nonconflicting fix groups against byte offsets in the source.
pub fn apply_fixes<'a>(source: &str, fixes: impl Iterator<Item = &'a Fix>) -> Option<String> {
    let mut groups: Vec<_> = fixes
        .filter_map(|fix| {
            let mut edits: Vec<_> = fix.edits.iter().collect();
            edits.sort_by_key(|edit| (edit.range.start(), edit.range.end()));
            let span = edits.first()?.range.cover(edits.last()?.range);
            if edits
                .iter()
                .any(|edit| source.get(Range::<usize>::from(edit.range)).is_none())
                || edits
                    .windows(2)
                    .any(|pair| overlaps(pair[0].range, pair[1].range))
            {
                return None;
            }
            Some((span, edits))
        })
        .collect();
    groups.sort_by_key(|(span, _)| (span.start(), Reverse(span.len())));

    // Resolve conflicts before flattening edits, so a fix is applied as a unit.
    let mut previous = None;
    let edits: Vec<_> = groups
        .into_iter()
        .filter_map(|(span, edits)| {
            if previous.is_some_and(|other| overlaps(other, span)) {
                return None;
            }
            previous = Some(span);
            Some(edits)
        })
        .flatten()
        .collect();
    if edits.is_empty() {
        return None;
    }
    let mut result = source.to_owned();
    for edit in edits.into_iter().rev() {
        result.replace_range(Range::<usize>::from(edit.range), &edit.new_text);
    }
    Some(result)
}

// Ranges are ordered by start. Two edits inserting at the same point conflict.
fn overlaps(left: TextRange, right: TextRange) -> bool {
    left.end() > right.start() || (left.is_empty() && left.start() == right.start())
}

#[cfg(test)]
mod tests {
    use banshee_hir::TextEdit;

    use super::*;

    #[test]
    fn discards_an_entire_fix_when_one_edit_conflicts() {
        let fixes = [
            Fix::new(
                "First",
                vec![TextEdit::replace(TextRange::new(0.into(), 3.into()), "one")],
            ),
            Fix::new(
                "Overlapping",
                vec![
                    TextEdit::replace(TextRange::new(2.into(), 3.into()), "two"),
                    TextEdit::replace(TextRange::new(4.into(), 5.into()), "three"),
                ],
            ),
        ];
        assert_eq!(
            apply_fixes("abcdef", fixes.iter()).as_deref(),
            Some("onedef")
        );
    }

    #[test]
    fn validation_preserves_template_parameter_identity() {
        let templater = TemplaterSettings {
            style: Some(crate::PlaceholderStyle::Colon),
        };
        let source = "select :value, 2";
        assert!(validate_fix_output(source, "SELECT :value, 2", &templater, false).is_ok());
        assert!(validate_fix_output(source, "SELECT :value, 3", &templater, true).is_ok());
        assert!(validate_fix_output(source, "SELECT :other, 2", &templater, false).is_err());
        assert!(validate_fix_output(source, "SELECT :valueextra, 2", &templater, true).is_err());
        assert!(validate_fix_output(source, "SELECT 1, 2", &templater, true).is_err());
    }
}
