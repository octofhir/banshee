//! Signature help (`textDocument/signatureHelp`).
//!
//! Finds the innermost function call surrounding the cursor, then renders its
//! signature with the active parameter highlighted, using built-in function
//! metadata (and any function provider).

use banshee_completion::{
    generators::functions::get_function_signature,
    providers::FunctionProvider,
    types::{ArgMode, FunctionInfo},
};
use banshee_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use lsp_types::{
    Documentation, ParameterInformation, ParameterLabel, SignatureHelp, SignatureInformation,
};

/// Computes signature help at `offset`, if the cursor is inside a call.
pub fn signature_help(
    root: &SyntaxNode,
    offset: u32,
    provider: Option<&dyn FunctionProvider>,
) -> Option<SignatureHelp> {
    let (call, arguments) = enclosing_call(root, offset)?;
    let (schema, name) = call_name(call)?;
    let mut overloads = Vec::new();
    if matches!(call.kind(), SyntaxKind::FUNC_CALL | SyntaxKind::TABLE_REF)
        && let Some(provider) = provider
    {
        overloads = provider.overloads(schema.as_deref(), &name);
    }
    if overloads.is_empty()
        && schema
            .as_deref()
            .is_none_or(|schema| schema == "pg_catalog")
        && !name.bytes().any(|b| b.is_ascii_uppercase())
    {
        overloads.extend(get_function_signature(None, &name));
    }
    let (active, _) = overloads
        .iter()
        .enumerate()
        .min_by_key(|(_, info)| signature_rank(info, &arguments))?;
    let signatures: Vec<_> = overloads
        .into_iter()
        .map(|info| signature_information(info, &arguments))
        .collect();

    Some(SignatureHelp {
        active_parameter: signatures[active].active_parameter,
        active_signature: Some(active as u32),
        signatures,
    })
}

fn signature_information(info: FunctionInfo, arguments: &CallArguments) -> SignatureInformation {
    let active = named_parameter(&info, arguments.name.as_deref()).or_else(|| {
        info.args
            .len()
            .checked_sub(1)
            .map(|last| last.min(arguments.active))
    });
    let parameters: Vec<ParameterInformation> = info
        .args
        .iter()
        .map(|a| ParameterInformation {
            label: ParameterLabel::Simple(a.label()),
            documentation: None,
        })
        .collect();

    SignatureInformation {
        label: info.signature(),
        documentation: info.description.map(Documentation::String),
        parameters: Some(parameters),
        active_parameter: active.map(|index| index as u32),
    }
}

fn named_parameter(info: &FunctionInfo, name: Option<&str>) -> Option<usize> {
    let name = name?;
    info.args
        .iter()
        .position(|arg| arg.name.as_deref() == Some(name))
}

fn signature_rank(info: &FunctionInfo, arguments: &CallArguments) -> (bool, u8, usize) {
    let name_mismatch =
        arguments.name.is_some() && named_parameter(info, arguments.name.as_deref()).is_none();
    let total = info.args.len();
    let required = info.args.iter().filter(|arg| arg.default.is_none()).count();
    let variadic = info
        .args
        .last()
        .is_some_and(|arg| arg.mode == ArgMode::Variadic);
    let count = arguments.count;
    let (rank, distance) = if count == total && !variadic {
        (0, 0)
    } else if count >= required && (count <= total || variadic) {
        (1, total.abs_diff(count))
    } else if count < required {
        (2, required - count)
    } else {
        (3, count - total)
    };
    (name_mismatch, rank, distance)
}

struct CallArguments {
    active: usize,
    count: usize,
    name: Option<String>,
}

/// Finds the innermost argument list enclosing the cursor.
fn enclosing_call(root: &SyntaxNode, offset: u32) -> Option<(&SyntaxNode, CallArguments)> {
    root.descendants()
        .filter(|node| {
            matches!(
                node.kind(),
                SyntaxKind::FUNC_CALL
                    | SyntaxKind::TABLE_REF
                    | SyntaxKind::COALESCE_EXPR
                    | SyntaxKind::NULLIF_EXPR
                    | SyntaxKind::GREATEST_EXPR
                    | SyntaxKind::LEAST_EXPR
            ) && u32::from(node.text_range().start()) <= offset
                && offset <= u32::from(node.text_range().end())
        })
        .filter_map(|node| arguments_at(node, offset).map(|arguments| (node, arguments)))
        .min_by_key(|(node, _)| node.text_range().len())
}

fn arguments_at(call: &SyntaxNode, offset: u32) -> Option<CallArguments> {
    use SyntaxKind::*;
    let mut children = call.children_with_tokens();
    let open = children.find(|e| e.kind() == L_PAREN)?;
    if offset < u32::from(open.text_range().end()) {
        return None;
    }
    let mut active = 0;
    let mut separators = 0;
    let mut has_argument = false;
    let mut in_order_by = false;
    let mut name = None;
    for element in children {
        let range = element.text_range();
        let start = u32::from(range.start());
        match element.kind() {
            R_PAREN if start < offset => return None,
            R_PAREN => break,
            ORDER_KW => in_order_by = true,
            kind if in_order_by || kind.is_trivia() => {}
            COMMA | FROM_KW | FOR_KW => {
                separators += 1;
                if start < offset {
                    active += 1;
                    name = None;
                }
            }
            NAMED_ARG if start <= offset && offset <= u32::from(range.end()) => {
                has_argument = true;
                name = element
                    .as_node()
                    .and_then(|node| node.first_token())
                    .and_then(identifier);
            }
            _ => has_argument = true,
        }
    }
    let count = separators + usize::from(has_argument || separators > 0);
    Some(CallArguments {
        active,
        count,
        name,
    })
}

fn call_name(call: &SyntaxNode) -> Option<(Option<String>, String)> {
    let mut schema = None;
    let mut name = None;
    for token in call.children_with_tokens().filter_map(|e| e.into_token()) {
        match token.kind() {
            SyntaxKind::L_PAREN => break,
            SyntaxKind::LATERAL_KW if name.is_none() => continue,
            SyntaxKind::DOT => schema = name.take(),
            SyntaxKind::IDENT | SyntaxKind::QUOTED_IDENT => name = identifier(token),
            k if k.is_keyword() => name = identifier(token),
            _ => {}
        }
    }
    Some((schema, name?))
}

fn identifier(token: &SyntaxToken) -> Option<String> {
    if token.kind() == SyntaxKind::QUOTED_IDENT {
        let name = token.text().strip_prefix('"')?.strip_suffix('"')?;
        Some(name.replace("\"\"", "\""))
    } else {
        Some(token.text().to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use banshee_completion::{providers::MemoryFunctionProvider, types::FunctionArg};

    fn help_at_cursor(sql: &str, provider: Option<&dyn FunctionProvider>) -> Option<SignatureHelp> {
        let (before, after) = sql.split_once('|').expect("cursor marker");
        let parse = banshee_parser::parse(&format!("{before}{after}"));
        signature_help(&parse.syntax(), before.len() as u32, provider)
    }

    #[test]
    fn offline_signature_uses_the_innermost_argument_list() {
        let inner = help_at_cursor("SELECT COALESCE(LOWER('😀'|), 'b')", None).unwrap();
        assert_eq!(inner.signatures[0].label, "lower(text) -> text");
        assert_eq!(inner.active_parameter, Some(0));

        let outer = help_at_cursor("SELECT COALESCE(LOWER('😀')|, 'b')", None).unwrap();
        assert!(outer.signatures[0].label.starts_with("coalesce("));
        assert_eq!(outer.active_parameter, Some(0));

        let next = help_at_cursor("SELECT NULLIF(LOWER('😀'), |'a')", None).unwrap();
        assert_eq!(next.active_parameter, Some(1));
        assert!(help_at_cursor("SELECT lower('x')| FROM t", None).is_none());
        assert!(help_at_cursor("SELECT lower(|", None).is_some());
    }

    #[test]
    fn signature_respects_qualified_and_quoted_names() {
        let provider = MemoryFunctionProvider::new()
            .add_function(FunctionInfo::new("lower", "uuid").with_schema("app"));
        let app = help_at_cursor("SELECT app.lower(|'x')", Some(&provider)).unwrap();
        assert_eq!(app.signatures[0].label, "lower() -> uuid");

        let pg = help_at_cursor("SELECT PG_CATALOG.\"lower\"(|'x')", Some(&provider)).unwrap();
        assert_eq!(pg.signatures[0].label, "lower(text) -> text");
        assert!(help_at_cursor("SELECT other.lower(|'x')", Some(&provider)).is_none());
        assert!(help_at_cursor("SELECT \"LOWER\"(|'x')", None).is_none());
    }

    #[test]
    fn signature_handles_variadic_and_sql_argument_separators() {
        let path =
            help_at_cursor("SELECT jsonb_extract_path('{}', 'first', |'second')", None).unwrap();
        assert_eq!(path.active_parameter, Some(1));
        assert!(
            path.signatures[0]
                .label
                .contains("VARIADIC path_elems text[]")
        );
        let substring = help_at_cursor("SELECT substring('abc' FROM 1 FOR |2)", None).unwrap();
        assert_eq!(substring.active_parameter, Some(2));
    }

    #[test]
    fn signature_lists_overloads_and_selects_the_best_argument_match() {
        let provider = MemoryFunctionProvider::new()
            .add_function(
                FunctionInfo::new("pick", "integer")
                    .with_schema("app")
                    .with_arg(FunctionArg::new("integer")),
            )
            .add_function(
                FunctionInfo::new("pick", "text")
                    .with_schema("app")
                    .with_arg(FunctionArg::new("text").with_name("value"))
                    .with_arg(
                        FunctionArg::new("integer")
                            .with_name("Repeat Count")
                            .with_default("2"),
                    ),
            );
        let single = help_at_cursor("SELECT app.pick(|123)", Some(&provider)).unwrap();
        assert_eq!(single.signatures.len(), 2);
        assert_eq!(single.active_signature, Some(0));

        let double = help_at_cursor("SELECT app.pick('a', |2)", Some(&provider)).unwrap();
        assert_eq!(double.active_signature, Some(1));
        assert_eq!(double.active_parameter, Some(1));
        assert_eq!(
            double.signatures[1].label,
            "pick(value text, \"Repeat Count\" integer = 2) -> text"
        );

        let named = help_at_cursor(
            "SELECT app.pick(\"Repeat Count\" => |2, value => 'a')",
            Some(&provider),
        )
        .unwrap();
        assert_eq!(named.active_signature, Some(1));
        assert_eq!(named.active_parameter, Some(1));
    }

    #[test]
    fn signature_for_set_returning_function_in_from() {
        let provider = MemoryFunctionProvider::new().add_function(
            FunctionInfo::new("list_values", "TABLE(label text)")
                .with_schema("app")
                .with_arg(FunctionArg::new("text").with_name("seed")),
        );
        let help = help_at_cursor(
            "SELECT * FROM LATERAL app.list_values(|'a')",
            Some(&provider),
        )
        .expect("signature for a function used as a table source");
        assert_eq!(
            help.signatures[0].label,
            "list_values(seed text) -> TABLE(label text)"
        );
    }
}
