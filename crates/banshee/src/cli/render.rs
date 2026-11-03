//! Rich, rustc-style diagnostic rendering for human output.
//!
//! Uses `annotate-snippets` (the renderer the Rust compiler itself uses) to
//! print each finding with a `severity[CODE]:` header, the offending source
//! line, and a caret underline.

use std::io::IsTerminal;

use annotate_snippets::{AnnotationKind, Group, Level, Renderer, Snippet};
use banshee_hir::{Diagnostic, Severity};

use super::io::{InputFile, json_escape, line_col};
use super::rules_cmd;

fn level_of<'a>(severity: Severity) -> Level<'a> {
    match severity {
        Severity::Error => Level::ERROR,
        Severity::Warning => Level::WARNING,
        Severity::Info => Level::INFO,
        Severity::Hint => Level::NOTE,
        _ => Level::WARNING,
    }
}

/// Whether colored output should be used: a TTY, `NO_COLOR` unset, and not
/// explicitly disabled by the caller.
pub fn use_color(force_no_color: bool) -> bool {
    if force_no_color || std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    std::io::stdout().is_terminal()
}

/// Renders all diagnostics for one source file as a single string.
#[must_use]
pub fn render(origin: &str, source: &str, diagnostics: &[Diagnostic], color: bool) -> String {
    let renderer = if color {
        Renderer::styled()
    } else {
        Renderer::plain()
    };

    let mut out = String::new();
    for diag in diagnostics {
        let level = level_of(diag.severity);
        let (start, end) = diag
            .range
            .map(|r| (u32::from(r.start()) as usize, u32::from(r.end()) as usize))
            .unwrap_or((0, 0));
        // Clamp to the source bounds so a stale range never panics the renderer.
        let start = start.min(source.len());
        let end = end.min(source.len()).max(start);

        // Primary span carries no label: the message is already on the title
        // line, so repeating it under the caret is noise. Secondary (related)
        // spans below DO carry labels, since they add new information.
        let mut snippet = Snippet::source(source)
            .path(origin)
            .fold(true)
            .annotation(AnnotationKind::Primary.span(start..end));

        // Render related info that points into the same source as secondary
        // caret spans (rustc-style "note: ... defined here"). Related entries
        // without a range fall through to a footer below.
        for rel in &diag.related {
            if let Some(r) = rel.range {
                let rs = (u32::from(r.start()) as usize).min(source.len());
                let re = (u32::from(r.end()) as usize).min(source.len()).max(rs);
                snippet =
                    snippet.annotation(AnnotationKind::Context.span(rs..re).label(&rel.message));
            }
        }

        let mut title = level.primary_title(diag.message.as_str());
        if let Some(code) = diag.code {
            title = title.id(code.as_str());
        }
        let mut message = title.element(snippet);

        if let Some(help) = &diag.help {
            message = message.element(Level::HELP.message(help.as_str()));
        }

        // Related info without a span becomes a plain note line.
        for rel in diag.related.iter().filter(|rel| rel.range.is_none()) {
            message = message.element(Level::NOTE.message(rel.message.as_str()));
        }

        let fix_text;
        if let Some(fix) = diag.fixes.first() {
            fix_text = format!("{} — run `banshee fix`", fix.title);
            message = message.element(Level::NOTE.message(fix_text.as_str()));
        }

        out.push_str(&renderer.render(&[message]).to_string());
        out.push('\n');
    }

    // rustc-style trailer: list each distinct rule code once and point at
    // `banshee explain` for the first, instead of repeating a hint per finding.
    let mut codes: Vec<&'static str> = Vec::new();
    for diag in diagnostics {
        if let Some(code) = diag.code {
            let code = code.as_str();
            if !codes.contains(&code) {
                codes.push(code);
            }
        }
    }
    if let Some(first) = codes.first() {
        let summary = format!(
            "Some lints have detailed explanations: {}.\nFor more information, try `banshee explain {}`.",
            codes.join(", "),
            first
        );
        let report = [Group::with_title(Level::NOTE.primary_title(&summary))];
        out.push_str(&renderer.render(&report).to_string());
        out.push('\n');
    }

    out
}

/// Renders all findings as a single SARIF 2.1.0 document.
pub fn render_sarif(files: &[(&InputFile, Vec<Diagnostic>)]) -> String {
    // Tool driver rules from the catalog.
    let rules: Vec<String> = rules_cmd::RULES
        .iter()
        .map(|r| {
            format!(
                r#"{{"id":{},"shortDescription":{{"text":{}}}}}"#,
                json_escape(r.code),
                json_escape(r.summary)
            )
        })
        .collect();

    let mut results: Vec<String> = Vec::new();
    for (input, diags) in files {
        for d in diags {
            let (sl, sc) = d
                .range
                .map(|r| line_col(&input.text, u32::from(r.start())))
                .unwrap_or((1, 1));
            let (el, ec) = d
                .range
                .map(|r| line_col(&input.text, u32::from(r.end())))
                .unwrap_or((1, 1));
            let level = match d.severity {
                Severity::Error => "error",
                Severity::Info | Severity::Hint => "note",
                _ => "warning",
            };
            let text = match &d.help {
                Some(h) => format!("{} (help: {h})", d.message),
                None => d.message.clone(),
            };
            let rule_id = d.code.map(|c| c.as_str()).unwrap_or("syntax");
            results.push(format!(
                r#"{{"ruleId":{},"level":"{}","message":{{"text":{}}},"locations":[{{"physicalLocation":{{"artifactLocation":{{"uri":{}}},"region":{{"startLine":{},"startColumn":{},"endLine":{},"endColumn":{}}}}}}}]}}"#,
                json_escape(rule_id),
                level,
                json_escape(&text),
                json_escape(&input.label),
                sl,
                sc,
                el,
                ec,
            ));
        }
    }

    format!(
        r#"{{"$schema":"https://json.schemastore.org/sarif-2.1.0.json","version":"2.1.0","runs":[{{"tool":{{"driver":{{"name":"banshee","rules":[{}]}}}},"results":[{}]}}]}}"#,
        rules.join(","),
        results.join(",")
    )
}
