//! Inline `noqa` suppression directives.
//!
//! A SQL comment can suppress lint diagnostics on its own line or across a
//! range of lines. The syntax uses a familiar `-- noqa` convention so existing
//! suppressions carry over:
//!
//! ```sql
//! SELECT * FROM t;            -- noqa            (suppress everything on this line)
//! SELECT * FROM t;            -- noqa: AM04      (suppress only AM04 on this line)
//! SELECT * FROM t;            -- noqa: AM,RF01   (a group prefix or explicit codes)
//!
//! SELECT * FROM t;            -- noqa: disable=AM04   (suppress AM04 from here on)
//! SELECT * FROM t;            -- noqa: enable=AM04     (resume AM04 from here on)
//! SELECT * FROM t;            -- noqa: disable=all     (suppress everything from here on)
//! SELECT * FROM t;            -- noqa: enable=all       (resume everything from here on)
//! ```
//!
//! Rule tokens match case-insensitively. A purely alphabetic token (e.g. `AM`)
//! matches every code sharing that prefix (`AM02`, `AM04`, …); an exact code
//! (`AM04`) matches only itself.

use banshee_syntax::{NodeOrToken, SyntaxKind, SyntaxNode};

use crate::analyze::Diagnostic;

/// A parsed `noqa` directive, tagged with the 1-based line it appears on.
#[derive(Debug, Clone)]
enum Directive {
    /// `-- noqa` — suppress every diagnostic on this line.
    LineAll,
    /// `-- noqa: A,B` — suppress the listed rules/groups on this line.
    LineRules(Vec<String>),
    /// `-- noqa: disable=all` — suppress everything from this line forward.
    RangeDisableAll,
    /// `-- noqa: enable=all` — resume everything from this line forward.
    RangeEnableAll,
    /// `-- noqa: disable=A,B` — suppress the listed rules from this line forward.
    RangeDisable(Vec<String>),
    /// `-- noqa: enable=A,B` — resume the listed rules from this line forward.
    RangeEnable(Vec<String>),
}

/// Drops every diagnostic suppressed by a `noqa` comment in `root`.
///
/// Diagnostics without a source range cannot be located on a line and are left
/// untouched.
pub fn apply(root: &SyntaxNode, diagnostics: &mut Vec<Diagnostic>) {
    let mask = Mask::build(root);
    if mask.is_empty() {
        return;
    }
    diagnostics.retain(|diag| {
        let Some(range) = diag.range else {
            return true;
        };
        let line = mask.line_of(u32::from(range.start()));
        let code = diag.code.map(|c| c.as_str());
        !mask.is_suppressed(line, code)
    });
}

struct Mask {
    /// Byte offset at which each line starts; `line_starts[0] == 0`.
    line_starts: Vec<u32>,
    /// (line, directive), in document order.
    directives: Vec<(usize, Directive)>,
}

impl Mask {
    fn build(root: &SyntaxNode) -> Self {
        let mut line_starts = vec![0u32];
        let mut directives = Vec::new();

        for element in root.descendants_with_tokens() {
            let NodeOrToken::Token(token) = element else {
                continue;
            };
            let start = u32::from(token.text_range().start());
            let text = token.text();

            // Record line boundaries from any token that contains newlines.
            for (i, byte) in text.bytes().enumerate() {
                if byte == b'\n' || (byte == b'\r' && text.as_bytes().get(i + 1) != Some(&b'\n')) {
                    line_starts.push(start + i as u32 + 1);
                }
            }

            if matches!(
                token.kind(),
                SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
            ) && let Some(directive) = parse_comment(text)
            {
                let line = offset_to_line(&line_starts, start);
                directives.push((line, directive));
            }
        }

        Mask {
            line_starts,
            directives,
        }
    }

    fn is_empty(&self) -> bool {
        self.directives.is_empty()
    }

    fn line_of(&self, offset: u32) -> usize {
        offset_to_line(&self.line_starts, offset)
    }

    fn is_suppressed(&self, line: usize, code: Option<&str>) -> bool {
        // Per-line directives on exactly this line.
        for (dline, directive) in &self.directives {
            if *dline != line {
                continue;
            }
            match directive {
                Directive::LineAll => return true,
                Directive::LineRules(rules) => {
                    if let Some(code) = code
                        && rules.iter().any(|r| rule_matches(r, code))
                    {
                        return true;
                    }
                }
                _ => {}
            }
        }

        // Range directives: replay disable/enable events up to and including
        // this line.
        let mut all_disabled = false;
        let mut disabled: Vec<String> = Vec::new();
        for (dline, directive) in &self.directives {
            if *dline > line {
                break;
            }
            match directive {
                Directive::RangeDisableAll => all_disabled = true,
                Directive::RangeEnableAll => {
                    all_disabled = false;
                    disabled.clear();
                }
                Directive::RangeDisable(rules) => {
                    for r in rules {
                        if !disabled.iter().any(|d| d.eq_ignore_ascii_case(r)) {
                            disabled.push(r.clone());
                        }
                    }
                }
                Directive::RangeEnable(rules) => {
                    disabled.retain(|d| !rules.iter().any(|r| r.eq_ignore_ascii_case(d)));
                }
                _ => {}
            }
        }
        if all_disabled {
            return true;
        }
        match code {
            Some(code) => disabled.iter().any(|r| rule_matches(r, code)),
            None => false,
        }
    }
}

/// 1-based line for `offset`.
fn offset_to_line(line_starts: &[u32], offset: u32) -> usize {
    // Index of the last line start that is <= offset.
    line_starts.partition_point(|&s| s <= offset)
}

/// Does directive token `token` match diagnostic code `code`?
///
/// An exact (case-insensitive) match always counts. A purely alphabetic token
/// matches any code that begins with it (so `AM` covers `AM02`/`AM04`).
fn rule_matches(token: &str, code: &str) -> bool {
    let token = token.trim();
    if token.is_empty() {
        return false;
    }
    if token.eq_ignore_ascii_case(code) {
        return true;
    }
    token.chars().all(|c| c.is_ascii_alphabetic())
        && code
            .to_ascii_uppercase()
            .starts_with(&token.to_ascii_uppercase())
}

/// Parses a `noqa` directive out of a comment's raw text (markers included).
fn parse_comment(text: &str) -> Option<Directive> {
    let spec = banshee_syntax::noqa_payload(text)?;
    if spec.is_empty() {
        return Some(Directive::LineAll);
    }

    if let Some(rules) = spec.strip_prefix("disable=") {
        let rules = rules.trim();
        return Some(if rules == "all" {
            Directive::RangeDisableAll
        } else {
            Directive::RangeDisable(split_rules(rules))
        });
    }
    if let Some(rules) = spec.strip_prefix("enable=") {
        let rules = rules.trim();
        return Some(if rules == "all" {
            Directive::RangeEnableAll
        } else {
            Directive::RangeEnable(split_rules(rules))
        });
    }

    Some(Directive::LineRules(split_rules(spec)))
}

fn split_rules(s: &str) -> Vec<String> {
    s.split(',')
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NullSchemaProvider;
    use crate::analyze::analyze_query;

    fn diags(sql: &str) -> Vec<String> {
        let parse = banshee_parser::parse(sql);
        let analysis = analyze_query(&parse, &NullSchemaProvider);
        analysis
            .diagnostics
            .iter()
            .filter_map(|d| d.code.map(|c| c.as_str().to_string()))
            .collect()
    }

    #[test]
    fn parse_variants() {
        assert!(matches!(parse_comment("-- noqa"), Some(Directive::LineAll)));
        assert!(matches!(
            parse_comment("-- noqa: AM04"),
            Some(Directive::LineRules(_))
        ));
        assert!(matches!(
            parse_comment("-- noqa: disable=all"),
            Some(Directive::RangeDisableAll)
        ));
        assert!(matches!(
            parse_comment("-- noqa: enable=AM04,RF01"),
            Some(Directive::RangeEnable(_))
        ));
        assert!(parse_comment("-- just a comment").is_none());
        assert!(parse_comment("-- noqalike").is_none());
        assert!(matches!(
            parse_comment("/* noqa */"),
            Some(Directive::LineAll)
        ));
    }

    #[test]
    fn rule_matching() {
        assert!(rule_matches("AM04", "AM04"));
        assert!(rule_matches("am04", "AM04"));
        assert!(rule_matches("AM", "AM04"));
        assert!(!rule_matches("AM04", "AM05"));
        assert!(!rule_matches("RF", "AM04"));
    }

    #[test]
    fn bare_noqa_suppresses_line() {
        // `SELECT *` on an unterminated statement triggers AM04 (+ CV06).
        assert!(diags("select * from t").contains(&"AM04".to_string()));
        assert!(diags("select * from t -- noqa").is_empty());
    }

    #[test]
    fn coded_noqa_is_selective() {
        // `from a, b` triggers both AM04 (SELECT *) and AM05 (implicit cross
        // join) on the same line; suppressing AM04 leaves AM05.
        let plain = diags("select * from a, b");
        assert!(plain.contains(&"AM04".to_string()));
        assert!(plain.contains(&"AM05".to_string()));

        let got = diags("select * from a, b -- noqa: AM04");
        assert!(!got.contains(&"AM04".to_string()));
        assert!(got.contains(&"AM05".to_string()));
    }

    #[test]
    fn group_prefix_suppresses_family() {
        assert!(!diags("select * from t -- noqa: AM").contains(&"AM04".to_string()));
    }

    #[test]
    fn range_disable_enable() {
        let sql = "\
-- noqa: disable=AM04
select * from a;
select * from b;
-- noqa: enable=AM04
select * from c";
        let parse = banshee_parser::parse(sql);
        let mut analysis = analyze_query(&parse, &NullSchemaProvider);
        // Find AM04 lines that survived.
        let mask = Mask::build(&parse.syntax());
        analysis.diagnostics.retain(|d| {
            let line = mask.line_of(u32::from(d.range.unwrap().start()));
            d.code.map(|c| c.as_str()) == Some("AM04") && line >= 5
        });
        // Only the `select * from c` (line 5) AM04 remains.
        assert_eq!(analysis.diagnostics.len(), 1);
    }
}
